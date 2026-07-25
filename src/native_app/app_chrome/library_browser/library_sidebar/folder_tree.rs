use radiant::prelude as ui;

use crate::native_app::app::GuiMessage;
use crate::native_app::app_chrome::view_models::library_sidebar::FolderTreeViewModel;
use crate::native_app::sample_library::folder_browser::model::VisibleFolder;
use crate::native_app::sample_library::folder_browser::view_contract::{
    FOLDER_TREE_LIST_ID, FOLDER_TREE_OVERSCAN_ROWS, TREE_ROW_HEIGHT,
};

mod identity;
mod rows;
mod status;
#[cfg(test)]
mod tests;

#[cfg(test)]
pub(in crate::native_app) use identity::retained_folder_row_input_id as folder_row_widget_id;
use rows::{folder_row, folder_tree_guide_style};
use status::selected_folder_status;

pub(super) fn folder_tree_section(model: FolderTreeViewModel) -> ui::View<GuiMessage> {
    ui::column([
        folder_tree_view(model.visible_folders, model.window),
        selected_folder_status(
            model.selected_folder_status_label,
            model.selected_source_missing,
            model.include_subfolders_available,
            model.include_subfolders,
            model.show_empty_folders,
            model.help_tooltips_enabled,
        ),
    ])
    .spacing(0.0)
    .fill_width()
    .fill_height()
}

fn folder_tree_view(
    visible_folders: Vec<VisibleFolder>,
    window: ui::VirtualListWindow,
) -> ui::View<GuiMessage> {
    folder_tree_window(visible_folders, window)
        .id(FOLDER_TREE_LIST_ID)
        .fill_width()
        .fill_height()
}

fn folder_tree_window(
    visible_folders: Vec<VisibleFolder>,
    window: ui::VirtualListWindow,
) -> ui::View<GuiMessage> {
    let mut view = radiant::application::virtual_tree_list_windowed(
        window,
        TREE_ROW_HEIGHT,
        &folder_tree_guide_rows(&visible_folders),
        folder_tree_guide_style(),
        |index| folder_row(&visible_folders[index]),
    )
    .overscan_px(TREE_ROW_HEIGHT * FOLDER_TREE_OVERSCAN_ROWS as f32)
    .view()
    .without_chrome();
    view = view.on_scroll_update(move |update| {
        let change = ui::virtual_list_window_change_for_scroll(
            update,
            TREE_ROW_HEIGHT,
            window,
            FOLDER_TREE_OVERSCAN_ROWS,
        );
        let boundary = virtual_window_needs_materialization(
            window,
            change.window,
            update.offset.y,
            TREE_ROW_HEIGHT,
            update.viewport.y,
        );
        GuiMessage::FolderTreeWindowChanged(boundary.then_some(change))
    });
    view.fill_height()
}

/// Keep scrolling inside the rows already projected by the host. The runtime
/// owns the pixel offset; only an edge crossing needs a new application window.
fn virtual_window_needs_materialization(
    current: ui::VirtualListWindow,
    next: ui::VirtualListWindow,
    offset_y: f32,
    row_height: f32,
    viewport_height: f32,
) -> bool {
    let row_height = row_height.max(1.0);
    let visible_end = ((offset_y.max(0.0) + viewport_height.max(0.0)) / row_height)
        .ceil()
        .max(0.0) as usize;
    let visible_end = visible_end.min(next.total_items);
    current.total_items != next.total_items
        || current.viewport_len() != next.viewport_len()
        || next.window_start < current.window_start
        || visible_end > current.window_end
}

fn folder_tree_guide_rows(folders: &[VisibleFolder]) -> Vec<ui::TreeGuideRow> {
    folders
        .iter()
        .map(|folder| {
            ui::TreeGuideRow::new(
                folder.depth,
                folder.has_children && folder.expanded && !folder.is_source_root,
            )
        })
        .collect()
}
