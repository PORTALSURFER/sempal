use super::style;
use super::overlay_layers::{self, OverlayLayer};
use crate::egui_app::state::ProgressOverlayState;
use eframe::egui::{self, Align2, Area, Color32, Frame, Id, ProgressBar, RichText, Stroke};

/// Render the modal progress overlay for long-running tasks.
pub(super) fn render_progress_overlay(ctx: &egui::Context, progress: &mut ProgressOverlayState) {
    if !progress.visible || !progress.modal {
        return;
    }
    overlay_layers::modal_backdrop(
        ctx,
        Id::new("progress_overlay_backdrop"),
        Color32::from_rgba_premultiplied(0, 0, 0, 160),
    );
    let palette = style::palette();
    let title = if progress.title.is_empty() {
        "Working...".to_string()
    } else {
        progress.title.clone()
    };
    Area::new(Id::new("progress_overlay_panel"))
        .order(OverlayLayer::Modal.order())
        .constrain(true)
        .anchor(Align2::CENTER_CENTER, egui::Vec2::ZERO)
        .show(ctx, |ui| {
            let frame = Frame::window(&ctx.style())
                .fill(style::compartment_fill())
                .stroke(Stroke::new(1.0, palette.panel_outline));
            frame.show(ui, |ui| {
                ui.vertical(|ui| {
                    ui.heading(RichText::new(&title).color(palette.text_primary));
                    if let Some(detail) = progress.detail.as_deref() {
                        ui.add_space(6.0);
                        ui.label(RichText::new(detail).color(palette.text_muted));
                    }
                    ui.add_space(8.0);
                    let fraction = progress.fraction();
                    let mut bar = ProgressBar::new(fraction)
                        .desired_width(260.0)
                        .animate(true);
                    if progress.total > 0 {
                        let pct = (fraction * 100.0).round().clamp(0.0, 100.0);
                        bar = bar.text(format!("{pct:.0}%"));
                    } else if progress.task == Some(crate::egui_app::state::ProgressTaskKind::Scan)
                        && progress.completed > 0
                    {
                        bar = bar.text(format!("{} files", progress.completed));
                    } else {
                        bar = bar.text("Working...");
                    }
                    ui.add(bar);
                    if progress.total > 0 {
                        ui.label(format!(
                            "{} of {} item(s)",
                            progress.completed.min(progress.total),
                            progress.total
                        ));
                    }
                    ui.add_space(6.0);
                    if progress.cancelable {
                        let canceling = progress.cancel_requested;
                        let cancel_label = if canceling { "Canceling..." } else { "Cancel" };
                        let button = egui::Button::new(cancel_label);
                        if ui.add_enabled(!canceling, button).clicked() {
                            progress.cancel_requested = true;
                        }
                    }
                });
            });
        });
}
