//! The window's view of the mail engine. QML owns layout and keys; this
//! object owns state and work. Reads come straight from the SQLite cache on
//! the UI thread (they take milliseconds); anything that touches the network
//! runs on a tokio runtime and lands back on the UI thread through
//! `qt_thread().queue`.
//!
//! Lists and threads cross into QML as JSON strings, which QML diffs into
//! its models so rows animate in and out instead of resetting.

#[cxx_qt::bridge]
pub mod qobject {
    unsafe extern "C++" {
        include!("cxx-qt-lib/qstring.h");
        type QString = cxx_qt_lib::QString;
    }

    #[auto_cxx_name]
    extern "RustQt" {
        #[qobject]
        #[qml_element]
        /// "ready", or "client" (no OAuth client yet), or "login".
        #[qproperty(QString, setup)]
        #[qproperty(QString, account)]
        /// The list on screen: inbox, starred, sent, drafts, snoozed,
        /// scheduled, all, trash, search, or label:<id>.
        #[qproperty(QString, view)]
        #[qproperty(QString, threads)]
        #[qproperty(QString, splits)]
        #[qproperty(i32, split)]
        #[qproperty(QString, labels)]
        #[qproperty(QString, query)]
        /// The open conversation.
        #[qproperty(QString, thread)]
        #[qproperty(bool, syncing)]
        #[qproperty(bool, loading)]
        #[qproperty(f64, last_sync)]
        #[qproperty(QString, sync_error)]
        #[qproperty(i32, pending)]
        /// A Neovim window is open on a draft.
        #[qproperty(bool, composing)]
        /// The draft waiting for review, as JSON, or empty.
        #[qproperty(QString, review)]
        /// Seconds until a sent message really goes; 0 when nothing waits.
        #[qproperty(i32, send_countdown)]
        #[qproperty(bool, active)]
        /// A directory to save a screenshot tour into, from CHORD_SNAPSHOT.
        /// Development only.
        #[qproperty(QString, snapshot)]
        type Backend = super::BackendRust;

        #[qinvokable]
        fn refresh(self: Pin<&mut Backend>);
        #[qinvokable]
        fn check_setup(self: Pin<&mut Backend>);
        #[qinvokable]
        fn login(self: Pin<&mut Backend>);
        #[qinvokable]
        fn sync_now(self: Pin<&mut Backend>);
        #[qinvokable]
        fn set_view_to(self: Pin<&mut Backend>, view: &QString);
        #[qinvokable]
        fn set_split_to(self: Pin<&mut Backend>, index: i32);
        #[qinvokable]
        fn search(self: Pin<&mut Backend>, query: &QString);
        #[qinvokable]
        fn open_thread(self: Pin<&mut Backend>, id: &QString);
        #[qinvokable]
        fn close_thread(self: Pin<&mut Backend>);
        #[qinvokable]
        fn message_html(
            self: Pin<&mut Backend>,
            id: &QString,
            allow_remote: bool,
            show_quotes: bool,
            style: &QString,
        ) -> QString;
        #[qinvokable]
        fn allow_images(self: Pin<&mut Backend>, address: &QString);
        #[qinvokable]
        fn save_attachment(self: Pin<&mut Backend>, message_id: &QString, index: i32);
        /// action: archive, inbox, trash, untrash, spam, star, unstar, read,
        /// unread, label, unlabel, move, snooze, wake. `ids` is
        /// comma-separated; `arg` is a label id or a time in milliseconds.
        #[qinvokable]
        fn act(self: Pin<&mut Backend>, action: &QString, ids: &QString, arg: &QString);
        #[qinvokable]
        fn undo(self: Pin<&mut Backend>);
        #[qinvokable]
        fn create_label(self: Pin<&mut Backend>, name: &QString, ids: &QString, archive: bool);
        #[qinvokable]
        fn snooze_presets(self: &Backend) -> QString;
        #[qinvokable]
        fn parse_when(self: &Backend, text: &QString) -> f64;
        #[qinvokable]
        fn compose(self: Pin<&mut Backend>, kind: &QString, message_id: &QString, link: &QString);
        #[qinvokable]
        fn poll_compose(self: Pin<&mut Backend>);
        #[qinvokable]
        fn edit_draft(self: Pin<&mut Backend>);
        /// Sends the reviewed draft. `later` schedules it (ms, 0 for now);
        /// `remind` brings the thread back then if nobody replied (0 for off).
        #[qinvokable]
        fn send_draft(self: Pin<&mut Backend>, later: f64, remind: f64);
        #[qinvokable]
        fn undo_send(self: Pin<&mut Backend>);
        #[qinvokable]
        fn save_draft(self: Pin<&mut Backend>);
        #[qinvokable]
        fn discard_draft(self: Pin<&mut Backend>);
        /// Sends a message still in its undo window right away, for quitting.
        #[qinvokable]
        fn flush_send(self: Pin<&mut Backend>);
        /// Saves a screenshot of the window, for the development tour.
        #[qinvokable]
        fn grab(self: &Backend, path: &QString) -> bool;
        /// Presses a key through the shortcut map, for the development tour.
        #[qinvokable]
        fn press(self: &Backend, keys: &QString);

        /// A toast for QML to word: kind names what happened, count how many
        /// conversations, detail a name or time.
        #[qsignal]
        fn notice(
            self: Pin<&mut Backend>,
            kind: QString,
            count: i32,
            detail: QString,
            undoable: bool,
        );
        #[qsignal]
        fn body_ready(self: Pin<&mut Backend>, message_id: QString);
        /// Another launch asked to open a conversation.
        #[qsignal]
        fn open_requested(self: Pin<&mut Backend>, thread_id: QString);
        #[qsignal]
        fn quit_ready(self: Pin<&mut Backend>);
    }

    impl cxx_qt::Initialize for Backend {}
    impl cxx_qt::Threading for Backend {}
}

use std::collections::{HashMap, HashSet};
use std::future::Future;
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::OnceLock;
use std::time::Duration;

use anyhow::{Result, anyhow};
use chord_core::compose::{self, Draft, Identity, Kind};
use chord_core::config::Config;
use chord_core::ops::{self, Action, Undo};
use chord_core::render::{self, Source};
use chord_core::service::{self, Session};
use chord_core::store::{Scheduled, ThreadRow};
use chord_core::{MailApi, Store, auth, paths, sync, when};
use cxx_qt::{CxxQtType, Threading};
use cxx_qt_lib::QString;
use serde_json::{Value, json};

use crate::instance::Request;

type Apply = Box<dyn FnOnce(Pin<&mut qobject::Backend>) + Send>;

