use super::style;
use super::*;
use crate::egui_app::state::LoopCrossfadeUnit;
use eframe::egui::{self, Align2, RichText};

impl EguiApp {
    pub(super) fn render_loop_crossfade_prompt(&mut self, ctx: &egui::Context) {
        let mut open = true;
        let mut apply = false;
        let mut close_prompt = false;
        let Some(prompt) = self.controller.ui.loop_crossfade_prompt.as_mut() else {
            return;
        };
        egui::Window::new("Seamless loop crossfade")
            .anchor(Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
            .collapsible(false)
            .resizable(false)
            .auto_sized()
            .open(&mut open)
            .show(ctx, |ui| {
                render_loop_crossfade_body(ui, prompt, &mut apply, &mut close_prompt);
            });
        if apply {
            if let Err(err) = self.controller.apply_loop_crossfade_prompt() {
                self.controller.set_status(err, style::StatusTone::Error);
            }
            return;
        }
        if close_prompt || !open {
            self.controller.clear_loop_crossfade_prompt();
        }
    }
}

fn render_loop_crossfade_body(
    ui: &mut egui::Ui,
    prompt: &mut crate::egui_app::state::LoopCrossfadePrompt,
    apply: &mut bool,
    close_prompt: &mut bool,
) {
    let palette = style::palette();
    ui.set_min_width(320.0);
    ui.label(
        RichText::new(prompt.relative_path.display().to_string()).color(palette.text_primary),
    );
    ui.add_space(8.0);
    ui.label("Crossfade depth");
    ui.horizontal(|ui| {
        match prompt.settings.unit {
            LoopCrossfadeUnit::Milliseconds => {
                let mut depth = prompt.settings.depth_ms.max(1);
                let drag = egui::DragValue::new(&mut depth)
                    .clamp_range(1..=5000)
                    .suffix(" ms");
                if ui.add(drag).changed() {
                    prompt.settings.depth_ms = depth;
                }
            }
            LoopCrossfadeUnit::Samples => {
                let mut depth = prompt.settings.depth_samples.max(1);
                let drag = egui::DragValue::new(&mut depth)
                    .clamp_range(1..=2_000_000)
                    .suffix(" samples");
                if ui.add(drag).changed() {
                    prompt.settings.depth_samples = depth;
                }
            }
        }
        egui::ComboBox::from_id_salt("loop_crossfade_unit")
            .selected_text(unit_label(prompt.settings.unit))
            .show_ui(ui, |ui| {
                ui.selectable_value(
                    &mut prompt.settings.unit,
                    LoopCrossfadeUnit::Milliseconds,
                    unit_label(LoopCrossfadeUnit::Milliseconds),
                );
                ui.selectable_value(
                    &mut prompt.settings.unit,
                    LoopCrossfadeUnit::Samples,
                    unit_label(LoopCrossfadeUnit::Samples),
                );
            });
    });
    ui.add_space(8.0);
    ui.label(
        RichText::new("Creates a new file; the original stays untouched.").color(palette.text_muted),
    );
    ui.add_space(8.0);
    ui.horizontal(|ui| {
        if ui.button("Cancel").clicked() {
            *close_prompt = true;
        }
        let apply_btn = egui::Button::new(RichText::new("Apply").color(palette.text_primary));
        if ui.add(apply_btn).clicked() {
            *apply = true;
        }
    });
}

fn unit_label(unit: LoopCrossfadeUnit) -> &'static str {
    match unit {
        LoopCrossfadeUnit::Milliseconds => "ms",
        LoopCrossfadeUnit::Samples => "samples",
    }
}
