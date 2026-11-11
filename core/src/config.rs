//! `~/.config/chord-mail/config.toml`: split inbox rules and a few knobs.
//! Everything has a default, so the file is optional.

use std::collections::HashMap;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use crate::store::ThreadRow;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    /// Inbox tabs, in order. A split matches a conversation when any of its
    /// labels or senders do. A split with no rules takes whatever no other
    /// split claims.
    pub split: Vec<Split>,
    pub notify: Notify,
    pub sync: SyncSettings,
    pub compose: ComposeSettings,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Split {
    pub name: String,
    /// Gmail label ids or names, such as `CATEGORY_UPDATES` or `Receipts`.
    #[serde(default)]
    pub labels: Vec<String>,
    /// Addresses or `@domain` suffixes.
    #[serde(default)]
    pub senders: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Notify {
    pub enabled: bool,
    /// Which splits notify for new mail.
    pub splits: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct SyncSettings {
    /// Seconds between background syncs.
    pub interval: u64,
    /// How many inbox messages the first sync downloads.
    pub inbox_limit: usize,
    /// How many of the newest inbox conversations are kept fully downloaded.
    pub prefetch: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ComposeSettings {
    /// Seconds a sent message waits before it really goes, so it can be undone.
    pub undo_seconds: u64,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            split: vec![
                Split {
                    name: "Primary".into(),
                    labels: vec![],
                    senders: vec![],
                },
                Split {
                    name: "Updates".into(),
                    labels: vec!["CATEGORY_UPDATES".into(), "CATEGORY_FORUMS".into()],
                    senders: vec![],
                },
                Split {
                    name: "Promotions".into(),
                    labels: vec!["CATEGORY_PROMOTIONS".into(), "CATEGORY_SOCIAL".into()],
                    senders: vec![],
                },
            ],
            notify: Notify::default(),
            sync: SyncSettings::default(),
            compose: ComposeSettings::default(),
        }
    }
}

impl Default for Notify {
    fn default() -> Self {
        Self {
            enabled: true,
            splits: vec!["Primary".into()],
        }
    }
}

impl Default for SyncSettings {
    fn default() -> Self {
        Self {
            interval: 60,
            inbox_limit: 500,
            prefetch: 50,
        }
    }
}

impl Default for ComposeSettings {
    fn default() -> Self {
        Self { undo_seconds: 10 }
    }
}

const TEMPLATE: &str = r#"# Chord settings. Delete anything to get the default back.

# Inbox tabs, in order. A tab matches a conversation when any of its labels
# (Gmail label ids or names) or senders (addresses or @domains) match. The tab
# with no rules gets everything the others don't claim.
[[split]]
name = "Primary"

[[split]]
name = "Updates"
labels = ["CATEGORY_UPDATES", "CATEGORY_FORUMS"]

[[split]]
name = "Promotions"
labels = ["CATEGORY_PROMOTIONS", "CATEGORY_SOCIAL"]

[notify]
enabled = true
splits = ["Primary"]

[sync]
interval = 60      # seconds between background syncs
inbox_limit = 500  # inbox messages the first sync downloads
prefetch = 50      # newest conversations kept fully downloaded

[compose]
undo_seconds = 10
"#;

impl Config {
    /// Loads the config, writing the commented default the first time.
    pub fn load() -> Result<Self> {
        let path = crate::paths::config_dir().join("config.toml");
        if let Some(dir) = path.parent() {
            let _ = crate::paths::private_dir(dir);
        }
        match std::fs::read_to_string(&path) {
            Ok(text) => Self::parse(&text).with_context(|| format!("in {}", path.display())),
            Err(_) => {
                let _ = std::fs::write(&path, TEMPLATE);
                Ok(Self::default())
            }
        }
    }

    pub fn parse(text: &str) -> Result<Self> {
        let mut config: Self = toml::from_str(text)?;
        if config.split.is_empty() {
            config.split = Self::default().split;
        }
        Ok(config)
    }

    /// Which split a conversation belongs in. `label_names` maps label ids to
    /// names so rules can use either.
    pub fn split_of(&self, row: &ThreadRow, label_names: &HashMap<String, String>) -> usize {
        let matches = |s: &Split| {
            let by_label = s.labels.iter().any(|rule| {
                // Categories follow the newest message, as Gmail's tabs do;
                // other labels count from any message in the conversation.
                if rule.to_ascii_uppercase().starts_with("CATEGORY_") {
                    return row
                        .category
                        .as_deref()
                        .is_some_and(|c| c.eq_ignore_ascii_case(rule));
                }
                row.labels.iter().any(|id| {
                    id.eq_ignore_ascii_case(rule)
                        || label_names
                            .get(id)
                            .is_some_and(|n| n.eq_ignore_ascii_case(rule))
                })
            });
            let by_sender = s.senders.iter().any(|rule| {
                let rule = rule.to_lowercase();
                row.sender_addresses.iter().any(|a| {
                    if rule.starts_with('@') {
                        a.ends_with(&rule)
                    } else {
                        *a == rule
                    }
                })
            });
            by_label || by_sender
        };
        let has_rules = |s: &Split| !s.labels.is_empty() || !s.senders.is_empty();
        self.split
            .iter()
            .position(|s| has_rules(s) && matches(s))
            .or_else(|| self.split.iter().position(|s| !has_rules(s)))
            .unwrap_or(0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn thread(labels: &[&str], senders: &[&str]) -> ThreadRow {
        ThreadRow {
            id: "t".into(),
            subject: String::new(),
            senders: vec![],
            sender_addresses: senders.iter().map(|s| s.to_string()).collect(),
            snippet: String::new(),
            date: 0,
            count: 1,
            unread: false,
            starred: false,
            draft: false,
            labels: labels.iter().map(|s| s.to_string()).collect(),
            category: labels
                .iter()
                .rev()
                .find(|l| l.starts_with("CATEGORY_"))
                .map(|s| s.to_string()),
            snoozed_until: None,
        }
    }

    #[test]
    fn mixed_categories_follow_the_newest_message() {
        // An Updates message, then a Primary one: Gmail shows it in Primary.
        let mut row = thread(&["INBOX", "CATEGORY_UPDATES", "CATEGORY_PERSONAL"], &[]);
        row.category = Some("CATEGORY_PERSONAL".into());
        assert_eq!(Config::default().split_of(&row, &HashMap::new()), 0);
        row.category = Some("CATEGORY_UPDATES".into());
        assert_eq!(Config::default().split_of(&row, &HashMap::new()), 1);
    }

    #[test]
    fn template_parses_to_the_default() {
        let parsed = Config::parse(TEMPLATE).unwrap();
        assert_eq!(parsed.split, Config::default().split);
        assert_eq!(parsed.sync.interval, 60);
    }

    #[test]
    fn splits_by_category_with_primary_as_fallback() {
        let c = Config::default();
        let names = HashMap::new();
        assert_eq!(
            c.split_of(&thread(&["INBOX", "CATEGORY_PERSONAL"], &[]), &names),
            0
        );
        assert_eq!(
            c.split_of(&thread(&["INBOX", "CATEGORY_UPDATES"], &[]), &names),
            1
        );
        assert_eq!(
            c.split_of(&thread(&["INBOX", "CATEGORY_SOCIAL"], &[]), &names),
            2
        );
        assert_eq!(c.split_of(&thread(&["INBOX"], &[]), &names), 0);
    }

    #[test]
    fn custom_splits_match_label_names_and_sender_domains() {
        let c = Config::parse(
            r#"
            [[split]]
            name = "Important"
            [[split]]
            name = "Code"
            senders = ["@github.com"]
            [[split]]
            name = "Money"
            labels = ["Receipts"]
            "#,
        )
        .unwrap();
        let names = HashMap::from([("Label_3".to_string(), "Receipts".to_string())]);
        assert_eq!(
            c.split_of(&thread(&["INBOX"], &["notifications@github.com"]), &names),
            1
        );
        assert_eq!(
            c.split_of(&thread(&["INBOX", "Label_3"], &["shop@store.com"]), &names),
            2
        );
        assert_eq!(
            c.split_of(&thread(&["INBOX"], &["mom@example.com"]), &names),
            0
        );
    }
}
