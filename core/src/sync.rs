//! Brings the local cache up to date with the mailbox.
//!
//! The first sync pulls the newest inbox messages. Every sync after that asks
//! Gmail's history log what changed since the last one and refetches only
//! those messages, which is usually a handful of requests.

use std::collections::HashSet;

use anyhow::Result;
use futures::{StreamExt, TryStreamExt};
use serde::Serialize;

use crate::gmail::{ApiError, MailApi};
use crate::store::{MessageRow, Store};

/// Parallel fetches. Four keeps a first sync quick while staying well under
/// Gmail's per-user quota, which a burst of parallel requests can exhaust.
const CONCURRENCY: usize = 4;

#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum SyncKind {
    Full,
    Incremental,
}

#[derive(Debug, Clone, Serialize)]
pub struct SyncReport {
    pub kind: SyncKind,
    pub fetched: usize,
    pub removed: usize,
    /// Messages that arrived since the last sync. Empty after a full sync,
    /// which can't tell new mail from old.
    pub arrived: Vec<String>,
}

/// Syncs `store` from `api`. `full_limit` caps how many inbox messages a full
/// sync downloads.
pub async fn sync(api: &impl MailApi, store: &mut Store, full_limit: usize) -> Result<SyncReport> {
    if let Some(start) = store.meta("history_id")? {
        match incremental(api, store, &start).await {
            Err(e) if matches!(e.downcast_ref(), Some(ApiError::HistoryExpired)) => {}
            other => return other,
        }
    }
    full(api, store, full_limit).await
}

async fn full(api: &impl MailApi, store: &mut Store, limit: usize) -> Result<SyncReport> {
    // Read the history id before listing, so anything that changes while the
    // listing runs is replayed by the next incremental sync instead of lost.
    let profile = api.profile().await?;

    let mut ids = Vec::new();
    let mut page_token: Option<String> = None;
    while ids.len() < limit {
        let page = api.list_messages("INBOX", page_token.as_deref()).await?;
        ids.extend(page.messages.into_iter().map(|m| m.id));
        match page.next_page_token {
            Some(t) => page_token = Some(t),
            None => break,
        }
    }
    ids.truncate(limit);

    let (rows, _gone) = fetch(api, ids).await?;
    store.apply(true, &rows, &[], Some(&profile.history_id))?;
    store.set_meta("email", &profile.email_address)?;
    Ok(SyncReport {
        kind: SyncKind::Full,
        fetched: rows.len(),
        removed: 0,
        arrived: Vec::new(),
    })
}

async fn incremental(api: &impl MailApi, store: &mut Store, start: &str) -> Result<SyncReport> {
    let mut touched = Vec::new();
    let mut seen = HashSet::new();
    let mut deleted = HashSet::new();
    let mut added = Vec::new();
    let mut page_token: Option<String> = None;

    let history_id = loop {
        let page = api.history(start, page_token.as_deref()).await?;
        for h in page.history {
            for e in h.messages_deleted {
                deleted.insert(e.message.id);
            }
            added.extend(h.messages_added.iter().map(|e| e.message.id.clone()));
            for e in h
                .messages_added
                .into_iter()
                .chain(h.labels_added)
                .chain(h.labels_removed)
            {
                if seen.insert(e.message.id.clone()) {
                    touched.push(e.message.id);
                }
            }
        }
        match page.next_page_token {
            Some(t) => page_token = Some(t),
            None => break page.history_id,
        }
    };

    touched.retain(|id| !deleted.contains(id));
    let (rows, gone) = fetch(api, touched).await?;
    let deletes: Vec<String> = deleted.into_iter().chain(gone).collect();
    store.apply(false, &rows, &deletes, Some(&history_id))?;
    let fetched: HashSet<&str> = rows.iter().map(|r| r.id.as_str()).collect();
    added.retain(|id| fetched.contains(id.as_str()));
    Ok(SyncReport {
        kind: SyncKind::Incremental,
        fetched: rows.len(),
        removed: deletes.len(),
        arrived: added,
    })
}