fn runtime() -> &'static tokio::runtime::Runtime {
    static RT: OnceLock<tokio::runtime::Runtime> = OnceLock::new();
    RT.get_or_init(|| {
        tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()
            .expect("tokio runtime")
    })
}

/// The request the window was launched with, consumed on startup.
pub static INITIAL: OnceLock<std::sync::Mutex<Option<Request>>> = OnceLock::new();

struct ComposeState {
    path: PathBuf,
    kind: Kind,
    source: Option<Source>,
    thread_id: Option<String>,
    draft: Option<Draft>,
    /// The "remind me" time chosen in review, kept across an undone send.
    remind: f64,
}

#[derive(Default)]
pub struct BackendRust {
    setup: QString,
    account: QString,
    view: QString,
    threads: QString,
    splits: QString,
    split: i32,
    labels: QString,
    query: QString,
    thread: QString,
    syncing: bool,
    loading: bool,
    last_sync: f64,
    sync_error: QString,
    pending: i32,
    composing: bool,
    review: QString,
    send_countdown: i32,
    active: bool,
    snapshot: QString,

    config: Config,
    open_thread_id: Option<String>,
    undo_stack: Vec<Undo>,
    search_ids: Vec<String>,
    fetching: HashSet<String>,
    compose: Option<ComposeState>,
    /// Bumped whenever a waiting send is cancelled or replaced.
    send_generation: u64,
    pending_raw: Option<(Vec<u8>, Option<String>, f64)>,
}

fn demo() -> bool {
    std::env::var_os("CHORD_DEMO").is_some()
}

fn store() -> Result<Store> {
    Store::open(&paths::database())
}

fn q(s: &str) -> QString {
    QString::from(s)
}

fn ids_of(ids: &QString) -> Vec<String> {
    ids.to_string()
        .split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(String::from)
        .collect()
}

fn drafts_dir() -> PathBuf {
    std::env::var_os("XDG_CACHE_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(".cache")
        })
        .join("chord-mail/drafts")
}

fn downloads_dir() -> PathBuf {
    std::process::Command::new("xdg-user-dir")
        .arg("DOWNLOAD")
        .output()
        .ok()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .filter(|s| !s.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join("Downloads")
        })
}

fn identity(store: &Store) -> Identity {
    let get = |k: &str| store.meta(k).ok().flatten().unwrap_or_default();
    let email = get("send_as_email");
    Identity {
        email: if email.is_empty() {
            get("email")
        } else {
            email
        },
        name: get("send_as_name"),
        signature: get("signature"),
    }
}

/// Label ids that are Gmail plumbing rather than something to show on a row.
fn hidden_label(id: &str, snooze_label: &str) -> bool {
    id == snooze_label
        || id.starts_with("CATEGORY_")
        || matches!(
            id,
            "INBOX"
                | "UNREAD"
                | "STARRED"
                | "IMPORTANT"
                | "SENT"
                | "DRAFT"
                | "TRASH"
                | "SPAM"
                | "CHAT"
        )
}

impl BackendRust {
    fn row_json(
        &self,
        r: &ThreadRow,
        names: &HashMap<String, String>,
        snooze_label: &str,
    ) -> Value {
        let labels: Vec<&str> = r
            .labels
            .iter()
            .filter(|l| !hidden_label(l, snooze_label))
            .filter_map(|l| names.get(l).map(String::as_str))
            .collect();
        json!({
            "id": r.id,
            "subject": r.subject,
            "senders": r.senders.join(", "),
            "count": r.count,
            "snippet": r.snippet,
            "date": r.date as f64,
            "unread": r.unread,
            "starred": r.starred,
            "draft": r.draft,
            "labels": labels.join(", "),
            "snoozed": r.snoozed_until.unwrap_or(0) as f64,
        })
    }
}

impl cxx_qt::Initialize for qobject::Backend {
    fn initialize(mut self: Pin<&mut Self>) {
        let config = Config::load().unwrap_or_default();
        self.as_mut().rust_mut().config = config;
        self.as_mut().set_view(q("inbox"));
        if let Some(dir) = std::env::var_os("CHORD_SNAPSHOT") {
            self.as_mut().set_snapshot(q(&dir.to_string_lossy()));
        }
        self.as_mut().check_setup();

        let thread = self.qt_thread();
        let _ = crate::instance::listen(move |request| {
            let _ = thread.queue(move |b| handle_request(b, request));
        });
        let initial = INITIAL.get().and_then(|m| m.lock().ok()?.take());
        if let Some(r) = initial {
            let thread = self.qt_thread();
            // After QML has connected to our signals.
            let _ = thread.queue(move |b| handle_request(b, r));
        }
        self.as_mut().refresh();
    }
}

fn handle_request(mut b: Pin<&mut qobject::Backend>, request: Request) {
    match request {
        Request::Focus => {}
        Request::Thread(id) => b.as_mut().open_requested(q(&id)),
        Request::Compose(link) => b.as_mut().compose(&q("new"), &q(""), &q(&link)),
    }
}

unsafe extern "C" {
    fn chord_grab_window(path: *const std::ffi::c_char) -> bool;
    fn chord_press(spec: *const std::ffi::c_char);
}

impl qobject::Backend {
    fn grab(&self, path: &QString) -> bool {
        let Ok(path) = std::ffi::CString::new(path.to_string()) else {
            return false;
        };
        // SAFETY: called on the UI thread with a valid C string.
        unsafe { chord_grab_window(path.as_ptr()) }
    }

    /// Presses each space-separated key in turn.
    fn press(&self, keys: &QString) {
        for key in keys.to_string().split(' ').filter(|k| !k.is_empty()) {
            let Ok(spec) = std::ffi::CString::new(key) else {
                continue;
            };
            // SAFETY: called on the UI thread with a valid C string.
            unsafe { chord_press(spec.as_ptr()) }
        }
    }

    /// Runs `work` on the runtime and applies its result on the UI thread.
    fn spawn(&self, work: impl Future<Output = Apply> + Send + 'static) {
        let thread = self.qt_thread();
        runtime().spawn(async move {
            // A bug in one task must not leave the window stuck "Syncing…".
            let apply =
                match futures::FutureExt::catch_unwind(std::panic::AssertUnwindSafe(work)).await {
                    Ok(apply) => apply,
                    Err(panic) => {
                        let msg = crate::daemon::panic_message(&panic);
                        Box::new(move |mut b: Pin<&mut qobject::Backend>| {
                            b.as_mut().set_syncing(false);
                            b.as_mut().set_loading(false);
                            b.fail(anyhow!("Chord hit a bug: {msg}"));
                        }) as Apply
                    }
                };
            let _ = thread.queue(apply);
        });
    }

