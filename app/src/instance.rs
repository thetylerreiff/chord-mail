//! One window at a time. A running window listens on a socket in the user's
//! runtime directory; launching Chord again (from the launcher, a mailto:
//! link, or a notification) hands its request to that window instead.

use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;

/// What a launch asks the window to do.
#[derive(Debug, Clone, PartialEq)]
pub enum Request {
    Focus,
    Thread(String),
    Compose(String),
}

impl Request {
    fn encode(&self) -> String {
        match self {
            Request::Focus => "focus".into(),
            Request::Thread(id) => format!("thread {id}"),
            Request::Compose(mailto) => format!("compose {mailto}"),
        }
    }

    fn decode(line: &str) -> Option<Self> {
        let line = line.trim();
        let (verb, arg) = line.split_once(' ').unwrap_or((line, ""));
        match verb {
            "focus" => Some(Request::Focus),
            "thread" if !arg.is_empty() => Some(Request::Thread(arg.to_string())),
            "compose" => Some(Request::Compose(arg.to_string())),
            _ => None,
        }
    }
}

fn socket_path() -> PathBuf {
    let dir = std::env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir);
    dir.join("chord-mail.sock")
}

/// Hands `request` to a running window. False when none is running.
pub fn send(request: &Request) -> bool {
    match UnixStream::connect(socket_path()) {
        Ok(mut s) => writeln!(s, "{}", request.encode()).is_ok(),
        Err(_) => false,
    }
}

/// Starts listening for later launches, calling `handle` on a background
/// thread for each request.
pub fn listen(handle: impl Fn(Request) + Send + 'static) -> std::io::Result<()> {
    let path = socket_path();
    // A socket left by a crashed window answers nothing; replace it.
    if path.exists() && UnixStream::connect(&path).is_err() {
        let _ = std::fs::remove_file(&path);
    }
    let listener = UnixListener::bind(&path)?;
    std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            let mut line = String::new();
            if BufReader::new(stream).read_line(&mut line).is_ok()
                && let Some(r) = Request::decode(&line)
            {
                handle(r);
            }
        }
    });
    Ok(())
}

/// Removes the socket when the window closes.
pub fn cleanup() {
    let _ = std::fs::remove_file(socket_path());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn requests_round_trip() {
        for r in [
            Request::Focus,
            Request::Thread("18c2".into()),
            Request::Compose("mailto:a@b.co?subject=Hi%20there".into()),
            Request::Compose(String::new()),
        ] {
            assert_eq!(Request::decode(&r.encode()), Some(r));
        }
        assert_eq!(Request::decode("bogus"), None);
    }
}
