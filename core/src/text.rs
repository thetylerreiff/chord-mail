//! Small text helpers for turning Gmail's raw fields into what people read.

/// The display name from a From header, or the address when there's no name.
/// `"Ada Lovelace" <ada@example.com>` becomes `Ada Lovelace`.
pub fn sender_name(from: &str) -> String {
    let from = from.trim();
    match from.find('<') {
        Some(i) => {
            let name = from[..i].trim().trim_matches('"').trim();
            if name.is_empty() {
                from[i + 1..].trim_end_matches('>').trim().to_string()
            } else {
                name.to_string()
            }
        }
        None => from.to_string(),
    }
}

/// The bare address from `Name <addr>` or `addr`.
pub fn address_of(entry: &str) -> String {
    let entry = entry.trim();
    match (entry.rfind('<'), entry.rfind('>')) {
        (Some(open), Some(close)) if close > open => entry[open + 1..close].trim().to_string(),
        _ => entry.trim_matches('"').to_string(),
    }
}

/// Splits an address header on commas that aren't inside quotes or angle
/// brackets, so `"Doe, Jane" <j@x>, bob@y` is two entries.
pub fn split_addresses(field: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut current = String::new();
    let mut quoted = false;
    let mut angle = false;
    for c in field.chars() {
        match c {
            '"' => quoted = !quoted,
            '<' if !quoted => angle = true,
            '>' if !quoted => angle = false,
            ',' | ';' if !quoted && !angle => {
                if !current.trim().is_empty() {
                    out.push(current.trim().to_string());
                }
                current.clear();
                continue;
            }
            _ => {}
        }
        current.push(c);
    }
    if !current.trim().is_empty() {
        out.push(current.trim().to_string());
    }
    out
}

/// Gmail snippets arrive HTML-escaped (`It&#39;s`). This decodes the named
/// entities Gmail uses and every numeric one, and leaves anything else as is.
pub fn decode_entities(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(amp) = rest.find('&') {
        out.push_str(&rest[..amp]);
        rest = &rest[amp..];
        // Entities are short; look for the ';' within a dozen bytes, walking
        // characters so multi-byte text (zero-width joiners are common in
        // marketing snippets) is never split.
        let Some(semi) = rest
            .char_indices()
            .take_while(|(i, _)| *i < 12)
            .find(|(_, c)| *c == ';')
            .map(|(i, _)| i)
        else {
            out.push('&');
            rest = &rest[1..];
            continue;
        };
        let entity = &rest[1..semi];
        let decoded = match entity {
            "amp" => Some('&'),
            "lt" => Some('<'),
            "gt" => Some('>'),
            "quot" => Some('"'),
            "apos" => Some('\''),
            "nbsp" => Some('\u{a0}'),
            _ => entity
                .strip_prefix("#x")
                .or_else(|| entity.strip_prefix("#X"))
                .and_then(|hex| u32::from_str_radix(hex, 16).ok())
                .or_else(|| entity.strip_prefix('#').and_then(|d| d.parse().ok()))
                .and_then(char::from_u32),
        };
        match decoded {
            Some(c) => {
                out.push(c);
                rest = &rest[semi + 1..];
            }
            None => {
                out.push('&');
                rest = &rest[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

/// Escapes text for safe inclusion in HTML.
pub fn escape_html(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            _ => out.push(c),
        }
    }
    out
}

/// Strips one `Re:`/`Fwd:` style prefix chain so replies don't pile them up.
pub fn base_subject(subject: &str) -> &str {
    let mut s = subject.trim();
    loop {
        let lower = s.to_ascii_lowercase();
        let cut = ["re:", "fwd:", "fw:", "aw:", "sv:"]
            .iter()
            .find(|p| lower.starts_with(*p))
            .map(|p| p.len());
        match cut {
            Some(n) => s = s[n..].trim_start(),
            None => return s,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sender_name_prefers_display_name() {
        assert_eq!(
            sender_name("\"Ada Lovelace\" <ada@example.com>"),
            "Ada Lovelace"
        );
        assert_eq!(sender_name("Ada <ada@example.com>"), "Ada");
        assert_eq!(sender_name("<ada@example.com>"), "ada@example.com");
        assert_eq!(sender_name("ada@example.com"), "ada@example.com");
    }

    #[test]
    fn splits_and_extracts_addresses() {
        let list = split_addresses("\"Doe, Jane\" <jane@x.com>, bob@y.com; Al <al@z.com>");
        assert_eq!(
            list,
            ["\"Doe, Jane\" <jane@x.com>", "bob@y.com", "Al <al@z.com>"]
        );
        assert_eq!(address_of(&list[0]), "jane@x.com");
        assert_eq!(address_of(&list[1]), "bob@y.com");
        assert!(split_addresses("  ").is_empty());
    }

    #[test]
    fn decodes_snippet_entities() {
        assert_eq!(decode_entities("It&#39;s &amp; &lt;b&gt;"), "It's & <b>");
        assert_eq!(decode_entities("&#x2014; caf&#233;"), "— café");
        assert_eq!(decode_entities("AT&T & co"), "AT&T & co");
        assert_eq!(decode_entities("&bogus; &"), "&bogus; &");
        // Real Gmail snippet: an ampersand followed by zero-width non-joiners.
        let zw = "Sale &\u{200c}\u{200c}\u{200c}\u{200c}\u{200c} more";
        assert_eq!(decode_entities(zw), zw);
        assert_eq!(decode_entities("&\u{200c};"), "&\u{200c};");
    }

    #[test]
    fn strips_reply_prefixes() {
        assert_eq!(base_subject("Re: RE: Fwd: Lunch"), "Lunch");
        assert_eq!(base_subject("Lunch"), "Lunch");
    }
}