    fn toast(self: Pin<&mut Self>, kind: &str, count: usize, detail: &str, undoable: bool) {
        self.notice(q(kind), count as i32, q(detail), undoable);
    }

    fn fail(self: Pin<&mut Self>, e: anyhow::Error) {
        self.toast("error", 0, &format!("{e:#}"), false);
    }

    fn check_setup(mut self: Pin<&mut Self>) {
        let state = if demo() {
            "ready"
        } else if !auth::Client::available(&paths::client_secret()) {
            "client"
        } else if !service::signed_in() {
            "login"
        } else {
            "ready"
        };
        if self.setup().to_string() != state {
            self.as_mut().set_setup(q(state));
            if state == "ready" {
                self.as_mut().sync_now();
            }
        }
        if let Ok(s) = store() {
            let account = s.account().unwrap_or_default();
            self.as_mut().set_account(q(&account));
        }
    }

    fn login(self: Pin<&mut Self>) {
        self.spawn(async move {
            let result: Result<String> = async {
                let session = Session::new()?;
                let token = auth::login(session.http(), session.client(), |url| {
                    let _ = std::process::Command::new("xdg-open").arg(url).spawn();
                })
                .await?;
                token.save(&paths::token())?;
                let profile = match async { session.gmail().await?.profile().await }.await {
                    Ok(p) => p,
                    Err(e) => {
                        // Don't leave the new token paired with the old account's data.
                        let _ = std::fs::remove_file(paths::token());
                        return Err(e);
                    }
                };
                let s = store()?;
                let previous = s.account()?;
                if !previous.is_empty() && !previous.eq_ignore_ascii_case(&profile.email_address) {
                    drop(s);
                    let _ = std::fs::remove_file(paths::database());
                }
                store()?.set_meta("email", &profile.email_address)?;
                Ok(profile.email_address)
            }
            .await;
            Box::new(move |mut b: Pin<&mut qobject::Backend>| match result {
                Ok(_) => b.as_mut().check_setup(),
                Err(e) => b.fail(e),
            }) as Apply
        });
    }

    fn sync_now(mut self: Pin<&mut Self>) {
        // Demo mode shows a seeded cache and never talks to Gmail.
        if *self.syncing() || self.setup().to_string() != "ready" || demo() {
            return;
        }
        self.as_mut().set_syncing(true);
        let config = self.config.clone();
        self.spawn(async move {
            let result: Result<service::TickReport> = async {
                let api = Session::new()?.gmail().await?;
                let _lock = service::lock().await?;
                let mut s = store()?;
                service::tick(&api, &mut s, &config).await
            }
            .await;
            Box::new(move |mut b: Pin<&mut qobject::Backend>| {
                b.as_mut().set_syncing(false);
                match result {
                    Ok(report) => {
                        b.as_mut().set_sync_error(QString::default());
                        b.as_mut().set_last_sync(sync::now_ms() as f64);
                        // The service notifies too; the window only does
                        // when you're looking elsewhere.
                        if !*b.active() {
                            for n in report.notices {
                                crate::daemon::notify(n);
                            }
                        }
                    }
                    Err(e) => b.as_mut().set_sync_error(q(&format!("{e:#}"))),
                }
                b.as_mut().refresh();
                b.as_mut().reload_thread();
            }) as Apply
        });
    }

    fn flush_later(mut self: Pin<&mut Self>) {
        if demo() {
            self.as_mut().update_pending();
            return;
        }
        self.spawn(async move {
            let result: Result<usize> = async {
                let api = Session::new()?.gmail().await?;
                let _lock = service::lock().await?;
                ops::flush(&api, &mut store()?).await
            }
            .await;
            Box::new(move |mut b: Pin<&mut qobject::Backend>| {
                if let Err(e) = result {
                    b.as_mut().set_sync_error(q(&format!("{e:#}")));
                }
                b.as_mut().update_pending();
            }) as Apply
        });
    }

    fn update_pending(self: Pin<&mut Self>) {
        let n = store()
            .and_then(|s| s.pending_ops())
            .map(|o| o.len())
            .unwrap_or(0);
        self.set_pending(n as i32);
    }

    fn refresh(mut self: Pin<&mut Self>) {
        let result: Result<()> = (|| {
            let s = store()?;
            let catalog = s.label_catalog()?;
            let snooze_label = s.meta("snooze_label")?.unwrap_or_default();
            let names: HashMap<String, String> = catalog
                .iter()
                .map(|l| (l.id.clone(), l.name.clone()))
                .collect();
            let labels: Vec<Value> = catalog
                .iter()
                .filter(|l| !l.system && l.id != snooze_label)
                .map(|l| json!({ "id": l.id, "name": l.name }))
                .collect();
            let view = self.view().to_string();
            let config = self.config.clone();
            let mut split_json = Vec::new();
            let rows: Vec<Value> = if view == "inbox" {
                let inbox = service::inbox(&s, &config)?;
                for (i, sp) in config.split.iter().enumerate() {
                    let mine: Vec<&ThreadRow> = inbox
                        .iter()
                        .filter(|(k, _)| *k == i)
                        .map(|(_, r)| r)
                        .collect();
                    split_json.push(json!({
                        "name": sp.name,
                        "count": mine.len(),
                        "unread": mine.iter().filter(|r| r.unread).count(),
                    }));
                }
                let current = (*self.split() as usize).min(config.split.len().saturating_sub(1));
                inbox
                    .iter()
                    .filter(|(k, _)| *k == current)
                    .map(|(_, r)| self.row_json(r, &names, &snooze_label))
                    .collect()
            } else {
                let ids: Vec<String> = match view.as_str() {
                    "starred" => s.thread_ids(Some("STARRED"))?,
                    "sent" => s.thread_ids(Some("SENT"))?,
                    "drafts" => s.thread_ids(Some("DRAFT"))?,
                    "all" => s.thread_ids(None)?,
                    "trash" => s.trashed_thread_ids()?,
                    "snoozed" => s.snoozes()?.into_iter().map(|z| z.thread_id).collect(),
                    "search" => self.search_ids.clone(),
                    "scheduled" => s
                        .scheduled()?
                        .into_iter()
                        .filter_map(|z| z.thread_id)
                        .collect(),
                    v if v.starts_with("label:") => s.thread_ids(Some(&v[6..]))?,
                    _ => vec![],
                };
                let mut rows = s.thread_rows(&ids)?;
                if view == "snoozed" {
                    rows.sort_by_key(|r| r.snoozed_until.unwrap_or(i64::MAX));
                }
                rows.iter()
                    .take(500)
                    .map(|r| self.row_json(r, &names, &snooze_label))
                    .collect()
            };
            self.as_mut()
                .set_threads(q(&Value::Array(rows).to_string()));
            self.as_mut()
                .set_splits(q(&Value::Array(split_json).to_string()));
            self.as_mut()
                .set_labels(q(&Value::Array(labels).to_string()));
            Ok(())
        })();
        if let Err(e) = result {
            self.as_mut().fail(e);
        }
        self.update_pending();
    }

