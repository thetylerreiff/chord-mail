//! Writing mail: the draft file Neovim edits, and turning it into MIME.
//!
//! A draft file is a few headers, a blank line, then the body in Markdown:
//!
//! ```text
//! # comment lines are ignored
//! To: Ada Lovelace <ada@example.com>
//! Cc:
//! Subject: Re: Engine notes
//! Attach: ~/notes.pdf
//!
//! Sounds great. **Thursday** works.
//! ```
//!
//! Replies quote the original and forwards carry it with its attachments;
//! both happen when the message is built, so the file stays short.

use std::path::PathBuf;

use anyhow::{Context, Result, bail};
use mail_builder::MessageBuilder;
use mail_builder::headers::address::Address;
use pulldown_cmark::{Event, Options, Parser};
use serde::Serialize;

use crate::render::Source;
use crate::text::{address_of, base_subject, escape_html, sender_name, split_addresses};

#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    New,
    Reply,
    ReplyAll,
    Forward,
}

/// Who mail is sent as.
#[derive(Debug, Clone, Default)]
pub struct Identity {
    pub email: String,
    pub name: String,
    /// Gmail's signature, as HTML.
    pub signature: String,
}

/// Fields a new message can start with, from a mailto: link or the CLI.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Prefill {
    pub to: String,
    pub cc: String,
    pub bcc: String,
    pub subject: String,
    pub body: String,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct Draft {
    pub to: Vec<String>,
    pub cc: Vec<String>,
    pub bcc: Vec<String>,
    pub subject: String,
    pub attach: Vec<PathBuf>,
    pub body: String,
}

impl Draft {
    pub fn recipients(&self) -> impl Iterator<Item = &String> {
        self.to.iter().chain(&self.cc).chain(&self.bcc)
    }

    /// Recipients that aren't a usable address yet.
    pub fn invalid_recipients(&self) -> Vec<String> {
        self.recipients()
            .filter(|r| !is_address(&address_of(r)))
            .cloned()
            .collect()
    }

    pub fn missing_attachments(&self) -> Vec<PathBuf> {
        self.attach
            .iter()
            .filter(|p| !p.is_file())
            .cloned()
            .collect()
    }
}

pub fn is_address(a: &str) -> bool {
    match a.split_once('@') {
        Some((user, domain)) => {
            !user.is_empty()
                && domain.contains('.')
                && !domain.starts_with('.')
                && !domain.ends_with('.')
                && !a.contains(char::is_whitespace)
        }
        None => false,
    }
}

const HINT: &str = "# Chord: write below the blank line in Markdown. Save and quit (:wq) to\n\
# review before sending, or quit without saving (:cq) to throw it away.\n\
# Attach files as a comma-separated list: Attach: ~/a.pdf, ~/b.png\n";

