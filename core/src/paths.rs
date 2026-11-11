//! Where Chord keeps its files, following the XDG base directory spec.

use std::path::{Path, PathBuf};

const APP: &str = "chord-mail";

fn xdg(var: &str, fallback: &str) -> PathBuf {
    match std::env::var_os(var) {
        Some(dir) if !dir.is_empty() => PathBuf::from(dir),
        _ => PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(fallback),
    }
}

/// Creates `dir` and makes it private to you, so the mail cache, sign-in, and
/// drafts inside are never readable by other accounts on the machine. It also
/// tightens a folder an older Chord created open to everyone.
pub fn private_dir(dir: &Path) -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::create_dir_all(dir)?;
    std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700))
}

pub fn config_dir() -> PathBuf {
    xdg("XDG_CONFIG_HOME", ".config").join(APP)
}

pub fn data_dir() -> PathBuf {
    xdg("XDG_DATA_HOME", ".local/share").join(APP)
}

/// Your own OAuth client, downloaded from Google Cloud Console. When it's
/// there it takes the place of the client built into release builds. It
/// identifies the app, not the user, so it lives with the config rather than
/// the data.
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