    fn set_view_to(mut self: Pin<&mut Self>, view: &QString) {
        let view = view.to_string();
        self.as_mut().set_view(q(&view));
        self.as_mut().refresh();
        // Only the inbox is synced in full; other views fill in from Gmail.
        let remote = match view.as_str() {
            "starred" => Some(Some("STARRED".to_string())),
            "sent" => Some(Some("SENT".to_string())),
            "drafts" => Some(Some("DRAFT".to_string())),
            "trash" => Some(Some("TRASH".to_string())),
            "all" => Some(None),
            v if v.starts_with("label:") => Some(Some(v[6..].to_string())),
            _ => None,
        };
        let Some(label) = remote else { return };
        if demo() || self.setup().to_string() != "ready" {
            return;
        }
        self.as_mut().set_loading(true);
        self.spawn(async move {
            let result: Result<()> = async {
                let api = Session::new()?.gmail().await?;
                let ids = api.search_threads("", label.as_deref(), 100).await?;
                let mut s = store()?;
                for id in &ids {
                    sync::ensure_thread(&api, &mut s, id, 0).await?;
                }
                Ok(())
            }
            .await;
            Box::new(move |mut b: Pin<&mut qobject::Backend>| {
                b.as_mut().set_loading(false);
                if let Err(e) = result {
                    b.as_mut().fail(e);
                }
                b.as_mut().refresh();
            }) as Apply
        });
    }

    fn set_split_to(mut self: Pin<&mut Self>, index: i32) {
        let n = self.config.split.len() as i32;
        self.as_mut().set_split(index.rem_euclid(n.max(1)));
        self.refresh();
    }

    fn search(mut self: Pin<&mut Self>, query: &QString) {
        let text = query.to_string();
        self.as_mut().set_query(query.clone());
        let local = store()
            .and_then(|s| s.search_local(&text, 50))
            .unwrap_or_default();
        self.as_mut().rust_mut().search_ids = local;
        self.as_mut().set_view(q("search"));
        self.as_mut().refresh();
        if text.trim().is_empty() || demo() || self.setup().to_string() != "ready" {
            return;
        }
        self.as_mut().set_loading(true);
        self.spawn(async move {
            let result: Result<Vec<String>> = async {
                let api = Session::new()?.gmail().await?;
                let ids = api.search_threads(&text, None, 50).await?;
                let mut s = store()?;
                for id in &ids {
                    sync::ensure_thread(&api, &mut s, id, 0).await?;
                }
                Ok(ids)
            }
            .await;
            Box::new(move |mut b: Pin<&mut qobject::Backend>| {
                b.as_mut().set_loading(false);
                // A newer search may have started meanwhile.
                if b.view().to_string() != "search" || b.query().to_string() != text {
                    return;
                }
                match result {
                    Ok(ids) => b.as_mut().rust_mut().search_ids = ids,
                    Err(e) => b.as_mut().fail(e),
                }
                b.as_mut().refresh();
            }) as Apply
        });
    }

    // ----- reading ------------------------------------------------------------

    fn thread_json(&self, s: &Store, id: &str) -> Result<Value> {
        let me = s.account()?;
        let messages = s.thread_messages(id)?;
        let catalog = s.label_catalog()?;
        let snooze_label = s.meta("snooze_label")?.unwrap_or_default();
        let names: HashMap<String, String> = catalog.into_iter().map(|l| (l.id, l.name)).collect();
        let row = s.thread_rows(&[id.to_string()])?.into_iter().next();
        let list: Vec<Value> = messages
            .iter()
            .filter(|m| !m.has("TRASH") || messages.iter().all(|x| x.has("TRASH")))
            .map(|m| {
                let address = chord_core::text::address_of(&m.from);
                json!({
                    "id": m.id,
                    "fromName": chord_core::text::sender_name(&m.from),
                    "address": address,
                    "to": m.to,
                    "cc": m.cc,
                    "date": m.date as f64,
                    "snippet": m.snippet,
                    "unread": m.is_unread(),
                    "draft": m.has("DRAFT"),
                    "me": !me.is_empty() && address.eq_ignore_ascii_case(&me),
                    "hasBody": s.body(&m.id).ok().flatten().is_some(),
                })
            })
            .collect();
        let labels: Vec<&str> = row
            .as_ref()
            .map(|r| {
                r.labels
                    .iter()
                    .filter(|l| !hidden_label(l, &snooze_label))
                    .filter_map(|l| names.get(l).map(String::as_str))
                    .collect()
            })
            .unwrap_or_default();
        Ok(json!({
            "id": id,
            "subject": row.as_ref().map(|r| r.subject.clone()).unwrap_or_default(),
            "starred": row.as_ref().is_some_and(|r| r.starred),
            "inInbox": row.as_ref().is_some_and(|r| r.labels.iter().any(|l| l == "INBOX")),
            "snoozed": row.as_ref().and_then(|r| r.snoozed_until).unwrap_or(0) as f64,
            "labels": labels.join(", "),
            "messages": list,
        }))
    }

    fn reload_thread(mut self: Pin<&mut Self>) {
        let Some(id) = self.open_thread_id.clone() else {
            return;
        };
        if let Ok(s) = store()
            && let Ok(json) = self.thread_json(&s, &id)
        {
            self.as_mut().set_thread(q(&json.to_string()));
        }
    }

    fn open_thread(mut self: Pin<&mut Self>, id: &QString) {
        let id = id.to_string();
        self.as_mut().rust_mut().open_thread_id = Some(id.clone());
        self.as_mut().reload_thread();
        // Reading marks it read, as everywhere else.
        let unread = store()
            .and_then(|s| s.thread_rows(std::slice::from_ref(&id)))
            .map(|r| r.first().is_some_and(|r| r.unread))
            .unwrap_or(false);
        if unread {
            if let Ok(mut s) = store() {
                let _ = ops::apply(&mut s, std::slice::from_ref(&id), &Action::MarkRead);
            }
            self.as_mut().refresh();
            self.as_mut().reload_thread();
            self.as_mut().flush_later();
        }
        self.as_mut().fetch_thread(id);
    }

