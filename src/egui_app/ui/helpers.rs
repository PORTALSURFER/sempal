use eframe::egui::{self, Align2, Color32, TextStyle, Ui};

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
        fill = Some(Color32::from_rgb(26, 26, 26));
    }
    if let Some(color) = fill {
        ui.painter().rect_filled(rect, 0.0, color);
    }
    let padding = ui.spacing().button_padding;
    let font_id = TextStyle::Button.resolve(ui.style());
    let text_pos = rect.left_center() + egui::vec2(padding.x, 0.0);
    ui.painter()
        .text(text_pos, Align2::LEFT_CENTER, label, font_id, text_color);
    response
}
