//! Gateway API client for creating GitHub issues.

use serde::{Deserialize, Serialize};
use std::time::Duration;
use uuid::Uuid;

use crate::http_client;

/// Base URL for the issue gateway API.
pub const BASE_URL: &str = "https://sempal-gitissue-gateway.portalsurfer.workers.dev";
/// Direct URL for starting the auth flow in a browser.
pub const AUTH_START_URL: &str =
    "https://sempal-gitissue-gateway.portalsurfer.workers.dev/auth/start";

const MAX_AUTH_RESPONSE_BYTES: usize = 64 * 1024;
const MAX_ISSUE_RESPONSE_BYTES: usize = 256 * 1024;
const ISSUE_RETRY_CONFIG: http_client::RetryConfig = http_client::RetryConfig {
    max_attempts: 3,
    base_delay: Duration::from_millis(200),
    max_delay: Duration::from_secs(2),
};

/// The kind of issue the user is submitting.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IssueKind {
    FeatureRequest,
    Bug,
}

impl IssueKind {
    /// Return the title prefix used when creating an issue of this kind.
    pub fn title_prefix(self) -> &'static str {
        match self {
            Self::FeatureRequest => "FR: ",
            Self::Bug => "Bug: ",
        }
    }
}

/// Payload sent to the issue gateway when creating a GitHub issue.
#[derive(Clone, Debug, Serialize)]
pub struct CreateIssueRequest {
    /// Issue title, including any prefix for routing.
    pub title: String,
    /// Optional issue body supplied by the user.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub body: Option<String>,
}

/// Successful issue creation response returned by the gateway.
#[derive(Clone, Debug, Deserialize)]
pub struct CreateIssueResponse {
    /// Whether the gateway reported success.
    pub ok: bool,
    /// URL for the created GitHub issue.
    pub issue_url: String,
    /// GitHub issue number.
    pub number: u64,
}

/// Error states surfaced when creating a GitHub issue.
#[derive(Debug, thiserror::Error)]
pub enum CreateIssueError {
    #[error("Token invalid or expired")]
    Unauthorized,
    #[error("Invalid input: {0}")]
    BadRequest(String),
    #[error("Rate limited; try again later")]
    RateLimited,
    #[error("Server error: {0}")]
    ServerError(String),
    #[error("HTTP error: {0}")]
    Transport(String),
    #[error("JSON error: {0}")]
    Json(String),
}

/// Error states surfaced when fetching or polling auth tokens.
#[derive(Debug, thiserror::Error)]
pub enum IssueAuthError {
    #[error("Invalid auth response: {0}")]
    InvalidResponse(String),
    #[error("Server error: {0}")]
    ServerError(String),
    #[error("HTTP error: {0}")]
    Transport(String),
}

/// Start an auth session and return the token produced by the gateway.
pub fn fetch_issue_token() -> Result<String, IssueAuthError> {
    let response = match get_with_retry(AUTH_START_URL) {
        Ok(response) => response,
        Err(ureq::Error::Status(code, response)) => {
            let body =
                read_body_limited(response, MAX_AUTH_RESPONSE_BYTES).unwrap_or_else(|err| err);
            return Err(IssueAuthError::ServerError(format!("HTTP {code}: {body}")));
        }
        Err(ureq::Error::Transport(err)) => {
            return Err(IssueAuthError::Transport(err.to_string()));
        }
    };

    let body = read_body_limited(response, MAX_AUTH_RESPONSE_BYTES)
        .map_err(IssueAuthError::InvalidResponse)?;
    parse_issue_token(&body)
}

/// Poll for a token using a request ID.
pub fn poll_issue_token(request_id: &str) -> Result<Option<String>, IssueAuthError> {
    let url = format!("{BASE_URL}/auth/poll?requestId={}", encodeURIComponent(request_id));
    let response = match get_with_retry(&url) {
        Ok(response) => response,
        Err(ureq::Error::Status(202, _)) => return Ok(None),
        Err(ureq::Error::Status(code, response)) => {
            let body =
                read_body_limited(response, MAX_AUTH_RESPONSE_BYTES).unwrap_or_else(|err| err);
            return Err(IssueAuthError::ServerError(format!("HTTP {code}: {body}")));
        }
        Err(ureq::Error::Transport(err)) => {
            return Err(IssueAuthError::Transport(err.to_string()));
        }
    };

    let body = read_body_limited(response, MAX_AUTH_RESPONSE_BYTES)
        .map_err(IssueAuthError::InvalidResponse)?;
    
    #[derive(Deserialize)]
    struct PollResponse {
        ok: bool,
        #[serde(rename = "sessionId")]
        session_id: Option<String>,
        error: Option<String>,
    }
    
    let parsed: PollResponse = serde_json::from_str(&body)
        .map_err(|err| IssueAuthError::InvalidResponse(err.to_string()))?;
        
    if let Some(token) = parsed.session_id {
        if looks_like_issue_token(&token) {
            return Ok(Some(token));
        }
    }
    
    if let Some(err) = parsed.error {
        return Err(IssueAuthError::ServerError(err));
    }
    
    Ok(None)
}

