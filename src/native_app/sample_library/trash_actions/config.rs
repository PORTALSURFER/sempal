use radiant::prelude as ui;
use radiant::prelude::PlatformResultExt as _;
use std::time::Instant;

use crate::native_app::app::{GuiMessage, NativeAppState, emit_gui_action};

impl NativeAppState {
    pub(in crate::native_app) fn pick_trash_folder(
        &mut self,
        context: &mut ui::UiUpdateContext<GuiMessage>,
    ) {
        let started_at = Instant::now();
        context.pick_folder(
            ui::FileDialogRequest::new().title("Choose trash folder"),
            GuiMessage::TrashFolderDialogFinished,
        );
        emit_gui_action(
            "settings.trash_folder.pick",
            Some("settings"),
            None,
            "requested",
            started_at,
            None,
        );
    }

    pub(in crate::native_app) fn choose_trash_folder_for_pending_move(
        &mut self,
        context: &mut ui::UiUpdateContext<GuiMessage>,
    ) {
        if self
            .ui
            .browser_interaction
            .pending_trash_folder_setup
            .is_some()
        {
            self.pick_trash_folder(context);
        }
    }

    pub(in crate::native_app) fn cancel_trash_folder_setup(&mut self) {
        if self
            .ui
            .browser_interaction
            .pending_trash_folder_setup
            .take()
            .is_some()
        {
            self.ui.status.sample =
                String::from("Trash move canceled: choose a trash folder first");
        }
    }

    pub(in crate::native_app) fn finish_trash_folder_dialog(
        &mut self,
        result: ui::PlatformResult,
        context: &mut ui::UiUpdateContext<GuiMessage>,
    ) {
        let started_at = Instant::now();
        let pending_move = self
            .ui
            .browser_interaction
            .pending_trash_folder_setup
            .take();
        let path = match result.into_path_or_canceled() {
            Ok(Some(path)) => path,
            Ok(None) => {
                if pending_move.is_some() {
                    self.ui.status.sample = String::from("Trash move canceled: no folder selected");
                }
                emit_gui_action(
                    "settings.trash_folder.pick",
                    Some("settings"),
                    None,
                    "cancelled",
                    started_at,
                    None,
                );
                return;
            }
            Err(error) => {
                self.ui.status.sample = format!("Trash folder selection failed: {error}");
                emit_gui_action(
                    "settings.trash_folder.pick",
                    Some("settings"),
                    None,
                    "error",
                    started_at,
                    Some(&error),
                );
                return;
            }
        };
        self.ui.settings.persisted.trash_folder = Some(path.clone());
        self.persist_user_configuration("settings.trash_folder.persist", started_at);
        self.ui.status.sample = format!("Trash folder set to {}", path.display());
        let target = path.display().to_string();
        emit_gui_action(
            "settings.trash_folder.pick",
            Some("settings"),
            Some(target.as_str()),
            "success",
            started_at,
            None,
        );
        if let Some(pending_move) = pending_move {
            self.move_file_paths_to_trash(
                pending_move.paths,
                pending_move.action,
                pending_move.started_at,
                context,
            );
        }
    }

    pub(in crate::native_app) fn clear_trash_folder(&mut self) {
        let started_at = Instant::now();
        self.ui.settings.persisted.trash_folder = None;
        self.persist_user_configuration("settings.trash_folder.clear", started_at);
        self.ui.status.sample = String::from("Trash folder cleared");
        emit_gui_action(
            "settings.trash_folder.clear",
            Some("settings"),
            None,
            "success",
            started_at,
            None,
        );
    }
}
