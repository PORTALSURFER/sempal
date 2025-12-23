use super::helpers::clamp_label_for_width;
use eframe::egui::Ui;
use std::path::{Path, PathBuf};

pub(super) fn folder_row_label(
    row: &crate::egui_app::state::FolderRowView,
    row_width: f32,
    ui: &Ui,
) -> String {
    if row.is_root {
        return ".".to_string();
    }
    let padding = ui.spacing().button_padding.x * 2.0;
    let indent = "  ".repeat(row.depth);
    let icon = if row.has_children {
        if row.expanded { "v" } else { ">" }
    } else {
        "-"
    };
    let raw = format!("{indent}{icon} {}", row.name);
    clamp_label_for_width(&raw, row_width - padding)
}

pub(super) fn sample_housing_folders(relative_path: &Path) -> Vec<PathBuf> {
    let parent = relative_path.parent().unwrap_or(Path::new(""));
    if parent.as_os_str().is_empty() || parent == Path::new(".") {
        return vec![PathBuf::new()];
    }

    let mut folders = Vec::new();
    let mut cursor = parent;
    while !cursor.as_os_str().is_empty() && cursor != Path::new(".") {
        folders.push(cursor.to_path_buf());
        cursor = cursor.parent().unwrap_or(Path::new(""));
    }
    folders
}

#[cfg(test)]
mod tests {
    use super::sample_housing_folders;
    use std::path::{Path, PathBuf};

    #[test]
    fn sample_housing_folders_root_returns_empty_path() {
        assert_eq!(sample_housing_folders(Path::new("kick.wav")), vec![PathBuf::new()]);
    }

    #[test]
    fn sample_housing_folders_includes_ancestors() {
        let folders = sample_housing_folders(Path::new("a/b/c.wav"));
        assert_eq!(folders, vec![PathBuf::from("a/b"), PathBuf::from("a")]);
    }
}
