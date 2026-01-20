use crate::sample_sources::Rating;
use eframe::egui::{
    Color32, Frame, Margin, Rect, Stroke, StrokeKind, Ui, Visuals,
    epaint::{CornerRadius, Shadow},
    style::WidgetVisuals,
};

/// Status tone variants used to pick badge colours.
#[derive(Clone, Copy, Debug)]
pub enum StatusTone {
    /// Idle/neutral status.
    Idle,
    /// Busy/working status.
    Busy,
    /// Informational status.
    Info,
    /// Warning status.
    Warning,
    /// Error status.
    Error,
}

/// Base palette for primary UI surfaces and text.
#[allow(dead_code)]
#[derive(Clone, Copy)]
pub struct Palette {
    /// Primary background color.
    pub bg_primary: Color32,
    /// Secondary background color.
    pub bg_secondary: Color32,
    /// Tertiary background color.
    pub bg_tertiary: Color32,
    /// Outline stroke for panels.
    pub panel_outline: Color32,
    /// Strong grid line color.
    pub grid_strong: Color32,
    /// Subtle grid line color.
    pub grid_soft: Color32,
    /// Primary text color.
    pub text_primary: Color32,
    /// Muted text color.
    pub text_muted: Color32,
    /// Mint accent color.
    pub accent_mint: Color32,
    /// Ice accent color.
    pub accent_ice: Color32,
    /// Copper accent color.
    pub accent_copper: Color32,
    /// Slate accent color.
    pub accent_slate: Color32,
    /// Warning accent color.
    pub warning: Color32,
    /// Success accent color.
    pub success: Color32,
}

