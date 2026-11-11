//! A thin client for the parts of the Gmail REST API that Chord uses.
//!
//! Sync and the action queue talk to `MailApi` rather than to `Gmail`
//! directly, so tests can run them against a fake mailbox, and another
//! provider can slot in later.

use anyhow::{Result, anyhow, bail};
use base64::Engine;
use base64::engine::general_purpose::{URL_SAFE, URL_SAFE_NO_PAD};
use serde::Deserialize;
use serde::de::DeserializeOwned;
use serde_json::{Value, json};
use std::time::Duration;

const BASE: &str = "https://gmail.googleapis.com/gmail/v1/users/me";

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Profile {
    pub email_address: String,
    pub history_id: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MessageRef {
    pub id: String,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MessagePage {
    #[serde(default)]
    pub messages: Vec<MessageRef>,
    pub next_page_token: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Header {
    pub name: String,
    pub value: String,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct Payload {
    #[serde(default)]
    pub headers: Vec<Header>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Message {
    pub id: String,
    pub thread_id: String,
    #[serde(default)]
    pub label_ids: Vec<String>,
    #[serde(default)]
    pub snippet: String,
    /// Milliseconds since the epoch, sent as a string.
    #[serde(default)]
    pub internal_date: String,
    #[serde(default)]
    pub size_estimate: i64,
    #[serde(default)]
    pub payload: Payload,
}

impl Message {
    pub fn header(&self, name: &str) -> Option<&str> {
        self.payload
            .headers
            .iter()
            .find(|h| h.name.eq_ignore_ascii_case(name))
            .map(|h| h.value.as_str())
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct Thread {
    pub id: String,
    #[serde(default)]
    pub messages: Vec<Message>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HistoryEvent {
    pub message: MessageRef,
}

/// One entry of the mailbox's change log. Chord only needs to know which
/// messages were touched and which are gone; it refetches the touched ones.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct History {
    #[serde(default)]
    pub messages_added: Vec<HistoryEvent>,
    #[serde(default)]
    pub messages_deleted: Vec<HistoryEvent>,
    #[serde(default)]
    pub labels_added: Vec<HistoryEvent>,
    #[serde(default)]
    pub labels_removed: Vec<HistoryEvent>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HistoryPage {
    #[serde(default)]
    pub history: Vec<History>,
    pub next_page_token: Option<String>,
    pub history_id: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Label {
    pub id: String,
    pub name: String,
    #[serde(rename = "type", default)]
    pub kind: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SendAs {
    pub send_as_email: String,
    #[serde(default)]
    pub display_name: String,
    #[serde(default)]
    pub signature: String,
    #[serde(default)]
    pub is_default: bool,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Draft {
    pub id: String,
    pub message: DraftMessage,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DraftMessage {
    pub id: String,
    #[serde(default)]
    pub thread_id: String,
}

/// Errors callers must react to rather than just report.
#[derive(Debug)]
pub enum ApiError {
    /// The message or thread no longer exists.
    NotFound,
    /// The history id is too old for Gmail to replay; a full sync is needed.
    HistoryExpired,
    /// Gmail rejected the request itself (bad label, bad id). Retrying the
    /// same request will never work.
    Rejected(String),
    /// Any other error status. Typed so the action queue can tell Gmail
    /// turning a request down from throttling or a server failure.
    Status {
        status: u16,
        /// A 403 that is really Gmail asking us to slow down.
        rate_limited: bool,
        message: String,
    },
}

impl std::fmt::Display for ApiError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ApiError::NotFound => write!(f, "not found"),
            ApiError::HistoryExpired => write!(f, "history is too old to replay"),
            ApiError::Rejected(m) => write!(f, "Gmail rejected the request: {m}"),
            ApiError::Status { message, .. } => write!(f, "{message}"),
        }
    }
}

impl std::error::Error for ApiError {}

pub fn is_not_found(e: &anyhow::Error) -> bool {
    matches!(e.downcast_ref(), Some(ApiError::NotFound))
}

/// Whether an error will happen again no matter how often it's retried.
pub fn is_permanent(e: &anyhow::Error) -> bool {
    matches!(
        e.downcast_ref(),
        Some(ApiError::NotFound | ApiError::Rejected(_))
    )
}

fn unsupported<T>() -> Result<T> {
    bail!("not supported by this mailbox")
}

// Futures from these methods are driven on one task at a time, so they don't
// need to be Send. Every method a fake doesn't need has a default.
#[allow(async_fn_in_trait)]
pub trait MailApi {
    async fn profile(&self) -> Result<Profile>;
    async fn list_messages(&self, label: &str, page_token: Option<&str>) -> Result<MessagePage>;
    /// Headers, labels, and snippet, but not the body.
    async fn message_metadata(&self, id: &str) -> Result<Message>;
    async fn history(&self, start: &str, page_token: Option<&str>) -> Result<HistoryPage>;

    /// Every message of a thread, as metadata.
    async fn thread(&self, _id: &str) -> Result<Thread> {
        unsupported()
    }
    /// The full RFC 822 source of a message.
    async fn message_raw(&self, _id: &str) -> Result<Vec<u8>> {
        unsupported()
    }
    async fn modify(&self, _ids: &[String], _add: &[String], _remove: &[String]) -> Result<()> {
        unsupported()
    }
    async fn trash_thread(&self, _id: &str) -> Result<()> {
        unsupported()
    }
    async fn untrash_thread(&self, _id: &str) -> Result<()> {
        unsupported()
    }
    async fn labels(&self) -> Result<Vec<Label>> {
        unsupported()
    }
    async fn create_label(&self, _name: &str) -> Result<Label> {
        unsupported()
    }
    /// Sends a message and returns (message id, thread id).
    async fn send(&self, _raw: &[u8], _thread_id: Option<&str>) -> Result<(String, String)> {
        unsupported()
    }
    async fn create_draft(&self, _raw: &[u8], _thread_id: Option<&str>) -> Result<Draft> {
        unsupported()
    }
    async fn send_draft(&self, _id: &str) -> Result<()> {
        unsupported()
    }
    async fn delete_draft(&self, _id: &str) -> Result<()> {
        unsupported()
    }
    async fn send_as(&self) -> Result<Vec<SendAs>> {
        unsupported()
    }
    /// Thread ids matching a Gmail search, newest first.
    async fn search_threads(
        &self,
        _query: &str,
        _label: Option<&str>,
        _max: u32,
    ) -> Result<Vec<String>> {
        unsupported()
    }
}

#[derive(Clone)]
pub struct Gmail {
    http: reqwest::Client,
    token: String,
}

impl Gmail {
    pub fn new(http: reqwest::Client, access_token: String) -> Self {
        Self {
            http,
            token: access_token,
        }
    }

    async fn call<T: DeserializeOwned>(
        &self,
        method: reqwest::Method,
        path: &str,
        query: &[(&str, &str)],
        body: Option<Value>,
    ) -> Result<T> {
        // Gmail meters each user per minute. When it says slow down, wait
        // and retry (1s, 2s, 4s, … about a minute in all) rather than fail
        // and redo the work.
        let mut delay = Duration::from_secs(1);
        let (status, text) = loop {
            let mut req = self
                .http
                .request(method.clone(), format!("{BASE}{path}"))
                .bearer_auth(&self.token)
                .query(query);
            if let Some(body) = &body {
                req = req.json(body);
            }
            let resp = req.send().await?;
            let status = resp.status();
            let text = resp.text().await?;
            if is_rate_limited(status, &text) && delay <= Duration::from_secs(32) {
                tokio::time::sleep(delay).await;
                delay *= 2;
                continue;
            }
            break (status, text);
        };
        if status == reqwest::StatusCode::NOT_FOUND {
            return Err(ApiError::NotFound.into());
        }
        if !status.is_success() {
            #[derive(Deserialize)]
            struct Envelope {
                error: Detail,
            }
            #[derive(Deserialize)]
            struct Detail {
                message: String,
            }
            let rate_limited = is_rate_limited(status, &text);
            let detail = serde_json::from_str::<Envelope>(&text)
                .map(|e| e.error.message)
                .unwrap_or(text);
            // 400 means the request itself is wrong; 401, 403 (rate limits
            // come back as 403), 429, and 5xx may pass on a retry.
            if status == reqwest::StatusCode::BAD_REQUEST {
                return Err(ApiError::Rejected(detail).into());
            }
            return Err(ApiError::Status {
                status: status.as_u16(),
                rate_limited,
                message: format!("Gmail returned {status} for {path}: {detail}"),
            }
            .into());
        }
        if text.trim().is_empty() {
            return Ok(serde_json::from_str("null")?);
        }
        Ok(serde_json::from_str(&text)?)
    }

    async fn get<T: DeserializeOwned>(&self, path: &str, query: &[(&str, &str)]) -> Result<T> {
        self.call(reqwest::Method::GET, path, query, None).await
    }

    async fn post<T: DeserializeOwned>(&self, path: &str, body: Value) -> Result<T> {
        self.call(reqwest::Method::POST, path, &[], Some(body))
            .await
    }
}

/// Gmail signals throttling as 429, or as 403 with a rate or quota reason.
fn is_rate_limited(status: reqwest::StatusCode, body: &str) -> bool {
    status == reqwest::StatusCode::TOO_MANY_REQUESTS
        || status == reqwest::StatusCode::FORBIDDEN
            && [
                "rateLimitExceeded",
                "userRateLimitExceeded",
                "Quota exceeded",
            ]
            .iter()
            .any(|reason| body.contains(reason))
}

fn encode_raw(raw: &[u8]) -> String {
    URL_SAFE_NO_PAD.encode(raw)
}

impl MailApi for Gmail {
    async fn profile(&self) -> Result<Profile> {
        self.get("/profile", &[]).await
    }

    async fn list_messages(&self, label: &str, page_token: Option<&str>) -> Result<MessagePage> {
        let mut query = vec![("labelIds", label), ("maxResults", "100")];
        if let Some(t) = page_token {
            query.push(("pageToken", t));
        }
        self.get("/messages", &query).await
    }

    async fn message_metadata(&self, id: &str) -> Result<Message> {
        self.get(
            &format!("/messages/{id}"),
            &[
                ("format", "metadata"),
                ("metadataHeaders", "From"),
                ("metadataHeaders", "To"),
                ("metadataHeaders", "Cc"),
                ("metadataHeaders", "Subject"),
            ],
        )
        .await
    }

    async fn history(&self, start: &str, page_token: Option<&str>) -> Result<HistoryPage> {
        let mut query = vec![("startHistoryId", start), ("maxResults", "500")];
        if let Some(t) = page_token {
            query.push(("pageToken", t));
        }
        // Gmail answers 404 when the start id has aged out of its log.
        self.get("/history", &query).await.map_err(|e| {
            if is_not_found(&e) {
                ApiError::HistoryExpired.into()
            } else {
                e
            }
        })
    }

    async fn thread(&self, id: &str) -> Result<Thread> {
        self.get(
            &format!("/threads/{id}"),
            &[
                ("format", "metadata"),
                ("metadataHeaders", "From"),
                ("metadataHeaders", "To"),
                ("metadataHeaders", "Cc"),
                ("metadataHeaders", "Subject"),
            ],
        )
        .await
    }

    async fn message_raw(&self, id: &str) -> Result<Vec<u8>> {
        #[derive(Deserialize)]
        struct Raw {
            raw: String,
        }
        let r: Raw = self
            .get(&format!("/messages/{id}"), &[("format", "raw")])
            .await?;
        // Gmail documents base64url but has been seen to pad it, so accept both.
        let trimmed = r.raw.trim_end_matches('=');
        URL_SAFE_NO_PAD
            .decode(trimmed)
            .or_else(|_| URL_SAFE.decode(&r.raw))
            .map_err(|e| anyhow!("Gmail sent an unreadable message body: {e}"))
    }

    async fn modify(&self, ids: &[String], add: &[String], remove: &[String]) -> Result<()> {
        for chunk in ids.chunks(1000) {
            let _: Value = self
                .post(
                    "/messages/batchModify",
                    json!({ "ids": chunk, "addLabelIds": add, "removeLabelIds": remove }),
                )
                .await?;
        }
        Ok(())
    }

    async fn trash_thread(&self, id: &str) -> Result<()> {
        let _: Value = self
            .post(&format!("/threads/{id}/trash"), json!({}))
            .await?;
        Ok(())
    }

    async fn untrash_thread(&self, id: &str) -> Result<()> {
        let _: Value = self
            .post(&format!("/threads/{id}/untrash"), json!({}))
            .await?;
        Ok(())
    }

    async fn labels(&self) -> Result<Vec<Label>> {
        #[derive(Deserialize)]
        struct List {
            #[serde(default)]
            labels: Vec<Label>,
        }
        Ok(self.get::<List>("/labels", &[]).await?.labels)
    }

    async fn create_label(&self, name: &str) -> Result<Label> {
        self.post(
            "/labels",
            json!({ "name": name, "labelListVisibility": "labelShow", "messageListVisibility": "show" }),
        )
        .await
    }

    async fn send(&self, raw: &[u8], thread_id: Option<&str>) -> Result<(String, String)> {
        let mut body = json!({ "raw": encode_raw(raw) });
        if let Some(t) = thread_id {
            body["threadId"] = json!(t);
        }
        let sent: DraftMessage = self.post("/messages/send", body).await?;
        Ok((sent.id, sent.thread_id))
    }

    async fn create_draft(&self, raw: &[u8], thread_id: Option<&str>) -> Result<Draft> {
        let mut message = json!({ "raw": encode_raw(raw) });
        if let Some(t) = thread_id {
            message["threadId"] = json!(t);
        }
        self.post("/drafts", json!({ "message": message })).await
    }

    async fn send_draft(&self, id: &str) -> Result<()> {
        let _: Value = self.post("/drafts/send", json!({ "id": id })).await?;
        Ok(())
    }

    async fn delete_draft(&self, id: &str) -> Result<()> {
        let _: Value = self
            .call(reqwest::Method::DELETE, &format!("/drafts/{id}"), &[], None)
            .await?;
        Ok(())
    }

    async fn send_as(&self) -> Result<Vec<SendAs>> {
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct List {
            #[serde(default)]
            send_as: Vec<SendAs>,
        }
        Ok(self.get::<List>("/settings/sendAs", &[]).await?.send_as)
    }

    async fn search_threads(
        &self,
        query: &str,
        label: Option<&str>,
        max: u32,
    ) -> Result<Vec<String>> {
        #[derive(Deserialize)]
        struct List {
            #[serde(default)]
            threads: Vec<MessageRef>,
        }
        let max = max.to_string();
        let mut q = vec![("maxResults", max.as_str())];
        if !query.is_empty() {
            q.push(("q", query));
        }
        if let Some(l) = label {
            q.push(("labelIds", l));
        }
        Ok(self
            .get::<List>("/threads", &q)
            .await?
            .threads
            .into_iter()
            .map(|t| t.id)
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognizes_gmail_throttling() {
        let quota = "Quota exceeded for quota metric 'Total Query Cost' and limit 'Units per minute per user'";
        assert!(is_rate_limited(reqwest::StatusCode::FORBIDDEN, quota));
        assert!(is_rate_limited(reqwest::StatusCode::TOO_MANY_REQUESTS, ""));
        assert!(!is_rate_limited(
            reqwest::StatusCode::FORBIDDEN,
            "Gmail API has not been used in project"
        ));
    }
}
