use super::style;
use eframe::egui::{self, Align2, Color32, TextStyle, Ui};
use crate::sample_sources::Rating;

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

#[derive(Clone, Copy)]
pub(super) enum RowBackground {
    None,
    Solid(Color32),
    Gradient { left: Color32, right: Color32 },
}

impl RowBackground {
    pub fn from_option(color: Option<Color32>) -> Self {
        color.map_or(Self::None, Self::Solid)
    }
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

/// Parse a BPM input string into a positive finite value.
pub(super) fn parse_bpm_input(input: &str) -> Option<f32> {
    let trimmed = input.trim().to_lowercase();
    let trimmed = trimmed
        .strip_suffix("bpm")
        .unwrap_or(trimmed.as_str())
        .trim();
    let bpm = trimmed.parse::<f32>().ok()?;
    if bpm.is_finite() && bpm > 0.0 {
        Some(bpm)
    } else {
        None
    }
}

/// Format a BPM value for single-line editing.
pub(super) fn format_bpm_input(value: f32) -> String {
    let rounded = value.round();
    if (value - rounded).abs() < 0.01 {
        format!("{rounded:.0}")
    } else {
        format!("{value:.2}")
    }
}

const LOOP_BADGE_TEXT: &str = "LOOP";
const LOOP_BADGE_PADDING_X: f32 = 6.0;
const LOOP_BADGE_PADDING_Y: f32 = 2.0;
const LOOP_BADGE_GAP: f32 = 6.0;
const BPM_BADGE_PADDING_X: f32 = 6.0;
const BPM_BADGE_PADDING_Y: f32 = 2.0;
const BPM_BADGE_GAP: f32 = 6.0;

/// Return the horizontal space needed for the loop badge, including the gap.
pub(super) fn loop_badge_space(ui: &Ui) -> f32 {
    let font_id = TextStyle::Button.resolve(ui.style());
    let text_width = ui
        .ctx()
        .fonts_mut(|fonts| {
            fonts.layout_no_wrap(LOOP_BADGE_TEXT.to_string(), font_id, Color32::WHITE)
        })
        .size()
        .x;
    LOOP_BADGE_GAP + text_width + LOOP_BADGE_PADDING_X * 2.0
}

pub(super) fn bpm_badge_space(ui: &Ui, label: &str) -> f32 {
    let font_id = TextStyle::Button.resolve(ui.style());
    let text_width = ui
        .ctx()
        .fonts_mut(|fonts| {
            fonts.layout_no_wrap(label.to_string(), font_id, style::bpm_badge_text())
        })
        .size()
        .x;
    BPM_BADGE_GAP + text_width + BPM_BADGE_PADDING_X * 2.0
}

pub(super) struct ListRow<'a> {
    pub label: &'a str,
    pub row_width: f32,
    pub row_height: f32,
    pub background: RowBackground,
    pub skip_hover: bool,
    pub text_color: Color32,
    pub sense: egui::Sense,
    pub number: Option<NumberColumn<'a>>,
    pub marker: Option<RowMarker>,
    pub rating: Option<Rating>,
    pub looped: bool,
    pub bpm_label: Option<&'a str>,
}

