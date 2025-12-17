use super::*;

impl EguiController {
    pub(crate) fn open_feedback_issue_prompt(&mut self) {
        self.ui.feedback_issue.open = true;
        self.ui.feedback_issue.focus_requested = true;
        self.ui.feedback_issue.last_error = None;
    }

    pub(crate) fn close_feedback_issue_prompt(&mut self) {
        self.ui.feedback_issue.open = false;
        self.ui.feedback_issue.submitting = false;
        self.ui.feedback_issue.focus_requested = false;
        self.ui.feedback_issue.last_error = None;
    }

    pub(crate) fn submit_feedback_issue(&mut self, kind: crate::github::issues::IssueKind) {
        if self.ui.feedback_issue.submitting {
            return;
        }
        let text = self.ui.feedback_issue.draft.trim().to_string();
        if text.is_empty() {
            self.ui.feedback_issue.last_error = Some("Issue text cannot be empty.".to_string());
            return;
        }
        let repo = std::env::var("SEMPAL_GITHUB_REPO").unwrap_or_else(|_| crate::updater::REPO_SLUG.to_string());
        let token = read_github_token().unwrap_or_default();
        if token.trim().is_empty() {
            self.ui.feedback_issue.last_error =
                Some("Missing token: set `SEMPAL_GITHUB_TOKEN`, `GITHUB_TOKEN`, or `GH_TOKEN`.".to_string());
            return;
        }
        self.ui.feedback_issue.submitting = true;
        self.ui.feedback_issue.last_error = None;
        self.runtime.jobs.begin_github_issue_create(super::jobs::GitHubIssueJob {
            repo,
            token,
            kind,
            text,
        });
    }
}

fn read_github_token() -> Option<String> {
    for key in ["SEMPAL_GITHUB_TOKEN", "GITHUB_TOKEN", "GH_TOKEN"] {
        if let Ok(value) = std::env::var(key)
            && !value.trim().is_empty()
        {
            return Some(value);
        }
    }
    None
}

