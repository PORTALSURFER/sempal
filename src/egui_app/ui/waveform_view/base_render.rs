use super::status_badges;
use super::style;
use super::*;
use eframe::egui::{self, Align2, Rgba, TextStyle, TextureOptions};

pub(super) fn render_waveform_base(
    app: &mut EguiApp,
    ui: &mut egui::Ui,
    rect: egui::Rect,
    palette: &style::Palette,
    is_loading: bool,
) -> bool {
    if let Some(message) = app.controller.ui.waveform.notice.as_ref() {
        ui.painter().rect_filled(rect, 0.0, palette.bg_primary);
        let font = TextStyle::Heading.resolve(ui.style());
        ui.painter().text(
            rect.center(),
            Align2::CENTER_CENTER,
            message,
            font,
            status_badges::missing_text_color(),
        );
        return false;
    }

    let tex_id = if let Some(image) = &app.controller.ui.waveform.image {
        let new_size = image.image.size;
        if let Some(tex) = app.waveform_tex.as_mut() {
            if tex.size() == new_size {
                tex.set(image.image.clone(), TextureOptions::LINEAR);
                Some(tex.id())
            } else {
                let tex = ui.ctx().load_texture(
                    "waveform_texture",
                    image.image.clone(),
                    TextureOptions::LINEAR,
                );
                let id = tex.id();
                app.waveform_tex = Some(tex);
                Some(id)
            }
        } else {
            let tex = ui.ctx().load_texture(
                "waveform_texture",
                image.image.clone(),
                TextureOptions::LINEAR,
            );
            let id = tex.id();
            app.waveform_tex = Some(tex);
            Some(id)
        }
    } else {
        app.waveform_tex = None;
        None
    };

    if let Some(id) = tex_id {
        let uv = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0));
        ui.painter()
            .image(id, rect, uv, style::high_contrast_text());
    } else {
        let loading_fill = waveform_loading_fill(ui, palette.bg_primary, palette.accent_copper);
        ui.painter().rect_filled(rect, 0.0, loading_fill);
    }

    let zero_line = style::with_alpha(palette.accent_copper, 140);
    ui.painter().line_segment(
        [egui::pos2(rect.left(), rect.center().y), egui::pos2(rect.right(), rect.center().y)],
        egui::Stroke::new(1.0, zero_line),
    );

    if is_loading {
        let glow = style::with_alpha(palette.accent_copper, 28);
        ui.painter().rect_filled(rect.shrink(2.0), 4.0, glow);
        let font = TextStyle::Heading.resolve(ui.style());
        ui.painter().text(
            rect.center(),
            Align2::CENTER_CENTER,
            "LOADING",
            font,
            style::with_alpha(style::high_contrast_text(), 180),
        );
    }

    true
}

fn waveform_loading_fill(
    ui: &egui::Ui,
    base: egui::Color32,
    accent: egui::Color32,
) -> egui::Color32 {
    let time = ui.input(|i| i.time) as f32;
    let pulse = ((time * 6.0).sin() * 0.5 + 0.5).clamp(0.0, 1.0);
    let base_rgba: Rgba = base.into();
    let accent_rgba: Rgba = accent.into();
    let mixed = base_rgba * (1.0 - pulse * 0.12) + accent_rgba * (pulse * 0.08);
    egui::Color32::from_rgba_unmultiplied(
        (mixed.r() * 255.0) as u8,
        (mixed.g() * 255.0) as u8,
        (mixed.b() * 255.0) as u8,
        255,
    )
}
