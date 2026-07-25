//! Wavecrate-specific colors layered over Radiant's shared dark theme.

use radiant::prelude::{DenseRowMarkerStyle, DenseRowPalette, Rgba8, WidgetStyle};

pub(in crate::native_app) const ACCENT: Rgba8 = Rgba8::new(233, 88, 67, 255);
pub(in crate::native_app) const ACCENT_SOFT: Rgba8 = Rgba8::new(241, 121, 98, 255);
pub(in crate::native_app) const DANGER: Rgba8 = Rgba8::new(239, 76, 61, 255);
pub(in crate::native_app) const TEXT_PRIMARY: Rgba8 = Rgba8::new(216, 215, 211, 255);
pub(in crate::native_app) const TEXT_MUTED: Rgba8 = Rgba8::new(153, 155, 154, 255);
pub(in crate::native_app) const COOL_SELECTION: Rgba8 = Rgba8::new(174, 176, 173, 255);
pub(in crate::native_app) const PALE_MARKER: Rgba8 = Rgba8::new(231, 229, 223, 255);
pub(in crate::native_app) const SELECTED_ROW_FILL: Rgba8 = ACCENT.with_alpha(8);
pub(in crate::native_app) const SELECTED_ROW_HOVER_FILL: Rgba8 = ACCENT.with_alpha(16);
pub(in crate::native_app) const SELECTION_FLASH_FILL: Rgba8 = ACCENT.with_alpha(88);
pub(in crate::native_app) const SELECTED_ROW_MARKER_WIDTH: f32 = 2.0;
pub(in crate::native_app) const FOCUSED_ROW_MARKER_WIDTH: f32 = 6.0;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(in crate::native_app) struct ListItemState {
    pub(in crate::native_app) selected: bool,
    pub(in crate::native_app) focused: bool,
    pub(in crate::native_app) focus_alpha: u8,
}

impl ListItemState {
    pub(in crate::native_app) const fn new(selected: bool, focused: bool) -> Self {
        Self {
            selected,
            focused,
            focus_alpha: if focused { u8::MAX } else { 0 },
        }
    }

    pub(in crate::native_app) const fn with_focus_alpha(mut self, focus_alpha: u8) -> Self {
        self.focus_alpha = if self.focused { focus_alpha } else { 0 };
        self
    }
}

pub(in crate::native_app) fn selected_row_palette(style: WidgetStyle) -> DenseRowPalette {
    let mut palette = radiant::gui::list::dense_row_palette_from_style(
        &radiant::prelude::ThemeTokens::default(),
        style,
    );
    palette.selected = Some(SELECTED_ROW_FILL);
    palette.selected_hovered = Some(SELECTED_ROW_HOVER_FILL);
    // Pointer-down uses the focus outline instead of Radiant's opaque pressed
    // fill. A selected row falls back to its quiet persistent selection fill.
    palette.pressed = None;
    palette
}

pub(in crate::native_app) fn selection_flash_palette(style: WidgetStyle) -> DenseRowPalette {
    let mut palette = selected_row_palette(style);
    palette.selected = Some(SELECTION_FLASH_FILL);
    palette.selected_hovered = Some(SELECTION_FLASH_FILL);
    palette
}

pub(in crate::native_app) fn selected_row_marker() -> DenseRowMarkerStyle {
    DenseRowMarkerStyle::new(
        radiant::gui::list::DenseRowMarkerParts::leading(SELECTED_ROW_MARKER_WIDTH)
            .edge_inset(0.0)
            .vertical_inset(0.0),
        ACCENT,
    )
}

pub(in crate::native_app) fn focused_row_marker() -> DenseRowMarkerStyle {
    focused_row_marker_with_alpha(u8::MAX)
}

fn focused_row_marker_with_alpha(alpha: u8) -> DenseRowMarkerStyle {
    DenseRowMarkerStyle::new(
        radiant::gui::list::DenseRowMarkerParts::leading(FOCUSED_ROW_MARKER_WIDTH)
            .edge_inset(0.0)
            .vertical_inset(0.0),
        PALE_MARKER.with_alpha(alpha),
    )
}

