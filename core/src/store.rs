//! The local cache: every message Chord has seen, its labels and body, the
//! sync position, and Chord's own state (queued actions, snoozes, scheduled
//! mail). The window reads only from here, which is what makes it instant.
//!
//! The message tables are a cache and can be rebuilt from Gmail at any time.
//! The queue, snoozes, and schedule are not, so schema changes must migrate
//! them rather than drop them.

use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::time::Duration;

use anyhow::Result;
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};

use crate::text::{address_of, sender_name, split_addresses};

const SCHEMA_VERSION: i32 = 2;

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct MessageRow {
    pub id: String,
    pub thread_id: String,
    /// Milliseconds since the epoch.
    pub date: i64,
    pub from: String,
    pub to: String,
    pub cc: String,
    pub subject: String,
    pub snippet: String,
    pub labels: Vec<String>,
    pub size: i64,
}

impl MessageRow {
    pub fn has(&self, label: &str) -> bool {
        self.labels.iter().any(|l| l == label)
    }

    pub fn is_unread(&self) -> bool {
        self.has("UNREAD")
    }
}

impl From<crate::gmail::Message> for MessageRow {
    fn from(m: crate::gmail::Message) -> Self {
        Self {
            from: m.header("From").unwrap_or_default().to_string(),
            to: m.header("To").unwrap_or_default().to_string(),
            cc: m.header("Cc").unwrap_or_default().to_string(),
            subject: m.header("Subject").unwrap_or_default().to_string(),
            date: m.internal_date.parse().unwrap_or(0),
            snippet: crate::text::decode_entities(&m.snippet),
            size: m.size_estimate,
            id: m.id,
            thread_id: m.thread_id,
            labels: m.label_ids,
        }
    }
}

/// One conversation as the list shows it.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ThreadRow {
    pub id: String,
    pub subject: String,
    /// Display names in order of first appearance; the account owner is "me".
    pub senders: Vec<String>,
    /// Sender addresses, for split rules.
    #[serde(skip)]
    pub sender_addresses: Vec<String>,
    pub snippet: String,
    /// The sort key in milliseconds: the newest message, or when a snooze
    /// brought the thread back, whichever is later.
    pub date: i64,
    pub count: usize,
    pub unread: bool,
    pub starred: bool,
    pub draft: bool,
    /// Every label on any message, including system ones.
    pub labels: Vec<String>,
    /// Gmail's category for the conversation: the newest message's
    /// CATEGORY_* label, which is the tab Gmail files it under.
    #[serde(skip)]
    pub category: Option<String>,
    pub snoozed_until: Option<i64>,
}

