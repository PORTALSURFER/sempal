//! Gateway API client for creating GitHub issues.

use serde::{Deserialize, Serialize};

pub const BASE_URL: &str = "https://sempal-gitissue-gateway.portalsurfer.workers.dev";
pub const AUTH_START_URL: &str = "https://sempal-gitissue-gateway.portalsurfer.workers.dev/auth/start";

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

pub fn create_issue(token: &str, request: &CreateIssueRequest) -> Result<CreateIssueResponse, CreateIssueError> {
    let url = format!("{BASE_URL}/issue");
    let req = ureq::post(&url)
        .set("Accept", "application/json")
        .set("Content-Type", "application/json")
        .set("Authorization", &format!("Bearer {}", token.trim()));

    let response = match req.send_json(request) {
        Ok(response) => response,
        Err(ureq::Error::Status(code, response)) => {
            let body = response.into_string().unwrap_or_default();
            return Err(map_status_error(code, body));
        }
        Err(ureq::Error::Transport(err)) => return Err(CreateIssueError::Transport(err.to_string())),
    };

    response
        .into_json::<CreateIssueResponse>()
        .map_err(|err| CreateIssueError::Json(err.to_string()))
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn issue_kind_prefixes_match_spec_examples() {
        assert_eq!(IssueKind::Bug.title_prefix(), "Bug: ");
        assert_eq!(IssueKind::FeatureRequest.title_prefix(), "FR: ");
    }
}