fn encodeURIComponent(s: &str) -> String {
    url::form_urlencoded::byte_serialize(s.as_bytes()).collect()
}

/// Create a GitHub issue through the gateway with idempotent retry support.
pub fn create_issue(
    token: &str,
    request: &CreateIssueRequest,
) -> Result<CreateIssueResponse, CreateIssueError> {
    create_issue_with_url(BASE_URL, token, request)
}

fn create_issue_with_url(
    base_url: &str,
    token: &str,
    request: &CreateIssueRequest,
) -> Result<CreateIssueResponse, CreateIssueError> {
    let url = format!("{base_url}/issue");
    let idempotency_key = format!("issue-{}", Uuid::new_v4());
    let response = match post_with_retry(&url, token, request, &idempotency_key) {
        Ok(response) => response,
        Err(ureq::Error::Status(code, response)) => {
            let body =
                read_body_limited(response, MAX_ISSUE_RESPONSE_BYTES).unwrap_or_else(|err| err);
            return Err(map_status_error(code, body));
        }
        Err(ureq::Error::Transport(err)) => {
            return Err(CreateIssueError::Transport(err.to_string()));
        }
    };

    let body =
        read_body_limited(response, MAX_ISSUE_RESPONSE_BYTES).map_err(CreateIssueError::Json)?;
    parse_create_issue_response(&body)
}

fn map_status_error(code: u16, body: String) -> CreateIssueError {
    match code {
        400 => CreateIssueError::BadRequest(body),
        401 => CreateIssueError::Unauthorized,
        429 => CreateIssueError::RateLimited,
        500..=599 => CreateIssueError::ServerError(body),
        _ => CreateIssueError::Transport(format!("HTTP {code}: {body}")),
    }
}

#[derive(Clone, Debug, Deserialize)]
struct CreateIssueResponseWire {
    #[serde(default)]
    ok: bool,
    issue_url: Option<String>,
    number: Option<u64>,
    error: Option<String>,
    message: Option<String>,
}

fn parse_create_issue_response(body: &str) -> Result<CreateIssueResponse, CreateIssueError> {
    let trimmed = body.trim();
    if trimmed.is_empty() {
        return Err(CreateIssueError::Json("Empty response body".to_string()));
    }
    let parsed: CreateIssueResponseWire = serde_json::from_str(trimmed)
        .map_err(|err| CreateIssueError::Json(format!("{err}: {trimmed}")))?;

    let ok = parsed.ok;
    if let (Some(issue_url), Some(number)) = (parsed.issue_url, parsed.number) {
        return Ok(CreateIssueResponse {
            ok: true,
            issue_url,
            number,
        });
    }
    let message = parsed
        .error
        .or(parsed.message)
        .unwrap_or_else(|| format!("Missing issue_url/number in response (ok={ok})"));
    if is_session_expired_message(&message) {
        return Err(CreateIssueError::Unauthorized);
    }
    Err(CreateIssueError::Json(message))
}

pub(crate) fn looks_like_issue_token(token: &str) -> bool {
    let trimmed = token.trim();
    if trimmed.len() < 20 || trimmed.len() > 200 {
        return false;
    }
    trimmed
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || ch == '_' || ch == '-')
}

fn parse_issue_token(body: &str) -> Result<String, IssueAuthError> {
    let trimmed = body.trim();
    if trimmed.is_empty() {
        return Err(IssueAuthError::InvalidResponse(
            "Empty response body".to_string(),
        ));
    }
    if looks_like_issue_token(trimmed) {
        return Ok(trimmed.to_string());
    }
    if trimmed.starts_with('{') {
        if let Ok(value) = serde_json::from_str::<serde_json::Value>(trimmed) {
            if let Some(token) = value.get("token").and_then(|token| token.as_str()) {
                if looks_like_issue_token(token) {
                    return Ok(token.to_string());
                }
            }
        }
    }

    let mut saw_marker = false;
    for line in trimmed.lines() {
        let candidate = line.trim();
        let lowered = candidate.to_ascii_lowercase();
        if lowered.contains("copy this token") || lowered.contains("paste this token") {
            saw_marker = true;
            continue;
        }
        if saw_marker && looks_like_issue_token(candidate) {
            return Ok(candidate.to_string());
        }
    }
    Err(IssueAuthError::InvalidResponse(
        "Token not found in response".to_string(),
    ))
}

fn is_session_expired_message(message: &str) -> bool {
    let lowered = message.trim().to_ascii_lowercase();
    lowered.contains("session") && lowered.contains("expired")
}

