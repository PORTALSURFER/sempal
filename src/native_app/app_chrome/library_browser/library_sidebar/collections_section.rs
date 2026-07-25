use radiant::prelude as ui;

use crate::native_app::app::{BrowserScrollSurface, GuiMessage};
use crate::native_app::app_chrome::view_models::library_sidebar::CollectionsSectionViewModel;
use crate::native_app::sample_library::folder_browser::commands::FolderBrowserMessage;
use crate::native_app::sample_library::folder_browser::view_contract::{
    COLLECTION_ROW_SPACING, COLLECTIONS_PANEL_HEADER_CONTENT_SPACING, COLLECTIONS_PANEL_PADDING,
    SIDEBAR_PANEL_HEADER_HEIGHT,
};
use crate::native_app::ui::ids as widget_ids;

mod identity;
mod rows;
#[cfg(test)]
mod tests;

use super::edge_aligned_resize_panel;
use rows::collection_row;

/// Stable layout node id for collection-panel resize regression coverage.
const COLLECTIONS_SECTION_NODE_ID: u64 = widget_ids::COLLECTIONS_SECTION_NODE_ID;
/// Stable layout node id for the collection rows scroll viewport.
const COLLECTIONS_LIST_SCROLL_NODE_ID: u64 = widget_ids::COLLECTIONS_LIST_SCROLL_NODE_ID;
const COLLECTIONS_RESIZE_HEADER_ID: u64 = widget_ids::COLLECTIONS_RESIZE_HEADER_ID;
const COLLECTIONS_OVERFLOW_FADE_RAMP: f32 = 12.0;
const COLLECTIONS_OVERFLOW_FADE_MAX_ALPHA: u8 = u8::MAX;

pub(super) fn collections_section(model: &CollectionsSectionViewModel) -> ui::View<GuiMessage> {
    let rows = model.rows.iter().map(collection_row).collect::<Vec<_>>();
    edge_aligned_resize_panel(
        "collections-resize-header",
        COLLECTIONS_RESIZE_HEADER_ID,
        SIDEBAR_PANEL_HEADER_HEIGHT,
        ui::scroll(
            ui::column(rows)
                .spacing(COLLECTION_ROW_SPACING)
                .fill_width()
                .height(model.list_height),
        )
        .id(COLLECTIONS_LIST_SCROLL_NODE_ID)
        .on_scroll_update(|_| GuiMessage::BrowserScrollAccepted(BrowserScrollSurface::Collections))
        .fill_width()
        .fill_height(),
        model.panel_height,
        COLLECTIONS_PANEL_PADDING,
        COLLECTIONS_PANEL_HEADER_CONTENT_SPACING,
        |message| GuiMessage::FolderBrowser(FolderBrowserMessage::ResizeCollectionsPanel(message)),
    )
    .id(COLLECTIONS_SECTION_NODE_ID)
}

/// Returns the opacity for the passive bottom-edge affordance.
///
/// The fade starts becoming visible at the first clipped pixel, then reaches
/// its intended strength over roughly one row. The ease-out curve deliberately
/// makes the initial clipping legible without making the cue pop to full
/// opacity.
pub(in crate::native_app) fn collection_overflow_fade_alpha(
    panel_height: f32,
    full_height: f32,
) -> u8 {
    let clipped_height = (full_height - panel_height).max(0.0);
    let progress = (clipped_height / COLLECTIONS_OVERFLOW_FADE_RAMP).clamp(0.0, 1.0);
    let strength = 1.0 - (1.0 - progress).powi(3);
    (f32::from(COLLECTIONS_OVERFLOW_FADE_MAX_ALPHA) * strength).round() as u8
}
