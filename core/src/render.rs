//! Turns a raw message into a self-contained HTML page that is safe to show.
//!
//! The page carries a Content-Security-Policy that blocks everything remote
//! unless the reader allowed it: no tracking pixels, no remote fonts, no
//! scripts (the view also runs with JavaScript off), no forms. Images
//! attached to the message itself (cid:) are inlined as data URIs, so they
//! always show.

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use mail_parser::{MessageParser, MessagePart};
use serde::Serialize;

use crate::text::escape_html;

#[derive(Debug, Clone, Default)]
pub struct Options {
    pub allow_remote: bool,
    pub show_quotes: bool,
    /// Colors and font for plain-text mail, which Chord styles itself.
    pub text_color: String,
    pub link_color: String,
    pub muted_color: String,
    pub font_family: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct Attachment {
    pub index: usize,
    pub name: String,
    pub mime: String,
    pub size: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct Rendered {
    pub html: String,
    /// The body was plain text, already styled for the theme, so it must not
    /// be darkened again.
    pub plain: bool,
    /// The message references remote content that the policy blocked.
    pub has_remote: bool,
    /// Quoted earlier messages exist (hidden unless `show_quotes`).
    pub has_quotes: bool,
    pub attachments: Vec<Attachment>,
}

/// What a reply or forward needs from the original.
#[derive(Debug, Clone, Default)]
pub struct Source {
    pub from: String,
    pub reply_to: String,
    pub to: String,
    pub cc: String,
    pub subject: String,
    pub date: String,
    pub message_id: String,
    pub references: String,
    /// The body as HTML (plain text is converted), with any wrapping
    /// `<html>`/`<body>` removed so it can be nested in a quote.
    pub html: String,
    pub text: String,
    pub attachments: Vec<(String, String, Vec<u8>)>,
}

const QUOTE_MARKERS: [&str; 5] = [
    "gmail_quote",
    "type=\"cite\"",
    "divrplyfwdmsg",
    "appendonsend",
    "yahoo_quoted",
];

const HIDE_QUOTES_CSS: &str = ".gmail_quote,.gmail_extra,blockquote[type=cite],#divRplyFwdMsg,\
#divRplyFwdMsg~*,#appendonsend~*,.yahoo_quoted{display:none!important}";

fn html_part<'a>(msg: &'a mail_parser::Message<'a>) -> Option<String> {
    msg.html_body()
        .find(|p| p.is_content_type("text", "html"))
        .and_then(|p| p.text().map(|t| t.into_owned()))
}

fn text_part(msg: &mail_parser::Message<'_>) -> Option<String> {
    msg.text_body()
        .find(|p| p.is_content_type("text", "plain") || p.content_type().is_none())
        .and_then(|p| p.text().map(|t| t.into_owned()))
        .or_else(|| msg.body_text(0).map(|t| t.into_owned()))
}

fn mime_of(part: &MessagePart<'_>) -> String {
    part.content_type()
        .map(|ct| match ct.subtype() {
            Some(sub) => format!("{}/{}", ct.ctype(), sub),
            None => ct.ctype().to_string(),
        })
        .unwrap_or_else(|| "application/octet-stream".into())
        .to_lowercase()
}

fn all_attachments<'a>(msg: &'a mail_parser::Message<'a>) -> Vec<MessagePart<'a>> {
    let mut parts: Vec<MessagePart> = msg.attachments().collect();
    for p in msg.other_parts() {
        if p.content_id().is_some() && !parts.iter().any(|q| q.id() == p.id()) {
            parts.push(p);
        }
    }
    parts
}

pub fn render(raw: &[u8], opts: &Options) -> Rendered {
    let Some(msg) = MessageParser::default().parse(raw) else {
        return Rendered {
            html: page_for_text("This message couldn't be read.", opts),
            plain: true,
            has_remote: false,
            has_quotes: false,
            attachments: vec![],
        };
    };

    let parts = all_attachments(&msg);
    let html = html_part(&msg);
    let mut used_cids = Vec::new();
    let (page, plain, has_remote, has_quotes) = match html {
        Some(mut html) => {
            for p in &parts {
                let Some(cid) = p.content_id() else { continue };
                let cid = cid.trim_matches(|c| c == '<' || c == '>');
                let needle = format!("cid:{cid}");
                if html.contains(&needle) {
                    let uri = format!(
                        "data:{};base64,{}",
                        mime_of(p),
                        STANDARD.encode(p.decoded())
                    );
                    html = html.replace(&needle, &uri);
                    used_cids.push(p.id());
                }
            }
            let lower = html.to_lowercase();
            let has_remote = references_remote(&lower);
            let has_quotes = QUOTE_MARKERS.iter().any(|m| lower.contains(m));
            (page_for_html(&html, opts), false, has_remote, has_quotes)
        }
        None => {
            let text = text_part(&msg).unwrap_or_default();
            let (trimmed, has_quotes) = trim_quote(&text);
            let body = if opts.show_quotes {
                text.as_str()
            } else {
                trimmed
            };
            (page_for_text(body, opts), true, false, has_quotes)
        }
    };

    let attachments = parts
        .iter()
        .enumerate()
        .filter(|(_, p)| !used_cids.contains(&p.id()))
        .filter(|(_, p)| p.attachment_name().is_some() || p.is_attachment())
        .map(|(i, p)| Attachment {
            index: i,
            name: p.attachment_name().unwrap_or("attachment").to_string(),
            mime: mime_of(p),
            size: p.decoded_len(),
        })
        .collect();

    Rendered {
        html: page,
        plain,
        has_remote,
        has_quotes,
        attachments,
    }
}

/// The bytes of attachment `index` as `render` numbered it.
pub fn attachment(raw: &[u8], index: usize) -> Option<(String, Vec<u8>)> {
    let msg = MessageParser::default().parse(raw)?;
    let parts = all_attachments(&msg);
    let p = parts.get(index)?;
    Some((
        sanitize_filename(p.attachment_name().unwrap_or("attachment")),
        p.decoded().into_owned(),
    ))
}

/// Keeps a sender-chosen name from escaping the download folder.
pub fn sanitize_filename(name: &str) -> String {
    let base = name.rsplit(['/', '\\']).next().unwrap_or(name);
    let cleaned: String = base
        .chars()
        .map(|c| if c.is_control() { '_' } else { c })
        .collect();
    let cleaned = cleaned.trim().trim_start_matches('.').to_string();
    if cleaned.is_empty() {
        "attachment".into()
    } else {
        cleaned
    }
}

/// What a reply or forward needs to quote, address, and thread the original.
pub fn source(raw: &[u8]) -> Option<Source> {
    let msg = MessageParser::default().parse(raw)?;
    let list = |a: Option<mail_parser::AddressList<'_>>| {
        a.map(|l| {
            l.mailboxes()
                .map(|m| match (m.name(), m.address()) {
                    (Some(n), Some(a)) if !n.is_empty() => {
                        format!("\"{}\" <{a}>", n.replace('"', ""))
                    }
                    (_, Some(a)) => a.to_string(),
                    _ => String::new(),
                })
                .filter(|s| !s.is_empty())
                .collect::<Vec<_>>()
                .join(", ")
        })
        .unwrap_or_default()
    };
    let text = text_part(&msg).unwrap_or_default();
    let html = match html_part(&msg) {
        Some(h) => inner_body(&h),
        None => text_to_html(&text),
    };
    let references = msg
        .references()
        .map(|r| {
            r.iter()
                .map(|s| format!("<{s}>"))
                .collect::<Vec<_>>()
                .join(" ")
        })
        .unwrap_or_default();
    let attachments = msg
        .attachments()
        .filter(|p| p.attachment_name().is_some())
        .map(|p| {
            (
                sanitize_filename(p.attachment_name().unwrap_or("attachment")),
                mime_of(&p),
                p.decoded().into_owned(),
            )
        })
        .collect();
    let date = msg
        .date()
        .map(|d| {
            chrono::DateTime::from_timestamp(d.to_timestamp(), 0)
                .map(|t| {
                    t.with_timezone(&chrono::Local)
                        .format("%a, %b %-d, %Y at %-I:%M %p")
                        .to_string()
                })
                .unwrap_or_default()
        })
        .unwrap_or_default();
    Some(Source {
        from: list(msg.from()),
        reply_to: list(msg.reply_to()),
        to: list(msg.to()),
        cc: list(msg.cc()),
        subject: msg.subject().unwrap_or_default().to_string(),
        date,
        message_id: msg
            .message_id()
            .map(|m| format!("<{m}>"))
            .unwrap_or_default(),
        references,
        html,
        text,
        attachments,
    })
}

