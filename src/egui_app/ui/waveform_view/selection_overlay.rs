use super::selection_geometry::{
    paint_selection_edge_bracket, selection_edge_handle_rect, selection_handle_rect,
};
use super::selection_menu;
use super::style;
use super::*;
use crate::egui_app::state::{DragSource, DragTarget, WaveformView};
use crate::selection::SelectionEdge;
use eframe::egui::{self, Color32, CursorIcon, TextStyle, text::LayoutJob};

pub(super) fn render_selection_overlay(
    app: &mut EguiApp,
    ui: &mut egui::Ui,
    rect: egui::Rect,
    palette: &style::Palette,
    view: WaveformView,
    view_width: f32,
    highlight: Color32,
    pointer_pos: Option<egui::Pos2>,
) -> bool {
    let Some(selection) = app.controller.ui.waveform.selection else {
        return false;
    };
    let painter = ui.painter();
    let to_wave_pos = |pos: egui::Pos2| {
        let normalized = ((pos.x - rect.left()) / rect.width()).clamp(0.0, 1.0);
        normalized.mul_add(view_width, view.start).clamp(0.0, 1.0)
    };

    let start_norm = ((selection.start() - view.start) / view_width).clamp(0.0, 1.0);
    let end_norm = ((selection.end() - view.start) / view_width).clamp(0.0, 1.0);
    let width = rect.width() * (end_norm - start_norm).max(0.0);
    let x = rect.left() + rect.width() * start_norm;
    let selection_rect = egui::Rect::from_min_size(
        egui::pos2(x, rect.top()),
        egui::vec2(width, rect.height()),
    );

    painter.rect_filled(selection_rect, 0.0, style::with_alpha(highlight, 60));

    let handle_rect = selection_handle_rect(selection_rect);
    let handle_response = ui.interact(handle_rect, ui.id().with("selection_handle"), egui::Sense::drag());
    let handle_hovered = handle_response.hovered() || handle_response.dragged();
    let handle_color = if handle_hovered {
        style::with_alpha(highlight, 235)
    } else {
        style::with_alpha(highlight, 205)
    };
    painter.rect_filled(handle_rect, 0.0, handle_color);
    if handle_response.drag_started() {
        if let Some(pos) = handle_response.interact_pointer_pos() {
            let alt = ui.input(|i| i.modifiers.alt);
            if alt {
                let anchor = to_wave_pos(pos);
                app.selection_slide = Some(super::SelectionSlide {
                    anchor,
                    range: selection,
                });
            } else {
                let keep_source_focused = ui.input(|i| i.modifiers.shift);
                app.controller
                    .start_selection_drag_payload(selection, pos, keep_source_focused);
            }
        }
    } else if handle_response.dragged() {
        if let Some(pos) = handle_response.interact_pointer_pos() {
            if let Some(slide) = app.selection_slide {
                let cursor = to_wave_pos(pos);
                let delta = cursor - slide.anchor;
                app.controller.set_selection_range(slide.range.shift(delta));
            } else {
                let shift_down = ui.input(|i| i.modifiers.shift);
                app.controller.update_active_drag(
                    pos,
                    DragSource::Waveform,
                    DragTarget::None,
                    shift_down,
                );
            }
        }
    } else if handle_response.drag_stopped() {
        if app.selection_slide.take().is_some() {
            app.controller.finish_selection_drag();
        } else {
            app.controller.finish_active_drag();
        }
    }
    if handle_response.dragged() {
        ui.output_mut(|o| o.cursor_icon = CursorIcon::Grabbing);
    } else if handle_response.hovered() {
        ui.output_mut(|o| o.cursor_icon = CursorIcon::Grab);
    }

    if let Some(duration_label) = app.controller.ui.waveform.selection_duration.as_deref() {
        let text_color = style::with_alpha(palette.bg_secondary, 240);
        let bar_color = style::with_alpha(highlight, 80);
        let galley = ui.ctx().fonts_mut(|f| {
            f.layout_job(LayoutJob::simple_singleline(
                duration_label.to_string(),
                TextStyle::Small.resolve(ui.style()),
                text_color,
            ))
        });
        let padding = egui::vec2(8.0, 2.0);
        let bar_height = galley.size().y + padding.y * 2.0;
        let bar_rect = egui::Rect::from_min_size(
            egui::pos2(selection_rect.left(), selection_rect.bottom() - bar_height),
            egui::vec2(selection_rect.width(), bar_height),
        );
        painter.rect_filled(bar_rect, 0.0, bar_color);
        let text_pos = egui::pos2(
            (bar_rect.right() - padding.x - galley.size().x).max(bar_rect.left() + padding.x),
            bar_rect.top() + (bar_height - galley.size().y) * 0.5,
        );
        painter.galley(text_pos, galley, text_color);
    }

    let start_edge_rect = selection_edge_handle_rect(selection_rect, SelectionEdge::Start);
    let end_edge_rect = selection_edge_handle_rect(selection_rect, SelectionEdge::End);
    let start_edge_response = ui.interact(
        start_edge_rect,
        ui.id().with("selection_edge_start"),
        egui::Sense::click_and_drag(),
    );
    let end_edge_response = ui.interact(
        end_edge_rect,
        ui.id().with("selection_edge_end"),
        egui::Sense::click_and_drag(),
    );
    let start_edge_pointer_down = start_edge_response.is_pointer_button_down_on();
    let end_edge_pointer_down = end_edge_response.is_pointer_button_down_on();
    let edge_dragging = start_edge_pointer_down
        || end_edge_pointer_down
        || start_edge_response.dragged()
        || start_edge_response.drag_started()
        || end_edge_response.dragged()
        || end_edge_response.drag_started();
    for (edge, edge_rect, edge_response) in [
        (SelectionEdge::Start, start_edge_rect, start_edge_response),
        (SelectionEdge::End, end_edge_rect, end_edge_response),
    ] {
        let pointer_down = edge_response.is_pointer_button_down_on();
        if edge_response.drag_started() || pointer_down {
            app.controller.start_selection_edge_drag(edge);
            if app.selection_edge_offset.is_none() {
                let edge_pos = match edge {
                    SelectionEdge::Start => selection_rect.left(),
                    SelectionEdge::End => selection_rect.right(),
                };
                if let Some(pos) = edge_response.interact_pointer_pos() {
                    app.selection_edge_offset = Some(pos.x - edge_pos);
                } else {
                    app.selection_edge_offset = Some(0.0);
                }
            }
        }
        if (pointer_down || edge_response.dragged())
            && let Some(pos) = edge_response.interact_pointer_pos()
        {
            let offset = app.selection_edge_offset.unwrap_or(0.0);
            let view_fraction = ((pos.x - offset - rect.left()) / rect.width()).clamp(0.0, 1.0);
            let absolute = view.start + view_width.max(f32::EPSILON) * view_fraction;
            let clamped = absolute.clamp(0.0, 1.0);
            app.controller.update_selection_drag(clamped);
        }
        if edge_response.drag_stopped() {
            app.selection_edge_offset = None;
            app.controller.finish_selection_drag();
        }
        let edge_hovered = pointer_pos.is_some_and(|p| edge_rect.contains(p))
            || edge_response.hovered()
            || pointer_down
            || edge_response.dragged();
        if edge_hovered {
            let color = highlight;
            paint_selection_edge_bracket(painter, edge_rect, edge, color);
            ui.output_mut(|o| o.cursor_icon = CursorIcon::ResizeHorizontal);
        }
    }
    if !ui.ctx().input(|i| i.pointer.primary_down()) {
        if app.controller.is_selection_dragging() {
            app.controller.finish_selection_drag();
        }
        app.selection_edge_offset = None;
    }

    let selection_menu = ui.interact(
        selection_rect,
        ui.id().with("selection_context_menu"),
        egui::Sense::click(),
    );
    selection_menu.context_menu(|ui| {
        selection_menu::render_selection_context_menu(app, ui);
    });

    edge_dragging
}