/// Makes sure every message of a thread is cached, with bodies up to
/// `max_body` bytes. The inbox sync only brings messages labeled INBOX, so a
/// thread's replies from you arrive here. History keeps complete threads
/// complete afterwards.
pub async fn ensure_thread(
    api: &impl MailApi,
    store: &mut Store,
    thread_id: &str,
    max_body: i64,
) -> Result<()> {
    if !store.is_complete(thread_id)? {
        match api.thread(thread_id).await {
            Ok(thread) => {
                let rows: Vec<MessageRow> =
                    thread.messages.into_iter().map(MessageRow::from).collect();
                store.apply(false, &rows, &[], None)?;
                store.mark_complete(thread_id)?;
            }
            Err(e) if crate::gmail::is_not_found(&e) => {
                let ids = store.message_ids_of(&[thread_id.to_string()])?;
                store.apply(false, &[], &ids, None)?;
                return Ok(());
            }
            Err(e) => return Err(e),
        }
    }
    let missing = store.missing_bodies(&[thread_id.to_string()], max_body)?;
    let bodies: Vec<(String, Option<Vec<u8>>)> = futures::stream::iter(missing)
        .map(|id| async move {
            match api.message_raw(&id).await {
                Ok(raw) => Ok((id, Some(raw))),
                Err(e) if crate::gmail::is_not_found(&e) => Ok((id, None)),
                Err(e) => Err(e),
            }
        })
        .buffer_unordered(CONCURRENCY)
        .try_collect()
        .await?;
    for (id, raw) in bodies {
        if let Some(raw) = raw {
            store.set_body(&id, &raw)?;
        }
    }
    Ok(())
}

/// Warms the cache for the threads most likely to be opened next, so opening
/// one is instant. Large messages wait until they're actually opened.
pub async fn prefetch(api: &impl MailApi, store: &mut Store, thread_ids: &[String]) -> Result<()> {
    for id in thread_ids {
        ensure_thread(api, store, id, PREFETCH_MAX_BODY).await?;
    }
    Ok(())
}

/// Bodies bigger than this (mostly attachments) aren't prefetched.
pub const PREFETCH_MAX_BODY: i64 = 2 * 1024 * 1024;

/// The name of the label Chord files snoozed threads under, visible in Gmail.
pub const SNOOZE_LABEL: &str = "Chord/Snoozed";

/// Refreshes what changes rarely: the label list (creating Chord's snooze
/// label the first time), and the name and signature mail is sent with.
pub async fn refresh_account(api: &impl MailApi, store: &mut Store) -> Result<()> {
    let mut labels = api.labels().await?;
    if !labels.iter().any(|l| l.name == SNOOZE_LABEL) {
        labels.push(api.create_label(SNOOZE_LABEL).await?);
    }
    if let Some(l) = labels.iter().find(|l| l.name == SNOOZE_LABEL) {
        store.set_meta("snooze_label", &l.id)?;
    }
    let rows: Vec<crate::store::LabelRow> = labels
        .into_iter()
        .map(|l| crate::store::LabelRow {
            system: l.kind == "system",
            id: l.id,
            name: l.name,
        })
        .collect();
    store.set_label_catalog(&rows)?;

    let send_as = api.send_as().await?;
    if let Some(me) = send_as.iter().find(|s| s.is_default).or(send_as.first()) {
        store.set_meta("send_as_email", &me.send_as_email)?;
        store.set_meta("send_as_name", &me.display_name)?;
        store.set_meta("signature", &me.signature)?;
    }
    store.set_meta("account_refreshed", &now_ms().to_string())?;
    Ok(())
}

