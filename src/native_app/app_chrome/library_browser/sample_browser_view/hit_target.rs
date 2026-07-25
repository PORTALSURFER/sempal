use radiant::prelude as ui;

use super::identity;
use crate::native_app::app::GuiMessage;
use crate::native_app::app_chrome::palette::{
    ListItemState, WavecrateListRowStyle, selected_row_marker, selected_row_palette,
    selection_flash_palette,
};

const SAMPLE_ROW_STYLE: ui::WidgetStyle = ui::WidgetStyle::subtle(ui::WidgetTone::Accent);
const COPY_FLASH_FILL: ui::Rgba8 = ui::Rgba8 {
    r: 71,
    g: 220,
    b: 255,
    a: 118,
};
const PROTECTED_SOURCE_ERROR_FILL: ui::Rgba8 = ui::Rgba8 {
    r: 255,
    g: 69,
    b: 54,
    a: 155,
};
const PROTECTED_SOURCE_ERROR_HOVER_FILL: ui::Rgba8 = ui::Rgba8 {
    r: 255,
    g: 82,
    b: 62,
    a: 185,
};
const CUT_PENDING_FILL: ui::Rgba8 = ui::Rgba8 {
    r: 182,
    g: 111,
    b: 44,
    a: 120,
};
const CUT_PENDING_HOVER_FILL: ui::Rgba8 = ui::Rgba8 {
    r: 214,
    g: 132,
    b: 52,
    a: 150,
};
const CUT_PENDING_MARKER: ui::Rgba8 = ui::Rgba8 {
    r: 255,
    g: 163,
    b: 73,
    a: 245,
};
const MISSING_FILL: ui::Rgba8 = ui::Rgba8 {
    r: 130,
    g: 30,
    b: 28,
    a: 145,
};
const MISSING_HOVER_FILL: ui::Rgba8 = ui::Rgba8 {
    r: 170,
    g: 42,
    b: 38,
    a: 175,
};
const MISSING_MARKER: ui::Rgba8 = ui::Rgba8 {
    r: 255,
    g: 69,
    b: 54,
    a: 250,
};
const CACHED_MARKER: ui::Rgba8 = ui::Rgba8 {
    r: 226,
    g: 226,
    b: 226,
    a: 210,
};
const SAMPLE_LIST_SCROLLBAR_WIDTH: f32 = 3.0;
const CACHED_MARKER_SCROLLBAR_GAP: f32 = 2.0;
const CACHED_MARKER_EDGE_INSET: f32 = SAMPLE_LIST_SCROLLBAR_WIDTH + CACHED_MARKER_SCROLLBAR_GAP;

pub(in crate::native_app) struct SampleFileHitTargetModel<'a> {
    pub(in crate::native_app) file_id: &'a str,
    pub(in crate::native_app) selected: bool,
    pub(in crate::native_app) focused: bool,
    pub(in crate::native_app) focus_alpha: u8,
    pub(in crate::native_app) selection_flash: bool,
    pub(in crate::native_app) copy_flash: bool,
    pub(in crate::native_app) protected_source_error_flash: bool,
    pub(in crate::native_app) cut_pending: bool,
    pub(in crate::native_app) drag_active: bool,
    pub(in crate::native_app) drag_source: bool,
    pub(in crate::native_app) cached: bool,
    pub(in crate::native_app) missing: bool,
    pub(in crate::native_app) hit_path: String,
    pub(in crate::native_app) help_tooltips_enabled: bool,
}

pub(super) fn sample_file_hit_target(
    content: ui::View<GuiMessage>,
    model: SampleFileHitTargetModel<'_>,
) -> ui::View<GuiMessage> {
    let actions = sample_file_row_actions(model.hit_path.clone());
    sample_file_hit_target_builder(content, &model)
        .stable_row_identity(
            identity::RETAINED_SAMPLE_ROW_INPUT_SCOPE,
            identity::retained_sample_row_key(model.file_id),
        )
        .actions(actions)
        .tooltip_if(
            model.help_tooltips_enabled,
            "Sample row: select, double-click to load, drag to copy, right-click for actions.",
        )
}

fn sample_file_hit_target_builder(
    content: ui::View<GuiMessage>,
    model: &SampleFileHitTargetModel<'_>,
) -> ui::InteractiveRowUnderlayBuilder<GuiMessage> {
    let underlay = ui::interactive_row_underlay(content)
        .dense_row_policy(sample_file_row_policy(model))
        .wavecrate_list_row_style(
            SAMPLE_ROW_STYLE,
            ListItemState::new(model.selected, model.focused).with_focus_alpha(model.focus_alpha),
        )
        .leading_marker_if(
            model.selected || model.cut_pending || model.missing,
            sample_file_leading_marker(model),
        )
        .trailing_marker_if(
            model.cached
                && !model.selected
                && !model.focused
                && !model.selection_flash
                && !model.copy_flash
                && !model.cut_pending,
            ui::DenseRowMarkerStyle::new(
                radiant::gui::list::DenseRowMarkerParts::trailing(2.0)
                    .edge_inset(CACHED_MARKER_EDGE_INSET),
                CACHED_MARKER,
            ),
        );
    if let Some(palette) = sample_file_row_palette(model) {
        underlay.dense_chrome_palette(palette)
    } else {
        underlay
    }
}

