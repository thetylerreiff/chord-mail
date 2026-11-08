---
name: chord-mail
description: Read, search, triage, and summarize the user's Gmail through Chord's CLI (chord-mail). Use when asked about their email, inbox, a conversation, or to archive, star, or mark mail.
---

# Chord mail

Chord is the user's mail client. Its CLI reads from a local cache that the
background service keeps current, so reads are instant and work offline.
Every command takes `--json` where it prints data and exits 1 with the error
on stderr.

| Command | What it does |
|---|---|
| `chord-mail inbox [--split Primary] [--limit 25] --json` | Inbox conversations, newest first, with their split |
| `chord-mail search "<gmail query>" --json` | Search all mail with Gmail syntax (`from:ada after:2026/11/01 has:attachment`) |
| `chord-mail show <thread-id> --json` | Every message in a conversation, with plain-text bodies |
| `chord-mail act <archive\|inbox\|trash\|spam\|star\|unstar\|read\|unread> <thread-id>...` | Change conversations; the window shows it at once |
| `chord-mail sync --json` | Pull new mail now |
| `chord-mail compose [mailto:...]` | Open a new message for the user to write in Neovim |

Rules:
- Never trash, spam, or archive without the user asking for that action.
- Chord can't send mail from the CLI on purpose: drafting is fine, but the
  user reviews and sends from the window.
- Thread ids come from `inbox` or `search` output (`id` field).