pub(super) fn render_list_row(ui: &mut Ui, row: ListRow<'_>) -> egui::Response {
    let (rect, response) =
        ui.allocate_exact_size(egui::vec2(row.row_width, row.row_height), row.sense);
    match row.background {
        RowBackground::None => {}
        RowBackground::Solid(color) => {
            ui.painter().rect_filled(rect, 0.0, color);
        }
        RowBackground::Gradient { left, right } => {
            let mut mesh = egui::epaint::Mesh::default();
            let idx = mesh.vertices.len() as u32;
            let uv = egui::epaint::WHITE_UV;
            mesh.vertices.push(egui::epaint::Vertex {
                pos: rect.left_top(),
                uv,
                color: left,
            });
            mesh.vertices.push(egui::epaint::Vertex {
                pos: rect.right_top(),
                uv,
                color: right,
            });
            mesh.vertices.push(egui::epaint::Vertex {
                pos: rect.right_bottom(),
                uv,
                color: right,
            });
            mesh.vertices.push(egui::epaint::Vertex {
                pos: rect.left_bottom(),
                uv,
                color: left,
            });
            mesh.indices
                .extend_from_slice(&[idx, idx + 1, idx + 2, idx, idx + 2, idx + 3]);
            ui.painter().add(egui::Shape::mesh(mesh));
        }
    }
    if let Some(marker) = row.marker {
        let width = marker.width.max(0.0);
        let marker_rect = egui::Rect::from_min_max(
            rect.right_top() - egui::vec2(width, 0.0),
            rect.right_bottom(),
        );
        ui.painter().rect_filled(marker_rect, 0.0, marker.color);
    }
    if response.hovered() && !row.skip_hover {
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
    let label_rect = ui.painter().text(
        egui::pos2(label_x, rect.center().y),
        Align2::LEFT_CENTER,
        row.label,
        font_id.clone(),
        row.text_color,
    );
    let mut trailing_x = label_rect.right();
    if row.looped {
        let badge_galley = ui.ctx().fonts_mut(|fonts| {
            fonts.layout_no_wrap(
                LOOP_BADGE_TEXT.to_string(),
                font_id.clone(),
                style::high_contrast_text(),
            )
        });
        let badge_min = egui::pos2(
            trailing_x + LOOP_BADGE_GAP,
            rect.center().y - badge_galley.size().y * 0.5 - LOOP_BADGE_PADDING_Y,
        );
        let badge_rect = egui::Rect::from_min_size(
            badge_min,
            egui::vec2(
                badge_galley.size().x + LOOP_BADGE_PADDING_X * 2.0,
                badge_galley.size().y + LOOP_BADGE_PADDING_Y * 2.0,
            ),
        );
        ui.painter()
            .rect_filled(badge_rect, 0.0, style::loop_badge_fill());
        ui.painter().text(
            badge_rect.center(),
            Align2::CENTER_CENTER,
            LOOP_BADGE_TEXT,
            font_id.clone(),
            style::loop_badge_text(),
        );
        trailing_x = badge_rect.right();
    }
    if let Some(label) = row.bpm_label {
        let badge_galley = ui.ctx().fonts_mut(|fonts| {
            fonts.layout_no_wrap(label.to_string(), font_id.clone(), style::bpm_badge_text())
        });
        let badge_min = egui::pos2(
            trailing_x + BPM_BADGE_GAP,
            rect.center().y - badge_galley.size().y * 0.5 - BPM_BADGE_PADDING_Y,
        );
        let badge_rect = egui::Rect::from_min_size(
            badge_min,
            egui::vec2(
                badge_galley.size().x + BPM_BADGE_PADDING_X * 2.0,
                badge_galley.size().y + BPM_BADGE_PADDING_Y * 2.0,
            ),
        );
        ui.painter()
            .rect_filled(badge_rect, 0.0, style::bpm_badge_fill());
        ui.painter().text(
            badge_rect.center(),
            Align2::CENTER_CENTER,
            label,
            font_id.clone(),
            style::bpm_badge_text(),
        );
        trailing_x = badge_rect.right();
    }
    if let Some(rating) = row.rating {
        if !rating.is_neutral() {
            let count = rating.val().abs();
            let color = if rating.is_keep() {
                style::semantic_palette().triage_keep
            } else {
                style::semantic_palette().triage_trash
            };

            let square_size = 6.0;
            let spacing = 3.0;
            let start_x = trailing_x + 6.0;
            let y = rect.center().y - square_size * 0.5;

            for i in 0..count {
                let x = start_x + (i as f32 * (square_size + spacing));
                let r = egui::Rect::from_min_size(
                    egui::pos2(x, y),
                    egui::vec2(square_size, square_size),
                );
                ui.painter().rect_filled(r, 0.0, color);
            }
        }
    }
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
    let submit =
        (response.has_focus() && enter_pressed) || (response.lost_focus() && enter_pressed);
    if submit {
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