pub(in crate::native_app) fn hovered_row_trailing_marker() -> DenseRowMarkerStyle {
    DenseRowMarkerStyle::new(
        radiant::gui::list::DenseRowMarkerParts::trailing(SELECTED_ROW_MARKER_WIDTH)
            .edge_inset(0.0)
            .vertical_inset(0.0),
        PALE_MARKER.with_alpha(180),
    )
}

pub(in crate::native_app) trait WavecrateListRowStyle<Message: 'static> {
    /// Apply the common interaction layer. Row-specific semantic states such
    /// as keep, anchor, cached, processing, warning, or error may override the
    /// palette or individual markers afterward without replacing focus state.
    fn wavecrate_list_row_style(self, style: WidgetStyle, state: ListItemState) -> Self;
}

impl<Message: 'static> WavecrateListRowStyle<Message>
    for radiant::application::InteractiveRowUnderlayBuilder<Message>
{
    fn wavecrate_list_row_style(self, style: WidgetStyle, state: ListItemState) -> Self {
        self.dense_chrome_palette(selected_row_palette(style))
            .leading_marker_if(state.selected, selected_row_marker())
            .leading_overlay_marker_if(
                state.focused && state.focus_alpha > 0,
                focused_row_marker_with_alpha(state.focus_alpha),
            )
            .pressed_leading_overlay_marker(focused_row_marker())
            .hover_trailing_marker(hovered_row_trailing_marker())
    }
}

pub(in crate::native_app) trait WavecrateTreeRowStyle {
    fn wavecrate_tree_row_style(self, style: WidgetStyle, state: ListItemState) -> Self;
}

impl WavecrateTreeRowStyle for radiant::application::TreeRowBuilder {
    fn wavecrate_tree_row_style(self, style: WidgetStyle, state: ListItemState) -> Self {
        self.palette(selected_row_palette(style))
            .selected_marker(selected_row_marker())
            .focus_marker(focused_row_marker_with_alpha(state.focus_alpha))
            .pressed_focus_marker(focused_row_marker())
            .hover_trailing_marker(hovered_row_trailing_marker())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn app_palette_matches_the_editorial_terminal_theme() {
        let theme = radiant::theme::ThemeTokens::default();

        assert_eq!(ACCENT, theme.accent_mint);
        assert_eq!(DANGER, theme.accent_danger);
        assert_eq!(TEXT_PRIMARY, theme.text_primary);
        assert_eq!(TEXT_MUTED, theme.text_muted);
    }

    #[test]
    fn selected_rows_share_one_quiet_fill_and_leading_rail() {
        let palette =
            selected_row_palette(WidgetStyle::subtle(radiant::prelude::WidgetTone::Accent));
        let marker = selected_row_marker();
        let hover = hovered_row_trailing_marker();
        let focus = focused_row_marker();

        assert_eq!(palette.selected, Some(SELECTED_ROW_FILL));
        assert_eq!(palette.selected_hovered, Some(SELECTED_ROW_HOVER_FILL));
        assert_eq!(marker.color, ACCENT);
        assert_eq!(marker.parts.width, SELECTED_ROW_MARKER_WIDTH);
        assert_eq!(marker.parts.vertical_inset, 0.0);
        assert_eq!(hover.color, PALE_MARKER.with_alpha(180));
        assert_eq!(focus.parts.width, FOCUSED_ROW_MARKER_WIDTH);
        assert_eq!(focus.color, PALE_MARKER);
    }

    #[test]
    fn selection_flash_uses_a_stronger_accent_fill() {
        let palette =
            selection_flash_palette(WidgetStyle::subtle(radiant::prelude::WidgetTone::Accent));

        assert_eq!(palette.selected, Some(SELECTION_FLASH_FILL));
        assert_eq!(palette.selected_hovered, Some(SELECTION_FLASH_FILL));
        assert!(SELECTION_FLASH_FILL.a > SELECTED_ROW_HOVER_FILL.a);
    }
}
