//! Gateway API client for creating GitHub issues.

use serde::{Deserialize, Serialize};

use crate::http_client;

pub const BASE_URL: &str = "https://sempal-gitissue-gateway.portalsurfer.workers.dev";
pub const AUTH_START_URL: &str =
    "https://sempal-gitissue-gateway.portalsurfer.workers.dev/auth/start";

const MAX_AUTH_RESPONSE_BYTES: usize = 64 * 1024;
const MAX_ISSUE_RESPONSE_BYTES: usize = 256 * 1024;

/// The kind of issue the user is submitting.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IssueKind {
    FeatureRequest,
    Bug,
}

impl IssueKind {
    pub fn title_prefix(self) -> &'static str {
        match self {
            Self::FeatureRequest => "FR: ",
            Self::Bug => "Bug: ",
        }
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct CreateIssueRequest {
    pub title: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub body: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct CreateIssueResponse {
    pub ok: bool,
    pub issue_url: String,
    pub number: u64,
}

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
    let response = match http_client::agent().get(AUTH_START_URL).call() {
        Ok(response) => response,
        Err(ureq::Error::Status(code, response)) => {
            let body =
                read_body_limited(response, MAX_AUTH_RESPONSE_BYTES).unwrap_or_else(|err| err);
            return Err(IssueAuthError::ServerError(format!(
                "HTTP {code}: {body}"
            )));
        }
        Err(ureq::Error::Transport(err)) => {
            return Err(IssueAuthError::Transport(err.to_string()));
        }
    };

    let body = read_body_limited(response, MAX_AUTH_RESPONSE_BYTES)
        .map_err(IssueAuthError::InvalidResponse)?;
    parse_issue_token(&body)
}

pub fn create_issue(
    token: &str,
    request: &CreateIssueRequest,
) -> Result<CreateIssueResponse, CreateIssueError> {
    let url = format!("{BASE_URL}/issue");
    let req = http_client::agent().post(&url)
        .set("Accept", "application/json")
        .set("Content-Type", "application/json")
        .set("Authorization", &format!("Bearer {}", token.trim()));

    let response = match req.send_json(request) {
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

    let body = read_body_limited(response, MAX_ISSUE_RESPONSE_BYTES)
        .map_err(CreateIssueError::Json)?;
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
    let bytes = http_client::read_response_bytes(response, max_bytes)
        .map_err(|err| err.to_string())?;
    String::from_utf8(bytes).map_err(|err| err.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

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
        let err =
            parse_create_issue_response(r#"{ "error": "Session expired. Reconnect." }"#).unwrap_err();
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
}
