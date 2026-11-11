//! The background work: push queued actions, sync, bring snoozed mail back,
//! send scheduled mail, keep the newest conversations downloaded, and decide
//! what deserves a notification. The service runs a tick every minute; the
//! window runs the same tick while it's open.

use std::collections::HashMap;
use std::fs::File;
use std::time::Duration;

use anyhow::Result;
use serde::Serialize;

use crate::auth;
use crate::config::Config;
use crate::gmail::{Gmail, MailApi, is_not_found};
use crate::ops::{self, Action};
use crate::paths;
use crate::store::{SnoozeKind, Store, ThreadRow};
use crate::sync::{self, SyncKind, SyncReport, now_ms};
use crate::text::{address_of, sender_name};

/// Signed-in access to Gmail: the OAuth client plus a shared HTTP client.
#[derive(Clone)]
pub struct Session {
    http: reqwest::Client,
    client: auth::Client,
}

impl Session {
    pub fn new() -> Result<Self> {
        Ok(Self {
            http: reqwest::Client::builder()
                .user_agent(concat!("chord-mail/", env!("CARGO_PKG_VERSION")))
                .timeout(Duration::from_secs(60))
                .build()?,
            client: auth::Client::resolve(&paths::client_secret())?,
        })
    }

    pub fn http(&self) -> &reqwest::Client {
        &self.http
    }

    pub fn client(&self) -> &auth::Client {
        &self.client
    }

    /// A Gmail client with a fresh access token.
    pub async fn gmail(&self) -> Result<Gmail> {
        let token = auth::access_token(&self.http, &self.client, &paths::token()).await?;
        Ok(Gmail::new(self.http.clone(), token))
    }
}

/// Whether someone has signed in on this machine, with the OAuth client Chord
/// would use now.
pub fn signed_in() -> bool {
    let Ok(client) = auth::Client::resolve(&paths::client_secret()) else {
        return false;
    };
    auth::Token::load(&paths::token()).is_ok_and(|t| t.issued_to(&client))
}