/// The draft file for a new message, reply, or forward, and the line the
/// cursor should start on.
pub fn template(
    kind: Kind,
    original: Option<&Source>,
    me: &Identity,
    prefill: &Prefill,
) -> (String, usize) {
    let me_addr = me.email.to_lowercase();
    let not_me = |entry: &String| address_of(entry).to_lowercase() != me_addr;
    // A line break in a header value or comment would start a new line.
    let one_line = |s: &str| s.replace(['\r', '\n'], " ");
    let (to, cc, subject, note) = match (kind, original) {
        (Kind::Reply | Kind::ReplyAll, Some(o)) => {
            // Replying to your own message means continuing to its recipients.
            let from_me = address_of(&o.from).to_lowercase() == me_addr;
            let primary = if from_me {
                o.to.clone()
            } else if !o.reply_to.is_empty() {
                o.reply_to.clone()
            } else {
                o.from.clone()
            };
            let mut to: Vec<String> = split_addresses(&primary);
            let mut cc: Vec<String> = Vec::new();
            if kind == Kind::ReplyAll {
                let primary_addrs: Vec<String> =
                    to.iter().map(|t| address_of(t).to_lowercase()).collect();
                let others = split_addresses(&o.to)
                    .into_iter()
                    .chain(split_addresses(&o.cc))
                    .filter(not_me)
                    .filter(|e| !primary_addrs.contains(&address_of(e).to_lowercase()));
                for e in others {
                    if !cc
                        .iter()
                        .any(|c| address_of(c).eq_ignore_ascii_case(&address_of(&e)))
                    {
                        cc.push(e);
                    }
                }
            }
            if to.is_empty() {
                to = split_addresses(&o.from);
            }
            let subject = format!("Re: {}", base_subject(&o.subject));
            let note = format!(
                "# Replying to {}. Their message is quoted below yours when it's sent.\n",
                one_line(&sender_name(&o.from))
            );
            (to.join(", "), cc.join(", "), subject, note)
        }
        (Kind::Forward, Some(o)) => {
            let n = o.attachments.len();
            let subject = one_line(&o.subject);
            let note = match n {
                0 => format!(
                    "# Forwarding \"{}\"; the original goes below your message.\n",
                    subject
                ),
                1 => format!("# Forwarding \"{}\" with its attachment.\n", subject),
                n => format!("# Forwarding \"{}\" with its {n} attachments.\n", subject),
            };
            (
                prefill.to.clone(),
                String::new(),
                format!("Fwd: {}", base_subject(&o.subject)),
                note,
            )
        }
        _ => (
            prefill.to.clone(),
            prefill.cc.clone(),
            prefill.subject.clone(),
            String::new(),
        ),
    };
    let head = format!(
        "{HINT}{note}To: {}\nCc: {}\nBcc: {}\nSubject: {}\nAttach: \n\n",
        one_line(&to),
        one_line(&cc),
        one_line(&prefill.bcc),
        one_line(&subject)
    );
    let line = head.lines().count() + 1;
    (format!("{head}{}", prefill.body), line)
}

/// Reads a draft file back.
pub fn parse(text: &str) -> Draft {
    let mut draft = Draft::default();
    let mut lines = text.lines();
    for line in lines.by_ref() {
        if line.trim().is_empty() {
            break;
        }
        if line.starts_with('#') {
            continue;
        }
        let Some((key, value)) = line.split_once(':') else {
            continue;
        };
        let value = value.trim();
        match key.trim().to_ascii_lowercase().as_str() {
            "to" => draft.to = split_addresses(value),
            "cc" => draft.cc = split_addresses(value),
            "bcc" => draft.bcc = split_addresses(value),
            "subject" => draft.subject = value.to_string(),
            "attach" | "attachments" => {
                draft.attach = value
                    .split(',')
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .map(expand_home)
                    .collect()
            }
            _ => {}
        }
    }
    draft.body = lines.collect::<Vec<_>>().join("\n").trim_end().to_string();
    draft
}

fn expand_home(path: &str) -> PathBuf {
    match path.strip_prefix("~/") {
        Some(rest) => PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(rest),
        None => PathBuf::from(path),
    }
}

/// Markdown to HTML, with single line breaks kept as breaks, since mail is
/// written line by line rather than in paragraphs.
pub fn markdown_html(md: &str) -> String {
    let mut opts = Options::empty();
    opts.insert(Options::ENABLE_TABLES);
    opts.insert(Options::ENABLE_STRIKETHROUGH);
    opts.insert(Options::ENABLE_TASKLISTS);
    let events = Parser::new_ext(md, opts).map(|e| match e {
        Event::SoftBreak => Event::HardBreak,
        // Raw HTML in a draft is shown as written, never interpreted.
        Event::Html(h) | Event::InlineHtml(h) => Event::Text(h),
        e => e,
    });
    let mut out = String::new();
    pulldown_cmark::html::push_html(&mut out, events);
    out
}

fn attribution(o: &Source) -> String {
    let who = sender_name(&o.from);
    let addr = address_of(&o.from);
    if o.date.is_empty() {
        format!("{who} <{addr}> wrote:")
    } else {
        format!("On {}, {who} <{addr}> wrote:", o.date)
    }
}

