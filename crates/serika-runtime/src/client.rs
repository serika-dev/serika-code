use std::time::Duration;

use futures_util::StreamExt;
use serde_json::{json, Value};
use tokio_util::sync::CancellationToken;

use crate::credentials::StoredSession;
use crate::error::{Error, Result};
use crate::model::{AccountProfile, ModelCatalog, UsageLimitBody, UsageSnapshot};

pub const VERSION: &str = env!("CARGO_PKG_VERSION");

#[derive(Clone)]
pub struct SerikaClient {
    http: reqwest::Client,
    pub api_base: String,
    pub accounts_base: String,
}

#[derive(Debug, Clone)]
pub struct ChatTurn {
    pub role: String,
    pub content: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SseItem {
    Content(String),
    Reasoning(String),
    Done,
    Error(String),
}

#[derive(Debug)]
pub enum LoginOutcome {
    SignedIn(StoredSession),
    NeedsSecondFactor { message: String },
}

impl SerikaClient {
    pub fn new(api_base: impl Into<String>, accounts_base: impl Into<String>) -> Result<Self> {
        let http = reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(10))
            .build()?;
        Ok(Self {
            http,
            api_base: api_base.into().trim_end_matches('/').to_string(),
            accounts_base: accounts_base.into().trim_end_matches('/').to_string(),
        })
    }

    pub fn http(&self) -> &reqwest::Client {
        &self.http
    }

    pub async fn models(&self) -> Result<ModelCatalog> {
        let value = self.get_json(&self.api("/api/chat/models"), None).await?;
        Ok(serde_json::from_value(value)?)
    }

    pub async fn usage(&self, token: &str) -> Result<UsageSnapshot> {
        let value = self.get_json(&self.api("/api/chat/usage"), Some(token)).await?;
        Ok(serde_json::from_value(value)?)
    }

    pub async fn account(&self, token: &str) -> Result<AccountProfile> {
        let value = self
            .get_json(&self.accounts("/api/user/me"), Some(token))
            .await?;
        AccountProfile::from_value(&value).ok_or_else(|| Error::msg("account response was missing a user"))
    }

    pub async fn login_password(
        &self,
        email: &str,
        password: &str,
        two_factor: Option<&str>,
    ) -> Result<LoginOutcome> {
        let mut body = json!({
            "email": email,
            "password": password,
            "rememberMe": true,
            "productId": "serika-code",
        });
        if let Some(code) = two_factor {
            body["twoFactorCode"] = json!(code);
        }
        self.finish_login(self.post_auth("/api/auth/login", body).await?)
    }

    pub async fn request_magic_link(&self, email: &str) -> Result<String> {
        let value = self
            .post_auth("/api/auth/magic-link", json!({ "email": email }))
            .await?;
        if let Some(message) = value.get("error").and_then(value_message) {
            return Err(Error::Auth(message));
        }
        Ok(value
            .get("message")
            .and_then(|item| item.as_str())
            .unwrap_or("If an account with email sign-in exists for this address, a link was sent.")
            .to_string())
    }

    pub async fn consume_magic_link(&self, token: &str, two_factor: Option<&str>) -> Result<LoginOutcome> {
        let mut body = json!({ "token": token, "rememberMe": true });
        if let Some(code) = two_factor {
            body["twoFactorCode"] = json!(code);
        }
        self.finish_login(self.post_auth("/api/auth/magic-link/consume", body).await?)
    }

    pub async fn refresh(&self, refresh_token: &str) -> Result<(String, String)> {
        let value = self
            .post_auth("/api/auth/refresh", json!({ "refreshToken": refresh_token }))
            .await?;
        let token = value
            .get("token")
            .and_then(|item| item.as_str())
            .ok_or_else(|| Error::Auth(message_of(&value)))?
            .to_string();
        let refresh = value
            .get("refreshToken")
            .and_then(|item| item.as_str())
            .unwrap_or(refresh_token)
            .to_string();
        Ok((token, refresh))
    }

