use super::*;

impl EguiController {
    pub(crate) fn open_feedback_issue_prompt(&mut self) {
        self.ui.feedback_issue.open = true;
        self.ui.feedback_issue.focus_title_requested = true;
        self.ui.feedback_issue.last_error = None;
        self.ui.feedback_issue.last_success_url = None;
        self.ui.feedback_issue.token_autofill_last = None;
        self.ui.feedback_issue.connecting = false;
        self.ui.feedback_issue.token_status = crate::egui_app::state::IssueTokenStatus::Unknown;
        self.ui.feedback_issue.token_cached = None;
        self.start_issue_token_load();
    }

    pub(crate) fn close_feedback_issue_prompt(&mut self) {
        self.ui.feedback_issue.open = false;
        self.ui.feedback_issue.submitting = false;
        self.ui.feedback_issue.focus_title_requested = false;
        self.ui.feedback_issue.token_modal_open = false;
        self.ui.feedback_issue.focus_token_requested = false;
        self.ui.feedback_issue.token_autofill_last = None;
        self.ui.feedback_issue.connecting = false;
        self.ui.feedback_issue.last_error = None;
        self.ui.feedback_issue.last_success_url = None;
        self.runtime.jobs.clear_issue_gateway_poll();
    }

    pub(crate) fn connect_github_issue_reporting(&mut self) {
        if self.ui.feedback_issue.connecting {
            return;
        }
        self.ui.feedback_issue.connecting = true;
        self.ui.feedback_issue.last_error = None;
        self.set_status("Opening GitHub auth page…", StatusTone::Info);
        
        // Generate a random request ID for automatic polling
        let request_id = format!("req_{}", uuid::Uuid::new_v4());
        let auth_url = format!("{}?requestId={}", crate::issue_gateway::api::AUTH_START_URL, request_id);
        
        if let Err(err) = open::that(&auth_url) {
            self.ui.feedback_issue.last_error = Some(format!(
                "Failed to open auth URL. Open it manually and paste the token: {} ({err})",
                crate::issue_gateway::api::AUTH_START_URL
            ));
            self.set_status("GitHub connect failed".to_string(), StatusTone::Error);
            self.ui.feedback_issue.connecting = false;
            self.ui.feedback_issue.token_modal_open = true;
            self.ui.feedback_issue.focus_token_requested = true;
        } else {
            // Start polling in the background
            self.runtime.jobs.begin_issue_gateway_poll(super::jobs::IssueGatewayPollJob {
                request_id,
            });
        }
    }

    pub(crate) fn save_github_issue_token(&mut self, token: &str) {
        self.persist_issue_token(token, true);
    }

    pub(crate) fn disconnect_github_issue_reporting(&mut self) {
        if self.ui.feedback_issue.token_deleting {
            return;
        }
        self.ui.feedback_issue.token_deleting = true;
        self.ui.feedback_issue.last_error = None;
        self.runtime.jobs.begin_issue_token_delete();
    }

    pub(crate) fn submit_feedback_issue(&mut self, kind: crate::issue_gateway::api::IssueKind) {
        if self.ui.feedback_issue.submitting {
            return;
        }
        let title = self.ui.feedback_issue.title.trim();
        if title.len() < 3 || title.len() > 200 {
            self.ui.feedback_issue.last_error = Some("Title must be 3–200 characters.".to_string());
            return;
        }
        let token = match self.ui.feedback_issue.token_cached.clone() {
            Some(token) => token,
            None => {
                let loading = self.ui.feedback_issue.token_loading
                    || self.ui.feedback_issue.token_saving
                    || self.ui.feedback_issue.token_deleting;
                if loading {
                    self.ui.feedback_issue.last_error =
                        Some("GitHub token is still loading. Try again.".to_string());
                } else {
                    self.ui.feedback_issue.last_error =
                        Some("Connect GitHub first.".to_string());
                    self.ui.feedback_issue.token_modal_open = true;
                    self.ui.feedback_issue.focus_token_requested = true;
                }
                return;
            }
        };

        let mut final_title = title.to_string();
        let prefix = kind.title_prefix();
        if !final_title.starts_with(prefix) {
            final_title = format!("{prefix}{final_title}");
        }

        let body = self.compose_issue_body();
        self.ui.feedback_issue.submitting = true;
        self.ui.feedback_issue.last_error = None;
        self.ui.feedback_issue.last_success_url = None;
        self.runtime
            .jobs
            .begin_issue_gateway_create(super::jobs::IssueGatewayJob {
                token,
                request: crate::issue_gateway::api::CreateIssueRequest {
                    title: final_title,
                    body,
                },
            });
    }

    pub(crate) fn complete_issue_gateway_auth(
        &mut self,
        result: Result<String, crate::issue_gateway::api::IssueAuthError>,
    ) {
        self.ui.feedback_issue.connecting = false;
        self.runtime.jobs.clear_issue_gateway_poll();
        match result {
            Ok(token) => {
                if !self.persist_issue_token(&token, false) {
                    self.set_status("Failed to save GitHub token".to_string(), StatusTone::Error);
                }
            }
            Err(err) => {
                self.ui.feedback_issue.last_error =
                    Some(format!("Auto-connect failed: {err}. Use Paste token…"));
                self.set_status(format!("GitHub connect failed: {err}"), StatusTone::Error);
            }
        }
    }

    fn compose_issue_body(&self) -> Option<String> {
        let user_body = self.ui.feedback_issue.body.trim();
        let mut parts = Vec::new();
        if !user_body.is_empty() {
            parts.push(user_body.to_string());
        }
        parts.push(self.diagnostics_block());
        Some(parts.join("\n\n"))
    }

    fn diagnostics_block(&self) -> String {
        let version = env!("CARGO_PKG_VERSION");
        let build_type = if cfg!(debug_assertions) {
            "debug"
        } else {
            "release"
        };
        let os = std::env::consts::OS;
        let arch = std::env::consts::ARCH;
        let logs = crate::app_dirs::logs_dir()
            .ok()
            .map(|p| p.display().to_string())
            .unwrap_or_else(|| "n/a".to_string());
        format!(
            "---\n\nDiagnostics\n- App version: {version}\n- OS: {os} ({arch})\n- Build: {build_type}\n- Logs: {logs}"
        )
    }

    fn persist_issue_token(&mut self, token: &str, reopen_modal: bool) -> bool {
        let token = token.trim();
        if token.len() < 20 {
            self.ui.feedback_issue.last_error =
                Some("Invalid token (must be at least 20 characters).".to_string());
            return false;
        }
        if self.ui.feedback_issue.token_saving {
            return false;
        }
        self.ui.feedback_issue.last_error = None;
        self.ui.feedback_issue.token_saving = true;
        self.runtime.jobs.begin_issue_token_save(super::jobs::IssueTokenSaveJob {
            token: token.to_string(),
            reopen_modal,
        });
        true
    }

    fn start_issue_token_load(&mut self) {
        if self.ui.feedback_issue.token_loading {
            return;
        }
        self.ui.feedback_issue.token_loading = true;
        self.runtime.jobs.begin_issue_token_load();
    }
}
