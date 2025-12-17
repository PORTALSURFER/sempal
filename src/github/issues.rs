//! Create GitHub issues via the REST API.

use serde::{Deserialize, Serialize};

/// The kind of issue to file.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IssueKind {
    /// Feature request.
    FeatureRequest,
    /// Bug report.
    Bug,
}

impl IssueKind {
    /// Human-facing label used both as a title prefix and (best-effort) GitHub label.
    pub fn tag(self) -> &'static str {
        match self {
            Self::FeatureRequest => "FR",
            Self::Bug => "BUG",
        }
    }
}

/// A successfully created GitHub issue.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
pub struct CreatedIssue {
    /// Issue number within the repository.
    pub number: u64,
    /// HTML URL of the created issue.
    pub html_url: String,
}

#[derive(Debug, thiserror::Error)]
pub enum CreateIssueError {
    #[error("Invalid repository slug (expected OWNER/REPO): {0}")]
    InvalidRepo(String),
    #[error("Missing GitHub token (set `SEMPAL_GITHUB_TOKEN`, `GITHUB_TOKEN`, or `GH_TOKEN`)")]
    MissingToken,
    #[error("HTTP error: {0}")]
    Http(String),
    #[error("JSON error: {0}")]
    Json(String),
}

#[derive(Clone, Debug, Serialize)]
struct IssueCreatePayload {
    title: String,
    body: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    labels: Vec<String>,
}

/// Create a new issue in `repo` (slug `OWNER/REPO`) using the provided token.
///
/// Notes:
/// - Adds a best-effort label matching `kind.tag()`; if the repository doesn't have the label,
///   the request will be retried without labels.
/// - Always prefixes the title with `[BUG]` or `[FR]`.
pub fn create_issue(
    repo: &str,
    token: &str,
    kind: IssueKind,
    text: &str,
) -> Result<CreatedIssue, CreateIssueError> {
    if !repo.contains('/') {
        return Err(CreateIssueError::InvalidRepo(repo.to_string()));
    }
    if token.trim().is_empty() {
        return Err(CreateIssueError::MissingToken);
    }
    let (title, body) = derive_title_and_body(kind, text);
    let url = format!("https://api.github.com/repos/{repo}/issues");
    let with_labels = IssueCreatePayload {
        title: title.clone(),
        body: body.clone(),
        labels: vec![kind.tag().to_string()],
    };
    match post_issue(&url, token, &with_labels) {
        Ok(issue) => Ok(issue),
        Err(CreateIssueError::Http(err)) if looks_like_missing_label(&err) => {
            let without_labels = IssueCreatePayload { title, body, labels: Vec::new() };
            post_issue(&url, token, &without_labels)
        }
        Err(err) => Err(err),
    }
}

fn post_issue(
    url: &str,
    token: &str,
    payload: &IssueCreatePayload,
) -> Result<CreatedIssue, CreateIssueError> {
    let request = ureq::post(url)
        .set("User-Agent", "sempal")
        .set("Accept", "application/vnd.github+json")
        .set("Authorization", &format!("Bearer {token}"));

    let response = match request.send_json(payload) {
        Ok(response) => response,
        Err(ureq::Error::Status(code, response)) => {
            let body = response.into_string().unwrap_or_default();
            return Err(CreateIssueError::Http(format!("HTTP {code}: {body}")));
        }
        Err(ureq::Error::Transport(err)) => {
            return Err(CreateIssueError::Http(err.to_string()));
        }
    };

    response.into_json::<CreatedIssue>().map_err(|err| CreateIssueError::Json(err.to_string()))
}

fn looks_like_missing_label(error: &str) -> bool {
    let lower = error.to_lowercase();
    lower.contains("http 422") && (lower.contains("label") || lower.contains("labels"))
}

fn derive_title_and_body(kind: IssueKind, text: &str) -> (String, String) {
    let trimmed = text.trim();
    let title_raw = trimmed
        .lines()
        .find(|line| !line.trim().is_empty())
        .unwrap_or_default()
        .trim();
    let mut title = title_raw.to_string();
    if title.is_empty() {
        title = "Feedback".to_string();
    }
    title = title.replace('\n', " ").replace('\r', " ");
    const MAX_TITLE: usize = 80;
    if title.chars().count() > MAX_TITLE {
        title = title.chars().take(MAX_TITLE - 1).collect::<String>();
        title.push('…');
    }
    (format!("[{}] {}", kind.tag(), title), trimmed.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn derives_title_from_first_non_empty_line() {
        let (title, body) = derive_title_and_body(IssueKind::Bug, "\n\nHello world\nMore\n");
        assert_eq!(title, "[BUG] Hello world");
        assert_eq!(body, "Hello world\nMore");
    }

    #[test]
    fn falls_back_to_feedback_title() {
        let (title, body) = derive_title_and_body(IssueKind::FeatureRequest, "   \n\t");
        assert_eq!(title, "[FR] Feedback");
        assert_eq!(body, "");
    }

    #[test]
    fn truncates_long_titles() {
        let long = "a".repeat(200);
        let (title, _) = derive_title_and_body(IssueKind::Bug, &long);
        assert!(title.starts_with("[BUG] "));
        assert!(title.chars().count() <= "[BUG] ".chars().count() + 80);
        assert!(title.ends_with('…'));
    }
}