    /// `POST /api/auth/logout` revokes the session stored in the `auth_token` cookie.
    /// The route does not read `x-auth-token`, so the cookie is how a native client targets one session.
    pub async fn logout(&self, token: &str) -> Result<()> {
        let url = self.accounts("/api/auth/logout");
        let response = self
            .http
            .post(url)
            .headers(common_headers(Some(token)))
            .header("cookie", format!("auth_token={token}"))
            .timeout(Duration::from_secs(20))
            .send()
            .await?;
        let status = response.status();
        if status.is_success() || status.as_u16() == 401 {
            return Ok(());
        }
        let text = response.text().await.unwrap_or_default();
        Err(Error::Auth(redact(&text)))
    }

    pub async fn chat(
        &self,
        token: &str,
        model_id: &str,
        message: &str,
        history: &[ChatTurn],
        cancel: &CancellationToken,
        mut on_content: impl FnMut(&str),
        mut on_reasoning: impl FnMut(&str),
    ) -> Result<String> {
        let history_json: Vec<Value> = history
            .iter()
            .map(|turn| json!({ "role": turn.role, "content": turn.content }))
            .collect();
        // Temporary chats are still metered by usageMeter.assertCanChat. They are not
        // stored in the user's chat.serika.dev history. mcp is disabled because those
        // connectors run on the server; local tools are executed by this process.
        let body = json!({
            "message": message,
            "modelID": model_id,
            "stream": true,
            "temporary": true,
            "conversationHistory": history_json,
            "webSearch": false,
            "deepResearch": false,
            "mcp": false,
            "imageRequest": false,
        });
        let url = self.api("/api/chat/message");
        let request = self
            .http
            .post(url)
            .headers(common_headers(Some(token)))
            .json(&body)
            .timeout(Duration::from_secs(600));
        let response = tokio::select! {
            _ = cancel.cancelled() => return Err(Error::Cancelled),
            response = request.send() => response?,
        };
        let status = response.status();
        if !status.is_success() {
            let text = response.text().await.unwrap_or_default();
            return Err(error_from_status(status.as_u16(), &text));
        }
        let mut stream = response.bytes_stream();
        let mut buffer = String::new();
        let mut full = String::new();
        loop {
            let next = tokio::select! {
                _ = cancel.cancelled() => return Err(Error::Cancelled),
                chunk = stream.next() => chunk,
            };
            let Some(chunk) = next else { break };
            let bytes = chunk?;
            buffer.push_str(&String::from_utf8_lossy(&bytes));
            for item in drain_sse(&mut buffer) {
                match item {
                    SseItem::Content(text) => {
                        full.push_str(&text);
                        on_content(&text);
                    }
                    SseItem::Reasoning(text) => on_reasoning(&text),
                    SseItem::Done => return Ok(full),
                    SseItem::Error(message) => return Err(Error::msg(message)),
                }
            }
        }
        if full.is_empty() && !buffer.trim().is_empty() {
            if let Ok(value) = serde_json::from_str::<Value>(&buffer) {
                if let Some(content) = value.get("content").and_then(|item| item.as_str()) {
                    return Ok(content.to_string());
                }
                if value.get("error").is_some() {
                    return Err(Error::msg(message_of(&value)));
                }
            }
        }
        Ok(full)
    }

    fn api(&self, path: &str) -> String {
        format!("{}{}", self.api_base, path)
    }

    fn accounts(&self, path: &str) -> String {
        format!("{}{}", self.accounts_base, path)
    }

    async fn get_json(&self, url: &str, token: Option<&str>) -> Result<Value> {
        let response = self
            .http
            .get(url)
            .headers(common_headers(token))
            .timeout(Duration::from_secs(30))
            .send()
            .await?;
        decode_json(response).await
    }

    async fn post_auth(&self, path: &str, body: Value) -> Result<Value> {
        let response = self
            .http
            .post(self.accounts(path))
            .headers(common_headers(None))
            .json(&body)
            .timeout(Duration::from_secs(30))
            .send()
            .await?;
        let status = response.status();
        let text = response.text().await.unwrap_or_default();
        let value: Value = serde_json::from_str(&text).unwrap_or(Value::String(text.clone()));
        if status.as_u16() == 403 && value.get("code").and_then(|c| c.as_str()) == Some("TWO_FACTOR_REQUIRED") {
            return Ok(value);
        }
        if !status.is_success() {
            return Err(error_from_status(status.as_u16(), &text));
        }
        Ok(value)
    }