/// The HTML part as it will be sent.
pub fn html_body(draft: &Draft, kind: Kind, original: Option<&Source>, me: &Identity) -> String {
    let mut html = format!("<div dir=\"ltr\">{}</div>", markdown_html(&draft.body));
    if !me.signature.trim().is_empty() {
        html.push_str(&format!(
            "<br><div dir=\"ltr\" class=\"gmail_signature\">{}</div>",
            me.signature
        ));
    }
    match (kind, original) {
        (Kind::Reply | Kind::ReplyAll, Some(o)) => html.push_str(&format!(
            "<br><div class=\"gmail_quote\"><div dir=\"ltr\" class=\"gmail_attr\">{}<br></div>\
             <blockquote class=\"gmail_quote\" style=\"margin:0 0 0 .8ex;border-left:1px #ccc solid;padding-left:1ex\">{}</blockquote></div>",
            escape_html(&attribution(o)),
            o.html
        )),
        (Kind::Forward, Some(o)) => html.push_str(&format!(
            "<br><div class=\"gmail_quote\"><div dir=\"ltr\" class=\"gmail_attr\">---------- Forwarded message ---------<br>\
             From: {}<br>Date: {}<br>Subject: {}<br>To: {}<br>{}</div><br>{}</div>",
            escape_html(&o.from),
            escape_html(&o.date),
            escape_html(&o.subject),
            escape_html(&o.to),
            if o.cc.is_empty() { String::new() } else { format!("Cc: {}<br>", escape_html(&o.cc)) },
            o.html
        )),
        _ => {}
    }
    html
}

/// The plain-text part as it will be sent.
pub fn text_body(draft: &Draft, kind: Kind, original: Option<&Source>) -> String {
    let mut text = draft.body.clone();
    match (kind, original) {
        (Kind::Reply | Kind::ReplyAll, Some(o)) => {
            text.push_str(&format!("\n\n{}\n", attribution(o)));
            for line in o.text.lines() {
                text.push_str(if line.is_empty() { ">" } else { "> " });
                text.push_str(line);
                text.push('\n');
            }
        }
        (Kind::Forward, Some(o)) => text.push_str(&format!(
            "\n\n---------- Forwarded message ---------\nFrom: {}\nDate: {}\nSubject: {}\nTo: {}\n\n{}",
            o.from, o.date, o.subject, o.to, o.text
        )),
        _ => {}
    }
    text
}

fn addresses(list: &[String]) -> Address<'static> {
    Address::new_list(
        list.iter()
            .map(|entry| {
                let addr = address_of(entry);
                let name = sender_name(entry);
                let name = (!name.eq_ignore_ascii_case(&addr) && !name.is_empty()).then_some(name);
                Address::new_address(name, addr)
            })
            .collect(),
    )
}

/// Builds the RFC 822 message to hand to Gmail.
pub fn build(
    draft: &Draft,
    kind: Kind,
    original: Option<&Source>,
    me: &Identity,
) -> Result<Vec<u8>> {
    if draft.to.is_empty() && draft.cc.is_empty() && draft.bcc.is_empty() {
        bail!("add at least one recipient");
    }
    let invalid = draft.invalid_recipients();
    if !invalid.is_empty() {
        bail!("not an email address: {}", invalid.join(", "));
    }
    let from = if me.name.is_empty() {
        Address::new_address(None::<String>, me.email.clone())
    } else {
        Address::new_address(Some(me.name.clone()), me.email.clone())
    };
    let domain = me
        .email
        .rsplit('@')
        .next()
        .unwrap_or("localhost")
        .to_string();
    // A Message-ID of our own keeps this machine's hostname out of the headers.
    let id: String = (0..20)
        .map(|_| {
            char::from(b"abcdefghijklmnopqrstuvwxyz0123456789"[rand::random::<u8>() as usize % 36])
        })
        .collect();
    let mut builder = MessageBuilder::new()
        .from(from)
        .subject(draft.subject.clone())
        .message_id(format!("chord.{id}@{domain}"))
        .text_body(text_body(draft, kind, original))
        .html_body(html_body(draft, kind, original, me));
    if !draft.to.is_empty() {
        builder = builder.to(addresses(&draft.to));
    }
    if !draft.cc.is_empty() {
        builder = builder.cc(addresses(&draft.cc));
    }
    if !draft.bcc.is_empty() {
        builder = builder.bcc(addresses(&draft.bcc));
    }
    if let (Kind::Reply | Kind::ReplyAll, Some(o)) = (kind, original) {
        let parent = o
            .message_id
            .trim_matches(|c| c == '<' || c == '>')
            .to_string();
        if !parent.is_empty() {
            let mut refs: Vec<String> = o
                .references
                .split_whitespace()
                .map(|r| r.trim_matches(|c| c == '<' || c == '>').to_string())
                .filter(|r| !r.is_empty())
                .collect();
            refs.push(parent.clone());
            builder = builder.in_reply_to(parent).references(refs);
        }
    }
    if let (Kind::Forward, Some(o)) = (kind, original) {
        for (name, mime, bytes) in &o.attachments {
            builder = builder.attachment(mime.clone(), name.clone(), bytes.clone());
        }
    }
    for path in &draft.attach {
        let bytes =
            std::fs::read(path).with_context(|| format!("can't attach {}", path.display()))?;
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| "attachment".into());
        let mime = mime_guess::from_path(path)
            .first_or_octet_stream()
            .to_string();
        builder = builder.attachment(mime, name, bytes);
    }
    Ok(builder.write_to_vec()?)
}