fn read_body_limited(response: ureq::Response, max_bytes: usize) -> Result<String, String> {
    let bytes =
        http_client::read_response_bytes(response, max_bytes).map_err(|err| err.to_string())?;
    String::from_utf8(bytes).map_err(|err| err.to_string())
}

fn get_with_retry(url: &str) -> Result<ureq::Response, ureq::Error> {
    http_client::retry_with_backoff(
        ISSUE_RETRY_CONFIG,
        || http_client::agent().get(url).call(),
        |err| match err {
            ureq::Error::Transport(_) => true,
            ureq::Error::Status(code, _) => (500..=599).contains(code),
        },
    )
}

fn post_with_retry(
    url: &str,
    token: &str,
    request: &CreateIssueRequest,
    idempotency_key: &str,
) -> Result<ureq::Response, ureq::Error> {
    http_client::retry_with_backoff(
        ISSUE_RETRY_CONFIG,
        || {
            http_client::agent()
                .post(url)
                .set("Accept", "application/json")
                .set("Content-Type", "application/json")
                .set("Authorization", &format!("Bearer {}", token.trim()))
                .set("Idempotency-Key", idempotency_key)
                .send_json(request)
        },
        |err| match err {
            ureq::Error::Transport(_) => true,
            ureq::Error::Status(code, _) => (500..=599).contains(code),
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::sync::{Arc, Mutex};
    use std::thread;

    #[test]
    fn issue_kind_prefixes_match_spec_examples() {
        assert_eq!(IssueKind::Bug.title_prefix(), "Bug: ");
        assert_eq!(IssueKind::FeatureRequest.title_prefix(), "FR: ");
    }

    #[test]
    fn parses_success_without_ok_field() {
        let body = r#"{ "issue_url": "https://github.com/PORTALSURFER/sempal/issues/123", "number": 123 }"#;
        let parsed = parse_create_issue_response(body).unwrap();
        assert!(parsed.ok);
        assert_eq!(parsed.number, 123);
    }

    #[test]
    fn reports_error_field() {
        let err = parse_create_issue_response(r#"{ "error": "nope" }"#).unwrap_err();
        assert!(err.to_string().contains("nope"));
    }

    #[test]
    fn maps_session_expired_to_unauthorized() {
        let err = parse_create_issue_response(r#"{ "error": "Session expired. Reconnect." }"#)
            .unwrap_err();
        assert!(matches!(err, CreateIssueError::Unauthorized));
    }

    #[test]
    fn parses_issue_token_from_auth_body() {
        let body = "✅ GitHub connected\n\nCopy this token into the app:\n\nabcDEF123_-xyz000000\n\nYou can close this tab.";
        let token = parse_issue_token(body).unwrap();
        assert_eq!(token, "abcDEF123_-xyz000000");
    }

    #[test]
    fn rejects_auth_body_without_token() {
        let err = parse_issue_token("No token here").unwrap_err();
        assert!(err.to_string().contains("Token not found"));
    }

    #[test]
    fn create_issue_retries_on_server_errors_with_idempotency_key() {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let addr = listener.local_addr().expect("addr");
        let seen_keys = Arc::new(Mutex::new(Vec::new()));
        let seen_keys_thread = Arc::clone(&seen_keys);
        thread::spawn(move || {
            for attempt in 0..3 {
                let (mut stream, _) = listener.accept().expect("accept");
                let mut buf = [0u8; 4096];
                let read = stream.read(&mut buf).unwrap_or(0);
                let request = String::from_utf8_lossy(&buf[..read]);
                for line in request.lines() {
                    if line.is_empty() {
                        break;
                    }
                    if let Some(value) = line.strip_prefix("Idempotency-Key:") {
                        seen_keys_thread
                            .lock()
                            .expect("lock")
                            .push(value.trim().to_string());
                    }
                }

                let response = if attempt < 2 {
                    let body = "transient error";
                    format!(
                        "HTTP/1.1 500 Internal Server Error\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                        body.len(),
                        body
                    )
                } else {
                    let body = r#"{"ok":true,"issue_url":"https://example.com/1","number":1}"#;
                    format!(
                        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                        body.len(),
                        body
                    )
                };
                let _ = stream.write_all(response.as_bytes());
            }
        });

        let request = CreateIssueRequest {
            title: "Bug: flaky".to_string(),
            body: Some("retry me".to_string()),
        };
        let url = format!("http://{}", addr);
        let response = create_issue_with_url(&url, "token", &request).expect("create issue");
        assert!(response.ok);
        assert_eq!(response.number, 1);

        let keys = seen_keys.lock().expect("lock");
        assert_eq!(keys.len(), 3);
        let unique: std::collections::HashSet<_> = keys.iter().collect();
        assert_eq!(unique.len(), 1);
        assert!(!keys[0].is_empty());
    }
}
