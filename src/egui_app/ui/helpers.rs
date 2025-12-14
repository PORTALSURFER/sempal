use super::style;
use eframe::egui::{self, Align2, Color32, TextStyle, Ui};

/// Metadata for rendering a fixed-width number column alongside a list row.
pub(super) struct NumberColumn<'a> {
    pub text: &'a str,
    pub width: f32,
    pub color: Color32,
}

/// Optional marker rendered along the trailing edge of a list row.
pub(super) struct RowMarker {
    pub width: f32,
    pub color: Color32,
}

/// Estimate a width that comfortably fits numbering for the given row count.
pub(super) fn number_column_width(total_rows: usize, ui: &Ui) -> f32 {
    let digits = total_rows.max(1).to_string().len() as f32;
    let approx_char_width = 8.0;
    let padding = ui.spacing().button_padding.x;
    padding * 1.5 + digits * approx_char_width
}

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

pub(super) struct ListRow<'a> {
    pub label: &'a str,
    pub row_width: f32,
    pub row_height: f32,
    pub bg: Option<Color32>,
    pub text_color: Color32,
    pub sense: egui::Sense,
    pub number: Option<NumberColumn<'a>>,
    pub marker: Option<RowMarker>,
}

pub(super) fn render_list_row(ui: &mut Ui, row: ListRow<'_>) -> egui::Response {
    let (rect, response) =
        ui.allocate_exact_size(egui::vec2(row.row_width, row.row_height), row.sense);
    if let Some(color) = row.bg {
        ui.painter().rect_filled(rect, 0.0, color);
    }
    if let Some(marker) = row.marker {
        let width = marker.width.max(0.0);
        let marker_rect = egui::Rect::from_min_max(
            rect.right_top() - egui::vec2(width, 0.0),
            rect.right_bottom(),
        );
        ui.painter().rect_filled(marker_rect, 0.0, marker.color);
    }
    if response.hovered() {
        ui.painter().rect_filled(rect, 0.0, style::row_hover_fill());
    }
    // Single divider to avoid stacking strokes between rows.
    ui.painter().line_segment(
        [rect.left_bottom(), rect.right_bottom()],
        style::inner_border(),
    );
    let font_id = TextStyle::Button.resolve(ui.style());
    let padding = ui.spacing().button_padding.x;
    let number_gap = padding * 0.5;
    let mut number_width = 0.0;
    if let Some(column) = row.number {
        number_width = column.width.max(0.0);
        let x = rect.left() + padding;
        ui.painter().text(
            egui::pos2(x, rect.center().y),
            Align2::LEFT_CENTER,
            column.text,
            font_id.clone(),
            column.color,
        );
        number_width += number_gap;
    }
    let label_x = rect.left() + padding + number_width;
    ui.painter().text(
        egui::pos2(label_x, rect.center().y),
        Align2::LEFT_CENTER,
        row.label,
        font_id,
        row.text_color,
    );
    response
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum InlineTextEditAction {
    None,
    Submit,
    Cancel,
}

pub(super) fn render_inline_text_edit(
    ui: &mut Ui,
    rect: egui::Rect,
    value: &mut String,
    hint: &str,
    focus_requested: &mut bool,
) -> InlineTextEditAction {
    let edit = egui::TextEdit::singleline(value)
        .hint_text(hint)
        .frame(false)
        .desired_width(rect.width());
    let response = ui.put(rect, edit);
    if *focus_requested && !response.has_focus() {
        response.request_focus();
        *focus_requested = false;
    }
    let enter_pressed = ui.input(|i| i.key_pressed(egui::Key::Enter));
    let escape_pressed = ui.input(|i| i.key_pressed(egui::Key::Escape));
    if enter_pressed && (response.has_focus() || response.lost_focus()) {
        InlineTextEditAction::Submit
    } else if escape_pressed && (response.has_focus() || response.lost_focus()) {
        InlineTextEditAction::Cancel
    } else if response.lost_focus() {
        InlineTextEditAction::Cancel
    } else {
        InlineTextEditAction::None
    }
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
