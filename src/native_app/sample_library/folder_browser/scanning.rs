mod discovery_merge;
mod entry;
mod file_entry_metadata;
mod metadata;
mod progress;
mod traversal;
mod verification;

pub(super) use discovery_merge::{merge_scan_discovery, upsert_file, upsert_folder};
pub(super) use entry::{BrowserEntryKind, classify_path_without_following};
pub(super) use file_entry_metadata::file_entry;
pub(in crate::native_app) use file_entry_metadata::file_entry_with_snapshot_metadata;
pub(in crate::native_app::sample_library::folder_browser) use metadata::refreshed_file_entries_for_paths;
pub(super) use metadata::{RatingSnapshot, SourceMetadataMap, file_entry_for_source_path};
#[cfg(test)]
pub(in crate::native_app) use progress::INDEX_PROGRESS_REPORT_INTERVAL;
#[cfg(test)]
pub(in crate::native_app) use progress::scan_source_with_progress;
pub(in crate::native_app) use progress::scan_source_with_progress_cancellable;
pub(in crate::native_app) use traversal::load_folder_at_path_with_browser_metadata;
pub(in crate::native_app) use traversal::refresh_folder_tree_only;
pub(super) use traversal::{load_folder_at_path, load_source_snapshot, placeholder_folder};
pub(in crate::native_app) use verification::verify_direct_folder;