/// Reads a `mailto:` link, including `?subject=`, `cc=`, `bcc=`, and `body=`.
pub fn mailto(link: &str) -> Prefill {
    let rest = link.strip_prefix("mailto:").unwrap_or(link);
    let (to, query) = rest.split_once('?').unwrap_or((rest, ""));
    let decode = |s: &str| {
        url::form_urlencoded::parse(format!("x={}", s.replace('+', "%2B")).as_bytes())
            .next()
            .map(|(_, v)| v.into_owned())
            .unwrap_or_default()
    };
    let mut p = Prefill {
        to: decode(to),
        ..Default::default()
    };
    for (k, v) in url::form_urlencoded::parse(query.replace('+', "%2B").as_bytes()) {
        let v = v.into_owned();
        match k.to_ascii_lowercase().as_str() {
            "to" => {
                p.to = [p.to.clone(), v]
                    .iter()
                    .filter(|s| !s.is_empty())
                    .cloned()
                    .collect::<Vec<_>>()
                    .join(", ")
            }
            "cc" => p.cc = v,
            "bcc" => p.bcc = v,
            "subject" => p.subject = v,
            "body" => p.body = v,
            _ => {}
        }
    }
    p
}

#[cfg(test)]
mod tests {
    use super::*;

    fn me() -> Identity {
        Identity {
            email: "me@example.com".into(),
            name: "Tyler".into(),
            signature: "<b>T</b>".into(),
        }
    }

    fn original() -> Source {
        Source {
            from: "\"Ada Lovelace\" <ada@example.com>".into(),
            to: "me@example.com, Grace <grace@example.com>".into(),
            cc: "bob@example.com".into(),
            subject: "Re: Engine notes".into(),
            date: "Mon, Nov 2, 2026 at 9:00 AM".into(),
            message_id: "<m2@example.com>".into(),
            references: "<m1@example.com>".into(),
            html: "<p>Original</p>".into(),
            text: "Original\n\nline".into(),
            attachments: vec![("a.txt".into(), "text/plain".into(), b"hi".to_vec())],
            ..Default::default()
        }
    }

    #[test]
    fn mailto_cannot_inject_headers() {
        let prefill = mailto("mailto:a@b.c?subject=Hello%0ABcc:%20x@y.z");
        let (text, _) = template(Kind::New, None, &me(), &prefill);
        let d = parse(&text);
        assert!(d.bcc.is_empty());
        assert_eq!(d.subject, "Hello Bcc: x@y.z");
    }

    #[test]
    fn received_subject_cannot_inject_headers_through_notes() {
        let evil = "hello\nTo: attacker@example.com\nAttach: ~/.ssh/id_rsa\n\n";
        for n in 0..3 {
            let mut o = original();
            o.subject = evil.into();
            o.from = format!("\"Ada{}\" <ada@example.com>", evil.replace('"', ""));
            o.attachments = (0..n)
                .map(|_| ("a".into(), "text/plain".into(), vec![]))
                .collect();
            for kind in [Kind::Forward, Kind::Reply] {
                let (text, _) = template(kind, Some(&o), &me(), &Prefill::default());
                let d = parse(&text);
                let head = text.split("\n\n").next().unwrap_or_default();
                let count = |k: &str| head.lines().filter(|l| l.starts_with(k)).count();
                assert_eq!((count("To:"), count("Attach:")), (1, 1), "{kind:?} {n}");
                assert!(d.attach.is_empty(), "{kind:?} {n}");
                if kind == Kind::Forward {
                    assert!(d.to.is_empty(), "{n}");
                }
            }
        }
    }

