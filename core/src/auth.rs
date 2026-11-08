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
}

impl Token {
    pub fn load(path: &Path) -> Result<Self> {
        let raw = std::fs::read_to_string(path)
            .map_err(|_| anyhow!("not signed in; run `chord-mail login` first"))?;
        serde_json::from_str(&raw).context("the saved token is corrupt; run `chord-mail login`")
    }

    /// Writes beside the target and renames, so a crash never leaves a
    /// half-written token, and creates the file 0600 so it is never readable
    /// by other users, even for an instant.
    pub fn save(&self, path: &Path) -> Result<()> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let tmp = path.with_extension("json.tmp");
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(&tmp)?;
        file.write_all(serde_json::to_string_pretty(self)?.as_bytes())?;
        file.sync_all()?;
        std::fs::rename(&tmp, path)?;
        Ok(())
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

async fn post_token(http: &reqwest::Client, form: &[(&str, &str)]) -> Result<TokenResponse> {
    let resp = http.post(TOKEN_URL).form(form).send().await?;
    let status = resp.status();
    let body = resp.text().await?;
    if !status.is_success() {
        let detail = serde_json::from_str::<TokenError>(&body)
            .map(|e| match e.error_description {
                Some(d) => format!("{}: {d}", e.error),
                None => e.error,
            })
            .unwrap_or(body);
        bail!("Google refused the token request ({status}): {detail}");
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
    if !token.is_fresh() {
        let resp = post_token(
            http,
            &[
                ("client_id", &client.id),
                ("client_secret", &client.secret),
                ("refresh_token", &token.refresh_token),
                ("grant_type", "refresh_token"),
            ],
        )
        .await
        .context("couldn't refresh the session; run `chord-mail login` again")?;
        token.access_token = resp.access_token;
        token.expires_at = now() + resp.expires_in;
        if let Some(refresh) = resp.refresh_token {
            token.refresh_token = refresh;
        }
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
        };
        token.save(&path).unwrap();
        let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
        assert_eq!(Token::load(&path).unwrap().refresh_token, "r");
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
}
