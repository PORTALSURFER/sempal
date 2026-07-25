use radiant::prelude as ui;

use crate::native_app::app::{BrowserScrollSurface, GuiMessage};
use crate::native_app::app_chrome::view_models::library_sidebar::FilterSectionViewModel;
use crate::native_app::sample_library::folder_browser::commands::FolderBrowserMessage;
use crate::native_app::sample_library::folder_browser::view_contract::{
    SIDEBAR_PANEL_HEADER_CONTENT_SPACING, SIDEBAR_PANEL_HEADER_HEIGHT,
};
use crate::native_app::ui::ids as widget_ids;

mod rows;
#[cfg(test)]
mod tests;

use super::edge_aligned_resize_panel;
use rows::{
    FILTER_CONTROL_HORIZONTAL_PADDING, FILTER_CONTROLS_CONTENT_HEIGHT,
    FILTER_LABEL_CONTROL_SPACING, FILTER_LABEL_WIDTH, FILTER_ROW_CONTROL_HEIGHT, FILTER_ROW_HEIGHT,
    FILTER_ROW_SPACING, FILTER_ROW_VERTICAL_INSET, curation_filter_dropdown_menu, filter_rows,
    harvest_filter_dropdown_menu,
};

pub(super) const FILTER_PANEL_PADDING: f32 = 10.0;
#[cfg(test)]
const FILTER_PANEL_HEADER_HEIGHT: f32 = SIDEBAR_PANEL_HEADER_HEIGHT;
const FILTER_PANEL_HEADER_CONTENT_SPACING: f32 = SIDEBAR_PANEL_HEADER_CONTENT_SPACING;
const FILTER_SECTION_SCROLL_NODE_ID: u64 = widget_ids::FILTER_SECTION_SCROLL_NODE_ID;
const FILTER_RESIZE_HEADER_ID: u64 = widget_ids::FILTER_RESIZE_HEADER_ID;

#[cfg(test)]
const FILTER_SECTION_NODE_ID: u64 = widget_ids::FILTER_SECTION_NODE_ID;
const CURATION_FILTER_ROW_INDEX: usize = 2;
const HARVEST_FILTER_ROW_INDEX: usize = 3;

pub(super) fn filter_section(model: &FilterSectionViewModel) -> ui::View<GuiMessage> {
    let panel = edge_aligned_resize_panel(
        "filter-resize-header",
        FILTER_RESIZE_HEADER_ID,
        SIDEBAR_PANEL_HEADER_HEIGHT,
        filter_controls(model),
        model.panel_height,
        FILTER_PANEL_PADDING,
        FILTER_PANEL_HEADER_CONTENT_SPACING,
        |message| GuiMessage::FolderBrowser(FolderBrowserMessage::ResizeFilterPanel(message)),
    );
    #[cfg(test)]
    {
        panel.id(FILTER_SECTION_NODE_ID)
    }
    #[cfg(not(test))]
    {
        panel
    }
}

fn filter_controls(model: &FilterSectionViewModel) -> ui::View<GuiMessage> {
    let rows = filter_rows(model);
    ui::scroll(
        ui::column(rows)
            .fill_width()
            .height(FILTER_CONTROLS_CONTENT_HEIGHT)
            .spacing(FILTER_ROW_SPACING),
    )
    .id(FILTER_SECTION_SCROLL_NODE_ID)
    .on_scroll_update(|_| GuiMessage::BrowserScrollAccepted(BrowserScrollSurface::Filter))
    .fill_width()
    .fill_height()
}

pub(super) fn curation_filter_dropdown_overlay(
    model: &FilterSectionViewModel,
    sidebar_inset_x: f32,
    filter_bottom_inset: f32,
) -> Option<ui::View<GuiMessage>> {
    let (menu, size) = curation_filter_dropdown_menu(model)?;
    Some(filter_dropdown_overlay(
        menu,
        size,
        model,
        sidebar_inset_x,
        filter_bottom_inset,
        CURATION_FILTER_ROW_INDEX,
        "curation-filter-dropdown-overlay",
    ))
}

pub(super) fn harvest_filter_dropdown_overlay(
    model: &FilterSectionViewModel,
    sidebar_inset_x: f32,
    filter_bottom_inset: f32,
) -> Option<ui::View<GuiMessage>> {
    let (menu, size) = harvest_filter_dropdown_menu(model)?;
    Some(filter_dropdown_overlay(
        menu,
        size,
        model,
        sidebar_inset_x,
        filter_bottom_inset,
        HARVEST_FILTER_ROW_INDEX,
        "harvest-filter-dropdown-overlay",
    ))
}

fn filter_dropdown_overlay(
    menu: ui::View<GuiMessage>,
    size: ui::Vector2,
    model: &FilterSectionViewModel,
    sidebar_inset_x: f32,
    filter_bottom_inset: f32,
    row_index: usize,
    key: &'static str,
) -> ui::View<GuiMessage> {
    let trigger_bottom_from_filter_top = FILTER_PANEL_PADDING
        + SIDEBAR_PANEL_HEADER_HEIGHT
        + FILTER_PANEL_HEADER_CONTENT_SPACING
        + row_index as f32 * (FILTER_ROW_HEIGHT + FILTER_ROW_SPACING)
        + FILTER_ROW_VERTICAL_INSET
        + FILTER_ROW_CONTROL_HEIGHT;
    let trigger_bottom_inset =
        filter_bottom_inset + (model.panel_height - trigger_bottom_from_filter_top).max(0.0);
    let menu_bottom_inset = (trigger_bottom_inset - FILTER_ROW_SPACING - size.y).max(0.0);
    let menu_inset_x = sidebar_inset_x
        + FILTER_PANEL_PADDING
        + FILTER_LABEL_WIDTH
        + FILTER_LABEL_CONTROL_SPACING
        + FILTER_CONTROL_HORIZONTAL_PADDING;
    ui::anchored_layer(
        menu,
        size,
        ui::LayerHorizontalAnchor::Start,
        ui::LayerVerticalAnchor::End,
        menu_inset_x,
        menu_bottom_inset,
    )
    .key(key)
}
