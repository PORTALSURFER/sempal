use super::style;
use super::EguiApp;
use eframe::egui::{self, Align2, RichText};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum FeedbackSubmitAction {
    None,
    SubmitFr,
    SubmitBug,
    Cancel,
}

impl EguiApp {
    pub(super) fn render_feedback_issue_prompt(&mut self, ctx: &egui::Context) {
        if !self.controller.ui.feedback_issue.open {
            return;
        }

        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            self.controller.close_feedback_issue_prompt();
            return;
        }

        let mut open = true;
        let mut action = FeedbackSubmitAction::None;
        egui::Window::new("Submit GitHub issue")
            .anchor(Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
            .collapsible(false)
            .resizable(false)
            .default_width(520.0)
            .open(&mut open)
            .show(ctx, |ui| {
                action = self.render_feedback_issue_prompt_body(ui);
            });

        if !open || action == FeedbackSubmitAction::Cancel {
            self.controller.close_feedback_issue_prompt();
            return;
        }

        match action {
            FeedbackSubmitAction::None => {}
            FeedbackSubmitAction::Cancel => {}
            FeedbackSubmitAction::SubmitFr => self
                .controller
                .submit_feedback_issue(crate::github::issues::IssueKind::FeatureRequest),
            FeedbackSubmitAction::SubmitBug => self
                .controller
                .submit_feedback_issue(crate::github::issues::IssueKind::Bug),
        }
    }

    fn render_feedback_issue_prompt_body(&mut self, ui: &mut egui::Ui) -> FeedbackSubmitAction {
        let palette = style::palette();
        ui.set_min_width(520.0);
        ui.label(
            RichText::new("Enter the issue text below. This will create a new issue on GitHub.")
                .color(palette.text_primary),
        );
        ui.label(
            RichText::new("Requires a token via `SEMPAL_GITHUB_TOKEN`, `GITHUB_TOKEN`, or `GH_TOKEN`.")
                .color(palette.text_muted),
        );
        if let Some(err) = self.controller.ui.feedback_issue.last_error.as_ref() {
            ui.add_space(8.0);
            ui.label(RichText::new(err).color(style::status_badge_color(style::StatusTone::Error)));
        }
        ui.add_space(8.0);

        let state = &mut self.controller.ui.feedback_issue;
        let submitting = state.submitting;
        let edit = egui::TextEdit::multiline(&mut state.draft)
            .hint_text("Describe the bug or feature request…")
            .desired_width(500.0)
            .desired_rows(7)
            .lock_focus(true);
        let response = ui.add_enabled(!submitting, edit);
        if state.focus_requested && !response.has_focus() && !submitting {
            response.request_focus();
            state.focus_requested = false;
        }

        ui.add_space(10.0);
        let mut action = FeedbackSubmitAction::None;
        ui.horizontal(|ui| {
            if ui.add_enabled(!submitting, egui::Button::new("Cancel")).clicked() {
                action = FeedbackSubmitAction::Cancel;
            }
            ui.add_space(8.0);
            let can_submit = !submitting && !state.draft.trim().is_empty();
            if ui
                .add_enabled(can_submit, egui::Button::new("Submit FR"))
                .clicked()
            {
                action = FeedbackSubmitAction::SubmitFr;
            }
            if ui
                .add_enabled(can_submit, egui::Button::new("Submit BUG"))
                .clicked()
            {
                action = FeedbackSubmitAction::SubmitBug;
            }
            if submitting {
                ui.add_space(8.0);
                ui.label(RichText::new("Submitting…").color(palette.text_muted));
            }
        });
        action
    }
}
