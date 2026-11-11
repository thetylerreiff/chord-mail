//! `chord-mail daemon`: the background service, run by systemd as a user
//! unit. It keeps the cache current while the window is closed, brings
//! snoozed mail back, sends scheduled mail, and posts notifications.

use std::time::Duration;

use anyhow::Result;
use chord_core::Store;
use chord_core::config::Config;
use chord_core::paths;
use chord_core::service::{self, Notice, Session};

pub async fn run() -> Result<()> {
    let mut waiting_logged = false;
    let mut failures: u32 = 0;
    loop {
        // Re-read each round so config edits apply without a restart.
        let config = Config::load().unwrap_or_default();
        let interval = Duration::from_secs(config.sync.interval.max(15));
        if !service::signed_in() {
            if !waiting_logged {
                eprintln!("chord: waiting for `chord-mail login`");
                waiting_logged = true;
            }
            tokio::time::sleep(interval).await;
            continue;
        }
        waiting_logged = false;
        // A bug in one round is reported and retried, not fatal.
        let outcome =
            futures::FutureExt::catch_unwind(std::panic::AssertUnwindSafe(round(&config)))
                .await
                .unwrap_or_else(|panic| {
                    Err(anyhow::anyhow!(
                        "Chord hit a bug: {}",
                        panic_message(&panic)
                    ))
                });
        match outcome {
            Ok(notices) => {
                failures = 0;
                for n in notices {
                    notify(n);
                }
            }
            Err(e) => {
                failures += 1;
                // Offline is normal for a laptop; say so once, then back off.
                if failures == 1 || failures.is_multiple_of(30) {
                    eprintln!("chord: sync failed: {e:#}");
                }
            }
        }
        let backoff = interval * failures.clamp(1, 5);
        tokio::time::sleep(backoff).await;
    }
}

async fn round(config: &Config) -> Result<Vec<Notice>> {
    let api = Session::new()?.gmail().await?;
    let _lock = service::lock().await?;
    let mut store = Store::open(&paths::database())?;
    Ok(service::tick(&api, &mut store, config).await?.notices)
}

/// Posts a notification; clicking it opens the conversation.
pub fn notify(n: Notice) {
    std::thread::spawn(move || {
        let output = std::process::Command::new("notify-send")
            .args([
                "--app-name=Chord",
                "--icon=chord-mail",
                "--action=open=Open",
                "--wait",
            ])
            .arg(&n.title)
            .arg(&n.body)
            .output();
        if let Ok(out) = output
            && String::from_utf8_lossy(&out.stdout).trim() == "open"
        {
            open_window(&n.thread_id);
        }
    });
}

/// Opens the window on a conversation. A child of the service would live in
/// its cgroup and die whenever the service restarts (every update does), so
/// it gets a scope of its own when systemd-run is there.
fn open_window(thread_id: &str) {
    let exe = own_exe();
    let args = ["open", "--thread", thread_id];
    let scoped = std::process::Command::new("systemd-run")
        .args(["--user", "--scope", "--collect", "--quiet", "--"])
        .arg(&exe)
        .args(args)
        .spawn();
    if scoped.is_err() {
        let _ = std::process::Command::new(&exe).args(args).spawn();
    }
}

/// This binary's path. After an upgrade replaces it under the running
/// service, Linux reports it as "/usr/bin/chord-mail (deleted)"; the new one
/// is at the same path without the suffix.
fn own_exe() -> std::path::PathBuf {
    use std::os::unix::ffi::OsStrExt;
    std::env::current_exe()
        .ok()
        .map(|p| {
            let bytes = p.as_os_str().as_bytes();
            let live = bytes.strip_suffix(b" (deleted)").unwrap_or(bytes);
            std::path::PathBuf::from(std::ffi::OsStr::from_bytes(live))
        })
        .filter(|p| p.exists())
        .unwrap_or_else(|| "chord-mail".into())
}

/// The text of a caught panic, for reporting it.
pub fn panic_message(panic: &Box<dyn std::any::Any + Send>) -> String {
    panic
        .downcast_ref::<&str>()
        .map(|s| s.to_string())
        .or_else(|| panic.downcast_ref::<String>().cloned())
        .unwrap_or_else(|| "unknown panic".into())
}