    fn fetch_thread(mut self: Pin<&mut Self>, id: String) {
        if demo()
            || self.setup().to_string() != "ready"
            || !self.as_mut().rust_mut().fetching.insert(id.clone())
        {
            return;
        }
        self.spawn(async move {
            let result: Result<Vec<String>> = async {
                let api = Session::new()?.gmail().await?;
                let mut s = store()?;
                sync::ensure_thread(&api, &mut s, &id, i64::MAX).await?;
                Ok(s.thread_messages(&id)?.into_iter().map(|m| m.id).collect())
            }
            .await;
            Box::new(move |mut b: Pin<&mut qobject::Backend>| {
                b.as_mut().rust_mut().fetching.remove(&id);
                match result {
                    Ok(messages) => {
                        if b.open_thread_id.as_deref() == Some(id.as_str()) {
                            b.as_mut().reload_thread();
                            for m in messages {
                                b.as_mut().body_ready(q(&m));
                            }
                        }
                    }
                    Err(e) => b.fail(e),
                }
            }) as Apply
        });
    }

    fn close_thread(mut self: Pin<&mut Self>) {
        self.as_mut().rust_mut().open_thread_id = None;
        self.set_thread(QString::default());
    }

    fn message_html(
        self: Pin<&mut Self>,
        id: &QString,
        allow_remote: bool,
        show_quotes: bool,
        style: &QString,
    ) -> QString {
        let id = id.to_string();
        let Ok(s) = store() else {
            return QString::default();
        };
        let Ok(Some(raw)) = s.body(&id) else {
            return q(&json!({ "ready": false }).to_string());
        };
        let style: Value = serde_json::from_str(&style.to_string()).unwrap_or(Value::Null);
        let field = |k: &str| {
            style
                .get(k)
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string()
        };
        let sender = s
            .message(&id)
            .ok()
            .flatten()
            .map(|m| chord_core::text::address_of(&m.from))
            .unwrap_or_default();
        let trusted = s.images_allowed(&sender).unwrap_or(false);
        let r = render::render(
            &raw,
            &render::Options {
                allow_remote: allow_remote || trusted,
                show_quotes,
                text_color: field("text"),
                link_color: field("link"),
                muted_color: field("muted"),
                font_family: field("font"),
            },
        );
        // loadHtml goes through a data: URL, which Chromium caps near 2 MB;
        // bigger messages (inline images) load from a private file instead.
        let mut url = String::new();
        let mut html = r.html;
        if html.len() > 1_500_000 {
            let dir = std::env::var_os("XDG_RUNTIME_DIR")
                .map(PathBuf::from)
                .unwrap_or_else(std::env::temp_dir)
                .join("chord-mail");
            let path = dir.join(format!("{id}.html"));
            let written = std::fs::create_dir_all(&dir)
                .and_then(|_| {
                    use std::os::unix::fs::PermissionsExt;
                    std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700))
                })
                .and_then(|_| std::fs::write(&path, &html));
            if written.is_ok() {
                url = format!("file://{}", path.display());
                html.clear();
            }
        }
        q(&json!({
            "ready": true,
            "url": url,
            "html": html,
            "plain": r.plain,
            "hasRemote": r.has_remote,
            "hasQuotes": r.has_quotes,
            "imagesAllowed": allow_remote || trusted,
            "sender": sender,
            "attachments": r.attachments,
        })
        .to_string())
    }

    fn allow_images(self: Pin<&mut Self>, address: &QString) {
        if let Ok(s) = store() {
            let _ = s.allow_images(&address.to_string());
        }
        self.toast("images-allowed", 0, &address.to_string(), false);
    }

    fn save_attachment(mut self: Pin<&mut Self>, message_id: &QString, index: i32) {
        let result: Result<PathBuf> = (|| {
            let raw = store()?
                .body(&message_id.to_string())?
                .ok_or_else(|| anyhow!("the message isn't downloaded yet"))?;
            let (name, bytes) = render::attachment(&raw, index.max(0) as usize)
                .ok_or_else(|| anyhow!("no such attachment"))?;
            let dir = downloads_dir();
            std::fs::create_dir_all(&dir)?;
            let mut path = dir.join(&name);
            let (stem, ext) = match name.rsplit_once('.') {
                Some((s, e)) => (s.to_string(), format!(".{e}")),
                None => (name.clone(), String::new()),
            };
            let mut n = 1;
            while path.exists() {
                path = dir.join(format!("{stem} ({n}){ext}"));
                n += 1;
            }
            std::fs::write(&path, bytes)?;
            Ok(path)
        })();
        match result {
            Ok(path) => {
                let _ = std::process::Command::new("xdg-open").arg(&path).spawn();
                let name = path
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default();
                self.as_mut().toast("saved", 0, &name, false);
            }
            Err(e) => self.as_mut().fail(e),
        }
    }

    // ----- actions --------------------------------------------------------------

    fn act(mut self: Pin<&mut Self>, action: &QString, ids: &QString, arg: &QString) {
        let ids = ids_of(ids);
        if ids.is_empty() {
            return;
        }
        let arg = arg.to_string();
        let ms = arg.parse::<f64>().unwrap_or(0.0) as i64;
        let (action, kind) = match action.to_string().as_str() {
            "archive" => (Action::Archive, "archived"),
            "inbox" => (Action::MoveToInbox, "inboxed"),
            "trash" => (Action::Trash, "trashed"),
            "untrash" => (Action::Untrash, "untrashed"),
            "spam" => (Action::Spam, "spammed"),
            "star" => (Action::Star, "starred"),
            "unstar" => (Action::Unstar, "unstarred"),
            "read" => (Action::MarkRead, "read"),
            "unread" => (Action::MarkUnread, "unread"),
            "label" => (Action::AddLabel(arg.clone()), "labeled"),
            "unlabel" => (Action::RemoveLabel(arg.clone()), "unlabeled"),
            "move" => (Action::MoveTo(arg.clone()), "moved"),
            "snooze" => (Action::Snooze { until: ms }, "snoozed"),
            "wake" => (Action::Wake, "woken"),
            _ => return,
        };
        let detail = match &action {
            Action::AddLabel(l) | Action::RemoveLabel(l) | Action::MoveTo(l) => store()
                .and_then(|s| s.label_catalog())
                .ok()
                .and_then(|c| c.into_iter().find(|x| x.id == *l).map(|x| x.name))
                .unwrap_or_default(),
            Action::Snooze { until } => until.to_string(),
            _ => String::new(),
        };
        let result = store().and_then(|mut s| ops::apply(&mut s, &ids, &action));
        match result {
            Ok(undo) => {
                self.as_mut().rust_mut().undo_stack.push(undo);
                self.as_mut().refresh();
                self.as_mut().reload_thread();
                // Read and unread are too routine to announce.
                if !matches!(action, Action::MarkRead) {
                    self.as_mut().toast(kind, ids.len(), &detail, true);
                }
                self.as_mut().flush_later();
            }
            Err(e) => self.as_mut().fail(e),
        }
    }

    fn undo(mut self: Pin<&mut Self>) {
        if *self.send_countdown() > 0 {
            self.undo_send();
            return;
        }
        let Some(u) = self.as_mut().rust_mut().undo_stack.pop() else {
            self.toast("nothing-to-undo", 0, "", false);
            return;
        };
        match store().and_then(|mut s| ops::undo(&mut s, &u)) {
            Ok(()) => {
                self.as_mut().refresh();
                self.as_mut().reload_thread();
                self.as_mut().toast("undone", 0, "", false);
                self.as_mut().flush_later();
            }
            Err(e) => self.as_mut().fail(e),
        }
    }

    fn create_label(self: Pin<&mut Self>, name: &QString, ids: &QString, archive: bool) {
        let name = name.to_string().trim().to_string();
        let ids = ids.to_string();
        if name.is_empty() {
            return;
        }
        self.spawn(async move {
            let result: Result<String> = async {
                let api = Session::new()?.gmail().await?;
                let label = api.create_label(&name).await?;
                let mut s = store()?;
                let mut catalog = s.label_catalog()?;
                catalog.push(chord_core::store::LabelRow {
                    id: label.id.clone(),
                    name: label.name,
                    system: false,
                });
                s.set_label_catalog(&catalog)?;
                Ok(label.id)
            }
            .await;
            Box::new(move |mut b: Pin<&mut qobject::Backend>| match result {
                Ok(id) => {
                    let action = if archive { "move" } else { "label" };
                    b.as_mut().act(&q(action), &q(&ids), &q(&id));
                }
                Err(e) => b.fail(e),
            }) as Apply
        });
    }

    fn snooze_presets(&self) -> QString {
        let list: Vec<Value> = when::presets()
            .into_iter()
            .map(|(label, at)| json!({ "label": label, "at": at as f64 }))
            .collect();
        q(&Value::Array(list).to_string())
    }

    fn parse_when(&self, text: &QString) -> f64 {
        when::parse(&text.to_string())
            .map(|t| t as f64)
            .unwrap_or(0.0)
    }

    // ----- writing ----------------------------------------------------------------

    fn compose(mut self: Pin<&mut Self>, kind: &QString, message_id: &QString, link: &QString) {
        if self.compose.is_some() {
            self.toast("already-composing", 0, "", false);
            return;
        }
        let kind = match kind.to_string().as_str() {
            "reply" => Kind::Reply,
            "replyall" => Kind::ReplyAll,
            "forward" => Kind::Forward,
            _ => Kind::New,
        };
        let message_id = message_id.to_string();
        let link = link.to_string();
        if kind == Kind::New || message_id.is_empty() {
            self.as_mut().start_compose(Kind::New, None, None, &link);
            return;
        }
        // Replies need the original's body; fetch it first if it's not here.
        let have = store()
            .and_then(|s| s.body(&message_id))
            .ok()
            .flatten()
            .is_some();
        if have {
            self.as_mut().start_compose_from(kind, &message_id, &link);
            return;
        }
        self.as_mut().set_loading(true);
        self.spawn(async move {
            let result: Result<()> = async {
                let api = Session::new()?.gmail().await?;
                let raw = api.message_raw(&message_id).await?;
                store()?.set_body(&message_id, &raw)?;
                Ok(())
            }
            .await;
            Box::new(move |mut b: Pin<&mut qobject::Backend>| {
                b.as_mut().set_loading(false);
                match result {
                    Ok(()) => b.as_mut().start_compose_from(kind, &message_id, &link),
                    Err(e) => b.fail(e),
                }
            }) as Apply
        });
    }

    fn start_compose_from(self: Pin<&mut Self>, kind: Kind, message_id: &str, link: &str) {
        let s = store().ok();
        let source = s
            .as_ref()
            .and_then(|s| s.body(message_id).ok().flatten())
            .and_then(|raw| render::source(&raw));
        let thread_id = s
            .as_ref()
            .and_then(|s| s.message(message_id).ok().flatten())
            .map(|m| m.thread_id);
        self.start_compose(
            kind,
            source,
            if kind == Kind::Forward {
                None
            } else {
                thread_id
            },
            link,
        );
    }

    fn start_compose(
        mut self: Pin<&mut Self>,
        kind: Kind,
        source: Option<Source>,
        thread_id: Option<String>,
        link: &str,
    ) {
        let me = store().map(|s| identity(&s)).unwrap_or_default();
        let prefill = if link.is_empty() {
            Default::default()
        } else {
            compose::mailto(link)
        };
        let (text, line) = compose::template(kind, source.as_ref(), &me, &prefill);
        let dir = drafts_dir();
        let path = dir.join(format!("{}.md", sync::now_ms()));
        let result = paths::private_dir(&dir).and_then(|_| std::fs::write(&path, text));
        if let Err(e) = result {
            self.fail(e.into());
            return;
        }
        // A reply starts in the body; a new message starts on the To line.
        let start = if kind == Kind::New && prefill.to.is_empty() {
            line.saturating_sub(6).max(1)
        } else {
            line
        };
        self.as_mut().rust_mut().compose = Some(ComposeState {
            path: path.clone(),
            kind,
            source,
            thread_id,
            draft: None,
            remind: 0.0,
        });
        self.as_mut().set_review(QString::default());
        self.as_mut().launch_editor(&path, start);
    }

    fn launch_editor(mut self: Pin<&mut Self>, path: &std::path::Path, line: usize) {
        let exit = path.with_extension("exit");
        let _ = std::fs::remove_file(&exit);
        let title = match self.compose.as_ref().map(|c| c.kind) {
            Some(Kind::Reply | Kind::ReplyAll) => "Chord: Reply",
            Some(Kind::Forward) => "Chord: Forward",
            _ => "Chord: New message",
        };
        // Neovim runs in Omarchy's floating terminal; the exit code lands in
        // a file because terminal launchers return before the editor quits.
        let script = "nvim \"+$2\" -c 'setlocal spell wrap linebreak textwidth=0' -c 'if getline(\".\") ==# \"\" | startinsert | else | startinsert! | endif' -- \"$1\"; echo $? > \"$3\"";
        // CHORD_COMPOSE_CMD stands in for the editor in the development tour:
        // a shell command run with the draft, start line, and exit file.
        let spawned = match std::env::var("CHORD_COMPOSE_CMD") {
            Ok(cmd) => std::process::Command::new("sh")
                .args(["-c", &cmd, "chord"])
                .arg(path)
                .arg(line.to_string())
                .arg(&exit)
                .spawn(),
            Err(_) => std::process::Command::new("xdg-terminal-exec")
                .arg("--app-id=TUI.float")
                .arg(format!("--title={title}"))
                .args(["-e", "sh", "-c", script, "chord"])
                .arg(path)
                .arg(line.to_string())
                .arg(&exit)
                .spawn(),
        };
        match spawned {
            Ok(_) => self.as_mut().set_composing(true),
            Err(e) => {
                self.as_mut().rust_mut().compose = None;
                self.fail(anyhow!("couldn't open Neovim in a terminal: {e}"));
            }
        }
    }

    fn edit_draft(mut self: Pin<&mut Self>) {
        let Some(path) = self.compose.as_ref().map(|c| c.path.clone()) else {
            return;
        };
        let body_line = std::fs::read_to_string(&path)
            .map(|t| {
                t.lines()
                    .position(|l| l.trim().is_empty())
                    .map(|i| i + 2)
                    .unwrap_or(1)
            })
            .unwrap_or(1);
        self.as_mut().set_review(QString::default());
        self.launch_editor(&path, body_line);
    }

    fn poll_compose(mut self: Pin<&mut Self>) {
        let Some(path) = self.compose.as_ref().map(|c| c.path.clone()) else {
            self.set_composing(false);
            return;
        };
        let exit = path.with_extension("exit");
        let Ok(code) = std::fs::read_to_string(&exit) else {
            return;
        };
        let _ = std::fs::remove_file(&exit);
        self.as_mut().set_composing(false);
        if code.trim() != "0" {
            self.as_mut().discard_draft();
            return;
        }
        let text = std::fs::read_to_string(&path).unwrap_or_default();
        let mut draft = compose::parse(&text);
        let empty =
            draft.body.trim().is_empty() && draft.subject.trim().is_empty() && draft.to.is_empty();
        if empty && self.compose.as_ref().is_some_and(|c| c.kind == Kind::New) {
            self.as_mut().discard_draft();
            return;
        }
        // Names typed without an address resolve when exactly one contact fits.
        let s = store().ok();
        let mut unresolved: Vec<Value> = Vec::new();
        for list in [&mut draft.to, &mut draft.cc, &mut draft.bcc] {
            for entry in list.iter_mut() {
                if compose::is_address(&chord_core::text::address_of(entry)) {
                    continue;
                }
                let found = s
                    .as_ref()
                    .and_then(|s| s.contacts(entry, 5).ok())
                    .unwrap_or_default();
                if found.len() == 1 {
                    let c = &found[0];
                    *entry = if c.name.is_empty() {
                        c.address.clone()
                    } else {
                        format!("\"{}\" <{}>", c.name, c.address)
                    };
                } else {
                    unresolved.push(json!({
                        "entry": entry,
                        "suggestions": found.iter().map(|c| if c.name.is_empty() { c.address.clone() } else { format!("{} <{}>", c.name, c.address) }).collect::<Vec<_>>(),
                    }));
                }
            }
        }
        let me = s.as_ref().map(identity).unwrap_or_default();
        let (kind, source) = match self.compose.as_ref() {
            Some(c) => (c.kind, c.source.clone()),
            None => return,
        };
        let preview = render::page_for_html(
            &compose::html_body(&draft, kind, source.as_ref(), &me),
            &render::Options {
                allow_remote: false,
                show_quotes: true,
                ..Default::default()
            },
        );
        let attachments: Vec<Value> = draft
            .attach
            .iter()
            .map(|p| {
                json!({
                    "name": p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(),
                    "size": std::fs::metadata(p).map(|m| m.len() as f64).unwrap_or(0.0),
                    "missing": !p.is_file(),
                })
            })
            .chain(source.iter().filter(|_| kind == Kind::Forward).flat_map(|s| {
                s.attachments.iter().map(|(n, _, b)| json!({ "name": n, "size": b.len() as f64, "missing": false }))
            }))
            .collect();
        let ok = unresolved.is_empty()
            && draft.missing_attachments().is_empty()
            && draft.recipients().next().is_some();
        let review = json!({
            "kind": kind,
            "from": me.email,
            "to": draft.to.join(", "),
            "cc": draft.cc.join(", "),
            "bcc": draft.bcc.join(", "),
            "subject": draft.subject,
            "html": preview,
            "attachments": attachments,
            "unresolved": unresolved,
            "noRecipients": draft.recipients().next().is_none(),
            "remind": self.compose.as_ref().map(|c| c.remind).unwrap_or(0.0),
            "canSend": ok,
        });
        if let Some(c) = self.as_mut().rust_mut().compose.as_mut() {
            c.draft = Some(draft);
        }
        self.set_review(q(&review.to_string()));
    }

    fn build_raw(&self) -> Result<(Vec<u8>, Option<String>)> {
        let c = self.compose.as_ref().ok_or_else(|| anyhow!("no draft"))?;
        let draft = c.draft.as_ref().ok_or_else(|| anyhow!("no draft"))?;
        let me = identity(&store()?);
        if me.email.is_empty() {
            return Err(anyhow!("not signed in"));
        }
        let raw = compose::build(draft, c.kind, c.source.as_ref(), &me)?;
        Ok((raw, c.thread_id.clone()))
    }

    fn finish_compose(mut self: Pin<&mut Self>) {
        if let Some(c) = self.as_mut().rust_mut().compose.take() {
            let _ = std::fs::remove_file(&c.path);
        }
        self.as_mut().set_review(QString::default());
        self.set_composing(false);
    }

    fn send_draft(mut self: Pin<&mut Self>, later: f64, remind: f64) {
        if let Some(c) = self.as_mut().rust_mut().compose.as_mut() {
            c.remind = remind;
        }
        let (raw, thread_id) = match self.build_raw() {
            Ok(r) => r,
            Err(e) => {
                self.fail(e);
                return;
            }
        };
        let subject = self
            .compose
            .as_ref()
            .and_then(|c| c.draft.as_ref())
            .map(|d| d.subject.clone())
            .unwrap_or_default();
        if later > 0.0 {
            let recipients = self
                .compose
                .as_ref()
                .and_then(|c| c.draft.as_ref())
                .map(|d| d.to.join(", "))
                .unwrap_or_default();
            self.as_mut().set_loading(true);
            self.spawn(async move {
                let result: Result<()> = async {
                    let api = Session::new()?.gmail().await?;
                    let d = api.create_draft(&raw, thread_id.as_deref()).await?;
                    let mut s = store()?;
                    s.add_scheduled(&Scheduled {
                        draft_id: d.id,
                        send_at: later as i64,
                        thread_id: Some(d.message.thread_id.clone()),
                        subject,
                        recipients,
                    })?;
                    if remind > 0.0 {
                        // The reminder counts replies from when it goes out.
                        sync::ensure_thread(&api, &mut s, &d.message.thread_id, 0).await?;
                        s.set_snooze(&chord_core::store::Snooze {
                            thread_id: d.message.thread_id,
                            until: remind as i64,
                            kind: chord_core::store::SnoozeKind::Remind,
                            since: later as i64,
                        })?;
                    }
                    Ok(())
                }
                .await;
                Box::new(move |mut b: Pin<&mut qobject::Backend>| {
                    b.as_mut().set_loading(false);
                    match result {
                        Ok(()) => {
                            b.as_mut().finish_compose();
                            b.as_mut()
                                .toast("scheduled", 0, &(later as i64).to_string(), false);
                            b.as_mut().refresh();
                        }
                        Err(e) => b.fail(e),
                    }
                }) as Apply
            });
            return;
        }

        // Hold the message for the undo window, then send it.
        let seconds = self.config.compose.undo_seconds as i32;
        let generation = self.send_generation + 1;
        self.as_mut().rust_mut().send_generation = generation;
        self.as_mut().rust_mut().pending_raw = Some((raw, thread_id, remind));
        self.as_mut().set_review(QString::default());
        self.as_mut().set_send_countdown(seconds.max(1));
        self.as_mut().tick_send(generation);
    }

    fn tick_send(self: Pin<&mut Self>, generation: u64) {
        self.spawn(async move {
            tokio::time::sleep(Duration::from_secs(1)).await;
            Box::new(move |mut b: Pin<&mut qobject::Backend>| {
                if b.send_generation != generation || b.pending_raw.is_none() {
                    return;
                }
                let left = *b.send_countdown() - 1;
                if left > 0 {
                    b.as_mut().set_send_countdown(left);
                    b.as_mut().tick_send(generation);
                } else {
                    b.as_mut().set_send_countdown(0);
                    b.as_mut().really_send(false);
                }
            }) as Apply
        });
    }

    fn really_send(mut self: Pin<&mut Self>, then_quit: bool) {
        let Some((raw, thread_id, remind)) = self.as_mut().rust_mut().pending_raw.take() else {
            if then_quit {
                self.quit_ready();
            }
            return;
        };
        self.spawn(async move {
            let result: Result<()> = async {
                let api = Session::new()?.gmail().await?;
                let (_, thread) = api.send(&raw, thread_id.as_deref()).await?;
                if remind > 0.0 {
                    let mut s = store()?;
                    sync::ensure_thread(&api, &mut s, &thread, 0).await?;
                    ops::apply(
                        &mut s,
                        &[thread],
                        &Action::Remind {
                            until: remind as i64,
                        },
                    )?;
                }
                Ok(())
            }
            .await;
            Box::new(move |mut b: Pin<&mut qobject::Backend>| {
                match result {
                    Ok(()) => {
                        b.as_mut().finish_compose();
                        b.as_mut().toast("sent", 0, "", false);
                        b.as_mut().sync_now();
                    }
                    Err(e) => {
                        // Keep the draft so nothing typed is lost.
                        b.as_mut().fail(e);
                        b.as_mut().poll_review_again();
                    }
                }
                if then_quit {
                    b.quit_ready();
                }
            }) as Apply
        });
    }

    /// Shows the review again for a draft whose send failed or was undone.
    fn poll_review_again(mut self: Pin<&mut Self>) {
        let Some(path) = self.compose.as_ref().map(|c| c.path.clone()) else {
            return;
        };
        let _ = std::fs::write(path.with_extension("exit"), "0");
        self.as_mut().poll_compose();
    }

    fn undo_send(mut self: Pin<&mut Self>) {
        if self.pending_raw.is_none() {
            return;
        }
        let generation = self.send_generation + 1;
        self.as_mut().rust_mut().send_generation = generation;
        self.as_mut().rust_mut().pending_raw = None;
        self.as_mut().set_send_countdown(0);
        self.as_mut().poll_review_again();
        self.toast("send-undone", 0, "", false);
    }

    fn flush_send(mut self: Pin<&mut Self>) {
        let generation = self.send_generation + 1;
        self.as_mut().rust_mut().send_generation = generation;
        self.as_mut().set_send_countdown(0);
        self.really_send(true);
    }

    fn save_draft(mut self: Pin<&mut Self>) {
        let built = self.build_raw();
        let (raw, thread_id) = match built {
            Ok(r) => r,
            // Unsendable drafts (no recipient yet) stay as the local file.
            Err(_) => {
                if let Some(c) = self.as_mut().rust_mut().compose.take() {
                    let kept = c.path.with_extension("kept.md");
                    let _ = std::fs::rename(&c.path, &kept);
                    self.as_mut().set_review(QString::default());
                    self.as_mut()
                        .toast("kept-local", 0, &kept.display().to_string(), false);
                }
                return;
            }
        };
        // Close the compose UI now, but keep the file until the upload works.
        let path = self.as_mut().rust_mut().compose.take().map(|c| c.path);
        self.as_mut().set_review(QString::default());
        self.as_mut().set_composing(false);
        self.spawn(async move {
            let result: Result<()> = async {
                let api = Session::new()?.gmail().await?;
                api.create_draft(&raw, thread_id.as_deref()).await?;
                Ok(())
            }
            .await;
            Box::new(move |mut b: Pin<&mut qobject::Backend>| match result {
                Ok(()) => {
                    if let Some(path) = &path {
                        let _ = std::fs::remove_file(path);
                    }
                    b.as_mut().toast("draft-saved", 0, "", false);
                }
                Err(e) => {
                    // One toast slot: carry the kept path in the error itself.
                    let e = match &path {
                        Some(path) => {
                            let kept = path.with_extension("kept.md");
                            let _ = std::fs::rename(path, &kept);
                            e.context(format!("draft kept at {}", kept.display()))
                        }
                        None => e,
                    };
                    b.fail(e);
                }
            }) as Apply
        });
    }

    fn discard_draft(mut self: Pin<&mut Self>) {
        self.as_mut().finish_compose();
        self.toast("discarded", 0, "", false);
    }
}
