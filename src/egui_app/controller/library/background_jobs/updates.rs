use super::*;
use crate::egui_app::controller::jobs::{
    IssueGatewayAuthResult,
    IssueGatewayCreateResult,
    IssueTokenDeleteResult,
    IssueTokenLoadResult,
    IssueTokenSaveResult,
};

pub(crate) fn handle_update_checked(controller: &mut EguiController, message: UpdateCheckResult) {
    controller.runtime.jobs.clear_update_check();
    match message.result {
        Ok(outcome) => controller.apply_update_check_result(outcome),
        Err(err) => controller.apply_update_check_error(err),
    }
}

pub(crate) fn handle_issue_gateway_created(
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
                controller.ui.feedback_issue.token_deleting = true;
                controller.runtime.jobs.begin_issue_token_delete();
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

pub(crate) fn handle_issue_gateway_authed(
    controller: &mut EguiController,
    message: IssueGatewayAuthResult,
) {
    controller.runtime.jobs.clear_issue_gateway_auth();
    controller.complete_issue_gateway_auth(message.result);
}

/// Apply token load results to the feedback issue UI state.
pub(crate) fn handle_issue_token_loaded(
    controller: &mut EguiController,
    message: IssueTokenLoadResult,
) {
    controller.runtime.jobs.clear_issue_token_load();
    controller.ui.feedback_issue.token_loading = false;
    match message.result {
        Ok(Some(token)) => {
            controller.ui.feedback_issue.token_cached = Some(token);
            controller.ui.feedback_issue.token_status =
                crate::egui_app::state::IssueTokenStatus::Connected;
        }
        Ok(None) => {
            controller.ui.feedback_issue.token_cached = None;
            controller.ui.feedback_issue.token_status =
                crate::egui_app::state::IssueTokenStatus::NotConnected;
            if controller.ui.feedback_issue.open {
                controller.connect_github_issue_reporting();
            }
        }
        Err(err) => {
            controller.ui.feedback_issue.token_cached = None;
            controller.ui.feedback_issue.token_status =
                crate::egui_app::state::IssueTokenStatus::Error(err.to_string());
            controller.ui.feedback_issue.last_error = Some(err.to_string());
        }
    }
}

/// Apply token save results to the feedback issue UI state.
pub(crate) fn handle_issue_token_saved(
    controller: &mut EguiController,
    message: IssueTokenSaveResult,
) {
    controller.runtime.jobs.clear_issue_token_save();
    controller.ui.feedback_issue.token_saving = false;
    match message.result {
        Ok(()) => {
            controller.ui.feedback_issue.token_cached = Some(message.token);
            controller.ui.feedback_issue.token_status =
                crate::egui_app::state::IssueTokenStatus::Connected;
            controller.ui.feedback_issue.token_modal_open = false;
            controller.ui.feedback_issue.token_input.clear();
            controller.ui.feedback_issue.token_autofill_last = None;
            controller.set_status(
                "GitHub connected for issue reporting".to_string(),
                crate::egui_app::ui::style::StatusTone::Info,
            );
        }
        Err(err) => {
            controller.ui.feedback_issue.token_status =
                crate::egui_app::state::IssueTokenStatus::Error(err.to_string());
            controller.ui.feedback_issue.last_error = Some(err.to_string());
            if message.reopen_modal {
                controller.ui.feedback_issue.token_modal_open = true;
                controller.ui.feedback_issue.focus_token_requested = true;
            }
        }
    }
}

/// Apply token delete results to the feedback issue UI state.
pub(crate) fn handle_issue_token_deleted(
    controller: &mut EguiController,
    message: IssueTokenDeleteResult,
) {
    controller.runtime.jobs.clear_issue_token_delete();
    controller.ui.feedback_issue.token_deleting = false;
    match message.result {
        Ok(()) => {
            controller.ui.feedback_issue.token_cached = None;
            controller.ui.feedback_issue.token_status =
                crate::egui_app::state::IssueTokenStatus::NotConnected;
            controller.set_status(
                "GitHub disconnected".to_string(),
                crate::egui_app::ui::style::StatusTone::Info,
            );
        }
        Err(err) => {
            controller.ui.feedback_issue.token_status =
                crate::egui_app::state::IssueTokenStatus::Error(err.to_string());
            controller.ui.feedback_issue.last_error = Some(err.to_string());
        }
    }
}