    #[test]
    fn reply_all_addresses_everyone_but_me() {
        let (text, line) = template(
            Kind::ReplyAll,
            Some(&original()),
            &me(),
            &Prefill::default(),
        );
        let d = parse(&text);
        assert_eq!(d.to, ["\"Ada Lovelace\" <ada@example.com>"]);
        assert_eq!(d.cc, ["Grace <grace@example.com>", "bob@example.com"]);
        assert_eq!(d.subject, "Re: Engine notes");
        assert_eq!(
            text.lines().nth(line - 2),
            Some(""),
            "cursor starts after the blank line"
        );
    }

    #[test]
    fn replying_to_myself_continues_to_my_recipients() {
        let mut o = original();
        o.from = "Tyler <me@example.com>".into();
        o.to = "ada@example.com".into();
        let (text, _) = template(Kind::Reply, Some(&o), &me(), &Prefill::default());
        assert_eq!(parse(&text).to, ["ada@example.com"]);
    }

    #[test]
    fn parse_reads_headers_attachments_and_body() {
        let d = parse(
            "# hint\nTo: a@b.co, \"X, Y\" <x@y.co>\nSubject: Hi\nAttach: ~/f.pdf, /tmp/g.png\n\nLine one\nLine two\n\n",
        );
        assert_eq!(d.to.len(), 2);
        assert_eq!(d.attach[1], PathBuf::from("/tmp/g.png"));
        assert!(d.attach[0].ends_with("f.pdf") && d.attach[0].is_absolute());
        assert_eq!(d.body, "Line one\nLine two");
    }

    #[test]
    fn markdown_keeps_line_breaks_and_neutralizes_html() {
        let html = markdown_html("**bold**\nnext <script>x</script>");
        assert!(html.contains("<strong>bold</strong><br />"));
        assert!(html.contains("&lt;script&gt;"));
    }

    #[test]
    fn reply_builds_threaded_mime_with_quote_and_signature() {
        let d = Draft {
            to: vec!["ada@example.com".into()],
            subject: "Re: Engine notes".into(),
            body: "Thanks!".into(),
            ..Default::default()
        };
        let raw = build(&d, Kind::Reply, Some(&original()), &me()).unwrap();
        let msg = mail_parser::MessageParser::default().parse(&raw).unwrap();
        assert_eq!(msg.in_reply_to().unwrap().first(), Some("m2@example.com"));
        let refs: Vec<_> = msg.references().unwrap().iter().collect();
        assert_eq!(refs, ["m1@example.com", "m2@example.com"]);
        let html = msg.body_html(0).unwrap();
        assert!(
            html.contains("Thanks!")
                && html.contains("<b>T</b>")
                && html.contains("<p>Original</p>")
        );
        let text = msg.body_text(0).unwrap().replace("\r\n", "\n");
        assert!(text.contains("> Original\n>\n> line"), "{text}");
        assert!(msg.message_id().unwrap().ends_with("@example.com"));
    }

    #[test]
    fn forward_carries_original_attachments() {
        let d = Draft {
            to: vec!["x@y.co".into()],
            subject: "Fwd: Engine notes".into(),
            body: "FYI".into(),
            ..Default::default()
        };
        let raw = build(&d, Kind::Forward, Some(&original()), &me()).unwrap();
        let msg = mail_parser::MessageParser::default().parse(&raw).unwrap();
        assert_eq!(msg.attachments().count(), 1);
        assert!(msg.body_html(0).unwrap().contains("Forwarded message"));
    }

    #[test]
    fn build_refuses_bad_recipients() {
        let d = Draft {
            to: vec!["ada".into()],
            ..Default::default()
        };
        assert!(
            build(&d, Kind::New, None, &me())
                .unwrap_err()
                .to_string()
                .contains("ada")
        );
        assert!(build(&Draft::default(), Kind::New, None, &me()).is_err());
    }

    #[test]
    fn mailto_links() {
        let p =
            mailto("mailto:ada@example.com?subject=Hello%20there&cc=b@c.co&body=Line%201%0ALine+2");
        assert_eq!(p.to, "ada@example.com");
        assert_eq!(p.subject, "Hello there");
        assert_eq!(p.cc, "b@c.co");
        assert_eq!(p.body, "Line 1\nLine+2");
    }
}
