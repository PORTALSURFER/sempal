use radiant::prelude as ui;

use crate::native_app::app::{GuiMessage, NativeAppState};
use crate::native_app::app_chrome::browser_context_menu;
use crate::native_app::app_chrome::library_browser::library_sidebar;
use crate::native_app::app_chrome::metadata_tag_library;
use crate::native_app::app_chrome::modals;
use crate::native_app::app_chrome::sample_workspace;
use crate::native_app::app_chrome::view_models::{
    library_sidebar::LibrarySidebarViewModel, sample_workspace::SampleWorkspaceViewModel,
};
use crate::native_app::app_chrome::waveform_context_menu;

const BOTTOM_STATUS_BAR_HEIGHT: f32 = 30.0;
const LIBRARY_SIDEBAR_RESIZE_HIT_WIDTH: f32 = 5.0;
const LIBRARY_SIDEBAR_RESIZE_RAIL_WIDTH: f32 = 1.0;

pub(in crate::native_app) fn library_sidebar_region(
    state: &NativeAppState,
) -> ui::View<GuiMessage> {
    ui::row([
        library_sidebar_view(state),
        ui::spacer().width(LIBRARY_SIDEBAR_RESIZE_RAIL_WIDTH),
    ])
    .spacing(0.0)
    .fill_height()
}

pub(in crate::native_app) fn library_sidebar_resize_overlay(
    state: &NativeAppState,
) -> ui::View<GuiMessage> {
    let resize_handle = ui::drag_handle()
        .hover_chrome_only()
        .trailing_rail(LIBRARY_SIDEBAR_RESIZE_RAIL_WIDTH)
        .mapped(GuiMessage::ResizeFolder)
        .key("library-sidebar-resize-handle")
        .id(crate::native_app::ui::ids::LIBRARY_SIDEBAR_RESIZE_HANDLE_ID)
        .style(ui::WidgetStyle::subtle(ui::WidgetTone::Accent))
        .width(LIBRARY_SIDEBAR_RESIZE_HIT_WIDTH)
        .fill_height();
    let leading_width = (state.ui.chrome.folder_panel.size() + LIBRARY_SIDEBAR_RESIZE_RAIL_WIDTH
        - LIBRARY_SIDEBAR_RESIZE_HIT_WIDTH)
        .max(0.0);

    ui::row([
        ui::spacer().width(leading_width),
        resize_handle,
        ui::spacer().fill_width(),
    ])
    .spacing(0.0)
    .fill()
}

fn library_sidebar_view(state: &NativeAppState) -> ui::View<GuiMessage> {
    let model = LibrarySidebarViewModel::from_app_state(state);
    library_sidebar::library_sidebar(model)
}

pub(in crate::native_app) fn metadata_tag_library_region_visible(state: &NativeAppState) -> bool {
    state.metadata.tag_library_open && state.library.folder_browser.selected_file_id().is_some()
}

pub(in crate::native_app) fn metadata_tag_library_region(
    state: &NativeAppState,
) -> ui::View<GuiMessage> {
    metadata_tag_library::panel(state)
}

pub(in crate::native_app) fn sample_workspace_region(
    state: &NativeAppState,
) -> ui::View<GuiMessage> {
    sample_workspace::region(SampleWorkspaceViewModel::from_app_state(state))
}

pub(in crate::native_app) fn library_sidebar_overlays(
    state: &NativeAppState,
) -> ui::Overlays<GuiMessage> {
    ui::overlays()
        .floating_opt(metadata_completion_overlay(state))
        .dismissible_context_menu_opt(
            curation_filter_dropdown_overlay(state),
            GuiMessage::CloseCurationFilterDropdown,
        )
        .dismissible_context_menu_opt(
            harvest_filter_dropdown_overlay(state),
            GuiMessage::CloseHarvestFilterDropdown,
        )
}