/// Semantic colours used across the UI.
#[derive(Clone, Copy)]
pub struct SemanticPalette {
    /// Idle badge color.
    pub badge_idle: Color32,
    /// Busy badge color.
    pub badge_busy: Color32,
    /// Info badge color.
    pub badge_info: Color32,
    /// Warning badge color.
    pub badge_warning: Color32,
    /// Error badge color.
    pub badge_error: Color32,
    /// Drag highlight fill color.
    pub drag_highlight: Color32,
    /// Destructive action color.
    pub destructive: Color32,
    /// Soft warning fill color.
    pub warning_soft: Color32,
    /// Duplicate hover fill color.
    pub duplicate_hover_fill: Color32,
    /// Duplicate hover stroke color.
    pub duplicate_hover_stroke: Color32,
    /// Triage trash color.
    pub triage_trash: Color32,
    /// Subtle triage trash color.
    pub triage_trash_subtle: Color32,
    /// Triage keep color.
    pub triage_keep: Color32,
    /// Playback age light color.
    pub playback_age_light: Color32,
    /// Playback age medium color.
    pub playback_age_medium: Color32,
    /// Playback age dark color.
    pub playback_age_dark: Color32,
    /// High-contrast text color.
    pub text_contrast: Color32,
    /// Missing-item indicator color.
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
        accent_slate: Color32::from_rgb(120, 146, 188),
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
        playback_age_light: Color32::from_rgb(200, 200, 200),
        playback_age_medium: Color32::from_rgb(150, 150, 150),
        playback_age_dark: Color32::from_rgb(110, 110, 110),
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

/// Fill used for the "missing similarity data" indicator dot.
pub fn similarity_missing_dot_fill() -> Color32 {
    semantic_palette().destructive
}

/// Radius for the "missing similarity data" indicator dot.
pub fn similarity_missing_dot_radius() -> f32 {
    3.5
}

/// Fill used when hovering a duplicate drop target.
pub fn duplicate_hover_fill() -> Color32 {
    semantic_palette().duplicate_hover_fill
}

/// Outline used when hovering a duplicate drop target.
pub fn duplicate_hover_stroke() -> Color32 {
    semantic_palette().duplicate_hover_stroke
}

/// Highlight used for the anchor sample of a similarity query.
pub fn similar_anchor_fill() -> Color32 {
    Color32::from_rgb(88, 110, 148)
}

/// Fill used for similarity-ranked rows (expects 0.0 = least similar, 1.0 = most similar).
pub fn similar_score_fill(strength: f32) -> Color32 {
    let similarity = strength.clamp(0.0, 1.0);
    let color = similarity_map_color(1.0 - similarity);
    with_alpha(color, 160)
}

/// Convert similarity scores into a conservative 0.0-1.0 display strength.
pub fn similarity_display_strength(score: f32) -> f32 {
    let normalized = ((score.clamp(-1.0, 1.0) + 1.0) * 0.5).clamp(0.0, 1.0);
    normalized.powf(2.0)
}

/// Smooth map gradient from similar (green) to dissimilar (red).
pub fn similarity_map_color(t: f32) -> Color32 {
    let t = smoothstep(t.clamp(0.0, 1.0));
    let blue = Color32::from_rgb(76, 122, 218);
    let green = Color32::from_rgb(92, 184, 124);
    let yellow = Color32::from_rgb(224, 196, 112);
    let red = Color32::from_rgb(214, 92, 92);
    if t <= 0.33 {
        let local = smoothstep((t / 0.33).clamp(0.0, 1.0));
        blend_rgb(blue, green, local)
    } else if t <= 0.66 {
        let local = smoothstep(((t - 0.33) / 0.33).clamp(0.0, 1.0));
        blend_rgb(green, yellow, local)
    } else {
        let local = smoothstep(((t - 0.66) / 0.34).clamp(0.0, 1.0));
        blend_rgb(yellow, red, local)
    }
}

fn smoothstep(t: f32) -> f32 {
    t * t * (3.0 - 2.0 * t)
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
pub fn triage_marker_color(tag: Rating) -> Option<Color32> {
    let palette = semantic_palette();
    if tag == Rating::TRASH_3 {
        Some(with_alpha(palette.triage_trash, 220))
    } else if tag == Rating::KEEP_3 {
        Some(with_alpha(palette.triage_keep, 220))
    } else {
        None
    }
}

/// Text colour to match the triage flag used for a sample row.
pub fn triage_label_color(tag: Rating) -> Color32 {
    let palette = semantic_palette();
    if tag == Rating::TRASH_3 {
        palette.triage_trash
    } else if tag == Rating::KEEP_3 {
        palette.triage_keep
    } else {
        crate::egui_app::ui::style::palette().text_primary
    }
}

/// Fill color for the loop marker badge shown in the sample browser list.
pub fn loop_badge_fill() -> Color32 {
    palette().accent_copper
}

/// Text color for the loop marker badge shown in the sample browser list.
pub fn loop_badge_text() -> Color32 {
    high_contrast_text()
}

/// Fill color for the long sample badge shown in the sample browser list.
pub fn long_sample_badge_fill() -> Color32 {
    palette().accent_slate
}

/// Text color for the long sample badge shown in the sample browser list.
pub fn long_sample_badge_text() -> Color32 {
    high_contrast_text()
}

/// Fill color for the BPM badge shown in the sample browser list.
pub fn bpm_badge_fill() -> Color32 {
    palette().accent_mint
}

/// Text color for the BPM badge shown in the sample browser list.
pub fn bpm_badge_text() -> Color32 {
    high_contrast_text()
}

/// Text colour representing the playback age bucket for a sample.
pub fn playback_age_label_color(last_played_at: Option<i64>, now_epoch: i64) -> Color32 {
    const WEEK_SECS: i64 = 60 * 60 * 24 * 7;
    const TWO_WEEKS_SECS: i64 = WEEK_SECS * 2;
    const MONTH_SECS: i64 = 60 * 60 * 24 * 30;

    let palette = semantic_palette();
    let Some(last_played_at) = last_played_at else {
        return palette.playback_age_dark;
    };
    let age_secs = now_epoch.saturating_sub(last_played_at).max(0);
    if age_secs < WEEK_SECS {
        palette.playback_age_light
    } else if age_secs < TWO_WEEKS_SECS {
        palette.playback_age_medium
    } else if age_secs >= MONTH_SECS {
        palette.playback_age_dark
    } else {
        palette.playback_age_medium
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

fn blend_rgb(a: Color32, b: Color32, t: f32) -> Color32 {
    let t = t.clamp(0.0, 1.0);
    let lerp = |from: u8, to: u8| -> u8 {
        let from = from as f32;
        let to = to as f32;
        (from + (to - from) * t).round().clamp(0.0, 255.0) as u8
    };
    Color32::from_rgb(lerp(a.r(), b.r()), lerp(a.g(), b.g()), lerp(a.b(), b.b()))
}

/// Border stroke for outer panels and frames.
pub fn outer_border() -> Stroke {
    section_stroke()
}

/// Border stroke for list rows and interior dividers.
pub fn inner_border() -> Stroke {
    let palette = palette();
    Stroke::new(1.0, palette.grid_soft)
}

/// Background used when hovering list rows.
pub fn row_hover_fill() -> Color32 {
    with_alpha(palette().accent_ice, 66)
}

/// Background used for the primary selected list row (last selected).
pub fn row_primary_selection_fill() -> Color32 {
    let palette = palette();
    Color32::from_rgba_unmultiplied(
        palette.accent_mint.r(),
        palette.accent_mint.g(),
        palette.accent_mint.b(),
        70,
    )
}

/// Softer background for secondary selected rows in multi-selection sets.
pub fn row_secondary_selection_fill() -> Color32 {
    let palette = palette();
    Color32::from_rgba_unmultiplied(
        palette.text_muted.r(),
        palette.text_muted.g(),
        palette.text_muted.b(),
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

/// Single-stroke frame used for panels and cards.
pub fn section_frame() -> Frame {
    Frame::new()
        .fill(compartment_fill())
        .stroke(Stroke::NONE)
        .inner_margin(Margin::symmetric(6, 4))
}

/// Stroke used to separate adjacent sections without doubling borders.
pub fn section_stroke() -> Stroke {
    let palette = palette();
    Stroke::new(1.5, palette.panel_outline)
}

/// Paint a section border, optionally highlighting focus without stacking strokes.
pub fn paint_section_border(ui: &Ui, rect: Rect, focused: bool) {
    let stroke = if focused {
        focused_row_stroke()
    } else {
        section_stroke()
    };
    ui.painter()
        .rect_stroke(rect, 0.0, stroke, StrokeKind::Inside);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sample_sources::Rating;

    #[test]
    fn triage_label_color_matches_flags() {
        let semantic = semantic_palette();
        assert_eq!(triage_label_color(Rating::TRASH_3), semantic.triage_trash);
        assert_eq!(triage_label_color(Rating::KEEP_3), semantic.triage_keep);
        assert_eq!(
            triage_label_color(Rating::NEUTRAL),
            palette().text_primary
        );
        assert_eq!(
            triage_label_color(Rating::TRASH_1),
            palette().text_primary
        );
        assert_eq!(
            triage_label_color(Rating::KEEP_1),
            palette().text_primary
        );
    }

    #[test]
    fn playback_age_label_color_uses_buckets() {
        let semantic = semantic_palette();
        let now = 1_000_000;
        let one_day = 60 * 60 * 24;
        assert_eq!(
            playback_age_label_color(Some(now - one_day), now),
            semantic.playback_age_light
        );
        assert_eq!(
            playback_age_label_color(Some(now - one_day * 8), now),
            semantic.playback_age_medium
        );
        assert_eq!(
            playback_age_label_color(Some(now - one_day * 40), now),
            semantic.playback_age_dark
        );
        assert_eq!(
            playback_age_label_color(None, now),
            semantic.playback_age_dark
        );
    }
}