fn references_remote(lower_html: &str) -> bool {
    [
        "src=\"http",
        "src='http",
        "src=http",
        "url(http",
        "url('http",
        "url(\"http",
        "background=\"http",
        "srcset=\"http",
        "href=\"http",
    ]
    .iter()
    .any(|p| {
        // A plain link isn't remote content; only a stylesheet link is.
        if *p == "href=\"http" {
            lower_html.contains("<link") && lower_html.contains(p)
        } else {
            lower_html.contains(p)
        }
    })
}

fn csp(allow_remote: bool) -> String {
    let remote = if allow_remote { " https: http:" } else { "" };
    format!(
        "default-src 'none'; img-src data:{remote}; style-src 'unsafe-inline'{remote}; \
         font-src data:{remote}; media-src data:; form-action 'none'; base-uri 'none'; frame-src 'none'"
    )
}

/// Wraps HTML in the locked-down page the reader shows.
pub fn page_for_html(html: &str, opts: &Options) -> String {
    // No doctype: much mail is laid out for quirks mode, as webmail renders it.
    format!(
        "<meta charset=\"utf-8\">\
         <meta http-equiv=\"Content-Security-Policy\" content=\"{}\">\
         <style>html,body{{margin:0}}body{{padding:2px;overflow-wrap:anywhere;\
         font-family:Arial,Helvetica,sans-serif}}\
         img{{max-width:100%;height:auto}}{}</style>{html}",
        csp(opts.allow_remote),
        if opts.show_quotes {
            ""
        } else {
            HIDE_QUOTES_CSS
        },
    )
}

