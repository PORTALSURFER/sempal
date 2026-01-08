use super::*;
use crate::egui_app::state::FolderActionPrompt;
use rfd::{MessageButtons, MessageDialog, MessageDialogResult, MessageLevel};
use std::path::Path;

impl EguiController {
    pub(crate) fn open_folder_in_file_explorer(&mut self, relative_folder: &Path) {
        let Some(source) = self.current_source() else {
            self.set_status("Select a source first", StatusTone::Info);
            return;
        };
        let absolute = source.root.join(relative_folder);
        if !absolute.exists() {
            self.set_status(
                format!("Folder missing: {}", absolute.display()),
                StatusTone::Warning,
            );
            return;
        }
        if !absolute.is_dir() {
            self.set_status(
                format!("Not a folder: {}", absolute.display()),
                StatusTone::Warning,
            );
            return;
        }
        if let Err(err) = crate::egui_app::controller::ui::os_explorer::open_folder_in_file_explorer(&absolute) {
            self.set_status(err, StatusTone::Error);
        }
    }

    pub(crate) fn start_folder_rename(&mut self) {
        let Some(target) = self.focused_folder_path() else {
            self.set_status("Focus a folder to rename it", StatusTone::Info);
            return;
        };
        if target.as_os_str().is_empty() {
            self.set_status("Root folder cannot be renamed", StatusTone::Info);
            return;
        }
        let default = target
            .file_name()
            .and_then(|n| n.to_str())
            .map(str::to_string)
            .unwrap_or_else(|| target.to_string_lossy().into_owned());
        self.focus_folder_context();
        self.cancel_new_folder_creation();
        self.ui.sources.folders.pending_action = Some(FolderActionPrompt::Rename {
            target,
            name: default,
        });
        self.ui.sources.folders.rename_focus_requested = true;
    }

    pub(crate) fn cancel_folder_rename(&mut self) {
        if matches!(
            self.ui.sources.folders.pending_action,
            Some(FolderActionPrompt::Rename { .. })
        ) {
            self.ui.sources.folders.pending_action = None;
            self.ui.sources.folders.rename_focus_requested = false;
        }
    }

    pub(crate) fn confirm_folder_delete(&self, target: &Path) -> bool {
        if cfg!(test) {
            return true;
        }
        let message = format!(
            "Delete {} and all files inside it? This cannot be undone.",
            target.display()
        );
        matches!(
            MessageDialog::new()
                .set_title("Delete folder")
                .set_description(message)
                .set_level(MessageLevel::Warning)
                .set_buttons(MessageButtons::YesNo)
                .show(),
            MessageDialogResult::Yes
        )
    }
}
