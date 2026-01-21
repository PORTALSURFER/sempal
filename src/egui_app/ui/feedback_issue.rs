use super::overlay_layers::OverlayLayer;
use super::EguiApp;
use super::style;
use eframe::egui::{self, Align2, RichText};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum FeedbackSubmitAction {
    None,
    SubmitFr,
    SubmitBug,
    Cancel,
}

impl EguiApp {
    /// Render the modal feedback issue prompt and any nested token modal.
    pub(super) fn render_feedback_issue_prompt(&mut self, ctx: &egui::Context) {
        if !self.controller.ui.feedback_issue.open {
            return;
        }

        self.render_feedback_issue_backdrop(ctx);

        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            self.controller.close_feedback_issue_prompt();
            return;
        }

        let mut open = true;
        let mut action = FeedbackSubmitAction::None;
        egui::Window::new("Submit GitHub issue")
            .anchor(Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
            .order(OverlayLayer::Modal.order())
            .collapsible(false)
            .resizable(false)
            .default_width(560.0)
            .open(&mut open)
            .show(ctx, |ui| {
                action = self.render_feedback_issue_prompt_body(ui);
            });

        if !open || action == FeedbackSubmitAction::Cancel {
            self.controller.close_feedback_issue_prompt();
            return;
        }

        self.render_feedback_issue_token_modal(ctx);

        match action {
            FeedbackSubmitAction::None => {}
            FeedbackSubmitAction::Cancel => {}
            FeedbackSubmitAction::SubmitFr => self
                .controller
                .submit_feedback_issue(crate::issue_gateway::api::IssueKind::FeatureRequest),
            FeedbackSubmitAction::SubmitBug => self
                .controller
                .submit_feedback_issue(crate::issue_gateway::api::IssueKind::Bug),
        }
    }

    fn render_feedback_issue_backdrop(&mut self, ctx: &egui::Context) {
        let rect = ctx.viewport_rect();
        let painter = ctx.layer_painter(OverlayLayer::Modal.layer_id(
            egui::Id::new("feedback_issue_backdrop_paint"),
        ));
        painter.rect_filled(
            rect,
            0.0,
            egui::Color32::from_rgba_premultiplied(0, 0, 0, 160),
        );

        egui::Area::new(egui::Id::new("feedback_issue_backdrop_blocker"))
            .order(OverlayLayer::Modal.order())
            .fixed_pos(rect.min)
            .show(ctx, |ui| {
                let response = ui.allocate_rect(rect, egui::Sense::click_and_drag());
                if response.clicked() {
                    ui.ctx().request_repaint();
                }
            });
    }

