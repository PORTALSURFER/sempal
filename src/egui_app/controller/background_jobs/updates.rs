use super::*;
use crate::egui_app::controller::jobs::{IssueGatewayAuthResult, IssueGatewayCreateResult};

pub(super) fn handle_update_checked(controller: &mut EguiController, message: UpdateCheckResult) {
    controller.runtime.jobs.clear_update_check();
    match message.result {
        Ok(outcome) => controller.apply_update_check_result(outcome),
        Err(err) => controller.apply_update_check_error(err),
    }
}

pub(super) fn handle_issue_gateway_created(
    controller: &mut EguiController,
    message: IssueGatewayCreateResult,
) {
    controller.runtime.jobs.clear_issue_gateway_create();
    controller.ui.feedback_issue.submitting = false;
    match message.result {
        Ok(outcome) => {
            if outcome.ok {
                controller.ui.feedback_issue.last_error = None;
                controller.ui.feedback_issue.last_success_url = Some(outcome.issue_url.clone());
                controller.ui.feedback_issue.title.clear();
                controller.ui.feedback_issue.body.clear();
                controller.ui.feedback_issue.focus_title_requested = true;
                controller.set_status(
                    format!("Created GitHub issue #{}", outcome.number),
                    crate::egui_app::ui::style::StatusTone::Info,
                );
            } else {
                controller.ui.feedback_issue.last_error =
                    Some("Issue creation failed.".to_string());
                controller.set_status(
                    "Failed to create issue".to_string(),
                    crate::egui_app::ui::style::StatusTone::Error,
                );
            }
        }
        Err(err) => {
            if matches!(
                err,
                crate::issue_gateway::api::CreateIssueError::Unauthorized
            ) {
                if let Ok(store) = crate::issue_gateway::IssueTokenStore::new() {
                    let _ = store.delete();
                }
                controller.ui.feedback_issue.token_modal_open = true;
                controller.ui.feedback_issue.focus_token_requested = true;
                controller.ui.feedback_issue.last_error =
                    Some("GitHub connection expired. Reconnect and paste a new token.".to_string());
            } else {
                controller.ui.feedback_issue.last_error = Some(err.to_string());
            }
            controller.set_status(
                format!("Failed to create issue: {err}"),
                crate::egui_app::ui::style::StatusTone::Error,
            );
        }
    }
}

pub(super) fn handle_issue_gateway_authed(
    controller: &mut EguiController,
    message: IssueGatewayAuthResult,
) {
    controller.runtime.jobs.clear_issue_gateway_auth();
    controller.complete_issue_gateway_auth(message.result);
}
