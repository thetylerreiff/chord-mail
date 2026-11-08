//! Where Chord keeps its files, following the XDG base directory spec.

use std::path::PathBuf;

const APP: &str = "chord-mail";

fn xdg(var: &str, fallback: &str) -> PathBuf {
    match std::env::var_os(var) {
        Some(dir) if !dir.is_empty() => PathBuf::from(dir),
        _ => PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(fallback),
    }
}

pub fn config_dir() -> PathBuf {
    xdg("XDG_CONFIG_HOME", ".config").join(APP)
}

pub fn data_dir() -> PathBuf {
    xdg("XDG_DATA_HOME", ".local/share").join(APP)
}

/// The OAuth client downloaded from Google Cloud Console. It identifies the
/// app, not the user, so it lives with the config rather than the data.
pub fn client_secret() -> PathBuf {
    config_dir().join("client_secret.json")
}

/// The user's refresh token. Anyone holding it can read the mailbox, so it is
/// written with mode 0600.
pub fn token() -> PathBuf {
    data_dir().join("token.json")
}

pub fn database() -> PathBuf {
    data_dir().join("mail.db")
}