/// A change waiting to be pushed to Gmail. These are recorded after the cache
/// already reflects them, so the window never waits on the network.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum Op {
    Modify {
        ids: Vec<String>,
        add: Vec<String>,
        remove: Vec<String>,
    },
    Trash {
        thread: String,
    },
    Untrash {
        thread: String,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SnoozeKind {
    /// Hidden from the inbox until the time comes.
    Snooze,
    /// Already out of the inbox (a sent message); comes back only if nobody
    /// has replied by then.
    Remind,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Snooze {
    pub thread_id: String,
    pub until: i64,
    pub kind: SnoozeKind,
    /// For reminders: when the message was sent, so a later reply cancels it.
    pub since: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Scheduled {
    pub draft_id: String,
    pub send_at: i64,
    pub thread_id: Option<String>,
    pub subject: String,
    pub recipients: String,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct LabelRow {
    pub id: String,
    pub name: String,
    pub system: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Contact {
    pub name: String,
    pub address: String,
}

pub struct Store {
    db: Connection,
}

impl Store {
    pub fn open(path: &Path) -> Result<Self> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        Self::init(Connection::open(path)?)
    }

    pub fn in_memory() -> Result<Self> {
        Self::init(Connection::open_in_memory()?)
    }

    fn init(db: Connection) -> Result<Self> {
        // The window and the service each hold connections; a writer waits
        // for the other rather than failing.
        db.busy_timeout(Duration::from_secs(10))?;
        db.pragma_update(None, "journal_mode", "WAL")?;
        db.pragma_update(None, "foreign_keys", "ON")?;
        let version: i32 = db.pragma_query_value(None, "user_version", |r| r.get(0))?;
        if version == 1 {
            // Version 1 held only a message cache, which Gmail can rebuild.
            db.execute_batch(
                "DROP TABLE IF EXISTS message_labels;
                 DROP TABLE IF EXISTS messages;
                 DELETE FROM meta WHERE key = 'history_id';",
            )?;
        }
        db.execute_batch(
            "CREATE TABLE IF NOT EXISTS meta (
                 key   TEXT PRIMARY KEY,
                 value TEXT NOT NULL
             );
             CREATE TABLE IF NOT EXISTS messages (
                 id        TEXT PRIMARY KEY,
                 thread_id TEXT NOT NULL,
                 date      INTEGER NOT NULL,
                 sender    TEXT NOT NULL,
                 to_addrs  TEXT NOT NULL,
                 cc_addrs  TEXT NOT NULL,
                 subject   TEXT NOT NULL,
                 snippet   TEXT NOT NULL,
                 size      INTEGER NOT NULL DEFAULT 0
             );
             CREATE INDEX IF NOT EXISTS messages_by_thread ON messages(thread_id, date);
             CREATE INDEX IF NOT EXISTS messages_by_date ON messages(date DESC);
             CREATE TABLE IF NOT EXISTS message_labels (
                 message_id TEXT NOT NULL REFERENCES messages(id) ON DELETE CASCADE,
                 label      TEXT NOT NULL,
                 PRIMARY KEY (message_id, label)
             );
             CREATE INDEX IF NOT EXISTS labels_by_label ON message_labels(label);
             CREATE TABLE IF NOT EXISTS bodies (
                 message_id TEXT PRIMARY KEY REFERENCES messages(id) ON DELETE CASCADE,
                 raw        BLOB NOT NULL
             );
             CREATE TABLE IF NOT EXISTS complete_threads (
                 thread_id TEXT PRIMARY KEY
             );
             CREATE TABLE IF NOT EXISTS labels (
                 id     TEXT PRIMARY KEY,
                 name   TEXT NOT NULL,
                 system INTEGER NOT NULL
             );
             CREATE TABLE IF NOT EXISTS ops (
                 id         INTEGER PRIMARY KEY AUTOINCREMENT,
                 op         TEXT NOT NULL,
                 attempts   INTEGER NOT NULL DEFAULT 0,
                 last_error TEXT
             );
             CREATE TABLE IF NOT EXISTS snoozes (
                 thread_id TEXT PRIMARY KEY,
                 until     INTEGER NOT NULL,
                 kind      TEXT NOT NULL,
                 since     INTEGER NOT NULL
             );
             CREATE TABLE IF NOT EXISTS surfaced (
                 thread_id TEXT PRIMARY KEY,
                 at        INTEGER NOT NULL
             );
             CREATE TABLE IF NOT EXISTS scheduled (
                 draft_id   TEXT PRIMARY KEY,
                 send_at    INTEGER NOT NULL,
                 thread_id  TEXT,
                 subject    TEXT NOT NULL,
                 recipients TEXT NOT NULL
             );
             CREATE TABLE IF NOT EXISTS image_senders (
                 address TEXT PRIMARY KEY
             );
             CREATE TABLE IF NOT EXISTS contacts (
                 address TEXT PRIMARY KEY,
                 name    TEXT NOT NULL,
                 count   INTEGER NOT NULL,
                 last    INTEGER NOT NULL
             );
             CREATE TABLE IF NOT EXISTS notified (
                 message_id TEXT PRIMARY KEY
             );",
        )?;
        db.pragma_update(None, "user_version", SCHEMA_VERSION)?;
        Ok(Self { db })
    }

    // ----- meta -------------------------------------------------------------

    pub fn meta(&self, key: &str) -> Result<Option<String>> {
        Ok(self
            .db
            .query_row("SELECT value FROM meta WHERE key = ?1", [key], |r| r.get(0))
            .optional()?)
    }

    pub fn set_meta(&self, key: &str, value: &str) -> Result<()> {
        self.db.execute(
            "INSERT INTO meta (key, value) VALUES (?1, ?2)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            [key, value],
        )?;
        Ok(())
    }

    /// The signed-in address, lowercased, once a sync has learned it.
    pub fn account(&self) -> Result<String> {
        Ok(self.meta("email")?.unwrap_or_default().to_lowercase())
    }

    // ----- messages ---------------------------------------------------------

    /// Applies one sync's results in a single transaction, so the cache and
    /// its history id never disagree, even if Chord is killed mid-write.
    ///
    /// Actions still waiting in the queue are replayed over what Gmail sent,
    /// so a sync that lands before the queue flushes can't undo them on screen.
    pub fn apply(
        &mut self,
        reset: bool,
        upserts: &[MessageRow],
        deletes: &[String],
        history_id: Option<&str>,
    ) -> Result<()> {
        let pending = self.pending_ops()?;
        let tx = self.db.transaction()?;
        if reset {
            tx.execute("DELETE FROM messages", [])?;
            tx.execute("DELETE FROM complete_threads", [])?;
        }
        for m in upserts {
            let labels = replay(&pending, m);
            tx.execute(
                "INSERT INTO messages (id, thread_id, date, sender, to_addrs, cc_addrs, subject, snippet, size)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
                 ON CONFLICT(id) DO UPDATE SET
                     thread_id = excluded.thread_id, date = excluded.date,
                     sender = excluded.sender, to_addrs = excluded.to_addrs,
                     cc_addrs = excluded.cc_addrs, subject = excluded.subject,
                     snippet = excluded.snippet, size = excluded.size",
                params![m.id, m.thread_id, m.date, m.from, m.to, m.cc, m.subject, m.snippet, m.size],
            )?;
            tx.execute("DELETE FROM message_labels WHERE message_id = ?1", [&m.id])?;
            for label in &labels {
                tx.execute(
                    "INSERT OR IGNORE INTO message_labels (message_id, label) VALUES (?1, ?2)",
                    [&m.id, label],
                )?;
            }
            record_contacts(&tx, m)?;
        }
        for id in deletes {
            tx.execute("DELETE FROM messages WHERE id = ?1", [id])?;
        }
        if let Some(h) = history_id {
            tx.execute(
                "INSERT INTO meta (key, value) VALUES ('history_id', ?1)
                 ON CONFLICT(key) DO UPDATE SET value = excluded.value",
                [h],
            )?;
        }
        tx.commit()?;
        Ok(())
    }

    fn row_from(r: &rusqlite::Row) -> rusqlite::Result<MessageRow> {
        let labels: Option<String> = r.get(9)?;
        Ok(MessageRow {
            id: r.get(0)?,
            thread_id: r.get(1)?,
            date: r.get(2)?,
            from: r.get(3)?,
            to: r.get(4)?,
            cc: r.get(5)?,
            subject: r.get(6)?,
            snippet: r.get(7)?,
            size: r.get(8)?,
            labels: labels
                .unwrap_or_default()
                .split(' ')
                .filter(|s| !s.is_empty())
                .map(String::from)
                .collect(),
        })
    }

    const MESSAGE_COLUMNS: &str = "m.id, m.thread_id, m.date, m.sender, m.to_addrs, m.cc_addrs,
         m.subject, m.snippet, m.size,
         (SELECT group_concat(label, ' ') FROM message_labels WHERE message_id = m.id)";

    pub fn message(&self, id: &str) -> Result<Option<MessageRow>> {
        Ok(self
            .db
            .query_row(
                &format!(
                    "SELECT {} FROM messages m WHERE m.id = ?1",
                    Self::MESSAGE_COLUMNS
                ),
                [id],
                Self::row_from,
            )
            .optional()?)
    }

    /// A thread's messages, oldest first.
    pub fn thread_messages(&self, thread_id: &str) -> Result<Vec<MessageRow>> {
        let mut stmt = self.db.prepare(&format!(
            "SELECT {} FROM messages m WHERE m.thread_id = ?1 ORDER BY m.date",
            Self::MESSAGE_COLUMNS
        ))?;
        let rows = stmt.query_map([thread_id], Self::row_from)?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    /// The newest messages carrying `label`, newest first.
    pub fn messages_with_label(&self, label: &str, limit: usize) -> Result<Vec<MessageRow>> {
        let mut stmt = self.db.prepare(&format!(
            "SELECT {} FROM messages m
             JOIN message_labels l ON l.message_id = m.id AND l.label = ?1
             ORDER BY m.date DESC LIMIT ?2",
            Self::MESSAGE_COLUMNS
        ))?;
        let rows = stmt.query_map(params![label, limit as i64], Self::row_from)?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    pub fn count(&self) -> Result<usize> {
        Ok(self
            .db
            .query_row("SELECT count(*) FROM messages", [], |r| r.get::<_, i64>(0))?
            as usize)
    }

    /// Ids of threads with a live (not trashed or spam) message carrying
    /// `label`. With no label, every live thread.
    pub fn thread_ids(&self, label: Option<&str>) -> Result<Vec<String>> {
        let sql = match label {
            Some(_) => {
                "SELECT DISTINCT m.thread_id FROM messages m
                 JOIN message_labels l ON l.message_id = m.id AND l.label = ?1
                 WHERE NOT EXISTS (SELECT 1 FROM message_labels x WHERE x.message_id = m.id
                                   AND x.label IN ('TRASH', 'SPAM'))"
            }
            None => {
                "SELECT DISTINCT m.thread_id FROM messages m
                 WHERE ?1 IS NULL AND NOT EXISTS (SELECT 1 FROM message_labels x
                   WHERE x.message_id = m.id AND x.label IN ('TRASH', 'SPAM'))"
            }
        };
        let mut stmt = self.db.prepare(sql)?;
        let rows = stmt.query_map([label], |r| r.get(0))?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    /// Threads whose subject, sender, or snippet contains `query`, newest
    /// first: instant results while Gmail's search runs.
    pub fn search_local(&self, query: &str, limit: usize) -> Result<Vec<String>> {
        let pattern = format!("%{}%", query.trim());
        let mut stmt = self.db.prepare(
            "SELECT thread_id FROM messages
             WHERE subject LIKE ?1 OR sender LIKE ?1 OR snippet LIKE ?1 OR to_addrs LIKE ?1
             GROUP BY thread_id ORDER BY max(date) DESC LIMIT ?2",
        )?;
        let rows = stmt.query_map(params![pattern, limit as i64], |r| r.get(0))?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    /// Ids of threads in the trash.
    pub fn trashed_thread_ids(&self) -> Result<Vec<String>> {
        let mut stmt = self.db.prepare(
            "SELECT DISTINCT m.thread_id FROM messages m
             JOIN message_labels l ON l.message_id = m.id AND l.label = 'TRASH'",
        )?;
        let rows = stmt.query_map([], |r| r.get(0))?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    /// Builds list rows for `ids`, newest first. Ids with no cached messages
    /// are skipped.
    pub fn thread_rows(&self, ids: &[String]) -> Result<Vec<ThreadRow>> {
        let me = self.account()?;
        let snoozes: HashMap<String, i64> = self
            .snoozes()?
            .into_iter()
            .map(|s| (s.thread_id, s.until))
            .collect();
        let mut surfaced_stmt = self
            .db
            .prepare("SELECT at FROM surfaced WHERE thread_id = ?1")?;
        let mut rows = Vec::with_capacity(ids.len());
        for id in ids {
            let messages = self.thread_messages(id)?;
            let live: Vec<&MessageRow> = messages
                .iter()
                .filter(|m| !m.has("TRASH") && !m.has("SPAM"))
                .collect();
            let shown: Vec<&MessageRow> = if live.is_empty() {
                messages.iter().collect()
            } else {
                live
            };
            let Some(latest) = shown
                .iter()
                .rev()
                .find(|m| !m.has("DRAFT"))
                .or(shown.last())
            else {
                continue;
            };
            let mut senders = Vec::new();
            let mut sender_addresses = Vec::new();
            for m in &shown {
                if m.has("DRAFT") {
                    continue;
                }
                let address = address_of(&m.from).to_lowercase();
                let name = if !me.is_empty() && address == me {
                    "me".to_string()
                } else {
                    sender_name(&m.from)
                };
                if !senders.contains(&name) {
                    senders.push(name);
                }
                if !sender_addresses.contains(&address) {
                    sender_addresses.push(address);
                }
            }
            let mut labels: Vec<String> = Vec::new();
            for m in &shown {
                for l in &m.labels {
                    if !labels.contains(l) {
                        labels.push(l.clone());
                    }
                }
            }
            let surfaced: Option<i64> = surfaced_stmt.query_row([id], |r| r.get(0)).optional()?;
            let newest = shown.iter().map(|m| m.date).max().unwrap_or(0);
            let subject = shown
                .iter()
                .find(|m| !m.subject.is_empty())
                .map(|m| m.subject.clone())
                .unwrap_or_default();
            rows.push(ThreadRow {
                id: id.clone(),
                subject,
                senders,
                sender_addresses,
                snippet: latest.snippet.clone(),
                date: newest.max(surfaced.unwrap_or(0)),
                count: shown.iter().filter(|m| !m.has("DRAFT")).count(),
                unread: shown.iter().any(|m| m.is_unread()),
                starred: shown.iter().any(|m| m.has("STARRED")),
                draft: shown.iter().any(|m| m.has("DRAFT")),
                labels,
                category: latest
                    .labels
                    .iter()
                    .find(|l| l.starts_with("CATEGORY_"))
                    .cloned(),
                snoozed_until: snoozes.get(id).copied(),
            });
        }
        rows.sort_by_key(|r| std::cmp::Reverse(r.date));
        Ok(rows)
    }

    /// Message ids of the given threads, for actions that touch every message.
    pub fn message_ids_of(&self, thread_ids: &[String]) -> Result<Vec<String>> {
        let mut out = Vec::new();
        let mut stmt = self
            .db
            .prepare("SELECT id FROM messages WHERE thread_id = ?1")?;
        for t in thread_ids {
            for id in stmt.query_map([t], |r| r.get::<_, String>(0))? {
                out.push(id?);
            }
        }
        Ok(out)
    }

    /// Every message's labels for the given threads, for exact undo.
    pub fn label_snapshot(&self, thread_ids: &[String]) -> Result<Vec<(String, Vec<String>)>> {
        let mut out = Vec::new();
        for t in thread_ids {
            for m in self.thread_messages(t)? {
                out.push((m.id, m.labels));
            }
        }
        Ok(out)
    }

    /// Sets one message's labels locally, leaving Gmail to the queue.
    pub fn set_message_labels(&mut self, id: &str, labels: &[String]) -> Result<()> {
        let tx = self.db.transaction()?;
        tx.execute("DELETE FROM message_labels WHERE message_id = ?1", [id])?;
        for l in labels {
            tx.execute(
                "INSERT OR IGNORE INTO message_labels (message_id, label) VALUES (?1, ?2)",
                [id, l],
            )?;
        }
        tx.commit()?;
        Ok(())
    }

    // ----- bodies and thread completeness -----------------------------------

    pub fn body(&self, message_id: &str) -> Result<Option<Vec<u8>>> {
        Ok(self
            .db
            .query_row(
                "SELECT raw FROM bodies WHERE message_id = ?1",
                [message_id],
                |r| r.get(0),
            )
            .optional()?)
    }

    pub fn set_body(&self, message_id: &str, raw: &[u8]) -> Result<()> {
        self.db.execute(
            "INSERT OR REPLACE INTO bodies (message_id, raw) VALUES (?1, ?2)",
            params![message_id, raw],
        )?;
        Ok(())
    }

    pub fn is_complete(&self, thread_id: &str) -> Result<bool> {
        Ok(self
            .db
            .query_row(
                "SELECT 1 FROM complete_threads WHERE thread_id = ?1",
                [thread_id],
                |_| Ok(()),
            )
            .optional()?
            .is_some())
    }

    pub fn mark_complete(&self, thread_id: &str) -> Result<()> {
        self.db.execute(
            "INSERT OR IGNORE INTO complete_threads (thread_id) VALUES (?1)",
            [thread_id],
        )?;
        Ok(())
    }

    /// Messages in `thread_ids` without a cached body, smallest first, up to
    /// `max_size` bytes each.
    pub fn missing_bodies(&self, thread_ids: &[String], max_size: i64) -> Result<Vec<String>> {
        let mut out = Vec::new();
        let mut stmt = self.db.prepare(
            "SELECT m.id FROM messages m
             WHERE m.thread_id = ?1 AND m.size <= ?2
               AND NOT EXISTS (SELECT 1 FROM bodies b WHERE b.message_id = m.id)",
        )?;
        for t in thread_ids {
            for id in stmt.query_map(params![t, max_size], |r| r.get::<_, String>(0))? {
                out.push(id?);
            }
        }
        Ok(out)
    }

    // ----- labels catalog ----------------------------------------------------

    pub fn set_label_catalog(&mut self, labels: &[LabelRow]) -> Result<()> {
        let tx = self.db.transaction()?;
        tx.execute("DELETE FROM labels", [])?;
        for l in labels {
            tx.execute(
                "INSERT INTO labels (id, name, system) VALUES (?1, ?2, ?3)",
                params![l.id, l.name, l.system],
            )?;
        }
        tx.commit()?;
        Ok(())
    }

    pub fn label_catalog(&self) -> Result<Vec<LabelRow>> {
        let mut stmt = self.db.prepare(
            "SELECT id, name, system FROM labels ORDER BY system DESC, name COLLATE NOCASE",
        )?;
        let rows = stmt.query_map([], |r| {
            Ok(LabelRow {
                id: r.get(0)?,
                name: r.get(1)?,
                system: r.get(2)?,
            })
        })?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    // ----- action queue -----------------------------------------------------

    pub fn enqueue(&self, op: &Op) -> Result<()> {
        self.db.execute(
            "INSERT INTO ops (op) VALUES (?1)",
            [serde_json::to_string(op)?],
        )?;
        Ok(())
    }

    pub fn pending_ops(&self) -> Result<Vec<(i64, Op)>> {
        let mut stmt = self.db.prepare("SELECT id, op FROM ops ORDER BY id")?;
        let rows = stmt.query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?)))?;
        let mut out = Vec::new();
        for row in rows {
            let (id, json) = row?;
            if let Ok(op) = serde_json::from_str(&json) {
                out.push((id, op));
            }
        }
        Ok(out)
    }

    pub fn op_done(&self, id: i64) -> Result<()> {
        self.db.execute("DELETE FROM ops WHERE id = ?1", [id])?;
        Ok(())
    }

    pub fn op_failed(&self, id: i64, error: &str) -> Result<()> {
        self.db.execute(
            "UPDATE ops SET attempts = attempts + 1, last_error = ?2 WHERE id = ?1",
            params![id, error],
        )?;
        Ok(())
    }

    /// The most recent error from a queued action that hasn't gone through.
    pub fn queue_error(&self) -> Result<Option<String>> {
        Ok(self
            .db
            .query_row(
                "SELECT last_error FROM ops WHERE last_error IS NOT NULL ORDER BY id LIMIT 1",
                [],
                |r| r.get(0),
            )
            .optional()?)
    }

    // ----- snoozes ----------------------------------------------------------

    pub fn set_snooze(&self, s: &Snooze) -> Result<()> {
        let kind = match s.kind {
            SnoozeKind::Snooze => "snooze",
            SnoozeKind::Remind => "remind",
        };
        self.db.execute(
            "INSERT OR REPLACE INTO snoozes (thread_id, until, kind, since) VALUES (?1, ?2, ?3, ?4)",
            params![s.thread_id, s.until, kind, s.since],
        )?;
        Ok(())
    }

    pub fn clear_snooze(&self, thread_id: &str) -> Result<()> {
        self.db
            .execute("DELETE FROM snoozes WHERE thread_id = ?1", [thread_id])?;
        Ok(())
    }

    pub fn snooze_of(&self, thread_id: &str) -> Result<Option<Snooze>> {
        Ok(self
            .snoozes()?
            .into_iter()
            .find(|s| s.thread_id == thread_id))
    }

    pub fn snoozes(&self) -> Result<Vec<Snooze>> {
        let mut stmt = self
            .db
            .prepare("SELECT thread_id, until, kind, since FROM snoozes ORDER BY until")?;
        let rows = stmt.query_map([], |r| {
            let kind: String = r.get(2)?;
            Ok(Snooze {
                thread_id: r.get(0)?,
                until: r.get(1)?,
                kind: if kind == "remind" {
                    SnoozeKind::Remind
                } else {
                    SnoozeKind::Snooze
                },
                since: r.get(3)?,
            })
        })?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    pub fn set_surfaced(&self, thread_id: &str, at: i64) -> Result<()> {
        self.db.execute(
            "INSERT OR REPLACE INTO surfaced (thread_id, at) VALUES (?1, ?2)",
            params![thread_id, at],
        )?;
        Ok(())
    }

    // ----- scheduled sends --------------------------------------------------

    pub fn add_scheduled(&self, s: &Scheduled) -> Result<()> {
        self.db.execute(
            "INSERT OR REPLACE INTO scheduled (draft_id, send_at, thread_id, subject, recipients)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![s.draft_id, s.send_at, s.thread_id, s.subject, s.recipients],
        )?;
        Ok(())
    }

    pub fn remove_scheduled(&self, draft_id: &str) -> Result<()> {
        self.db
            .execute("DELETE FROM scheduled WHERE draft_id = ?1", [draft_id])?;
        Ok(())
    }

    pub fn scheduled(&self) -> Result<Vec<Scheduled>> {
        let mut stmt = self.db.prepare(
            "SELECT draft_id, send_at, thread_id, subject, recipients FROM scheduled ORDER BY send_at",
        )?;
        let rows = stmt.query_map([], |r| {
            Ok(Scheduled {
                draft_id: r.get(0)?,
                send_at: r.get(1)?,
                thread_id: r.get(2)?,
                subject: r.get(3)?,
                recipients: r.get(4)?,
            })
        })?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    // ----- images, contacts, notifications -----------------------------------

    pub fn allow_images(&self, address: &str) -> Result<()> {
        self.db.execute(
            "INSERT OR IGNORE INTO image_senders (address) VALUES (?1)",
            [address.to_lowercase()],
        )?;
        Ok(())
    }

    pub fn images_allowed(&self, address: &str) -> Result<bool> {
        Ok(self
            .db
            .query_row(
                "SELECT 1 FROM image_senders WHERE address = ?1",
                [address.to_lowercase()],
                |_| Ok(()),
            )
            .optional()?
            .is_some())
    }

    /// People matching `query` by name or address, most frequent first.
    pub fn contacts(&self, query: &str, limit: usize) -> Result<Vec<Contact>> {
        let pattern = format!("%{}%", query.to_lowercase());
        let mut stmt = self.db.prepare(
            "SELECT name, address FROM contacts
             WHERE lower(name) LIKE ?1 OR address LIKE ?1
             ORDER BY count DESC, last DESC LIMIT ?2",
        )?;
        let rows = stmt.query_map(params![pattern, limit as i64], |r| {
            Ok(Contact {
                name: r.get(0)?,
                address: r.get(1)?,
            })
        })?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    /// Records message ids as notified and returns the ones that weren't.
    pub fn take_unnotified(&self, ids: &[String]) -> Result<Vec<String>> {
        let mut fresh = Vec::new();
        for id in ids {
            let n = self.db.execute(
                "INSERT OR IGNORE INTO notified (message_id) VALUES (?1)",
                [id],
            )?;
            if n > 0 {
                fresh.push(id.clone());
            }
        }
        Ok(fresh)
    }
}

/// What a message's labels will be once every queued action reaches Gmail.
fn replay(pending: &[(i64, Op)], m: &MessageRow) -> Vec<String> {
    let mut labels: Vec<String> = m.labels.clone();
    for (_, op) in pending {
        match op {
            Op::Modify { ids, add, remove } if ids.contains(&m.id) => {
                labels.retain(|l| !remove.contains(l));
                for a in add {
                    if !labels.contains(a) {
                        labels.push(a.clone());
                    }
                }
            }
            Op::Trash { thread } if *thread == m.thread_id => {
                if !labels.iter().any(|l| l == "TRASH") {
                    labels.push("TRASH".into());
                }
            }
            Op::Untrash { thread } if *thread == m.thread_id => labels.retain(|l| l != "TRASH"),
            _ => {}
        }
    }
    labels
}

fn record_contacts(tx: &rusqlite::Transaction, m: &MessageRow) -> Result<()> {
    let mut seen = HashSet::new();
    for field in [&m.from, &m.to, &m.cc] {
        for entry in split_addresses(field) {
            let address = address_of(&entry).to_lowercase();
            if !address.contains('@') || !seen.insert(address.clone()) {
                continue;
            }
            let name = sender_name(&entry);
            let name = if name.eq_ignore_ascii_case(&address) {
                String::new()
            } else {
                name
            };
            tx.execute(
                "INSERT INTO contacts (address, name, count, last) VALUES (?1, ?2, 1, ?3)
                 ON CONFLICT(address) DO UPDATE SET
                     count = count + 1,
                     last = max(last, excluded.last),
                     name = CASE WHEN excluded.name <> '' THEN excluded.name ELSE name END",
                params![address, name, m.date],
            )?;
        }
    }
    Ok(())
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    pub fn row(id: &str, thread: &str, date: i64, labels: &[&str]) -> MessageRow {
        MessageRow {
            id: id.into(),
            thread_id: thread.into(),
            date,
            from: "Ada Lovelace <ada@example.com>".into(),
            to: "Me <me@example.com>".into(),
            cc: String::new(),
            subject: format!("Subject {thread}"),
            snippet: format!("snippet {id}"),
            labels: labels.iter().map(|s| s.to_string()).collect(),
            size: 100,
        }
    }

    #[test]
    fn thread_rows_aggregate_and_sort() {
        let mut store = Store::in_memory().unwrap();
        store.set_meta("email", "me@example.com").unwrap();
        let mut reply = row("b", "t1", 5, &["SENT"]);
        reply.from = "Me <me@example.com>".into();
        store
            .apply(
                false,
                &[
                    row("a", "t1", 1, &["INBOX", "UNREAD"]),
                    reply,
                    row("c", "t2", 3, &["INBOX", "STARRED"]),
                ],
                &[],
                Some("1"),
            )
            .unwrap();
        let ids = store.thread_ids(Some("INBOX")).unwrap();
        let rows = store.thread_rows(&ids).unwrap();
        assert_eq!(
            rows.iter().map(|r| r.id.as_str()).collect::<Vec<_>>(),
            ["t1", "t2"]
        );
        assert_eq!(rows[0].senders, ["Ada Lovelace", "me"]);
        assert_eq!(rows[0].count, 2);
        assert!(rows[0].unread);
        assert_eq!(rows[0].snippet, "snippet b");
        assert!(rows[1].starred);
    }

    #[test]
    fn trashed_threads_leave_label_views() {
        let mut store = Store::in_memory().unwrap();
        store
            .apply(false, &[row("a", "t1", 1, &["INBOX", "TRASH"])], &[], None)
            .unwrap();
        assert!(store.thread_ids(Some("INBOX")).unwrap().is_empty());
        assert_eq!(store.trashed_thread_ids().unwrap(), ["t1"]);
    }

    #[test]
    fn queued_actions_survive_a_sync() {
        let mut store = Store::in_memory().unwrap();
        store
            .apply(false, &[row("a", "t1", 1, &["INBOX"])], &[], None)
            .unwrap();
        store
            .enqueue(&Op::Modify {
                ids: vec!["a".into()],
                add: vec![],
                remove: vec!["INBOX".into()],
            })
            .unwrap();
        // Gmail still says INBOX because the archive hasn't been pushed yet.
        store
            .apply(false, &[row("a", "t1", 1, &["INBOX"])], &[], Some("2"))
            .unwrap();
        assert!(store.thread_ids(Some("INBOX")).unwrap().is_empty());
    }

    #[test]
    fn reset_keeps_queue_and_snoozes() {
        let mut store = Store::in_memory().unwrap();
        store
            .apply(false, &[row("a", "t1", 1, &["INBOX"])], &[], None)
            .unwrap();
        store
            .enqueue(&Op::Trash {
                thread: "t1".into(),
            })
            .unwrap();
        store
            .set_snooze(&Snooze {
                thread_id: "t9".into(),
                until: 5,
                kind: SnoozeKind::Snooze,
                since: 0,
            })
            .unwrap();
        store
            .apply(true, &[row("n", "t2", 2, &["INBOX"])], &[], Some("5"))
            .unwrap();
        assert_eq!(store.pending_ops().unwrap().len(), 1);
        assert_eq!(store.snoozes().unwrap().len(), 1);
        assert_eq!(store.count().unwrap(), 1);
    }

    #[test]
    fn contacts_are_learned_from_headers() {
        let mut store = Store::in_memory().unwrap();
        let mut m = row("a", "t1", 1, &["INBOX"]);
        m.cc = "\"Grace Hopper\" <grace@navy.mil>, bob@example.com".into();
        store.apply(false, &[m], &[], None).unwrap();
        let found = store.contacts("grace", 5).unwrap();
        assert_eq!(
            found,
            [Contact {
                name: "Grace Hopper".into(),
                address: "grace@navy.mil".into()
            }]
        );
        assert_eq!(store.contacts("bob", 5).unwrap()[0].name, "");
    }

    #[test]
    fn notified_ids_are_taken_once() {
        let store = Store::in_memory().unwrap();
        assert_eq!(
            store
                .take_unnotified(&["a".into(), "b".into()])
                .unwrap()
                .len(),
            2
        );
        assert_eq!(
            store.take_unnotified(&["b".into(), "c".into()]).unwrap(),
            ["c"]
        );
    }
}
