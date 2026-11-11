//! Google OAuth for an installed app: the browser signs the user in and
//! redirects to a one-shot listener on 127.0.0.1, and PKCE keeps the
//! authorization code useless to anyone who intercepts it.

use std::io::Write;
use std::os::unix::fs::OpenOptionsExt;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result, anyhow, bail};
use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

const AUTH_URL: &str = "https://accounts.google.com/o/oauth2/v2/auth";
const TOKEN_URL: &str = "https://oauth2.googleapis.com/token";

/// gmail.modify covers reading, archiving, labeling, trashing, and sending,
/// but not permanent deletion, which Chord never needs.
pub const SCOPE: &str = "https://www.googleapis.com/auth/gmail.modify";

/// Refresh this long before the access token actually expires, so a sync that
/// starts just under the wire doesn't fail halfway through.
const EXPIRY_MARGIN_SECS: i64 = 300;

#[derive(Debug, Clone)]
pub struct Client {
    pub id: String,
    pub secret: String,
}

impl Client {
    /// Chord's own Google client, compiled into release builds from the CI
    /// secrets `CHORD_GOOGLE_CLIENT_ID` and `CHORD_GOOGLE_CLIENT_SECRET`.
    /// Builds from source have none. Google documents an installed app's
    /// secret as not confidential, but its API terms keep developer
    /// credentials out of open-source code, so this keeps it out of the
    /// repository, not out of the binary.
    pub fn builtin() -> Option<Self> {
        Self::from_parts(
            option_env!("CHORD_GOOGLE_CLIENT_ID"),
            option_env!("CHORD_GOOGLE_CLIENT_SECRET"),
        )
    }

    fn from_parts(id: Option<&str>, secret: Option<&str>) -> Option<Self> {
        let id = id?.trim();
        let secret = secret?.trim();
        (id.ends_with(".apps.googleusercontent.com") && !secret.is_empty()).then(|| Self {
            id: id.to_string(),
            secret: secret.to_string(),
        })
    }

    /// The client Chord signs in with: your own `client_secret.json` when
    /// there is one, otherwise the built-in client. A file that's there but
    /// unreadable stays an error rather than quietly switching clients.
    pub fn resolve(path: &Path) -> Result<Self> {
        if path.exists() {
            return Self::load(path);
        }
        Self::builtin().ok_or_else(|| {
            anyhow!(
                "no OAuth client at {}; this build has no built-in Google client, \
                 so see the README for how to create your own",
                path.display()
            )
        })
    }

    /// Whether there is any client to sign in with.
    pub fn available(path: &Path) -> bool {
        path.exists() || Self::builtin().is_some()
    }