    fn finish_login(&self, value: Value) -> Result<LoginOutcome> {
        if value.get("code").and_then(|code| code.as_str()) == Some("TWO_FACTOR_REQUIRED") {
            return Ok(LoginOutcome::NeedsSecondFactor {
                message: message_of(&value),
            });
        }
        let token = value.get("token").and_then(|item| item.as_str()).unwrap_or("");
        let refresh = value.get("refreshToken").and_then(|item| item.as_str()).unwrap_or("");
        let profile = value.get("user").and_then(AccountProfile::from_value);
        if token.is_empty() || refresh.is_empty() || profile.is_none() {
            return Err(Error::Auth(message_of(&value)));
        }
        let profile = profile.unwrap();
        Ok(LoginOutcome::SignedIn(StoredSession {
            token: token.to_string(),
            refresh_token: refresh.to_string(),
            user_id: profile.id,
            username: profile.username,
            email: profile.email,
            is_premium: profile.is_premium,
        }))
    }
}

fn common_headers(token: Option<&str>) -> reqwest::header::HeaderMap {
    let mut headers = reqwest::header::HeaderMap::new();
    headers.insert(
        reqwest::header::USER_AGENT,
        format!("serika-code/{VERSION}")
            .parse()
            .unwrap_or(reqwest::header::HeaderValue::from_static("serika-code")),
    );
    headers.insert(
        reqwest::header::ACCEPT,
        reqwest::header::HeaderValue::from_static("application/json, text/event-stream"),
    );
    // The API's IP limiter exempts native desktop clients. This does not change usage quotas.
    headers.insert(
        "x-desktop-app",
        reqwest::header::HeaderValue::from_static("true"),
    );
    if let Some(token) = token {
        if let Ok(value) = reqwest::header::HeaderValue::from_str(token) {
            headers.insert("x-auth-token", value);
        }
    }
    headers
}

async fn decode_json(response: reqwest::Response) -> Result<Value> {
    let status = response.status();
    let text = response.text().await.unwrap_or_default();
    if !status.is_success() {
        return Err(error_from_status(status.as_u16(), &text));
    }
    serde_json::from_str(&text).map_err(|err| Error::msg(format!("invalid JSON from Serika ({err})")))
}

pub fn error_from_status(status: u16, text: &str) -> Error {
    let value: Value = serde_json::from_str(text).unwrap_or(Value::Null);
    if status == 429 {
        if let Ok(limit) = serde_json::from_value::<UsageLimitBody>(value.clone()) {
            if limit.limit_reached == Some(true) || limit.code.is_some() || limit.error.is_some() {
                return Error::UsageLimit(limit);
            }
        }
        return Error::RateLimited(message_of(&value).if_empty(text));
    }
    if status == 401 {
        return Error::Auth(message_of(&value).if_empty("authentication failed"));
    }
    Error::Api {
        status,
        message: redact(&message_of(&value).if_empty(text)),
        code: value.get("code").and_then(|code| code.as_str()).map(str::to_string),
    }
}

fn message_of(value: &Value) -> String {
    if let Some(text) = value.as_str() {
        return redact(text);
    }
    if let Some(text) = value.get("error").and_then(value_message) {
        return text;
    }
    for key in ["error_description", "details", "message", "msg"] {
        if let Some(text) = value.get(key).and_then(|item| item.as_str()) {
            return redact(text);
        }
    }
    "request failed".to_string()
}

fn value_message(value: &Value) -> Option<String> {
    match value {
        Value::String(text) => Some(redact(text)),
        Value::Object(map) => map
            .get("message")
            .and_then(|item| item.as_str())
            .map(redact),
        _ => None,
    }
}

trait IfEmpty {
    fn if_empty(self, fallback: &str) -> String;
}

impl IfEmpty for String {
    fn if_empty(self, fallback: &str) -> String {
        if self.trim().is_empty() || self == "request failed" {
            let fallback = fallback.trim();
            if fallback.is_empty() {
                self
            } else {
                redact(fallback)
            }
        } else {
            self
        }
    }
}

pub fn redact(text: &str) -> String {
    let mut out = String::new();
    for token in text.split_whitespace() {
        if !out.is_empty() {
            out.push(' ');
        }
        if looks_like_secret(token) {
            out.push_str("[redacted]");
        } else {
            out.push_str(token);
        }
    }
    if out.chars().count() > 500 {
        out.truncate(500);
        out.push('…');
    }
    out
}

