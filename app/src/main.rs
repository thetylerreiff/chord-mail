//! `chord-mail`: keyboard-driven mail for Omarchy.
//!
//! `chord-mail open` is the window. Every other command works without a
//! display, prints JSON with `--json`, and exits 0 or 1 with errors on stderr,
//! so scripts and agents can drive Chord too.

mod daemon;
mod gui;
mod instance;

use std::process::ExitCode;

use anyhow::{Result, bail};
use chord_core::config::Config;
use chord_core::ops::{self, Action};
use chord_core::service::{self, Session};
use chord_core::{MailApi, Store, auth, paths, render, sync, text};
use clap::{Parser, Subcommand, ValueEnum};

use instance::Request;

#[derive(Parser)]
#[command(
    name = "chord-mail",
    version,
    about = "Keyboard-driven mail for Omarchy"
)]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Subcommand)]
enum Command {
    /// Open the window (or bring the running one forward).
    Open {
        /// Open this conversation.
        #[arg(long)]
        thread: Option<String>,
        /// Start a new message, optionally from a mailto: link.
        #[arg(long, num_args = 0..=1, default_missing_value = "")]
        compose: Option<String>,
        /// A mailto: link, as the desktop passes it.
        link: Option<String>,
    },
    /// Write a new message, in the running window if there is one.
    Compose {
        /// A mailto: link or an address.
        to: Option<String>,
    },
    /// Sign in to Gmail in the browser.
    Login,
    /// Forget the saved sign-in. The local cache stays.
    Logout,
    /// Push queued actions and fetch what changed.
    Sync {
        #[arg(long)]
        json: bool,
    },
    /// List the inbox from the local cache, newest first.
    Inbox {
        /// Only this split (Primary, Updates, …).
        #[arg(long)]
        split: Option<String>,
        #[arg(long, default_value_t = 25)]
        limit: usize,
        #[arg(long)]
        json: bool,
    },
    /// Search all mail with Gmail's search syntax.
    Search {
        query: String,
        #[arg(long, default_value_t = 25)]
        limit: u32,
        #[arg(long)]
        json: bool,
    },
    /// Print a conversation as text.
    Show {
        thread: String,
        #[arg(long)]
        json: bool,
    },
    /// Archive, trash, star, or mark conversations.
    Act { action: Act, threads: Vec<String> },
    /// Run the background service: sync, snoozes, scheduled mail, notifications.
    Daemon,
    /// Make Chord the default email app.
    Default,
    /// Install the agent skill that teaches assistants these commands.
    Skill {
        #[arg(value_parser = ["install"])]
        action: String,
    },
}

#[derive(Clone, Copy, ValueEnum)]
enum Act {
    Archive,
    Inbox,
    Trash,
    Spam,
    Star,
    Unstar,
    Read,
    Unread,
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    // The window runs Qt's event loop on the main thread, outside tokio.
    let command = cli.command.unwrap_or(Command::Open {
        thread: None,
        compose: None,
        link: None,
    });
    match command {
        Command::Open {
            thread,
            compose,
            link,
        } => {
            let request = match (thread, compose, link) {
                (Some(t), _, _) => Request::Thread(t),
                (_, Some(c), _) => Request::Compose(c),
                (_, _, Some(l)) if l.starts_with("mailto:") => Request::Compose(l),
                _ => Request::Focus,
            };
            if instance::send(&request) {
                focus_window();
                return ExitCode::SUCCESS;
            }
            ExitCode::from(gui::run(request).clamp(0, 255) as u8)
        }
        Command::Compose { to } => {
            let link = match to {
                Some(t) if t.starts_with("mailto:") => t,
                Some(t) => format!("mailto:{t}"),
                None => String::new(),
            };
            if instance::send(&Request::Compose(link.clone())) {
                return ExitCode::SUCCESS;
            }
            ExitCode::from(gui::run(Request::Compose(link)).clamp(0, 255) as u8)
        }
        command => {
            let runtime = tokio::runtime::Runtime::new().expect("tokio runtime");
            match runtime.block_on(run(command)) {
                Ok(()) => ExitCode::SUCCESS,
                Err(e) => {
                    eprintln!("chord-mail: {e:#}");
                    ExitCode::FAILURE
                }
            }
        }
    }
}