    /// Reads the JSON file Google Cloud Console offers for a "Desktop app"
    /// OAuth client, which nests the fields under "installed".
    pub fn load(path: &Path) -> Result<Self> {
        #[derive(Deserialize)]
        struct File {
            installed: Installed,
        }
        #[derive(Deserialize)]
        struct Installed {
            client_id: String,
            client_secret: String,
        }
        let raw = std::fs::read_to_string(path).with_context(|| {
            format!(
                "no OAuth client at {}; see the README for how to create one",
                path.display()
            )
        })?;
        let file: File = serde_json::from_str(&raw).with_context(|| {
            format!(
                "{} isn't a Desktop app OAuth client from Google Cloud Console",
                path.display()
            )
        })?;
        Ok(Self {
            id: file.installed.client_id,
            secret: file.installed.client_secret,
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Token {
    pub access_token: String,
    pub refresh_token: String,
    /// Unix seconds.
    pub expires_at: i64,
    /// The OAuth client that issued this sign-in. Google only refreshes a
    /// token for the client it was issued to. Empty in tokens saved before
    /// Chord recorded it.
    #[serde(default)]
    pub client_id: String,
}

impl Token {
    pub fn load(path: &Path) -> Result<Self> {
        let raw = std::fs::read_to_string(path)
            .map_err(|_| anyhow!("not signed in; run `chord-mail login` first"))?;
        serde_json::from_str(&raw).context("the saved token is corrupt; run `chord-mail login`")
    }

    /// Writes beside the target and renames, so a crash never leaves a
    /// half-written token, and creates the file 0600 so it is never readable
    /// by other users, even for an instant. The window and the service can
    /// refresh at the same moment, so each writer gets its own temp file
    /// rather than truncating the other's mid-write.
    pub fn save(&self, path: &Path) -> Result<()> {
        if let Some(dir) = path.parent() {
            crate::paths::private_dir(dir)?;
        }
        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("token");
        let tmp = path.with_file_name(format!(
            ".{name}.{}.{:016x}.tmp",
            std::process::id(),
            rand::random::<u64>()
        ));
        let write = || -> Result<()> {
            let mut file = std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .mode(0o600)
                .open(&tmp)?;
            file.write_all(serde_json::to_string_pretty(self)?.as_bytes())?;
            file.sync_all()?;
            std::fs::rename(&tmp, path)?;
            Ok(())
        };
        write().inspect_err(|_| {
            let _ = std::fs::remove_file(&tmp);
        })
    }

    /// Whether this sign-in can be refreshed with `client`.
    pub fn issued_to(&self, client: &Client) -> bool {
        self.client_id.is_empty() || self.client_id == client.id
    }

    fn is_fresh(&self) -> bool {
        self.expires_at - EXPIRY_MARGIN_SECS > now()
    }
}

fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

fn random_urlsafe(bytes: usize) -> String {
    let buf: Vec<u8> = (0..bytes).map(|_| rand::random::<u8>()).collect();
    URL_SAFE_NO_PAD.encode(buf)
}

/// The S256 code challenge from RFC 7636.
pub fn pkce_challenge(verifier: &str) -> String {
    URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()))
}

#[derive(Deserialize)]
struct TokenResponse {
    access_token: String,
    expires_in: i64,
    refresh_token: Option<String>,
}

#[derive(Deserialize)]
struct TokenError {
    error: String,
    error_description: Option<String>,
}

/// Google's token endpoint turned a request down with an OAuth error code,
/// kept typed so a dead sign-in can be told apart from a passing failure.
#[derive(Debug)]
struct OAuthError {
    status: reqwest::StatusCode,
    code: String,
    description: Option<String>,
}

impl std::fmt::Display for OAuthError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "Google refused the token request ({}): {}",
            self.status, self.code
        )?;
        if let Some(d) = &self.description {
            write!(f, ": {d}")?;
        }
        Ok(())
    }
}

impl std::error::Error for OAuthError {}

/// The error for a failed token response. Built from Google's reply only,
/// never from the form, so no token or client secret ends up in it.
fn token_error(status: reqwest::StatusCode, body: &str) -> anyhow::Error {
    match serde_json::from_str::<TokenError>(body) {
        Ok(e) => OAuthError {
            status,
            code: e.error,
            description: e.error_description,
        }
        .into(),
        Err(_) => anyhow!("Google refused the token request ({status}): {body}"),
    }
}

/// Whether Google said the refresh token itself is no good: the user revoked
/// Chord's access, changed their password, or the sign-in expired. Retrying
/// will never work; only signing in again will.
fn sign_in_is_dead(e: &anyhow::Error) -> bool {
    matches!(e.downcast_ref::<OAuthError>(), Some(o) if o.code == "invalid_grant")
}

/// Deletes the saved token after Google rejected `rejected_refresh`, which
/// is what makes the app show the sign-in page again. A token with another
/// refresh token is a newer sign-in that landed meanwhile, so it stays.
fn forget_dead_sign_in(path: &Path, rejected_refresh: &str) -> Result<()> {
    match Token::load(path) {
        Ok(t) if t.refresh_token == rejected_refresh => match std::fs::remove_file(path) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e.into()),
            _ => Ok(()),
        },
        _ => Ok(()),
    }
}