    fn render_feedback_issue_token_modal(&mut self, ctx: &egui::Context) {
        if !self.controller.ui.feedback_issue.token_modal_open {
            return;
        }
        let mut open = true;
        egui::Window::new("Paste GitHub token")
            .anchor(Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
            .order(OverlayLayer::Modal.order())
            .collapsible(false)
            .resizable(false)
            .default_width(520.0)
            .open(&mut open)
            .show(ctx, |ui| {
                self.render_feedback_issue_token_modal_body(ui);
            });
        if !open {
            self.controller.ui.feedback_issue.token_modal_open = false;
            self.controller.ui.feedback_issue.token_input.clear();
            self.controller.ui.feedback_issue.focus_token_requested = false;
            self.controller.ui.feedback_issue.token_autofill_last = None;
        }
    }

    fn render_feedback_issue_token_modal_body(&mut self, ui: &mut egui::Ui) {
        let palette = style::palette();
        ui.set_min_width(520.0);
        ui.label(
            RichText::new(
                "If auto-connect fails, copy a token from the auth page and paste it here.",
            )
            .color(palette.text_primary),
        );
        ui.add_space(8.0);

        let mut auto_save_token = None;
        let (cancel_clicked, save_clicked, token_to_save) = {
            let state = &mut self.controller.ui.feedback_issue;
            if state.token_input.trim().is_empty() {
                if let Ok(clipboard_text) = crate::external_clipboard::read_text() {
                    let candidate = clipboard_text.trim();
                    if crate::issue_gateway::api::looks_like_issue_token(candidate)
                        && state.token_autofill_last.as_deref() != Some(candidate)
                    {
                        state.token_input = candidate.to_string();
                        state.token_autofill_last = Some(candidate.to_string());
                        auto_save_token = Some(candidate.to_string());
                    }
                }
            }
            let response = ui.add(
                egui::TextEdit::singleline(&mut state.token_input)
                    .hint_text("Paste GitHub token")
                    .desired_width(480.0),
            );
            if state.focus_token_requested && !response.has_focus() {
                response.request_focus();
                state.focus_token_requested = false;
            }

            ui.add_space(10.0);
            let mut cancel_clicked = false;
            let mut save_clicked = false;
            ui.horizontal(|ui| {
                if ui.button("Cancel").clicked() {
                    cancel_clicked = true;
                }
                let token_valid = state.token_input.trim().len() >= 20;
                if ui
                    .add_enabled(
                        token_valid && !state.token_saving && !state.token_deleting,
                        egui::Button::new("Save"),
                    )
                    .clicked()
                {
                    save_clicked = true;
                }
            });
            if state.token_saving {
                ui.add_space(6.0);
                ui.label(RichText::new("Saving token…").color(palette.text_muted));
            } else if state.token_deleting {
                ui.add_space(6.0);
                ui.label(RichText::new("Removing token…").color(palette.text_muted));
            }
            (cancel_clicked, save_clicked, state.token_input.clone())
        };

        if cancel_clicked {
            self.controller.ui.feedback_issue.token_modal_open = false;
            self.controller.ui.feedback_issue.token_input.clear();
            self.controller.ui.feedback_issue.focus_token_requested = false;
            self.controller.ui.feedback_issue.token_autofill_last = None;
        }
        if save_clicked {
            self.controller.save_github_issue_token(&token_to_save);
        } else if let Some(token) = auto_save_token {
            self.controller.save_github_issue_token(&token);
        }
    }

    fn render_feedback_issue_prompt_body(&mut self, ui: &mut egui::Ui) -> FeedbackSubmitAction {
        let palette = style::palette();
        ui.set_min_width(560.0);
        ui.label(
            RichText::new("Issues are created under your GitHub account.")
                .color(palette.text_primary),
        );

        ui.add_space(6.0);
        ui.horizontal(|ui| {
            if ui
                .add_enabled(
                    !self.controller.ui.feedback_issue.connecting,
                    egui::Button::new("Connect GitHub"),
                )
                .clicked()
            {
                self.controller.connect_github_issue_reporting();
            }
            if ui.button("Paste token…").clicked() {
                self.controller.ui.feedback_issue.token_modal_open = true;
                self.controller.ui.feedback_issue.focus_token_requested = true;
            }
            if ui.button("Disconnect").clicked() {
                self.controller.disconnect_github_issue_reporting();
            }
        });
        let state = &self.controller.ui.feedback_issue;
        let connecting = state.connecting || self.controller.is_issue_gateway_poll_in_progress();
        let token_busy = state.token_loading || state.token_saving || state.token_deleting;

        if connecting {
            ui.label(RichText::new("Status: connecting…").color(palette.text_muted));
        } else if token_busy {
            ui.label(RichText::new("Status: updating token…").color(palette.text_muted));
        } else {
            match &state.token_status {
                crate::egui_app::state::IssueTokenStatus::Connected => {
                    ui.label(
                        RichText::new("Status: connected")
                            .color(style::status_badge_color(style::StatusTone::Info)),
                    );
                }
                crate::egui_app::state::IssueTokenStatus::NotConnected => {
                    ui.label(RichText::new("Status: not connected").color(palette.text_muted));
                }
                crate::egui_app::state::IssueTokenStatus::Error(err) => {
                    ui.label(
                        RichText::new(format!("Status: token store error ({err})"))
                            .color(style::status_badge_color(style::StatusTone::Warning)),
                    );
                }
                crate::egui_app::state::IssueTokenStatus::Unknown => {
                    ui.label(RichText::new("Status: checking…").color(palette.text_muted));
                }
            }
        }

        if let Some(err) = self.controller.ui.feedback_issue.last_error.as_ref() {
            ui.add_space(8.0);
            ui.label(RichText::new(err).color(style::status_badge_color(style::StatusTone::Error)));
        }
        if let Some(url) = self.controller.ui.feedback_issue.last_success_url.clone() {
            ui.add_space(8.0);
            ui.label(
                RichText::new("Issue created successfully.")
                    .color(style::status_badge_color(style::StatusTone::Info)),
            );
            let mut open_clicked = false;
            let mut close_clicked = false;
            ui.horizontal(|ui| {
                if ui.button("Open issue in browser").clicked() {
                    open_clicked = true;
                }
                if ui.button("Close").clicked() {
                    close_clicked = true;
                }
            });
            if open_clicked {
                let _ = open::that(&url);
            }
            if close_clicked {
                self.controller.close_feedback_issue_prompt();
            }
            ui.add_space(8.0);
        }
        ui.add_space(8.0);

        let state = &mut self.controller.ui.feedback_issue;
        let submitting = state.submitting;

        ui.label(RichText::new("Title (required)").color(palette.text_primary));
        let title_response = ui.add_enabled(
            !submitting,
            egui::TextEdit::singleline(&mut state.title)
                .hint_text("Bug: … or FR: …")
                .desired_width(520.0),
        );
        if state.focus_title_requested && !title_response.has_focus() && !submitting {
            title_response.request_focus();
            state.focus_title_requested = false;
        }
        ui.add_space(8.0);

        ui.label(RichText::new("Body (optional, recommended)").color(palette.text_primary));
        ui.add_enabled(
            !submitting,
            egui::TextEdit::multiline(&mut state.body)
                .hint_text("Steps to reproduce…\nExpected…\nActual…")
                .desired_width(520.0)
                .desired_rows(7)
                .lock_focus(true),
        );

        ui.add_space(10.0);
        let mut action = FeedbackSubmitAction::None;
        ui.horizontal(|ui| {
            if ui
                .add_enabled(!submitting, egui::Button::new("Cancel"))
                .clicked()
            {
                action = FeedbackSubmitAction::Cancel;
            }
            ui.add_space(8.0);
            let title_len = state.title.trim().len();
            let can_submit = !submitting && (3..=200).contains(&title_len);
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
