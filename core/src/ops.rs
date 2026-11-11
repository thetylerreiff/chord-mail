//! Actions on conversations. Each one changes the cache at once, queues the
//! matching Gmail change, and returns what it takes to undo it exactly.

use anyhow::Result;
use serde::Serialize;

use crate::gmail::{ApiError, MailApi, is_permanent};
use crate::store::{Op, Snooze, SnoozeKind, Store};
use crate::sync::now_ms;

#[derive(Debug, Clone, PartialEq)]
pub enum Action {
    /// "Done": out of the inbox, still in All Mail.
    Archive,
    MoveToInbox,
    Trash,
    Untrash,
    Spam,
    Star,
    Unstar,
    MarkRead,
    MarkUnread,
    AddLabel(String),
    RemoveLabel(String),
    /// Add a label and archive, like moving to a folder.
    MoveTo(String),
    Snooze {
        until: i64,
    },
    /// A snooze or reminder coming due, or the user bringing it back early.
    Wake,
    /// Watch a sent thread and bring it back if nobody replies.
    Remind {
        until: i64,
    },
}

/// Everything needed to put the affected threads back as they were.
#[derive(Debug, Clone, Default, Serialize)]
pub struct Undo {
    labels: Vec<(String, Vec<String>)>,
    snoozes: Vec<(String, Option<Snooze>)>,
    trashed: Vec<String>,
    untrashed: Vec<String>,
}

const SYSTEM_TRASH: &str = "TRASH";