async fn post_token(http: &reqwest::Client, form: &[(&str, &str)]) -> Result<TokenResponse> {
    let resp = http.post(TOKEN_URL).form(form).send().await?;
    let status = resp.status();
    let body = resp.text().await?;
    if !status.is_success() {
        return Err(token_error(status, &body));
    }
    Ok(serde_json::from_str(&body)?)
}

/// Runs the browser sign-in. `on_url` receives the consent URL so the caller
/// can open it and also print it, in case no browser is reachable.
pub async fn login(
    http: &reqwest::Client,
    client: &Client,
    on_url: impl FnOnce(&str),
) -> Result<Token> {
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let redirect = format!("http://127.0.0.1:{}", listener.local_addr()?.port());
    let verifier = random_urlsafe(64);
    let state = random_urlsafe(16);

    let mut url = url::Url::parse(AUTH_URL)?;
    url.query_pairs_mut()
        .append_pair("client_id", &client.id)
        .append_pair("redirect_uri", &redirect)
        .append_pair("response_type", "code")
        .append_pair("scope", SCOPE)
        .append_pair("code_challenge", &pkce_challenge(&verifier))
        .append_pair("code_challenge_method", "S256")
        .append_pair("state", &state)
        // Offline access plus a forced consent screen is what makes Google
        // hand back a refresh token every time, not only on the first login.
        .append_pair("access_type", "offline")
        .append_pair("prompt", "consent");
    on_url(url.as_str());

    let code = await_redirect(&listener, &state).await?;
    let resp = post_token(
        http,
        &[
            ("code", &code),
            ("client_id", &client.id),
            ("client_secret", &client.secret),
            ("redirect_uri", &redirect),
            ("grant_type", "authorization_code"),
            ("code_verifier", &verifier),
        ],
    )
    .await?;
    Ok(Token {
        access_token: resp.access_token,
        refresh_token: resp
            .refresh_token
            .ok_or_else(|| anyhow!("Google didn't return a refresh token"))?,
        expires_at: now() + resp.expires_in,
        client_id: client.id.clone(),
    })
}

/// Waits for the browser's redirect and pulls the code out of it. Browsers
/// sometimes send other requests first (a favicon), so anything without our
/// state is answered and ignored.
async fn await_redirect(listener: &TcpListener, state: &str) -> Result<String> {
    loop {
        let (mut stream, _) = listener.accept().await?;
        let mut buf = vec![0u8; 8192];
        let n = stream.read(&mut buf).await?;
        let request = String::from_utf8_lossy(&buf[..n]);
        let path = request
            .lines()
            .next()
            .and_then(|line| line.split_whitespace().nth(1))
            .unwrap_or("/");
        let url = url::Url::parse(&format!("http://127.0.0.1{path}"))?;
        let param = |k: &str| {
            url.query_pairs()
                .find(|(key, _)| key == k)
                .map(|(_, v)| v.into_owned())
        };

        if param("state").as_deref() != Some(state) {
            respond(&mut stream, "404 Not Found", "").await;
            continue;
        }
        if let Some(error) = param("error") {
            respond(
                &mut stream,
                "200 OK",
                "Sign-in was cancelled. You can close this tab.",
            )
            .await;
            bail!("sign-in failed: {error}");
        }
        let code = param("code").ok_or_else(|| anyhow!("the redirect had no code"))?;
        respond(
            &mut stream,
            "200 OK",
            "Chord is signed in. You can close this tab.",
        )
        .await;
        return Ok(code);
    }
}