/// Brings the running window forward through Hyprland, with Omarchy's
/// helper when it's there since it speaks every Hyprland dispatcher syntax.
pub fn focus_window() {
    let quiet = |cmd: &mut std::process::Command| {
        cmd.stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .is_ok_and(|s| s.success())
    };
    if !quiet(std::process::Command::new("omarchy-hyprland-focus-app").arg("^chord-mail$")) {
        quiet(std::process::Command::new("hyprctl").args([
            "dispatch",
            "focuswindow",
            "class:^(chord-mail)$",
        ]));
    }
}

fn print_json(value: &impl serde::Serialize) -> Result<()> {
    println!("{}", serde_json::to_string(value)?);
    Ok(())
}

async fn run(command: Command) -> Result<()> {
    match command {
        Command::Open { .. } | Command::Compose { .. } => unreachable!("handled in main"),
        Command::Login => {
            let session = Session::new()?;
            let token = auth::login(session.http(), session.client(), |url| {
                eprintln!("Opening Google sign-in in your browser. If it doesn't open, visit:\n\n  {url}\n");
                let _ = std::process::Command::new("xdg-open")
                    .arg(url)
                    .stdout(std::process::Stdio::null())
                    .stderr(std::process::Stdio::null())
                    .spawn();
            })
            .await?;
            token.save(&paths::token())?;
            let profile = session.gmail().await?.profile().await?;
            let store = Store::open(&paths::database())?;
            let previous = store.account()?;
            if !previous.is_empty() && !previous.eq_ignore_ascii_case(&profile.email_address) {
                // A different account: the cache belongs to the old one.
                drop(store);
                std::fs::remove_file(paths::database())?;
            }
            Store::open(&paths::database())?.set_meta("email", &profile.email_address)?;
            println!("Signed in as {}", profile.email_address);
        }
        Command::Logout => match std::fs::remove_file(paths::token()) {
            Ok(()) => println!("Signed out"),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => println!("Not signed in"),
            Err(e) => return Err(e.into()),
        },
        Command::Sync { json } => {
            let config = Config::load()?;
            let api = Session::new()?.gmail().await?;
            let _lock = service::lock().await?;
            let mut store = Store::open(&paths::database())?;
            let report = service::tick(&api, &mut store, &config).await?;
            if json {
                print_json(&report)?;
            } else {
                let s = report.sync.as_ref();
                println!(
                    "{} sync: {} fetched, {} removed, {} actions pushed, {} cached",
                    match s.map(|s| s.kind) {
                        Some(chord_core::SyncKind::Full) => "Full",
                        _ => "Incremental",
                    },
                    s.map_or(0, |s| s.fetched),
                    s.map_or(0, |s| s.removed),
                    report.flushed,
                    store.count()?
                );
            }
        }
        Command::Inbox { split, limit, json } => {
            let config = Config::load()?;
            let store = Store::open(&paths::database())?;
            let wanted = match &split {
                Some(name) => match config
                    .split
                    .iter()
                    .position(|s| s.name.eq_ignore_ascii_case(name))
                {
                    Some(i) => Some(i),
                    None => bail!(
                        "no split named {name}; there are {}",
                        config
                            .split
                            .iter()
                            .map(|s| s.name.as_str())
                            .collect::<Vec<_>>()
                            .join(", ")
                    ),
                },
                None => None,
            };
            let rows: Vec<_> = service::inbox(&store, &config)?
                .into_iter()
                .filter(|(i, _)| wanted.is_none_or(|w| w == *i))
                .take(limit)
                .collect();
            if json {
                let out: Vec<_> = rows
                    .iter()
                    .map(
                        |(i, r)| serde_json::json!({ "split": config.split[*i].name, "thread": r }),
                    )
                    .collect();
                print_json(&out)?;
            } else if rows.is_empty() {
                println!("Nothing here (run `chord-mail sync` if you haven't yet)");
            } else {
                for (_, r) in rows {
                    print_row(&r);
                }
            }
        }
        Command::Search { query, limit, json } => {
            let api = Session::new()?.gmail().await?;
            let ids = api.search_threads(&query, None, limit).await?;
            let mut store = Store::open(&paths::database())?;
            for id in &ids {
                sync::ensure_thread(&api, &mut store, id, 0).await?;
            }
            let rows = store.thread_rows(&ids)?;
            if json {
                print_json(&rows)?;
            } else if rows.is_empty() {
                println!("No results");
            } else {
                for r in rows {
                    print_row(&r);
                }
            }
        }
        Command::Show { thread, json } => {
            let mut store = Store::open(&paths::database())?;
            if service::signed_in() {
                let api = Session::new()?.gmail().await?;
                sync::ensure_thread(&api, &mut store, &thread, i64::MAX).await?;
            }
            let messages = store.thread_messages(&thread)?;
            if messages.is_empty() {
                bail!("no conversation {thread}");
            }
            let mut out = Vec::new();
            for m in &messages {
                let body = store
                    .body(&m.id)?
                    .and_then(|raw| render::source(&raw))
                    .map(|s| {
                        if s.text.trim().is_empty() {
                            m.snippet.clone()
                        } else {
                            s.text
                        }
                    })
                    .unwrap_or_else(|| m.snippet.clone());
                out.push(serde_json::json!({
                    "id": m.id, "from": m.from, "to": m.to, "cc": m.cc,
                    "subject": m.subject, "date": m.date, "labels": m.labels, "body": body,
                }));
            }
            if json {
                print_json(&out)?;
            } else {
                for (m, o) in messages.iter().zip(&out) {
                    println!(
                        "From: {}\nTo: {}\nSubject: {}\n\n{}\n\n----",
                        m.from,
                        m.to,
                        m.subject,
                        o["body"].as_str().unwrap_or("")
                    );
                }
            }
        }
        Command::Act { action, threads } => {
            if threads.is_empty() {
                bail!("name at least one conversation id");
            }
            let action = match action {
                Act::Archive => Action::Archive,
                Act::Inbox => Action::MoveToInbox,
                Act::Trash => Action::Trash,
                Act::Spam => Action::Spam,
                Act::Star => Action::Star,
                Act::Unstar => Action::Unstar,
                Act::Read => Action::MarkRead,
                Act::Unread => Action::MarkUnread,
            };
            let mut store = Store::open(&paths::database())?;
            ops::apply(&mut store, &threads, &action)?;
            let api = Session::new()?.gmail().await?;
            let _lock = service::lock().await?;
            ops::flush(&api, &mut store).await?;
        }
        Command::Daemon => daemon::run().await?,
        Command::Default => set_default()?,
        Command::Skill { .. } => install_skill()?,
    }
    Ok(())
}

