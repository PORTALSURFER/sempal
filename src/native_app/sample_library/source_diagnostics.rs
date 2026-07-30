use std::{
    collections::BTreeSet,
    path::PathBuf,
    time::{SystemTime, UNIX_EPOCH},
};

use radiant::prelude as ui;
use wavecrate::sample_sources::{
    SourceDatabase, SourceDatabaseConnectionRole,
    readiness::{ReadinessClassification, ReadinessScopeKind, ReadinessView},
};

use crate::native_app::app::{GuiMessage, NativeAppState, UnsupportedFilesDialogState};
use crate::native_app::sample_library::context_menu_target::BrowserContextTargetKind;

impl NativeAppState {
    pub(in crate::native_app) fn open_unsupported_files(
        &mut self,
        source_id: String,
        context: &mut ui::UiUpdateContext<GuiMessage>,
    ) {
        let Some((source_root, database_root)) =
            self.library.folder_browser.source_roots(&source_id)
        else {
            self.ui.status.sample = String::from("Source is no longer available");
            return;
        };
        let source_label = self
            .library
            .folder_browser
            .source_label(&source_id)
            .unwrap_or(source_id.as_str())
            .to_string();
        self.ui.browser_interaction.unsupported_files_dialog = Some(UnsupportedFilesDialogState {
            source_id: source_id.clone(),
            source_label,
            loading: true,
            paths: Vec::new(),
            error: None,
        });
        let request_source_id = source_id.clone();
        context.business().blocking_io("gui-unsupported-files").run(
            move |_| load_unsupported_files(source_root, database_root, request_source_id.clone()),
            move |result| GuiMessage::UnsupportedFilesDialogFinished { source_id, result },
        );
    }

    pub(in crate::native_app) fn finish_unsupported_files_dialog(
        &mut self,
        source_id: String,
        result: Result<Vec<PathBuf>, String>,
    ) {
        let Some(dialog) = self
            .ui
            .browser_interaction
            .unsupported_files_dialog
            .as_mut()
        else {
            return;
        };
        if dialog.source_id != source_id {
            return;
        }
        dialog.loading = false;
        match result {
            Ok(paths) => {
                let status = format!(
                    "{} unsupported file(s) found in {}",
                    paths.len(),
                    dialog.source_label
                );
                dialog.paths = paths;
                dialog.error = None;
                self.ui.status.sample = status;
            }
            Err(error) => {
                let status = format!("Could not load unsupported files: {error}");
                dialog.paths.clear();
                dialog.error = Some(error);
                self.ui.status.sample = status;
            }
        }
    }

    pub(in crate::native_app) fn close_unsupported_files(&mut self) {
        self.ui.browser_interaction.unsupported_files_dialog = None;
    }

    pub(in crate::native_app) fn reveal_unsupported_file(
        &mut self,
        path: PathBuf,
        context: &mut ui::UiUpdateContext<GuiMessage>,
    ) {
        self.open_context_target(BrowserContextTargetKind::Sample, path, context);
    }

    pub(in crate::native_app) fn move_unsupported_file_to_trash(
        &mut self,
        path: PathBuf,
        context: &mut ui::UiUpdateContext<GuiMessage>,
    ) {
        self.close_unsupported_files();
        self.move_selected_files_to_trash(vec![path], std::time::Instant::now(), context);
    }
}

fn load_unsupported_files(
    source_root: PathBuf,
    database_root: PathBuf,
    source_id: String,
) -> Result<Vec<PathBuf>, String> {
    let connection = SourceDatabase::open_connection_with_role_and_database_root(
        &source_root,
        &database_root,
        SourceDatabaseConnectionRole::JobWorker,
    )
    .map_err(|error| format!("open source readiness database: {error}"))?;
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|error| format!("read current time: {error}"))?
        .as_secs() as i64;
    let snapshot = ReadinessView::new(&connection)
        .reconcile(&source_id, now)
        .map_err(|error| format!("read source readiness: {error}"))?;
    let mut paths = BTreeSet::new();
    for entry in snapshot.entries {
        if entry.target.scope_kind != ReadinessScopeKind::File
            || !matches!(
                entry.classification,
                ReadinessClassification::Unsupported { .. }
            )
        {
            continue;
        }
        if let Some(relative_path) = entry.target.relative_path {
            paths.insert(source_root.join(relative_path));
        }
    }
    Ok(paths.into_iter().collect())
}