async fn respond(stream: &mut tokio::net::TcpStream, status: &str, message: &str) {
    let body = format!(
        "<!doctype html><meta charset=utf-8><title>Chord</title>\
         <body style=\"font:16px system-ui;padding:3em;background:#111;color:#eee\">{message}"
    );
    let reply = format!(
        "HTTP/1.1 {status}\r\nContent-Type: text/html; charset=utf-8\r\n\
         Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    let _ = stream.write_all(reply.as_bytes()).await;
}

/// Returns a usable access token, refreshing and saving it first when it is
/// about to expire.
pub async fn access_token(http: &reqwest::Client, client: &Client, path: &Path) -> Result<String> {
    let mut token = Token::load(path)?;
    if !token.issued_to(client) {
        bail!(
            "you signed in with a different OAuth client than Chord is using now; run `chord-mail login` again"
        );
    }
    if !token.is_fresh() {
        let resp = match post_token(
            http,
            &[
                ("client_id", &client.id),
                ("client_secret", &client.secret),
                ("refresh_token", &token.refresh_token),
                ("grant_type", "refresh_token"),
            ],
        )
        .await
        {
            Ok(resp) => resp,
            Err(e) if sign_in_is_dead(&e) => {
                // Keeping the file would leave Chord "signed in" but offline
                // for good, re-sending the dead token every few minutes.
                forget_dead_sign_in(path, &token.refresh_token)?;
                return Err(e.context(
                    "the Google sign-in expired or was revoked; sign in again \
                     (`chord-mail login`)",
                ));
            }
            // Network trouble, a server error, or anything else: the sign-in
            // may well still be good, so keep it.
            Err(e) => {
                return Err(e.context("couldn't refresh the session; run `chord-mail login` again"));
            }
        };
        token.access_token = resp.access_token;
        token.expires_at = now() + resp.expires_in;
        if let Some(refresh) = resp.refresh_token {
            token.refresh_token = refresh;
        }
        // Google accepted the refresh, so the token belongs to this client.
        token.client_id = client.id.clone();
        token.save(path)?;
    }
    Ok(token.access_token)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pkce_matches_rfc_7636_example() {
        assert_eq!(
            pkce_challenge("dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk"),
            "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM"
        );
    }

    #[test]
    fn verifier_length_is_within_pkce_bounds() {
        let v = random_urlsafe(64);
        assert!((43..=128).contains(&v.len()), "{}", v.len());
    }

    #[test]
    fn token_round_trips_with_private_permissions() {
        use std::os::unix::fs::PermissionsExt;
        let dir = std::env::temp_dir().join(format!("chord-token-{}", random_urlsafe(6)));
        let path = dir.join("token.json");
        let token = Token {
            access_token: "a".into(),
            refresh_token: "r".into(),
            expires_at: 42,
            client_id: "c".into(),
        };
        token.save(&path).unwrap();
        let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
        assert_eq!(Token::load(&path).unwrap().refresh_token, "r");
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn saves_leave_no_temp_files_even_when_racing() {
        let dir = std::env::temp_dir().join(format!("chord-save-{}", random_urlsafe(6)));
        let path = dir.join("token.json");
        let token = |n: i64| Token {
            access_token: "a".into(),
            refresh_token: "r".into(),
            expires_at: n,
            client_id: "c".into(),
        };
        std::thread::scope(|s| {
            for n in 0..8 {
                let path = &path;
                s.spawn(move || {
                    for _ in 0..10 {
                        token(n).save(path).unwrap();
                    }
                });
            }
        });
        assert!((0..8).contains(&Token::load(&path).unwrap().expires_at));
        let names: Vec<_> = std::fs::read_dir(&dir)
            .unwrap()
            .map(|e| e.unwrap().file_name())
            .collect();
        assert_eq!(names, ["token.json"]);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn only_invalid_grant_means_the_sign_in_is_dead() {
        let bad = reqwest::StatusCode::BAD_REQUEST;
        let dead = token_error(
            bad,
            r#"{"error":"invalid_grant","error_description":"Token has been expired or revoked."}"#,
        );
        assert!(sign_in_is_dead(&dead));
        assert!(dead.to_string().contains("invalid_grant"), "{dead}");
        // Still recognised once the caller adds context.
        assert!(sign_in_is_dead(&dead.context("refreshing")));

        for alive in [
            token_error(bad, r#"{"error":"invalid_client"}"#),
            token_error(
                bad,
                r#"{"error":"invalid_request","error_description":"x"}"#,
            ),
            token_error(
                reqwest::StatusCode::SERVICE_UNAVAILABLE,
                "<html>try later</html>",
            ),
            token_error(reqwest::StatusCode::INTERNAL_SERVER_ERROR, ""),
            anyhow!("error sending request: connection refused"),
        ] {
            assert!(!sign_in_is_dead(&alive), "{alive}");
        }
    }

    #[test]
    fn a_dead_sign_in_is_forgotten_but_a_newer_one_kept() {
        let dir = std::env::temp_dir().join(format!("chord-dead-{}", random_urlsafe(6)));
        let path = dir.join("token.json");
        let token = |refresh: &str| Token {
            access_token: "a".into(),
            refresh_token: refresh.into(),
            expires_at: 0,
            client_id: "c".into(),
        };
        token("new").save(&path).unwrap();
        forget_dead_sign_in(&path, "old").unwrap();
        assert!(Token::load(&path).is_ok(), "a sign-in made meanwhile stays");
        forget_dead_sign_in(&path, "new").unwrap();
        assert!(!path.exists());
        assert!(Token::load(&path).is_err(), "so the app is signed out");
        forget_dead_sign_in(&path, "new").unwrap();
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn client_reads_console_download() {
        let dir = std::env::temp_dir().join(format!("chord-client-{}", random_urlsafe(6)));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("client_secret.json");
        std::fs::write(
            &path,
            r#"{"installed":{"client_id":"id.apps.googleusercontent.com","client_secret":"s","redirect_uris":["http://localhost"]}}"#,
        )
        .unwrap();
        let client = Client::load(&path).unwrap();
        assert_eq!(client.id, "id.apps.googleusercontent.com");
        assert_eq!(client.secret, "s");
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn builtin_client_needs_a_desktop_id_and_a_secret() {
        let id = Some("1-abc.apps.googleusercontent.com");
        assert_eq!(Client::from_parts(id, Some(" s ")).unwrap().secret, "s");
        assert!(Client::from_parts(id, Some("")).is_none());
        assert!(Client::from_parts(id, None).is_none());
        assert!(Client::from_parts(Some("not-a-client"), Some("s")).is_none());
        assert!(Client::from_parts(None, Some("s")).is_none());
    }

    #[test]
    fn your_own_client_file_wins_and_a_bad_one_stays_an_error() {
        let dir = std::env::temp_dir().join(format!("chord-resolve-{}", random_urlsafe(6)));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("client_secret.json");
        std::fs::write(
            &path,
            r#"{"installed":{"client_id":"own.apps.googleusercontent.com","client_secret":"s"}}"#,
        )
        .unwrap();
        assert_eq!(
            Client::resolve(&path).unwrap().id,
            "own.apps.googleusercontent.com"
        );
        std::fs::write(&path, "{").unwrap();
        assert!(Client::resolve(&path).is_err());
        std::fs::remove_file(&path).unwrap();
        // Without a file, a source build (no built-in client) has nothing to use.
        assert_eq!(Client::resolve(&path).is_ok(), Client::builtin().is_some());
        assert_eq!(Client::available(&path), Client::builtin().is_some());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_sign_in_belongs_to_the_client_that_issued_it() {
        let client = |id: &str| Client {
            id: id.into(),
            secret: "s".into(),
        };
        let token = |id: &str| Token {
            access_token: "a".into(),
            refresh_token: "r".into(),
            expires_at: 0,
            client_id: id.into(),
        };
        assert!(token("a").issued_to(&client("a")));
        assert!(!token("a").issued_to(&client("b")));
        // Saved before Chord recorded the client: assume it still matches.
        assert!(token("").issued_to(&client("b")));
        let old: Token =
            serde_json::from_str(r#"{"access_token":"a","refresh_token":"r","expires_at":1}"#)
                .unwrap();
        assert!(old.client_id.is_empty());
    }
}
