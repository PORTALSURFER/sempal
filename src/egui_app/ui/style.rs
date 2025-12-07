use eframe::egui::{
    Color32, Stroke, Visuals,
    epaint::{CornerRadius, Shadow},
    style::WidgetVisuals,
};

#[allow(dead_code)]
#[derive(Clone, Copy)]
pub struct Palette {
    pub bg_primary: Color32,
    pub bg_secondary: Color32,
    pub bg_tertiary: Color32,
    pub panel_outline: Color32,
    pub grid_strong: Color32,
    pub grid_soft: Color32,
    pub text_primary: Color32,
    pub text_muted: Color32,
    pub accent_mint: Color32,
    pub accent_ice: Color32,
    pub accent_copper: Color32,
    pub warning: Color32,
    pub success: Color32,
}

pub fn palette() -> Palette {
    Palette {
        bg_primary: Color32::from_rgb(12, 12, 12),
        bg_secondary: Color32::from_rgb(22, 22, 22),
        bg_tertiary: Color32::from_rgb(32, 32, 32),
        panel_outline: Color32::from_rgb(31, 31, 31),
        grid_strong: Color32::from_rgb(50, 50, 60),
        grid_soft: Color32::from_rgb(40, 40, 50),
        text_primary: Color32::from_rgb(201, 201, 211),
        text_muted: Color32::from_rgb(140, 156, 170),
        accent_mint: Color32::from_rgb(192, 92, 100),
        accent_ice: Color32::from_rgb(50, 50, 60),
        accent_copper: Color32::from_rgb(254, 164, 29),
        warning: Color32::from_rgb(218, 112, 41),
        success: Color32::from_rgb(236, 243, 210),
    }
}

pub fn apply_visuals(visuals: &mut Visuals) {
    let palette = palette();
    visuals.window_fill = palette.bg_primary;
    visuals.panel_fill = palette.bg_secondary;
    visuals.override_text_color = Some(palette.text_primary);
    visuals.hyperlink_color = palette.accent_ice;
    visuals.extreme_bg_color = palette.bg_primary;
    visuals.faint_bg_color = palette.bg_secondary;
    visuals.error_fg_color = palette.warning;
    visuals.warn_fg_color = palette.warning;
    visuals.selection.bg_fill = palette.grid_soft;
    visuals.selection.stroke = Stroke::new(1.0, palette.accent_ice);
    visuals.widgets.noninteractive.bg_fill = palette.bg_secondary;
    visuals.widgets.noninteractive.fg_stroke = Stroke::new(1.0, palette.text_primary);
    set_rectilinear(&mut visuals.widgets.inactive, palette);
    set_rectilinear(&mut visuals.widgets.hovered, palette);
    set_rectilinear(&mut visuals.widgets.active, palette);
    set_rectilinear(&mut visuals.widgets.open, palette);
    visuals.window_corner_radius = CornerRadius::ZERO;
    visuals.menu_corner_radius = CornerRadius::ZERO;
    visuals.popup_shadow = Shadow::NONE;
    visuals.button_frame = true;
}

fn set_rectilinear(vis: &mut WidgetVisuals, palette: Palette) {
    vis.corner_radius = CornerRadius::ZERO;
    vis.bg_fill = palette.bg_tertiary;
    vis.weak_bg_fill = palette.grid_soft;
    vis.bg_stroke = Stroke::new(1.0, palette.panel_outline);
    vis.fg_stroke = Stroke::new(1.0, palette.text_primary);
}

pub fn outer_border() -> Stroke {
    let palette = palette();
    Stroke::new(2.0, palette.panel_outline)
}

pub fn inner_border() -> Stroke {
    let palette = palette();
    Stroke::new(1.0, palette.grid_soft)
}

pub fn row_hover_fill() -> Color32 {
    let palette = palette();
    Color32::from_rgba_unmultiplied(
        palette.accent_ice.r(),
        palette.accent_ice.g(),
        palette.accent_ice.b(),
        28,
    )
}

pub fn row_selected_fill() -> Color32 {
    let palette = palette();
    Color32::from_rgba_unmultiplied(
        palette.accent_mint.r(),
        palette.accent_mint.g(),
        palette.accent_mint.b(),
        40,
    )
}

pub fn compartment_fill() -> Color32 {
    let palette = palette();
    palette.bg_secondary
}