fn print_row(r: &chord_core::ThreadRow) {
    let marker = if r.unread { "●" } else { " " };
    let who = r.senders.join(", ");
    let who = if r.count > 1 {
        format!("{who} ({})", r.count)
    } else {
        who
    };
    println!(
        "{marker} {:<16} {:<26.26}  {:<44.44}  {}",
        r.id,
        who,
        r.subject,
        text::decode_entities(&r.snippet)
            .chars()
            .take(50)
            .collect::<String>()
            .replace(['\n', '\r'], " ")
    );
}

/// Registers Chord for mailto: links, which is what makes it the system's
/// email app; the Omarchy keybindings follow whichever app that is.
fn set_default() -> Result<()> {
    let status = std::process::Command::new("xdg-mime")
        .args(["default", "chord-mail.desktop", "x-scheme-handler/mailto"])
        .env("XDG_CURRENT_DESKTOP", "")
        .status()?;
    if !status.success() {
        bail!("xdg-mime couldn't set the mailto handler");
    }
    // Hyprland sessions also read a desktop-specific list first.
    let config = std::env::var_os("XDG_CONFIG_HOME")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| {
            std::path::PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(".config")
        });
    let hypr_list = config.join("hyprland-mimeapps.list");
    if let Ok(text) = std::fs::read_to_string(&hypr_list)
        && text.contains("x-scheme-handler/mailto=")
    {
        let updated: String = text
            .lines()
            .map(|l| {
                if l.starts_with("x-scheme-handler/mailto=") {
                    "x-scheme-handler/mailto=chord-mail.desktop".to_string()
                } else {
                    l.to_string()
                }
            })
            .collect::<Vec<_>>()
            .join("\n")
            + "\n";
        let tmp = hypr_list.with_extension("list.tmp");
        std::fs::write(&tmp, updated)?;
        std::fs::rename(tmp, &hypr_list)?;
    }
    println!("Chord is now the default email app (mailto: links open it).");
    Ok(())
}

const SKILL: &str = include_str!("../skill/SKILL.md");

fn install_skill() -> Result<()> {
    let dir = std::path::PathBuf::from(std::env::var_os("HOME").unwrap_or_default())
        .join(".claude/skills/chord-mail");
    std::fs::create_dir_all(&dir)?;
    std::fs::write(dir.join("SKILL.md"), SKILL)?;
    println!("Installed {}", dir.join("SKILL.md").display());
    Ok(())
}