/// Applies `action` to `threads` and returns its undo.
pub fn apply(store: &mut Store, threads: &[String], action: &Action) -> Result<Undo> {
    let mut undo = Undo {
        labels: store.label_snapshot(threads)?,
        ..Default::default()
    };
    for t in threads {
        undo.snoozes.push((t.clone(), store.snooze_of(t)?));
    }
    let snooze_label = store.meta("snooze_label")?;
    let now = now_ms();

    let (add, remove): (Vec<String>, Vec<String>) = match action {
        Action::Archive => (vec![], vec!["INBOX".into()]),
        Action::MoveToInbox => (vec!["INBOX".into()], snooze_label.iter().cloned().collect()),
        Action::Spam => (vec!["SPAM".into()], vec!["INBOX".into()]),
        Action::Star => (vec!["STARRED".into()], vec![]),
        Action::Unstar => (vec![], vec!["STARRED".into()]),
        Action::MarkRead => (vec![], vec!["UNREAD".into()]),
        Action::MarkUnread => (vec!["UNREAD".into()], vec![]),
        Action::AddLabel(l) => (vec![l.clone()], vec![]),
        Action::RemoveLabel(l) => (vec![], vec![l.clone()]),
        Action::MoveTo(l) => (vec![l.clone()], vec!["INBOX".into()]),
        Action::Snooze { .. } => (snooze_label.iter().cloned().collect(), vec!["INBOX".into()]),
        Action::Wake => (vec!["INBOX".into()], snooze_label.iter().cloned().collect()),
        Action::Remind { .. } => (vec![], vec![]),
        Action::Trash | Action::Untrash => (vec![], vec![]),
    };

    for t in threads {
        let messages = store.thread_messages(t)?;
        // Unread and woken threads mark only the newest message, as Gmail
        // does, so your own replies don't turn bold.
        let only_latest = matches!(action, Action::MarkUnread | Action::Wake);
        let targets: Vec<_> = if only_latest {
            messages.iter().rev().take(1).collect()
        } else {
            messages.iter().collect()
        };
        match action {
            Action::Trash => {
                for m in &messages {
                    let mut labels = m.labels.clone();
                    if !labels.iter().any(|l| l == SYSTEM_TRASH) {
                        labels.push(SYSTEM_TRASH.into());
                    }
                    store.set_message_labels(&m.id, &labels)?;
                }
                store.clear_snooze(t)?;
                store.enqueue(&Op::Trash { thread: t.clone() })?;
                undo.trashed.push(t.clone());
                continue;
            }
            Action::Untrash => {
                for m in &messages {
                    let labels: Vec<String> = m
                        .labels
                        .iter()
                        .filter(|l| *l != SYSTEM_TRASH)
                        .cloned()
                        .collect();
                    store.set_message_labels(&m.id, &labels)?;
                }
                store.enqueue(&Op::Untrash { thread: t.clone() })?;
                undo.untrashed.push(t.clone());
                continue;
            }
            _ => {}
        }

        let mut ids = Vec::new();
        for m in &targets {
            let mut labels: Vec<String> = m
                .labels
                .iter()
                .filter(|l| !remove.contains(l))
                .cloned()
                .collect();
            // The wake also marks the newest message unread so it stands out.
            let extra: &[&str] = if matches!(action, Action::Wake) {
                &["UNREAD"]
            } else {
                &[]
            };
            for a in add.iter().map(String::as_str).chain(extra.iter().copied()) {
                if !labels.iter().any(|l| l == a) {
                    labels.push(a.to_string());
                }
            }
            if labels != m.labels {
                store.set_message_labels(&m.id, &labels)?;
            }
            ids.push(m.id.clone());
        }
        // Waking must also take the snooze label off older messages.
        if matches!(action, Action::Wake | Action::MoveToInbox) {
            for m in messages
                .iter()
                .filter(|m| !targets.iter().any(|t| t.id == m.id))
            {
                let labels: Vec<String> = m
                    .labels
                    .iter()
                    .filter(|l| !remove.contains(l))
                    .cloned()
                    .collect();
                if labels != m.labels {
                    store.set_message_labels(&m.id, &labels)?;
                    ids.push(m.id.clone());
                }
            }
        }

        match action {
            Action::Snooze { until } => store.set_snooze(&Snooze {
                thread_id: t.clone(),
                until: *until,
                kind: SnoozeKind::Snooze,
                since: now,
            })?,
            Action::Remind { until } => store.set_snooze(&Snooze {
                thread_id: t.clone(),
                until: *until,
                kind: SnoozeKind::Remind,
                since: now,
            })?,
            Action::Wake => {
                store.clear_snooze(t)?;
                store.set_surfaced(t, now)?;
            }
            Action::MoveToInbox | Action::Archive => store.clear_snooze(t)?,
            _ => {}
        }

        if !ids.is_empty()
            && (!add.is_empty() || !remove.is_empty() || matches!(action, Action::Wake))
        {
            let mut add = add.clone();
            if matches!(action, Action::Wake) {
                add.push("UNREAD".into());
            }
            // The newest-only actions still need the snooze label off every
            // message, so they queue in two parts.
            if only_latest {
                let latest: Vec<String> = targets.iter().map(|m| m.id.clone()).collect();
                store.enqueue(&Op::Modify {
                    ids: latest.clone(),
                    add,
                    remove: remove.clone(),
                })?;
                let rest: Vec<String> = ids.into_iter().filter(|id| !latest.contains(id)).collect();
                if !rest.is_empty() && !remove.is_empty() {
                    store.enqueue(&Op::Modify {
                        ids: rest,
                        add: vec![],
                        remove: remove.clone(),
                    })?;
                }
            } else {
                store.enqueue(&Op::Modify {
                    ids,
                    add,
                    remove: remove.clone(),
                })?;
            }
        }
    }
    Ok(undo)
}

/// Labels to add and labels to remove.
type LabelChange = (Vec<String>, Vec<String>);

