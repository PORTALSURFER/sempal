use crate::sample_sources::SampleTag;
use eframe::egui::{
    Color32, Stroke, Visuals,
    epaint::{CornerRadius, Shadow},
    style::WidgetVisuals,
};

/// Status tone variants used to pick badge colours.
#[derive(Clone, Copy, Debug)]
pub enum StatusTone {
    Idle,
    Busy,
    Info,
    Warning,
    Error,
}

/// Base palette for primary UI surfaces and text.
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

/// Semantic colours used across the UI.
#[derive(Clone, Copy)]
pub struct SemanticPalette {
    pub badge_idle: Color32,
    pub badge_busy: Color32,
    pub badge_info: Color32,
    pub badge_warning: Color32,
    pub badge_error: Color32,
    pub drag_highlight: Color32,
    pub destructive: Color32,
    pub warning_soft: Color32,
    pub duplicate_hover_fill: Color32,
    pub duplicate_hover_stroke: Color32,
    pub triage_trash: Color32,
    pub triage_trash_subtle: Color32,
    pub triage_keep: Color32,
    pub text_contrast: Color32,
    pub missing: Color32,
}

/// Primary UI palette values.
pub fn palette() -> Palette {
    Palette {
        bg_primary: Color32::from_rgb(12, 11, 10),
        bg_secondary: Color32::from_rgb(20, 18, 16),
        bg_tertiary: Color32::from_rgb(28, 26, 23),
        panel_outline: Color32::from_rgb(44, 40, 36),
        grid_strong: Color32::from_rgb(55, 50, 45),
        grid_soft: Color32::from_rgb(42, 38, 34),
        text_primary: Color32::from_rgb(224, 227, 234),
        text_muted: Color32::from_rgb(166, 173, 184),
        accent_mint: Color32::from_rgb(152, 172, 158),
        accent_ice: Color32::from_rgb(168, 150, 126),
        accent_copper: Color32::from_rgb(186, 148, 108),
        warning: Color32::from_rgb(194, 158, 108),
        success: Color32::from_rgb(186, 204, 186),
    }
}

/// Secondary palette for semantic colours not tied to the base background/foreground set.
pub fn semantic_palette() -> SemanticPalette {
    SemanticPalette {
        badge_idle: Color32::from_rgb(42, 46, 54),
        badge_busy: Color32::from_rgb(164, 146, 116),
        badge_info: Color32::from_rgb(156, 176, 158),
        badge_warning: Color32::from_rgb(192, 158, 112),
        badge_error: Color32::from_rgb(184, 112, 112),
        drag_highlight: Color32::from_rgb(180, 156, 126),
        destructive: Color32::from_rgb(184, 112, 112),
        warning_soft: Color32::from_rgb(204, 176, 132),
        duplicate_hover_fill: Color32::from_rgb(48, 52, 58),
        duplicate_hover_stroke: Color32::from_rgb(164, 146, 116),
        triage_trash: Color32::from_rgb(158, 102, 96),
        triage_trash_subtle: Color32::from_rgb(116, 78, 74),
        triage_keep: Color32::from_rgb(126, 156, 126),
        text_contrast: Color32::WHITE,
        missing: Color32::from_rgb(204, 132, 132),
    }
}

/// Apply an alpha channel to a solid colour.
pub fn with_alpha(color: Color32, alpha: u8) -> Color32 {
    Color32::from_rgba_unmultiplied(color.r(), color.g(), color.b(), alpha)
}

/// Colour for status badges by tone.
pub fn status_badge_color(tone: StatusTone) -> Color32 {
    let palette = semantic_palette();
    match tone {
        StatusTone::Idle => palette.badge_idle,
        StatusTone::Busy => palette.badge_busy,
        StatusTone::Info => palette.badge_info,
        StatusTone::Warning => palette.badge_warning,
        StatusTone::Error => palette.badge_error,
    }
}

/// Strongest contrast text colour for dark surfaces.
pub fn high_contrast_text() -> Color32 {
    semantic_palette().text_contrast
}

/// Destructive action text colour.
pub fn destructive_text() -> Color32 {
    semantic_palette().destructive
}

/// Text colour for missing entities.
pub fn missing_text() -> Color32 {
    semantic_palette().missing
}

/// Text colour used for soft warnings.
pub fn warning_soft_text() -> Color32 {
    semantic_palette().warning_soft
}

/// Fill used when hovering a duplicate drop target.
pub fn duplicate_hover_fill() -> Color32 {
    semantic_palette().duplicate_hover_fill
}

/// Outline used when hovering a duplicate drop target.
pub fn duplicate_hover_stroke() -> Color32 {
    semantic_palette().duplicate_hover_stroke
}

/// Stroke used to indicate drag targets.
pub fn drag_target_stroke() -> Stroke {
    Stroke::new(2.0, with_alpha(semantic_palette().drag_highlight, 180))
}

/// Width of the trailing marker used to denote triage flags in list rows.
pub fn triage_marker_width() -> f32 {
    25.0
}

/// Colour for the trailing triage marker based on tag.
pub fn triage_marker_color(tag: SampleTag) -> Option<Color32> {
    let palette = semantic_palette();
    match tag {
        SampleTag::Trash => Some(with_alpha(palette.triage_trash, 220)),
        SampleTag::Keep => Some(with_alpha(palette.triage_keep, 220)),
        SampleTag::Neutral => None,
    }
}

/// Apply the shared palette to egui visuals for a consistent frame look.
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

/// Border stroke for outer panels and frames.
pub fn outer_border() -> Stroke {
    let palette = palette();
    Stroke::new(2.0, palette.panel_outline)
}

/// Border stroke for list rows and interior dividers.
pub fn inner_border() -> Stroke {
    let palette = palette();
    Stroke::new(1.0, palette.grid_soft)
}

/// Background used when hovering list rows.
pub fn row_hover_fill() -> Color32 {
    let palette = palette();
    Color32::from_rgba_unmultiplied(
        palette.accent_ice.r(),
        palette.accent_ice.g(),
        palette.accent_ice.b(),
        28,
    )
}

/// Background used for selected list rows.
pub fn row_selected_fill() -> Color32 {
    let palette = palette();
    Color32::from_rgba_unmultiplied(
        palette.accent_mint.r(),
        palette.accent_mint.g(),
        palette.accent_mint.b(),
        40,
    )
}

/// Indicator used to show multi-selection membership.
pub fn selection_marker_fill() -> Color32 {
    with_alpha(palette().accent_ice, 190)
}

/// Outline used to indicate keyboard/pointer focus.
pub fn focused_row_stroke() -> Stroke {
    Stroke::new(2.0, palette().accent_ice)
}

/// Background for compartment frames.
pub fn compartment_fill() -> Color32 {
    let palette = palette();
    palette.bg_secondary
}
