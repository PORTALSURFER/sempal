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

        self.render_feedback_issue_backdrop(ctx);

        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            self.controller.close_feedback_issue_prompt();
            return;
        }

        self.render_feedback_issue_token_modal(ctx);

        let mut open = true;
        let mut action = FeedbackSubmitAction::None;
        egui::Window::new("Submit GitHub issue")
            .anchor(Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
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
        let backdrop_id = egui::Id::new("feedback_issue_backdrop");
        let rect = ctx.viewport_rect();
        egui::Area::new(backdrop_id)
            .order(egui::Order::Foreground)
            .fixed_pos(rect.min)
            .show(ctx, |ui| {
                let response = ui.allocate_rect(rect, egui::Sense::click_and_drag());
                ui.painter().rect_filled(
                    rect,
                    egui::CornerRadius::ZERO,
                    egui::Color32::from_black_alpha(160),
                );
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
        }
    }

    fn render_feedback_issue_token_modal_body(&mut self, ui: &mut egui::Ui) {
        let palette = style::palette();
        ui.set_min_width(520.0);
        ui.label(
            RichText::new("After authorizing in the browser, copy the token shown and paste it here.")
                .color(palette.text_primary),
        );
        ui.add_space(8.0);

        let (cancel_clicked, save_clicked, token_to_save) = {
            let state = &mut self.controller.ui.feedback_issue;
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
                if ui.add_enabled(token_valid, egui::Button::new("Save")).clicked() {
                    save_clicked = true;
                }
            });
            (cancel_clicked, save_clicked, state.token_input.clone())
        };

        if cancel_clicked {
            self.controller.ui.feedback_issue.token_modal_open = false;
            self.controller.ui.feedback_issue.token_input.clear();
            self.controller.ui.feedback_issue.focus_token_requested = false;
        }
        if save_clicked {
            self.controller.save_github_issue_token(&token_to_save);
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
            if ui.button("Connect GitHub").clicked() {
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
        match crate::issue_gateway::IssueTokenStore::new().and_then(|store| store.get()) {
            Ok(Some(_)) => ui.label(
                RichText::new("Status: connected")
                    .color(style::status_badge_color(style::StatusTone::Info)),
            ),
            Ok(None) => ui.label(RichText::new("Status: not connected").color(palette.text_muted)),
            Err(err) => ui.label(
                RichText::new(format!("Status: token store error ({err})"))
                    .color(style::status_badge_color(style::StatusTone::Warning)),
            ),
        };

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
            if ui.add_enabled(!submitting, egui::Button::new("Cancel")).clicked() {
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
