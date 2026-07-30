//! Wavecrate library browser modules for sources, folders, sample rows, and file workflows.

pub(in crate::native_app) const DRAG_PREVIEW_MAX_WIDTH: f32 = 280.0;
pub(in crate::native_app) const DRAG_PREVIEW_HEIGHT: f32 = 20.0;

pub(in crate::native_app) mod committed_file_mutations;
pub(in crate::native_app) mod context_menu_target;
pub(in crate::native_app) mod drag_drop_actions;
mod exclusive_file_transfer;
pub(in crate::native_app) mod file_actions;
pub(in crate::native_app) mod folder_browser;
pub(in crate::native_app) mod folder_browser_actions;
pub(in crate::native_app) mod folder_browser_rename_actions;
pub(in crate::native_app) mod folder_create_actions;
pub(in crate::native_app) mod folder_scan_actions;
pub(in crate::native_app) mod folder_startup_verify_actions;
pub(in crate::native_app) mod folder_tree_refresh_actions;
pub(in crate::native_app) mod harvest_tracking;
pub(in crate::native_app) mod native_file_drop_actions;
pub(in crate::native_app) mod native_file_open_actions;
pub(in crate::native_app) mod sample_collections;
pub(in crate::native_app) mod sample_list;
pub(in crate::native_app) mod sample_ratings;
pub(in crate::native_app) mod selected_file_actions;
pub(in crate::native_app) mod similarity_artifacts;
pub(in crate::native_app) mod similarity_controls;
pub(in crate::native_app) mod similarity_scores;
pub(in crate::native_app) mod source_diagnostics;
pub(in crate::native_app) mod source_prep;
pub(in crate::native_app) mod source_watcher;
pub(in crate::native_app) mod trash_actions;