fn page_for_text(text: &str, opts: &Options) -> String {
    format!(
        "<!doctype html><meta charset=\"utf-8\">\
         <meta http-equiv=\"Content-Security-Policy\" content=\"{}\">\
         <style>html,body{{margin:0;background:transparent}}\
         body{{color:{};font-family:{};font-size:15px;line-height:1.55;padding:2px;\
         white-space:pre-wrap;overflow-wrap:anywhere}}a{{color:{}}}.q{{color:{}}}</style>\
         <body>{}</body>",
        csp(false),
        css_value(&opts.text_color, "#ddd"),
        css_value(&opts.font_family, "sans-serif"),
        css_value(&opts.link_color, "#7aa2f7"),
        css_value(&opts.muted_color, "#888"),
        linkify_and_mark_quotes(text),
    )
}

/// Keeps a theme value from breaking out of the style block.
fn css_value<'a>(v: &'a str, fallback: &'a str) -> &'a str {
    if v.is_empty() || v.contains(['<', '>', ';', '{', '}']) {
        fallback
    } else {
        v
    }
}

fn linkify_and_mark_quotes(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 64);
    for (i, line) in text.lines().enumerate() {
        if i > 0 {
            out.push('\n');
        }
        let quoted = line.trim_start().starts_with('>');
        if quoted {
            out.push_str("<span class=\"q\">");
        }
        out.push_str(&linkify(line));
        if quoted {
            out.push_str("</span>");
        }
    }
    out
}

fn linkify(line: &str) -> String {
    let mut out = String::new();
    let mut rest = line;
    loop {
        let next = ["https://", "http://"]
            .iter()
            .filter_map(|p| rest.find(p))
            .min();
        let Some(start) = next else {
            out.push_str(&escape_html(rest));
            return out;
        };
        out.push_str(&escape_html(&rest[..start]));
        let tail = &rest[start..];
        let mut end = tail
            .find(|c: char| c.is_whitespace() || matches!(c, '<' | '>' | '"'))
            .unwrap_or(tail.len());
        // Trailing punctuation usually ends the sentence, not the URL.
        while end > 0 && tail[..end].ends_with(['.', ',', ')', ';', ':', '!', '?']) {
            end -= 1;
        }
        let url = &tail[..end];
        out.push_str(&format!("<a href=\"{0}\">{0}</a>", escape_html(url)));
        rest = &tail[end..];
    }
}

/// Cuts the quoted history off a plain-text reply: an "On … wrote:" line
/// followed only by quoted or blank lines, or a trailing run of quoted lines.
/// Returns the kept text and whether anything was cut.
pub fn trim_quote(text: &str) -> (&str, bool) {
    let lines: Vec<&str> = text.split_inclusive('\n').collect();
    let is_quote = |l: &str| l.trim_start().starts_with('>');
    let is_blank = |l: &str| l.trim().is_empty();
    let only_quote_after = |from: usize| {
        lines[from..].iter().all(|l| is_blank(l) || is_quote(l))
            && lines[from..].iter().any(|l| is_quote(l))
    };
    let mut cut = lines.iter().enumerate().position(|(i, l)| {
        let t = l.trim();
        t.starts_with("On ") && t.ends_with("wrote:") && only_quote_after(i + 1)
    });
    if cut.is_none() {
        let mut i = lines.len();
        while i > 0 && (is_blank(lines[i - 1]) || is_quote(lines[i - 1])) {
            i -= 1;
        }
        // A message that is nothing but quote keeps it all.
        if i > 0 && i < lines.len() && only_quote_after(i) {
            cut = Some(i);
        }
    }
    match cut {
        Some(n) => {
            let byte: usize = lines[..n].iter().map(|l| l.len()).sum();
            (text[..byte].trim_end(), true)
        }
        None => (text, false),
    }
}

fn inner_body(html: &str) -> String {
    let lower = html.to_lowercase();
    let start = lower
        .find("<body")
        .and_then(|i| lower[i..].find('>').map(|j| i + j + 1))
        .unwrap_or(0);
    let end = lower
        .rfind("</body>")
        .filter(|e| *e >= start)
        .unwrap_or(html.len());
    html[start..end].to_string()
}

