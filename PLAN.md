# Chord v1 plan

Chord is a keyboard-driven Gmail client for Omarchy: fast, focused, and
local-first. This is the plan for v1, decided on 2026-11-07.

## Decisions

| Area | Decision |
|---|---|
| Layout | Single column: the list *or* a thread, full width. Enter opens, Esc returns. |
| Language | Rust everywhere. The UI is QML via `cxx-qt`; there is no C++ of our own. |
| Mail API | Gmail REST API with OAuth (PKCE, loopback). `gmail.modify` scope. |
| Cache | SQLite (WAL). The UI only ever reads the cache, so it never waits on the network. |
| Actions | Optimistic: applied to the cache at once, queued, pushed to Gmail in the background, and retried. Every action can be undone. |
| Split inbox | Tabs from Gmail categories: Primary, Updates (updates + forums), Promotions (promotions + social). Configurable in `config.toml`. |
| Reading | One WebEngineView for the open message, JavaScript off, a strict CSP, links open in the browser. Earlier messages in the thread are collapsed rows. |
| Images | Remote images blocked; `i` loads them once, `I` always allows that sender. Attached (cid:) images always show. |
| Dark mail | Chromium force-dark on by default; `D` shows the current message's original colors. |
| Composer | Neovim in a floating terminal (`TUI.float`) on a Markdown file with headers. Chord then shows a review screen, sends HTML plus a plain-text copy, appends the Gmail signature and quote, and holds the message 10 s so you can undo. |
| Snooze | `h` with presets or typed times ("2h", "tom 9am", "fri"). Uses a `Chord/Snoozed` Gmail label plus a local schedule; woken threads rise to the top. |
| Reminders | "Remind me if no reply": a sent thread comes back at the chosen time unless someone answered. |
| Send later | Saved as a Gmail draft, sent on schedule by the service. |
| Service | `chord-mail daemon` as a systemd user service: syncs every 60 s, flushes the action queue, wakes snoozes, sends scheduled mail, and notifies for new Primary mail only. |
| Default email | Chord handles `mailto:`; `chord-mail default` makes it the system's default email app, and Super+Shift+E / Super+Shift+Alt+E follow whatever app that is. |
| Theme | Follows the Omarchy theme live (full palette, Hyprland rounding, Omarchy font for the interface). Inbox zero shows the theme's wallpaper. |
| i18n | Every string through `qsTr`, with the Omarchy language loader in place. English only for v1. |
| Multi-account | Out of v1; everything is per-account so work mail can be added later. |
| AI | Out of v1. The `--json` CLI is how agents drive Chord. |

## Keys

Gmail-compatible, so habits carry over.

| Key | List | Thread |
|---|---|---|
| `j` / `k` | Next / previous conversation | Next / previous conversation |
| `Enter` / `o` | Open | Expand the selected message |
| `Esc` | Clear selection or search | Back to the list |
| `n` / `p` | | Next / previous message in the thread |
| `Space` / `Shift+Space` | Open | Page down / up |
| `Tab` / `Shift+Tab` | Next / previous split | |
| `e` | Done (archive) | Done, then open the next one |
| `Shift+E` | Move back to the inbox | Move back to the inbox |
| `#` | Trash | Trash |
| `!` | Spam | Spam |
| `s` | Star | Star |
| `Shift+U` | Toggle unread | Mark unread and go back |
| `h` | Snooze | Snooze |
| `l` / `v` | Label / move to label | Label / move to label |
| `x` | Select | |
| `r` / `a` / `f` | | Reply / reply all / forward |
| `c` | Compose | Compose |
| `/` | Search | Search |
| `g` then `i` `s` `t` `d` `h` `a` | Inbox, starred, sent, drafts, snoozed, all mail | |
| `i` / `I` / `D` | | Images once / always for sender / original colors |
| `z`, `Ctrl+Z` | Undo | Undo |
| `Ctrl+K` | Command palette | Command palette |
| `?` | All keys | All keys |
| `q` | Quit | Quit |

## Architecture

```text
core/  chord-core: everything that isn't a window
  auth      OAuth sign-in and refresh
  gmail     REST client behind the MailApi trait
  store     SQLite: messages, labels, bodies, ops, snoozes, schedule, contacts
  sync      full + history sync, body prefetch
  ops       the optimistic action queue: apply locally, flush, undo
  splits    split-inbox rules from config.toml
  render    MIME → safe HTML for the reader (CSP, cid inlining, plain text)
  compose   draft file ⇄ headers + Markdown → MIME (reply/forward threading)
  when      typed snooze times ("tom 9am") → timestamps
  service   one tick of the daemon's work, shared with the window
app/   chord-mail: the binary
  cli       login, logout, sync, inbox, search, compose, open, daemon, default
  gui       cxx-qt bridge (Backend, Theme), the single-instance socket
  qml/      Main.qml and its components
pkgbuild/  PKGBUILD, .desktop (mailto handler), icon, systemd user unit
```

The window and the daemon are separate processes that share the database.
Each task opens its own SQLite connection (WAL plus a busy timeout), and sync
and queue flushing take a file lock, so the two never run the same work at
once.

## Build order

1. Core: schema v2, Gmail write calls, body fetch, ops queue, splits, render,
   compose, snooze times, service tick. Each one tested against the fake
   mailbox.
2. CLI: `search`, `compose`, `daemon`, `default`, `open`.
3. cxx-qt bridge, then the window: list, splits, thread reader, actions, undo,
   toasts, search, labels, snooze, palette, help, composer flow.
4. Theme (live) and the language loader.
5. Package: PKGBUILD, desktop entry, icon, user service; `bin/install`.
6. Offscreen screenshots against a demo mailbox to check the look.
7. Install, make Chord the default email app and wire up the keys, enable the
   service, sign in, sync, launch.