/// Puts everything back as it was before the action, locally and in Gmail.
pub fn undo(store: &mut Store, undo: &Undo) -> Result<()> {
    for t in &undo.trashed {
        store.enqueue(&Op::Untrash { thread: t.clone() })?;
    }
    for t in &undo.untrashed {
        store.enqueue(&Op::Trash { thread: t.clone() })?;
    }
    // Group identical changes so each becomes one request.
    let mut groups: Vec<(LabelChange, Vec<String>)> = Vec::new();
    for (id, old) in &undo.labels {
        let Some(current) = store.message(id)? else {
            continue;
        };
        let add: Vec<String> = old
            .iter()
            .filter(|l| *l != SYSTEM_TRASH && !current.labels.contains(l))
            .cloned()
            .collect();
        let remove: Vec<String> = current
            .labels
            .iter()
            .filter(|l| *l != SYSTEM_TRASH && !old.contains(l))
            .cloned()
            .collect();
        store.set_message_labels(id, old)?;
        if add.is_empty() && remove.is_empty() {
            continue;
        }
        match groups.iter_mut().find(|(k, _)| k.0 == add && k.1 == remove) {
            Some((_, ids)) => ids.push(id.clone()),
            None => groups.push(((add, remove), vec![id.clone()])),
        }
    }
    for ((add, remove), ids) in groups {
        store.enqueue(&Op::Modify { ids, add, remove })?;
    }
    for (thread, before) in &undo.snoozes {
        match before {
            Some(s) => store.set_snooze(s)?,
            None => store.clear_snooze(thread)?,
        }
    }
    Ok(())
}

/// How many times Gmail may turn a queued action down (a 403 for missing
/// permission, say) before it's dropped so the actions behind it can go.
const MAX_REJECTIONS: i64 = 5;

/// Whether Gmail itself answered and turned the request down: a 4xx status.
/// Not 429 or a rate-limit 403 (slow down, not no), 408 (a timeout), or 401
/// (the sign-in, not the action); and never a network error or a 5xx.
fn is_rejection(e: &anyhow::Error) -> bool {
    match e.downcast_ref() {
        Some(ApiError::Status {
            status,
            rate_limited,
            ..
        }) => (400..500).contains(status) && !rate_limited && !matches!(status, 401 | 408 | 429),
        _ => false,
    }
}