pub fn text_to_html(text: &str) -> String {
    format!(
        "<div style=\"white-space:pre-wrap\">{}</div>",
        escape_html(text)
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    const HTML_MAIL: &str = "From: Ada <ada@example.com>\r\nTo: me@example.com\r\nSubject: Hi\r\n\
Message-ID: <abc@example.com>\r\nMIME-Version: 1.0\r\n\
Content-Type: multipart/related; boundary=\"b\"\r\n\r\n\
--b\r\nContent-Type: text/html; charset=utf-8\r\n\r\n\
<html><body><p>Hello</p><img src=\"cid:logo\"><img src=\"https://track.example/p.gif\">\
<div class=\"gmail_quote\">old stuff</div></body></html>\r\n\
--b\r\nContent-Type: image/png\r\nContent-ID: <logo>\r\nContent-Transfer-Encoding: base64\r\n\r\niVBORw0KGgo=\r\n\
--b\r\nContent-Type: application/pdf; name=\"../../etc/report.pdf\"\r\n\
Content-Disposition: attachment; filename=\"../../etc/report.pdf\"\r\n\
Content-Transfer-Encoding: base64\r\n\r\nJVBERi0=\r\n--b--\r\n";

    const TEXT_MAIL: &str = "From: Bob <bob@example.com>\r\nSubject: Re: Plans\r\n\r\n\
Sounds good, see https://example.com/plan.\r\n\r\nOn Mon, Nov 2, 2026 at 9:00 AM Me wrote:\r\n> earlier\r\n> text\r\n";

    #[test]
    fn html_mail_is_locked_down_and_cids_inlined() {
        let r = render(HTML_MAIL.as_bytes(), &Options::default());
        assert!(!r.plain);
        assert!(r.has_remote);
        assert!(r.has_quotes);
        assert!(
            r.html.contains("img-src data:;"),
            "remote images blocked: {}",
            r.html
        );
        assert!(r.html.contains("src=\"data:image/png;base64,"));
        assert!(r.html.contains(".gmail_quote"));
        assert_eq!(r.attachments.len(), 1, "the inlined logo isn't listed");
        assert_eq!(r.attachments[0].name, "../../etc/report.pdf");

        let allowed = render(
            HTML_MAIL.as_bytes(),
            &Options {
                allow_remote: true,
                show_quotes: true,
                ..Default::default()
            },
        );
        assert!(allowed.html.contains("img-src data: https: http:"));
        assert!(!allowed.html.contains("display:none"));
    }

    #[test]
    fn attachments_get_safe_names() {
        let r = render(HTML_MAIL.as_bytes(), &Options::default());
        let (name, bytes) = attachment(HTML_MAIL.as_bytes(), r.attachments[0].index).unwrap();
        assert_eq!(name, "report.pdf");
        assert_eq!(bytes, b"%PDF-");
        assert_eq!(sanitize_filename("..hidden"), "hidden");
        assert_eq!(sanitize_filename(""), "attachment");
    }

    #[test]
    fn plain_text_is_escaped_linked_and_trimmed() {
        let r = render(
            TEXT_MAIL.as_bytes(),
            &Options {
                text_color: "red".into(),
                ..Default::default()
            },
        );
        assert!(r.plain);
        assert!(r.has_quotes);
        assert!(
            r.html
                .contains("<a href=\"https://example.com/plan\">https://example.com/plan</a>.")
        );
        assert!(!r.html.contains("earlier"));
        assert!(r.html.contains("color:red"));

        let full = render(
            TEXT_MAIL.as_bytes(),
            &Options {
                show_quotes: true,
                ..Default::default()
            },
        );
        assert!(full.html.contains("<span class=\"q\">&gt; earlier</span>"));
    }

    #[test]
    fn theme_values_cannot_inject_css() {
        let r = render(
            TEXT_MAIL.as_bytes(),
            &Options {
                text_color: "red}</style><script>".into(),
                ..Default::default()
            },
        );
        assert!(!r.html.contains("<script>"));
    }

    #[test]
    fn trim_quote_keeps_inline_replies() {
        assert_eq!(trim_quote("hi\n\n> old\n> older"), ("hi", true));
        assert_eq!(
            trim_quote("> question?\nanswer\n> next?\nanswer two"),
            ("> question?\nanswer\n> next?\nanswer two", false)
        );
        assert_eq!(trim_quote("no quotes here"), ("no quotes here", false));
    }

    #[test]
    fn source_has_threading_and_quote_material() {
        let s = source(HTML_MAIL.as_bytes()).unwrap();
        assert_eq!(s.from, "\"Ada\" <ada@example.com>");
        assert_eq!(s.message_id, "<abc@example.com>");
        assert!(s.html.starts_with("<p>Hello</p>"));
        assert_eq!(s.attachments.len(), 1);
    }
}
