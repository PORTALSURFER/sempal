//! Helpers to convert domain data into egui-facing view structs.
// Transitional helpers; wiring into the egui renderer will consume these.

use crate::egui_app::state::{DropTargetRowView, SourceRowView};
use crate::sample_sources::config::DropTargetColor;
use crate::sample_sources::{Rating, SampleSource};
use std::path::Path;

/// Convert a sample source into a UI row.
pub fn source_row(source: &SampleSource, missing: bool) -> SourceRowView {
    let name = source
        .root
        .file_name()
        .and_then(|n| n.to_str())
        .map(|n| n.to_string())
        .unwrap_or_else(|| source.root.to_string_lossy().to_string());
    SourceRowView {
        id: source.id.clone(),
        name,
        path: source.root.to_string_lossy().to_string(),
        missing,
    }
}

/// Convert a drop target path into a UI row.
pub fn drop_target_row(
    path: &Path,
    color: Option<DropTargetColor>,
    missing: bool,
) -> DropTargetRowView {
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .map(|name| name.to_string())
        .unwrap_or_else(|| path.to_string_lossy().to_string());
    let drag_label = format!("Drop target: {name}");
    let tooltip_path = path.display().to_string();
    DropTargetRowView {
        path: path.to_path_buf(),
        name,
        drag_label,
        tooltip_path,
        missing,
        color,
    }
}

/// Helper to derive a browser index from a tag and absolute row position.
pub fn sample_browser_index_for(
    tag: Rating,
    index: usize,
) -> crate::egui_app::state::SampleBrowserIndex {
    use crate::egui_app::state::TriageFlagColumn::*;
    let column = if tag.is_trash() {
        Trash
    } else if tag.is_keep() {
        Keep
    } else {
        Neutral
    };
    crate::egui_app::state::SampleBrowserIndex { column, row: index }
}

/// Produce a user-facing sample label that omits folders and extensions.
pub fn sample_display_label(path: &Path) -> String {
    path.file_stem()
        .and_then(|stem| stem.to_str())
        .map(|stem| stem.to_string())
        .or_else(|| {
            path.file_name()
                .and_then(|name| name.to_str())
                .map(|name| name.to_string())
        })
        .unwrap_or_else(|| path.to_string_lossy().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn sample_display_label_strips_directories_and_extension() {
        let label = sample_display_label(Path::new("kits/sub/hihat_open.WAV"));
        assert_eq!(label, "hihat_open");
    }

    #[test]
    fn sample_display_label_handles_files_without_extension() {
        let label = sample_display_label(Path::new("clips/snare_roll"));
        assert_eq!(label, "snare_roll");
    }
}