/// Held while syncing or flushing, so the window and the service never do
/// the same work at once. Released on drop.
pub struct Lock(#[allow(dead_code)] File);

pub async fn lock() -> Result<Lock> {
    let path = paths::data_dir().join("sync.lock");
    paths::private_dir(&paths::data_dir())?;
    let file = File::options()
        .create(true)
        .truncate(false)
        .write(true)
        .open(path)?;
    loop {
        match file.try_lock() {
            Ok(()) => return Ok(Lock(file)),
            Err(std::fs::TryLockError::WouldBlock) => {
                tokio::time::sleep(Duration::from_millis(150)).await
            }
            Err(std::fs::TryLockError::Error(e)) => return Err(e.into()),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct Notice {
    pub thread_id: String,
    pub title: String,
    pub body: String,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct TickReport {
    pub sync: Option<SyncReport>,
    pub flushed: usize,
    pub woke: usize,
    pub sent: usize,
    pub notices: Vec<Notice>,
}

/// Rows for the inbox, each with the index of its split.
pub fn inbox(store: &Store, config: &Config) -> Result<Vec<(usize, ThreadRow)>> {
    let names: HashMap<String, String> = store
        .label_catalog()?
        .into_iter()
        .map(|l| (l.id, l.name))
        .collect();
    let ids = store.thread_ids(Some("INBOX"))?;
    Ok(store
        .thread_rows(&ids)?
        .into_iter()
        .map(|r| (config.split_of(&r, &names), r))
        .collect())
}

/// How long the label list and signature stay fresh.
const ACCOUNT_REFRESH_MS: i64 = 6 * 60 * 60 * 1000;

/// One round of background work. The caller holds the lock.
pub async fn tick(api: &impl MailApi, store: &mut Store, config: &Config) -> Result<TickReport> {
    let mut report = TickReport {
        flushed: ops::flush(api, store).await?,
        ..Default::default()
    };

    let refreshed: i64 = store
        .meta("account_refreshed")?
        .and_then(|v| v.parse().ok())
        .unwrap_or(0);
    if now_ms() - refreshed > ACCOUNT_REFRESH_MS
        && let Err(e) = sync::refresh_account(api, store).await
    {
        eprintln!("chord: couldn't refresh labels and signature: {e:#}");
    }

    let synced = sync::sync(api, store, config.sync.inbox_limit).await?;
    if synced.kind == SyncKind::Full {
        // Everything already there on the first sync is old news.
        let ids: Vec<String> = store
            .messages_with_label("INBOX", usize::MAX >> 1)?
            .into_iter()
            .map(|m| m.id)
            .collect();
        store.take_unnotified(&ids)?;
    }

    report
        .notices
        .extend(notices_for_arrivals(store, config, &synced.arrived)?);
    report.sync = Some(synced);

    let (woke, wake_notices) = wake_due(store)?;
    report.woke = woke;
    report.notices.extend(wake_notices);
    report.flushed += ops::flush(api, store).await?;

    report.sent = send_due(api, store).await?;

    let newest: Vec<String> = inbox(store, config)?
        .into_iter()
        .take(config.sync.prefetch)
        .map(|(_, r)| r.id)
        .collect();
    if let Err(e) = sync::prefetch(api, store, &newest).await {
        eprintln!("chord: couldn't download recent conversations: {e:#}");
    }
    if !config.notify.enabled {
        report.notices.clear();
    }
    Ok(report)
}

fn notices_for_arrivals(store: &Store, config: &Config, arrived: &[String]) -> Result<Vec<Notice>> {
    let fresh = store.take_unnotified(arrived)?;
    if fresh.is_empty() {
        return Ok(vec![]);
    }
    let me = store.account()?;
    let names: HashMap<String, String> = store
        .label_catalog()?
        .into_iter()
        .map(|l| (l.id, l.name))
        .collect();
    let mut by_thread: Vec<Notice> = Vec::new();
    for id in fresh {
        let Some(m) = store.message(&id)? else {
            continue;
        };
        if !m.has("INBOX") || !m.is_unread() || address_of(&m.from).eq_ignore_ascii_case(&me) {
            continue;
        }
        let Some(row) = store
            .thread_rows(std::slice::from_ref(&m.thread_id))?
            .into_iter()
            .next()
        else {
            continue;
        };
        let split = &config.split[config.split_of(&row, &names)].name;
        if !config
            .notify
            .splits
            .iter()
            .any(|s| s.eq_ignore_ascii_case(split))
        {
            continue;
        }
        if by_thread.iter().any(|n| n.thread_id == m.thread_id) {
            continue;
        }
        by_thread.push(Notice {
            thread_id: m.thread_id.clone(),
            title: sender_name(&m.from),
            body: if m.subject.is_empty() {
                m.snippet.clone()
            } else {
                m.subject.clone()
            },
        });
    }
    Ok(by_thread)
}

/// Brings back snoozes that are due. A reminder only comes back when nobody
/// else wrote in the thread since it was set.
pub fn wake_due(store: &mut Store) -> Result<(usize, Vec<Notice>)> {
    let now = now_ms();
    let me = store.account()?;
    let mut woke = 0;
    let mut notices = Vec::new();
    for s in store.snoozes()?.into_iter().filter(|s| s.until <= now) {
        let messages = store.thread_messages(&s.thread_id)?;
        if messages.is_empty() {
            store.clear_snooze(&s.thread_id)?;
            continue;
        }
        let subject = messages
            .iter()
            .find(|m| !m.subject.is_empty())
            .map(|m| m.subject.clone())
            .unwrap_or_default();
        if s.kind == SnoozeKind::Remind {
            let answered = messages
                .iter()
                .any(|m| m.date > s.since && !address_of(&m.from).eq_ignore_ascii_case(&me));
            if answered {
                store.clear_snooze(&s.thread_id)?;
                continue;
            }
        }
        ops::apply(store, std::slice::from_ref(&s.thread_id), &Action::Wake)?;
        woke += 1;
        notices.push(Notice {
            thread_id: s.thread_id.clone(),
            title: match s.kind {
                SnoozeKind::Snooze => "Back from snooze".into(),
                SnoozeKind::Remind => "No reply yet".into(),
            },
            body: subject,
        });
    }
    Ok((woke, notices))
}

/// Sends scheduled drafts whose time has come.
pub async fn send_due(api: &impl MailApi, store: &mut Store) -> Result<usize> {
    let now = now_ms();
    let mut sent = 0;
    for s in store.scheduled()?.into_iter().filter(|s| s.send_at <= now) {
        match api.send_draft(&s.draft_id).await {
            Ok(()) => {
                store.remove_scheduled(&s.draft_id)?;
                sent += 1;
            }
            // Deleted in Gmail meanwhile: nothing left to send.
            Err(e) if is_not_found(&e) => store.remove_scheduled(&s.draft_id)?,
            Err(e) => return Err(e),
        }
    }
    Ok(sent)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ops::apply;
    use crate::store::tests::row;

    #[test]
    fn due_snooze_wakes_with_a_notice() {
        let mut store = Store::in_memory().unwrap();
        store
            .apply(false, &[row("a", "t", 1, &["INBOX"])], &[], None)
            .unwrap();
        apply(&mut store, &["t".into()], &Action::Snooze { until: 1 }).unwrap();
        let (woke, notices) = wake_due(&mut store).unwrap();
        assert_eq!(woke, 1);
        assert_eq!(notices[0].title, "Back from snooze");
        assert_eq!(store.thread_ids(Some("INBOX")).unwrap(), ["t"]);
    }

    #[test]
    fn answered_reminder_is_dropped_quietly() {
        let mut store = Store::in_memory().unwrap();
        store.set_meta("email", "me@example.com").unwrap();
        let mut mine = row("a", "t", 1, &["SENT"]);
        mine.from = "me@example.com".into();
        store.apply(false, &[mine], &[], None).unwrap();
        apply(&mut store, &["t".into()], &Action::Remind { until: 1 }).unwrap();
        // Ada replied after the reminder was set.
        store
            .apply(
                false,
                &[row("b", "t", now_ms() + 10, &["INBOX"])],
                &[],
                None,
            )
            .unwrap();
        let (woke, notices) = wake_due(&mut store).unwrap();
        assert_eq!((woke, notices.len()), (0, 0));
        assert!(store.snoozes().unwrap().is_empty());
    }

    #[test]
    fn unanswered_reminder_comes_back() {
        let mut store = Store::in_memory().unwrap();
        store.set_meta("email", "me@example.com").unwrap();
        let mut mine = row("a", "t", 1, &["SENT"]);
        mine.from = "me@example.com".into();
        store.apply(false, &[mine], &[], None).unwrap();
        apply(&mut store, &["t".into()], &Action::Remind { until: 1 }).unwrap();
        let (woke, notices) = wake_due(&mut store).unwrap();
        assert_eq!(woke, 1);
        assert_eq!(notices[0].title, "No reply yet");
        assert_eq!(store.thread_ids(Some("INBOX")).unwrap(), ["t"]);
    }

    #[test]
    fn notices_only_for_unread_primary_mail_from_others() {
        let mut store = Store::in_memory().unwrap();
        store.set_meta("email", "me@example.com").unwrap();
        let mut mine = row("m", "t3", 3, &["INBOX", "UNREAD"]);
        mine.from = "Me <me@example.com>".into();
        store
            .apply(
                false,
                &[
                    row("a", "t1", 1, &["INBOX", "UNREAD", "CATEGORY_PERSONAL"]),
                    row("b", "t2", 2, &["INBOX", "UNREAD", "CATEGORY_PROMOTIONS"]),
                    mine,
                ],
                &[],
                None,
            )
            .unwrap();
        let ids = vec!["a".into(), "b".into(), "m".into()];
        let n = notices_for_arrivals(&store, &Config::default(), &ids).unwrap();
        assert_eq!(n.len(), 1);
        assert_eq!(n[0].title, "Ada Lovelace");
        assert!(
            notices_for_arrivals(&store, &Config::default(), &ids)
                .unwrap()
                .is_empty(),
            "once only"
        );
    }
}