fn looks_like_secret(token: &str) -> bool {
    let token = token.trim_matches(|ch| matches!(ch, '"' | '\'' | ',' | ';' ));
    token.starts_with("sk-")
        || token.starts_with("ghp_")
        || token.starts_with("gho_")
        || token.matches('.').count() == 2 && token.len() > 40
}

/// Pull complete SSE events out of `buffer`, leaving a partial event in place.
pub fn drain_sse(buffer: &mut String) -> Vec<SseItem> {
    let mut items = Vec::new();
    while let Some(index) = buffer.find("\n\n") {
        let raw = buffer[..index].to_string();
        buffer.replace_range(..index + 2, "");
        if let Some(item) = parse_sse_event(&raw) {
            items.push(item);
        }
    }
    items
}

fn parse_sse_event(raw: &str) -> Option<SseItem> {
    let mut data = String::new();
    for line in raw.lines() {
        let line = line.trim_end_matches('\r');
        if let Some(rest) = line.strip_prefix("data:") {
            if !data.is_empty() {
                data.push('\n');
            }
            data.push_str(rest.trim_start());
        }
    }
    let data = data.trim();
    if data.is_empty() {
        return None;
    }
    if data == "[DONE]" {
        return Some(SseItem::Done);
    }
    if data == "[ERROR]" {
        return Some(SseItem::Error("AI generation failed".into()));
    }
    let value: Value = serde_json::from_str(data).ok()?;
    if let Some(message) = value.get("error").and_then(value_message) {
        let details = value.get("details").and_then(|item| item.as_str()).unwrap_or("");
        if details.is_empty() {
            return Some(SseItem::Error(message));
        }
        return Some(SseItem::Error(format!("{message}: {details}")));
    }
    let content = value.get("content").and_then(|item| item.as_str()).unwrap_or("");
    let reasoning = value.get("reasoning").and_then(|item| item.as_str()).unwrap_or("");
    if !content.is_empty() {
        return Some(SseItem::Content(content.to_string()));
    }
    if !reasoning.is_empty() {
        return Some(SseItem::Reasoning(reasoning.to_string()));
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sse_survives_split_chunks_and_redacts_tokens() {
        let mut buffer = String::new();
        buffer.push_str("data: {\"content\":\"hel");
        assert!(drain_sse(&mut buffer).is_empty());
        buffer.push_str("lo\"}\n\ndata: {\"reasoning\":\"think\"}\n\ndata: [DONE]\n\n");
        let items = drain_sse(&mut buffer);
        assert_eq!(
            items,
            vec![
                SseItem::Content("hello".into()),
                SseItem::Reasoning("think".into()),
                SseItem::Done,
            ]
        );
        let err = error_from_status(
            429,
            r#"{"error":"You've used this week's free allowance.","code":"WEEKLY_LIMIT_REACHED","limitReached":true,"limit":10000,"used":10000}"#,
        );
        assert!(matches!(err, Error::UsageLimit(_)));
        assert!(redact("header eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiIxIn0.signature tail").contains("[redacted]"));
        assert!(!redact("normal sentence").contains("[redacted]"));
    }

    #[tokio::test]
    async fn chat_stream_against_a_local_server() {
        let (base, _task) = crate::testsupport::spawn(|request| {
            assert!(request.body.contains("temporary"));
            assert!(request.body.contains("\"mcp\":false"));
            crate::testsupport::Response {
                status: 200,
                content_type: "text/event-stream",
                body: "data: {\"content\":\"Hello\"}\n\ndata: {\"content\":\" there\"}\n\ndata: [DONE]\n\n".into(),
            }
        })
        .await;
        let client = SerikaClient::new(base, "http://127.0.0.1:1").unwrap();
        let mut seen = String::new();
        let full = client
            .chat(
                "token",
                "deepseek/deepseek-v4.1-flash",
                "hi",
                &[],
                &CancellationToken::new(),
                |chunk| seen.push_str(chunk),
                |_| {},
            )
            .await
            .unwrap();
        assert_eq!(full, "Hello there");
        assert_eq!(seen, full);
    }
}