fn metadata_completion_overlay(state: &NativeAppState) -> Option<ui::View<GuiMessage>> {
    state.library.folder_browser.selected_file_id()?;
    let completion_options = state.metadata_tag_completion_options();
    if completion_options.is_empty() {
        return None;
    }
    let tag_field_content_width =
        library_sidebar::tag_field_content_width(state.ui.chrome.folder_panel.size());
    let inset_x =
        library_sidebar::LIBRARY_SIDEBAR_PADDING + library_sidebar::METADATA_PANEL_PADDING;
    let metadata_panel_height = state.library.folder_browser.metadata_panel_height();
    let inset_y = BOTTOM_STATUS_BAR_HEIGHT
        + library_sidebar::LIBRARY_SIDEBAR_PADDING
        + metadata_panel_height
        + library_sidebar::TAG_COMPLETION_POPUP_GAP;
    Some(library_sidebar::tag_completion_overlay(
        completion_options.as_slice(),
        tag_field_content_width,
        inset_x,
        inset_y,
    ))
}

fn curation_filter_dropdown_overlay(state: &NativeAppState) -> Option<ui::View<GuiMessage>> {
    let model = LibrarySidebarViewModel::from_app_state(state);
    library_sidebar::curation_filter_dropdown_overlay(&model, BOTTOM_STATUS_BAR_HEIGHT)
}

fn harvest_filter_dropdown_overlay(state: &NativeAppState) -> Option<ui::View<GuiMessage>> {
    let model = LibrarySidebarViewModel::from_app_state(state);
    library_sidebar::harvest_filter_dropdown_overlay(&model, BOTTOM_STATUS_BAR_HEIGHT)
}

pub(in crate::native_app) fn sample_workspace_overlays(
    state: &NativeAppState,
) -> ui::Overlays<GuiMessage> {
    ui::overlays()
        .dismissible_context_menu_opt(context_menu_overlay(state), GuiMessage::CloseContextMenu)
        .blocking_modal_opt(trash_folder_setup_overlay(state))
        .blocking_modal_opt(unsupported_files_overlay(state))
        .blocking_modal_opt(folder_delete_confirmation_overlay(state))
        .blocking_modal_opt(protected_extraction_target_source_overlay(state))
        .blocking_modal_opt(waveform_destructive_edit_overlay(state))
        .blocking_modal_opt(file_move_conflict_overlay(state))
        .blocking_modal_opt(shortcut_help_overlay(state))
}

fn trash_folder_setup_overlay(state: &NativeAppState) -> Option<ui::View<GuiMessage>> {
    state
        .ui
        .browser_interaction
        .pending_trash_folder_setup
        .is_some()
        .then(modals::trash_folder_setup)
}

fn unsupported_files_overlay(state: &NativeAppState) -> Option<ui::View<GuiMessage>> {
    state
        .ui
        .browser_interaction
        .unsupported_files_dialog
        .is_some()
        .then(|| modals::unsupported_files(state))
}

fn context_menu_overlay(state: &NativeAppState) -> Option<ui::View<GuiMessage>> {
    let harvest_active = state
        .library
        .folder_browser
        .harvest_context_menu_actions_active();
    state
        .ui
        .browser_interaction
        .context_menu
        .as_ref()
        .map(|menu| browser_context_menu::overlay(menu, harvest_active))
        .or_else(|| {
            state
                .ui
                .browser_interaction
                .waveform_context_menu
                .as_ref()
                .map(waveform_context_menu::overlay)
        })
}

fn file_move_conflict_overlay(state: &NativeAppState) -> Option<ui::View<GuiMessage>> {
    state
        .library
        .folder_browser
        .pending_file_move_conflict_view()
        .is_some()
        .then(|| modals::file_move_conflict(state))
}

fn folder_delete_confirmation_overlay(state: &NativeAppState) -> Option<ui::View<GuiMessage>> {
    state
        .ui
        .browser_interaction
        .pending_folder_delete
        .is_some()
        .then(|| modals::folder_delete_confirmation(state))
}

fn waveform_destructive_edit_overlay(state: &NativeAppState) -> Option<ui::View<GuiMessage>> {
    state
        .ui
        .browser_interaction
        .pending_waveform_destructive_edit
        .is_some()
        .then(|| modals::waveform_destructive_edit_confirmation(state))
}

fn protected_extraction_target_source_overlay(
    state: &NativeAppState,
) -> Option<ui::View<GuiMessage>> {
    state
        .ui
        .browser_interaction
        .pending_protected_extraction_target_source
        .is_some()
        .then(|| modals::protected_extraction_target_source(state))
}

fn shortcut_help_overlay(state: &NativeAppState) -> Option<ui::View<GuiMessage>> {
    state
        .ui
        .chrome
        .shortcut_help_open
        .then(|| modals::shortcut_help(state))
}