/// Pushes queued actions to Gmail in order. Stops at the first one that
/// fails for a reason a retry might fix, so later actions can't overtake it;
/// drops ones Gmail will never accept, or has rejected too often.
pub async fn flush(api: &impl MailApi, store: &mut Store) -> Result<usize> {
    let mut done = 0;
    for (id, op) in store.pending_ops()? {
        let result = match &op {
            Op::Modify { ids, add, remove } => api.modify(ids, add, remove).await,
            Op::Trash { thread } => api.trash_thread(thread).await,
            Op::Untrash { thread } => api.untrash_thread(thread).await,
        };
        match result {
            Ok(()) => {
                store.op_done(id)?;
                done += 1;
            }
            Err(e) if is_permanent(&e) => {
                eprintln!("chord: dropping an action Gmail won't accept: {e:#}");
                store.op_done(id)?;
            }
            Err(e) => {
                let rejected = is_rejection(&e);
                let rejections = store.op_failed(id, &format!("{e:#}"), rejected)?;
                if rejected && rejections >= MAX_REJECTIONS {
                    eprintln!("chord: dropping an action Gmail rejected {rejections} times: {e:#}");
                    store.op_done(id)?;
                    continue;
                }
                return Err(e);
            }
        }
    }
    Ok(done)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::gmail::{MessagePage, Profile};
    use crate::store::tests::row;
    use std::cell::RefCell;

    pub(crate) type MakeError = fn() -> anyhow::Error;

    /// An empty mailbox whose `trash_thread` fails for chosen threads, and
    /// that records the threads it trashed.
    #[derive(Default)]
    pub(crate) struct QueueFake {
        pub failing: RefCell<Vec<(String, MakeError)>>,
        pub trashed: RefCell<Vec<String>>,
    }

    impl MailApi for QueueFake {
        async fn profile(&self) -> Result<Profile> {
            Ok(Profile {
                email_address: "me@example.com".into(),
                history_id: "1".into(),
            })
        }
        async fn list_messages(&self, _label: &str, _page: Option<&str>) -> Result<MessagePage> {
            Ok(MessagePage::default())
        }
        async fn message_metadata(&self, _id: &str) -> Result<crate::gmail::Message> {
            Err(ApiError::NotFound.into())
        }
        async fn history(
            &self,
            _start: &str,
            _page: Option<&str>,
        ) -> Result<crate::gmail::HistoryPage> {
            Err(ApiError::HistoryExpired.into())
        }
        async fn trash_thread(&self, id: &str) -> Result<()> {
            if let Some((_, error)) = self.failing.borrow().iter().find(|(t, _)| t == id) {
                return Err(error());
            }
            self.trashed.borrow_mut().push(id.into());
            Ok(())
        }
    }

    pub(crate) fn status(status: u16, rate_limited: bool) -> anyhow::Error {
        ApiError::Status {
            status,
            rate_limited,
            message: format!("Gmail returned {status} for /threads/bad/trash: no"),
        }
        .into()
    }

    pub(crate) fn forbidden() -> anyhow::Error {
        status(403, false)
    }

    fn queue_bad_then_good(store: &mut Store, api: &QueueFake, error: MakeError) {
        api.failing.borrow_mut().push(("bad".into(), error));
        for thread in ["bad", "good"] {
            store
                .enqueue(&Op::Trash {
                    thread: thread.into(),
                })
                .unwrap();
        }
    }

    fn block_on<F: std::future::Future>(f: F) -> F::Output {
        tokio::runtime::Builder::new_current_thread()
            .build()
            .unwrap()
            .block_on(f)
    }

    #[test]
    fn an_action_gmail_keeps_rejecting_is_dropped_after_the_cap() {
        let mut store = Store::in_memory().unwrap();
        let api = QueueFake::default();
        queue_bad_then_good(&mut store, &api, forbidden);
        for _ in 1..MAX_REJECTIONS {
            assert!(block_on(flush(&api, &mut store)).is_err());
            assert_eq!(
                store.pending_ops().unwrap().len(),
                2,
                "nothing overtakes it"
            );
            assert!(api.trashed.borrow().is_empty());
        }
        assert_eq!(block_on(flush(&api, &mut store)).unwrap(), 1);
        assert!(store.pending_ops().unwrap().is_empty());
        assert_eq!(*api.trashed.borrow(), ["good"]);
    }

    #[test]
    fn network_server_and_throttling_errors_never_drop_an_action() {
        let transient: [MakeError; 6] = [
            || anyhow::anyhow!("error sending request: connection refused"),
            || status(503, false),
            || status(500, false),
            || status(429, false),
            || status(403, true),
            || status(408, false),
        ];
        for error in transient {
            let mut store = Store::in_memory().unwrap();
            let api = QueueFake::default();
            queue_bad_then_good(&mut store, &api, error);
            for _ in 0..(MAX_REJECTIONS * 10) {
                assert!(block_on(flush(&api, &mut store)).is_err());
            }
            assert_eq!(store.pending_ops().unwrap().len(), 2, "{}", error());
            assert!(api.trashed.borrow().is_empty());
        }
    }

    #[test]
    fn only_gmail_turning_the_action_down_counts_as_a_rejection() {
        assert!(is_rejection(&status(403, false)));
        assert!(is_rejection(&status(409, false)));
        for e in [
            status(401, false),
            status(403, true),
            status(408, false),
            status(429, false),
            status(500, false),
            status(503, false),
            anyhow::anyhow!("timed out"),
            // Already dropped at once as permanent.
            ApiError::NotFound.into(),
        ] {
            assert!(!is_rejection(&e), "{e}");
        }
    }

    fn store_with_thread() -> Store {
        let mut store = Store::in_memory().unwrap();
        store.set_meta("snooze_label", "Label_9").unwrap();
        store
            .apply(
                false,
                &[
                    row("a", "t", 1, &["INBOX", "UNREAD", "CATEGORY_PERSONAL"]),
                    row("b", "t", 2, &["INBOX", "UNREAD"]),
                ],
                &[],
                None,
            )
            .unwrap();
        store
    }

    fn labels(store: &Store, id: &str) -> Vec<String> {
        let mut l = store.message(id).unwrap().unwrap().labels;
        l.sort();
        l
    }

    fn t() -> Vec<String> {
        vec!["t".into()]
    }

    #[test]
    fn archive_is_local_queued_and_undoable() {
        let mut store = store_with_thread();
        let u = apply(&mut store, &t(), &Action::Archive).unwrap();
        assert!(store.thread_ids(Some("INBOX")).unwrap().is_empty());
        assert_eq!(
            store.pending_ops().unwrap()[0].1,
            Op::Modify {
                ids: vec!["a".into(), "b".into()],
                add: vec![],
                remove: vec!["INBOX".into()]
            }
        );

        undo(&mut store, &u).unwrap();
        assert_eq!(store.thread_ids(Some("INBOX")).unwrap(), ["t"]);
        assert_eq!(
            labels(&store, "a"),
            ["CATEGORY_PERSONAL", "INBOX", "UNREAD"]
        );
        let ops = store.pending_ops().unwrap();
        assert_eq!(
            ops[1].1,
            Op::Modify {
                ids: vec!["a".into(), "b".into()],
                add: vec!["INBOX".into()],
                remove: vec![]
            }
        );
    }

    #[test]
    fn trash_and_undo_use_thread_calls() {
        let mut store = store_with_thread();
        let u = apply(&mut store, &t(), &Action::Trash).unwrap();
        assert!(store.thread_ids(Some("INBOX")).unwrap().is_empty());
        undo(&mut store, &u).unwrap();
        let ops: Vec<Op> = store
            .pending_ops()
            .unwrap()
            .into_iter()
            .map(|(_, o)| o)
            .collect();
        assert_eq!(
            ops,
            [
                Op::Trash { thread: "t".into() },
                Op::Untrash { thread: "t".into() }
            ]
        );
        assert_eq!(store.thread_ids(Some("INBOX")).unwrap(), ["t"]);
    }

    #[test]
    fn mark_unread_touches_only_the_newest() {
        let mut store = store_with_thread();
        apply(&mut store, &t(), &Action::MarkRead).unwrap();
        apply(&mut store, &t(), &Action::MarkUnread).unwrap();
        assert!(!labels(&store, "a").contains(&"UNREAD".to_string()));
        assert!(labels(&store, "b").contains(&"UNREAD".to_string()));
    }

    #[test]
    fn snooze_hides_then_wake_surfaces_unread_on_top() {
        let mut store = store_with_thread();
        apply(&mut store, &t(), &Action::MarkRead).unwrap();
        apply(&mut store, &t(), &Action::Snooze { until: 99 }).unwrap();
        assert!(store.thread_ids(Some("INBOX")).unwrap().is_empty());
        assert_eq!(store.thread_ids(Some("Label_9")).unwrap(), ["t"]);
        assert_eq!(store.snoozes().unwrap()[0].until, 99);

        apply(&mut store, &t(), &Action::Wake).unwrap();
        assert_eq!(store.thread_ids(Some("INBOX")).unwrap(), ["t"]);
        assert!(store.thread_ids(Some("Label_9")).unwrap().is_empty());
        assert!(store.snoozes().unwrap().is_empty());
        let row = &store.thread_rows(&t()).unwrap()[0];
        assert!(row.unread);
        assert!(row.date > 2, "woken threads sort by when they woke");
    }

    #[test]
    fn undo_restores_a_snooze() {
        let mut store = store_with_thread();
        apply(&mut store, &t(), &Action::Snooze { until: 99 }).unwrap();
        let u = apply(&mut store, &t(), &Action::Wake).unwrap();
        undo(&mut store, &u).unwrap();
        assert_eq!(store.snoozes().unwrap().len(), 1);
        assert!(store.thread_ids(Some("INBOX")).unwrap().is_empty());
    }
}