pub fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// Fetches metadata for `ids`, returning the messages found and the ids that
/// were deleted before Chord got to them.
async fn fetch(api: &impl MailApi, ids: Vec<String>) -> Result<(Vec<MessageRow>, Vec<String>)> {
    let results: Vec<(String, Option<MessageRow>)> = futures::stream::iter(ids)
        .map(|id| async move {
            match api.message_metadata(&id).await {
                Ok(m) => Ok((id, Some(MessageRow::from(m)))),
                Err(e) if matches!(e.downcast_ref(), Some(ApiError::NotFound)) => Ok((id, None)),
                Err(e) => Err(e),
            }
        })
        .buffer_unordered(CONCURRENCY)
        .try_collect()
        .await?;

    let mut rows = Vec::new();
    let mut gone = Vec::new();
    for (id, row) in results {
        match row {
            Some(r) => rows.push(r),
            None => gone.push(id),
        }
    }
    Ok((rows, gone))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gmail::*;
    use std::cell::RefCell;
    use std::collections::HashMap;

    /// An in-memory mailbox with a history log that can be made to expire.
    #[derive(Default)]
    struct Fake {
        messages: RefCell<HashMap<String, Message>>,
        history: RefCell<Vec<History>>,
        history_id: RefCell<u64>,
        expired: RefCell<bool>,
        metadata_calls: RefCell<usize>,
    }

    fn msg(id: &str, date: i64, labels: &[&str]) -> Message {
        Message {
            id: id.into(),
            thread_id: format!("t{id}"),
            label_ids: labels.iter().map(|s| s.to_string()).collect(),
            snippet: "It&#39;s here".into(),
            internal_date: date.to_string(),
            size_estimate: 1000,
            payload: Payload {
                headers: vec![
                    Header {
                        name: "From".into(),
                        value: "Ada <ada@example.com>".into(),
                    },
                    Header {
                        name: "Subject".into(),
                        value: format!("Hello {id}"),
                    },
                ],
            },
        }
    }

    fn event(id: &str) -> HistoryEvent {
        HistoryEvent {
            message: MessageRef { id: id.into() },
        }
    }

    impl Fake {
        fn put(&self, m: Message) {
            self.messages.borrow_mut().insert(m.id.clone(), m);
        }
        fn log(&self, h: History) {
            self.history.borrow_mut().push(h);
            *self.history_id.borrow_mut() += 1;
        }
    }

    impl MailApi for Fake {
        async fn profile(&self) -> Result<Profile> {
            Ok(Profile {
                email_address: "me@example.com".into(),
                history_id: self.history_id.borrow().to_string(),
            })
        }

        async fn list_messages(
            &self,
            label: &str,
            page_token: Option<&str>,
        ) -> Result<MessagePage> {
            let mut all: Vec<_> = self
                .messages
                .borrow()
                .values()
                .filter(|m| m.label_ids.iter().any(|l| l == label))
                .map(|m| (m.internal_date.parse::<i64>().unwrap(), m.id.clone()))
                .collect();
            all.sort_by(|a, b| b.cmp(a));
            // Two per page, to exercise paging.
            let start: usize = page_token.map_or(0, |t| t.parse().unwrap());
            let end = (start + 2).min(all.len());
            Ok(MessagePage {
                messages: all[start..end]
                    .iter()
                    .map(|(_, id)| MessageRef { id: id.clone() })
                    .collect(),
                next_page_token: (end < all.len()).then(|| end.to_string()),
            })
        }

        async fn message_metadata(&self, id: &str) -> Result<Message> {
            *self.metadata_calls.borrow_mut() += 1;
            self.messages
                .borrow()
                .get(id)
                .cloned()
                .ok_or_else(|| ApiError::NotFound.into())
        }

        async fn thread(&self, id: &str) -> Result<Thread> {
            let messages: Vec<Message> = self
                .messages
                .borrow()
                .values()
                .filter(|m| m.thread_id == id)
                .cloned()
                .collect();
            if messages.is_empty() {
                return Err(ApiError::NotFound.into());
            }
            Ok(Thread {
                id: id.into(),
                messages,
            })
        }

        async fn message_raw(&self, id: &str) -> Result<Vec<u8>> {
            let m = self
                .messages
                .borrow()
                .get(id)
                .cloned()
                .ok_or(ApiError::NotFound)?;
            Ok(format!(
                "From: x@y\r\nSubject: {}\r\n\r\nbody of {id}",
                m.header("Subject").unwrap_or("")
            )
            .into_bytes())
        }

        async fn history(&self, start: &str, _page: Option<&str>) -> Result<HistoryPage> {
            if *self.expired.borrow() {
                return Err(ApiError::HistoryExpired.into());
            }
            let start: usize = start.parse().unwrap();
            Ok(HistoryPage {
                history: self.history.borrow()[start..].to_vec(),
                next_page_token: None,
                history_id: self.history_id.borrow().to_string(),
            })
        }
    }

    fn block_on<F: std::future::Future>(f: F) -> F::Output {
        tokio::runtime::Builder::new_current_thread()
            .build()
            .unwrap()
            .block_on(f)
    }

    fn inbox(store: &Store) -> Vec<String> {
        store
            .messages_with_label("INBOX", 100)
            .unwrap()
            .into_iter()
            .map(|m| m.id)
            .collect()
    }

    #[test]
    fn ensure_thread_fetches_replies_and_bodies_once() {
        let api = Fake::default();
        api.put(msg("a", 1, &["INBOX"]));
        api.put(msg("b", 2, &["SENT"]));
        api.put(msg("big", 3, &["SENT"]));
        for id in ["a", "b", "big"] {
            api.messages.borrow_mut().get_mut(id).unwrap().thread_id = "t".into();
        }
        api.messages
            .borrow_mut()
            .get_mut("big")
            .unwrap()
            .size_estimate = 10_000_000;
        let mut store = Store::in_memory().unwrap();
        block_on(sync(&api, &mut store, 100)).unwrap();
        assert_eq!(store.thread_messages("t").unwrap().len(), 1);

        block_on(prefetch(&api, &mut store, &["t".to_string()])).unwrap();
        assert_eq!(store.thread_messages("t").unwrap().len(), 3);
        assert!(store.body("a").unwrap().is_some());
        assert!(
            store.body("big").unwrap().is_none(),
            "large bodies wait for an open"
        );

        block_on(ensure_thread(&api, &mut store, "t", i64::MAX)).unwrap();
        assert!(store.body("big").unwrap().is_some());
    }

    #[test]
    fn first_sync_is_full_and_capped() {
        let api = Fake::default();
        for (i, id) in ["a", "b", "c", "d", "e"].iter().enumerate() {
            api.put(msg(id, i as i64, &["INBOX"]));
        }
        api.put(msg("sent", 99, &["SENT"]));
        let mut store = Store::in_memory().unwrap();

        let report = block_on(sync(&api, &mut store, 3)).unwrap();

        assert_eq!(report.kind, SyncKind::Full);
        assert_eq!(inbox(&store), ["e", "d", "c"]);
        let first = &store.messages_with_label("INBOX", 1).unwrap()[0];
        assert_eq!(first.snippet, "It's here");
        assert_eq!(first.subject, "Hello e");
    }

    #[test]
    fn incremental_applies_adds_label_changes_and_deletes() {
        let api = Fake::default();
        api.put(msg("a", 1, &["INBOX"]));
        api.put(msg("b", 2, &["INBOX", "UNREAD"]));
        let mut store = Store::in_memory().unwrap();
        block_on(sync(&api, &mut store, 100)).unwrap();
        *api.metadata_calls.borrow_mut() = 0;

        // A new message arrives, "b" is archived, and "a" is deleted.
        api.put(msg("c", 3, &["INBOX", "UNREAD"]));
        api.log(History {
            messages_added: vec![event("c")],
            ..Default::default()
        });
        api.put(msg("b", 2, &[]));
        api.log(History {
            labels_removed: vec![event("b")],
            ..Default::default()
        });
        api.messages.borrow_mut().remove("a");
        api.log(History {
            messages_deleted: vec![event("a")],
            ..Default::default()
        });

        let report = block_on(sync(&api, &mut store, 100)).unwrap();

        assert_eq!(report.kind, SyncKind::Incremental);
        assert_eq!(report.arrived, ["c"]);
        assert_eq!(inbox(&store), ["c"]);
        assert_eq!(
            store.count().unwrap(),
            2,
            "archived b stays cached, deleted a is gone"
        );
        assert_eq!(
            *api.metadata_calls.borrow(),
            2,
            "only touched messages are refetched"
        );
        assert_eq!(store.meta("history_id").unwrap().as_deref(), Some("3"));
    }

    #[test]
    fn message_deleted_before_fetch_is_dropped() {
        let api = Fake::default();
        api.put(msg("a", 1, &["INBOX"]));
        let mut store = Store::in_memory().unwrap();
        block_on(sync(&api, &mut store, 100)).unwrap();

        // Labeled, then deleted, but the deletion isn't in the log yet.
        api.log(History {
            labels_added: vec![event("a")],
            ..Default::default()
        });
        api.messages.borrow_mut().remove("a");

        let report = block_on(sync(&api, &mut store, 100)).unwrap();
        assert_eq!(report.removed, 1);
        assert_eq!(store.count().unwrap(), 0);
    }

    #[test]
    fn expired_history_falls_back_to_full_sync() {
        let api = Fake::default();
        api.put(msg("a", 1, &["INBOX"]));
        let mut store = Store::in_memory().unwrap();
        block_on(sync(&api, &mut store, 100)).unwrap();

        api.messages.borrow_mut().clear();
        api.put(msg("z", 9, &["INBOX"]));
        *api.expired.borrow_mut() = true;

        let report = block_on(sync(&api, &mut store, 100)).unwrap();
        assert_eq!(report.kind, SyncKind::Full);
        assert_eq!(inbox(&store), ["z"]);
    }
}