fn sample_file_row_policy(model: &SampleFileHitTargetModel<'_>) -> ui::DenseRowPolicy {
    let visual_state = radiant::widgets::InteractiveRowVisualStateParts {
        selected: model.selected
            || model.selection_flash
            || model.copy_flash
            || model.protected_source_error_flash
            || model.cut_pending
            || model.missing,
        ..radiant::widgets::InteractiveRowVisualStateParts::default()
    };
    ui::DenseRowPolicy::with_visual_state(visual_state)
        .tracked_drag_source(model.drag_active, model.drag_source)
        .drag_session_motion(model.drag_active)
        .activation_modifiers()
        .style(SAMPLE_ROW_STYLE)
}

fn sample_file_row_actions(path: String) -> ui::InteractiveRowActions<GuiMessage> {
    ui::row_actions()
        .primary_with_modifiers_and_double_key(
            path.clone(),
            |path, modifiers| GuiMessage::SelectSampleWithModifiers { path, modifiers },
            |path| GuiMessage::SelectSampleWithModifiers {
                path,
                modifiers: Default::default(),
            },
        )
        .secondary_key(path.clone(), |path, position| {
            GuiMessage::OpenSampleContextMenu { path, position }
        })
        .drag_key(path, |path, drag| GuiMessage::DragSampleFile { path, drag })
}

fn sample_file_row_palette(model: &SampleFileHitTargetModel<'_>) -> Option<ui::DenseRowPalette> {
    if model.copy_flash {
        return Some(
            ui::DenseRowPalette::new()
                .selected(COPY_FLASH_FILL)
                .selected_hovered(COPY_FLASH_FILL)
                .interaction_fills(COPY_FLASH_FILL, COPY_FLASH_FILL),
        );
    }
    if model.protected_source_error_flash {
        return Some(
            ui::DenseRowPalette::new()
                .selected(PROTECTED_SOURCE_ERROR_FILL)
                .selected_hovered(PROTECTED_SOURCE_ERROR_HOVER_FILL)
                .interaction_fills(
                    PROTECTED_SOURCE_ERROR_HOVER_FILL,
                    PROTECTED_SOURCE_ERROR_HOVER_FILL,
                ),
        );
    }
    if model.missing {
        return Some(
            ui::DenseRowPalette::new()
                .selected(MISSING_FILL)
                .selected_hovered(MISSING_HOVER_FILL)
                .interaction_fills(MISSING_HOVER_FILL, MISSING_HOVER_FILL),
        );
    }
    if model.cut_pending {
        return Some(
            ui::DenseRowPalette::new()
                .selected(CUT_PENDING_FILL)
                .selected_hovered(CUT_PENDING_HOVER_FILL)
                .interaction_fills(CUT_PENDING_HOVER_FILL, CUT_PENDING_HOVER_FILL),
        );
    }
    if model.selection_flash {
        return Some(selection_flash_palette(SAMPLE_ROW_STYLE));
    }
    if model.selected {
        return Some(selected_row_palette(SAMPLE_ROW_STYLE));
    }
    None
}

fn sample_file_leading_marker(model: &SampleFileHitTargetModel<'_>) -> ui::DenseRowMarkerStyle {
    if model.missing {
        return ui::DenseRowMarkerStyle::new(
            radiant::gui::list::DenseRowMarkerParts::leading(3.0).vertical_inset(4.0),
            MISSING_MARKER,
        );
    }
    if model.cut_pending {
        return ui::DenseRowMarkerStyle::new(
            radiant::gui::list::DenseRowMarkerParts::leading(3.0).vertical_inset(4.0),
            CUT_PENDING_MARKER,
        );
    }
    selected_row_marker()
}

#[cfg(test)]
pub(in crate::native_app) fn sample_file_hit_target_for_tests(
    content: ui::View<GuiMessage>,
    model: SampleFileHitTargetModel<'_>,
    input_id: u64,
) -> ui::View<GuiMessage> {
    let actions = sample_file_row_actions(model.hit_path.clone());
    sample_file_hit_target_builder(content, &model)
        .input_id(input_id)
        .actions(actions)
}

#[cfg(test)]
fn sample_row_palette_for_tests() -> ui::DenseRowPalette {
    selected_row_palette(SAMPLE_ROW_STYLE)
}

#[cfg(test)]
#[path = "hit_target_tests.rs"]
mod tests;
