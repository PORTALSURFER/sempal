use super::style;
use eframe::egui::{self, Align2, Color32, StrokeKind, TextStyle, Ui};

#[derive(Clone, Copy)]
pub(super) struct RowMetrics {
    pub height: f32,
    pub spacing: f32,
}

impl RowMetrics {
    pub fn pitch(self) -> f32 {
        self.height + self.spacing
    }
}

pub(super) fn list_row_height(ui: &Ui) -> f32 {
    ui.spacing().interact_size.y
}

pub(super) fn clamp_label_for_width(text: &str, available_width: f32) -> String {
    // Rough character-based truncation to avoid layout thrash.
    let width = available_width.max(1.0);
    let approx_char_width = 8.0;
    let max_chars = (width / approx_char_width).floor().max(6.0) as usize;
    if text.chars().count() <= max_chars {
        return text.to_string();
    }
    let keep = max_chars.saturating_sub(3);
    let mut clipped = String::with_capacity(max_chars);
    for (i, ch) in text.chars().enumerate() {
        if i >= keep {
            clipped.push_str("...");
            break;
        }
        clipped.push(ch);
    }
    clipped
}

pub(super) fn render_list_row(
    ui: &mut Ui,
    label: &str,
    row_width: f32,
    row_height: f32,
    bg: Option<Color32>,
    text_color: Color32,
    sense: egui::Sense,
) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(egui::vec2(row_width, row_height), sense);
    let mut fill = bg;
    if response.hovered() && bg.is_none() {
        fill = Some(style::row_hover_fill());
    }
    if let Some(color) = fill {
        ui.painter().rect_filled(rect, 0.0, color);
    }
    ui.painter()
        .rect_stroke(rect, 0.0, style::inner_border(), StrokeKind::Inside);
    let padding = ui.spacing().button_padding;
    let font_id = TextStyle::Button.resolve(ui.style());
    let text_pos = rect.left_center() + egui::vec2(padding.x, 0.0);
    ui.painter()
        .text(text_pos, Align2::LEFT_CENTER, label, font_id, text_color);
    response
}

pub(super) fn scroll_offset_to_reveal_row(
    current_offset: f32,
    row: usize,
    metrics: RowMetrics,
    viewport_height: f32,
    padding_rows: f32,
) -> f32 {
    if viewport_height <= 0.0 {
        return current_offset;
    }
    let padding = (metrics.pitch() * padding_rows).max(0.0);
    let row_top = row as f32 * metrics.pitch();
    let row_bottom = row_top + metrics.height;
    // Valid offsets that keep the row inside the viewport with padding on both sides.
    let min_offset = (row_bottom + padding - viewport_height).max(0.0);
    let max_offset = row_top - padding;
    if max_offset <= min_offset {
        return (row_top - padding).max(0.0);
    }
    if current_offset < min_offset {
        return min_offset;
    }
    if current_offset > max_offset {
        return max_offset;
    }
    // Already inside the valid band; keep offset stable to avoid drift.
    current_offset
}
