use radiant::prelude as ui;

use crate::native_app::app::GuiMessage;
use crate::native_app::app_chrome::view_models::library_sidebar::{
    FilterSectionViewModel, LibrarySidebarViewModel,
};

mod collections_section;
mod filter_section;
mod folder_tree;
mod harvest_family;
mod overflow_fade;
mod sidebar_row;
mod source_section;
mod tag_completion;
mod tag_editor;
mod tag_entry_layout;
#[cfg(test)]
mod test_support;

pub(in crate::native_app) use collections_section::collection_overflow_fade_alpha;
use collections_section::collections_section;
use filter_section::filter_section;
use folder_tree::folder_tree_section;
use harvest_family::harvest_family_section;
pub(in crate::native_app) use overflow_fade::{
    paint_vertical_scroll_overflow_fades, paint_waveform_scroll_fades,
};
use source_section::source_selector;
use tag_editor::tag_editor_section;

pub(in crate::native_app) const LIBRARY_SIDEBAR_PADDING: f32 = 0.0;
pub(in crate::native_app) const METADATA_PANEL_PADDING: f32 = 10.0;
const LIBRARY_SIDEBAR_SECTION_SPACING: f32 = 0.0;
const LIBRARY_SIDEBAR_SECTION_DIVIDER_HEIGHT: f32 = 1.0;

#[cfg(test)]
pub(in crate::native_app) use folder_tree::folder_row_widget_id;
pub(in crate::native_app) use source_section::source_row_widget_id;
pub(in crate::native_app) use tag_completion::{TAG_COMPLETION_POPUP_GAP, tag_completion_overlay};
pub(in crate::native_app) use tag_entry_layout::tag_field_content_width;
#[cfg(test)]
pub(in crate::native_app) use test_support::library_sidebar_view;

pub(in crate::native_app) fn library_sidebar(
    model: LibrarySidebarViewModel,
) -> ui::View<GuiMessage> {
    let sidebar_width = model.sidebar_width;
    library_sidebar_content(model)
        .width(sidebar_width)
        .fill_height()
}

/// Builds a resizable sidebar panel whose drag rail sits on the section edge.
///
/// Panel content keeps its normal inset, while the resize header remains outside
/// that padding so its visual rail and hit target align with the divider above.
fn edge_aligned_resize_panel(
    key: &'static str,
    header_id: u64,
    header_height: f32,
    content: ui::View<GuiMessage>,
    panel_height: f32,
    content_padding: f32,
    content_spacing: f32,
    map: impl Fn(ui::DragHandleMessage) -> GuiMessage + Send + Sync + 'static,
) -> ui::View<GuiMessage> {
    ui::panel_section_from_header_parts(
        radiant::application::PanelSectionHeaderParts::resize_header(
            key,
            header_height,
            content.padding(content_padding).fill_width().fill_height(),
            map,
        )
        .header_id(header_id)
        .padding(0.0)
        .spacing(content_spacing)
        .without_chrome()
        .height(panel_height),
    )
    .fill_width()
}

pub(in crate::native_app) fn curation_filter_dropdown_overlay(
    model: &LibrarySidebarViewModel,
    bottom_status_bar_height: f32,
) -> Option<ui::View<GuiMessage>> {
    filter_dropdown_overlay(
        model,
        bottom_status_bar_height,
        |filter, inset_x, bottom_inset| {
            filter_section::curation_filter_dropdown_overlay(filter, inset_x, bottom_inset)
        },
    )
}

pub(in crate::native_app) fn harvest_filter_dropdown_overlay(
    model: &LibrarySidebarViewModel,
    bottom_status_bar_height: f32,
) -> Option<ui::View<GuiMessage>> {
    filter_dropdown_overlay(
        model,
        bottom_status_bar_height,
        |filter, inset_x, bottom_inset| {
            filter_section::harvest_filter_dropdown_overlay(filter, inset_x, bottom_inset)
        },
    )
}

fn filter_dropdown_overlay(
    model: &LibrarySidebarViewModel,
    bottom_status_bar_height: f32,
    overlay: impl FnOnce(&FilterSectionViewModel, f32, f32) -> Option<ui::View<GuiMessage>>,
) -> Option<ui::View<GuiMessage>> {
    let harvest_family_inset = model
        .harvest_family
        .is_some()
        .then_some(
            harvest_family::HARVEST_FAMILY_SECTION_HEIGHT + LIBRARY_SIDEBAR_SECTION_DIVIDER_HEIGHT,
        )
        .unwrap_or(0.0);
    let filter_bottom_inset = bottom_status_bar_height
        + LIBRARY_SIDEBAR_PADDING
        + model.metadata_panel_height
        + LIBRARY_SIDEBAR_SECTION_DIVIDER_HEIGHT
        + harvest_family_inset;
    overlay(&model.filter, LIBRARY_SIDEBAR_PADDING, filter_bottom_inset)
}

fn library_sidebar_content(model: LibrarySidebarViewModel) -> ui::View<GuiMessage> {
    let mut sections = Vec::with_capacity(11);
    sections.push(source_selector(&model.source_selector));
    sections.push(section_divider());
    sections.push(folder_tree_section(model.folder_tree));
    sections.push(section_divider());
    sections.push(collections_section(&model.collections));
    sections.push(section_divider());
    sections.push(filter_section(&model.filter));
    if model.filter.harvest.family_open
        && let Some(harvest_family) = model.harvest_family.as_ref()
    {
        sections.push(section_divider());
        sections.push(harvest_family_section(harvest_family));
    }
    sections.push(section_divider());
    sections.push(tag_editor_section(
        &model.tag_editor,
        model.sidebar_width,
        model.metadata_panel_height,
    ));
    ui::column(sections)
        .spacing(LIBRARY_SIDEBAR_SECTION_SPACING)
        .fill_width()
        .padding_x(LIBRARY_SIDEBAR_PADDING)
        .fill_height()
}

fn section_divider() -> ui::View<GuiMessage> {
    ui::feedback_overlay()
        .background(ui::ThemeTokens::default().border_emphasis)
        .view()
        .fill_width()
        .height(LIBRARY_SIDEBAR_SECTION_DIVIDER_HEIGHT)
}

#[cfg(test)]
mod tests {
    use super::*;
    use radiant::prelude::IntoView;

    #[test]
    fn sidebar_section_divider_paints_one_pixel_without_an_outline() {
        let frame = section_divider().view_frame_at_size_with_default_theme(ui::Vector2::new(
            240.0,
            LIBRARY_SIDEBAR_SECTION_DIVIDER_HEIGHT,
        ));
        let divider_color = ui::ThemeTokens::default().border_emphasis;

        assert!(frame.paint_plan.fill_rects().any(|fill| {
            fill.color == divider_color && fill.rect.width() == 240.0 && fill.rect.height() == 1.0
        }));
        assert_eq!(frame.paint_plan.stroke_rects().count(), 0);
    }
}
