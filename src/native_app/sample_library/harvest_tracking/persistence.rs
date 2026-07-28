use super::{
    FolderMoveRequest, GuiMessage, HarvestDerivationOperation, HarvestSeenPersistResult,
    HarvestSourceRange, NativeAppState, NewHarvestDerivation, Path, PathBuf, SelectionRange,
    persist_harvest_seen,
};
use radiant::prelude as ui;

impl NativeAppState {
    pub(in crate::native_app) fn invalidate_harvest_touched_for_path(&mut self, path: &Path) {
        let Some(request) = self.harvest_touched_persist_request_for_path(path) else {
            return;
        };
        self.background.harvest_touched_persist.invalidate(&request);
    }

    pub(in crate::native_app) fn schedule_harvest_seen_for_path(
        &self,
        path: &Path,
        context: &mut radiant::prelude::UiUpdateContext<GuiMessage>,
    ) {
        let Some(request) = self.harvest_seen_persist_request_for_path(path) else {
            return;
        };
        context
            .business()
            .priority(
                "gui-harvest-seen-persist",
                radiant::prelude::TaskPriority::Idle,
            )
            .run(
                move |_| persist_harvest_seen(request),
                GuiMessage::HarvestSeenPersisted,
            );
    }

    pub(in crate::native_app) fn finish_harvest_seen_persist(
        &mut self,
        result: HarvestSeenPersistResult,
    ) {
        if let Err(error) = result.result {
            tracing::warn!(
                file_id = %result.file_id,
                "failed to mark harvest file as seen in background: {error}"
            );
        }
    }

    pub(in crate::native_app) fn schedule_harvest_touched_for_path(
        &mut self,
        path: &Path,
        context: &mut ui::UiUpdateContext<GuiMessage>,
    ) {
        let Some(request) = self.harvest_touched_persist_request_for_path(path) else {
            return;
        };
        match self.background.harvest_touched_persist.enqueue(request) {
            crate::native_app::app::HarvestTouchedPersistAdmission::Saturated => {
                tracing::warn!(path = %path.display(), "harvest touched persistence queue saturated; request deferred");
            }
            crate::native_app::app::HarvestTouchedPersistAdmission::Queued
            | crate::native_app::app::HarvestTouchedPersistAdmission::Replaced => {}
        }
        self.background
            .harvest_touched_persist
            .schedule_if_idle(context);
    }

    pub(in crate::native_app) fn finish_harvest_touched_persist(
        &mut self,
        completion: ui::TaskCompletion<crate::native_app::app::HarvestTouchedPersistBatchResult>,
        context: &mut ui::UiUpdateContext<GuiMessage>,
    ) {
        let Some(result) = self.background.harvest_touched_persist.finish(completion) else {
            return;
        };
        for persisted in result.results {
            if let Some(persisted) = persisted.result {
                if let Err(error) = persisted.result {
                    tracing::warn!(
                        file_id = %persisted.file_id,
                        "failed to mark harvest file as touched in background: {error}"
                    );
                }
            }
        }
        self.background
            .harvest_touched_persist
            .schedule_if_idle(context);
    }

    pub(in crate::native_app) fn poll_harvest_touched_persist_admission(
        &mut self,
        ticket: ui::TaskTicket,
        context: &mut ui::UiUpdateContext<GuiMessage>,
    ) {
        self.background
            .harvest_touched_persist
            .poll_admission(ticket, context);
    }

    pub(in crate::native_app) fn schedule_harvest_touched_for_paths<I>(
        &mut self,
        paths: I,
        context: &mut ui::UiUpdateContext<GuiMessage>,
    ) where
        I: IntoIterator,
        I::Item: AsRef<Path>,
    {
        for path in paths {
            self.schedule_harvest_touched_for_path(path.as_ref(), context);
        }
    }

    pub(in crate::native_app) fn record_harvest_extraction_with_source_duration(
        &self,
        source_path: &Path,
        selection: SelectionRange,
        child_path: &Path,
        source_duration_seconds: f64,
    ) {
        self.record_harvest_selection_derivation_with_source_duration(
            source_path,
            selection,
            child_path,
            source_duration_seconds,
            HarvestDerivationOperation::Extract,
        );
    }

    pub(in crate::native_app) fn record_harvest_selection_derivation_with_source_duration(
        &self,
        source_path: &Path,
        selection: SelectionRange,
        child_path: &Path,
        source_duration_seconds: f64,
        operation: HarvestDerivationOperation,
    ) {
        let Some(parent) = self.harvest_identity_for_path(source_path) else {
            return;
        };
        let Some(child) = self.harvest_identity_for_path(child_path) else {
            return;
        };
        let duration = source_duration_seconds.max(0.0);
        let source_range = HarvestSourceRange {
            start_seconds: selection.start() as f64 * duration,
            end_seconds: selection.end() as f64 * duration,
        };
        let edge = NewHarvestDerivation {
            parent,
            child,
            operation,
            source_range: Some(source_range),
            output_duration_seconds: Some(
                (source_range.end_seconds - source_range.start_seconds).max(0.0),
            ),
            destination_folder: child_path.parent().map(Path::to_path_buf),
            inherited_metadata: self.harvest_metadata_snapshot_for_path(source_path),
            tool_version: format!("wavecrate-{}", env!("CARGO_PKG_VERSION")),
        };
        if let Err(error) = wavecrate::sample_sources::library::record_harvest_derivation(&edge) {
            tracing::warn!(
                source = %source_path.display(),
                child = %child_path.display(),
                "failed to record harvest derivation: {error}"
            );
        }
    }

    pub(in crate::native_app) fn record_harvest_whole_file_derivation(
        &self,
        source_path: &Path,
        child_path: &Path,
        operation: HarvestDerivationOperation,
    ) {
        let Some(parent) = self.harvest_identity_for_path(source_path) else {
            return;
        };
        let Some(child) = self.harvest_identity_for_path(child_path) else {
            return;
        };
        let edge = NewHarvestDerivation {
            parent,
            child,
            operation,
            source_range: None,
            output_duration_seconds: None,
            destination_folder: child_path.parent().map(Path::to_path_buf),
            inherited_metadata: self.harvest_metadata_snapshot_for_path(source_path),
            tool_version: format!("wavecrate-{}", env!("CARGO_PKG_VERSION")),
        };
        if let Err(error) = wavecrate::sample_sources::library::record_harvest_derivation(&edge) {
            tracing::warn!(
                source = %source_path.display(),
                child = %child_path.display(),
                "failed to record harvest whole-file derivation: {error}"
            );
        }
    }

    pub(in crate::native_app) fn reconcile_harvest_graph_after_folder_move(
        &self,
        request: &FolderMoveRequest,
        moved_paths: &[(PathBuf, PathBuf)],
    ) {
        match request {
            FolderMoveRequest::Folder {
                source_root, moves, ..
            } => {
                for (old_prefix, new_prefix) in moves {
                    self.remap_harvest_file_prefix_for_folder_move(
                        source_root,
                        old_prefix,
                        new_prefix,
                    );
                }
            }
            FolderMoveRequest::Files { .. } => {
                for (old_path, new_path) in moved_paths {
                    self.remap_harvest_file_key_for_move(old_path, new_path);
                }
            }
            FolderMoveRequest::SourcedFiles { file_moves, .. } => {
                for (old_path, new_path) in moved_paths {
                    let Some(file_move) = file_moves
                        .iter()
                        .find(|file_move| Path::new(&file_move.file_id) == old_path.as_path())
                    else {
                        continue;
                    };
                    if file_move.copy_only {
                        self.record_harvest_copy_derivation(old_path, new_path);
                    } else {
                        self.remap_harvest_file_key_for_move(old_path, new_path);
                    }
                }
            }
            FolderMoveRequest::ExtractedFile { .. } => {}
        }
    }

    fn record_harvest_copy_derivation(&self, source_path: &Path, child_path: &Path) {
        let Some((source, _)) = self
            .library
            .folder_browser
            .sample_source_for_file_path(source_path)
        else {
            return;
        };
        let Some((child_source, _)) = self
            .library
            .folder_browser
            .sample_source_for_file_path(child_path)
        else {
            return;
        };
        if !source.is_protected() {
            return;
        }
        let Some(parent) = self.harvest_identity_for_path(source_path) else {
            return;
        };
        let Some(child) = self.harvest_identity_for_path(child_path) else {
            return;
        };
        let operation = if child_source.is_primary() {
            HarvestDerivationOperation::CopyToPrimary
        } else {
            HarvestDerivationOperation::Copy
        };
        let edge = NewHarvestDerivation {
            parent,
            child,
            operation,
            source_range: None,
            output_duration_seconds: None,
            destination_folder: child_path.parent().map(Path::to_path_buf),
            inherited_metadata: self.harvest_metadata_snapshot_for_path(source_path),
            tool_version: format!("wavecrate-{}", env!("CARGO_PKG_VERSION")),
        };
        if let Err(error) = wavecrate::sample_sources::library::record_harvest_derivation(&edge) {
            tracing::warn!(
                source = %source_path.display(),
                child = %child_path.display(),
                "failed to record harvest copy derivation: {error}"
            );
        }
    }

    fn remap_harvest_file_key_for_move(&self, old_path: &Path, new_path: &Path) {
        let Some(old_key) = self.harvest_key_for_path(old_path) else {
            return;
        };
        let Some(new_key) = self.harvest_key_for_path(new_path) else {
            return;
        };
        if let Err(error) =
            wavecrate::sample_sources::library::remap_harvest_file_key(&old_key, &new_key)
        {
            tracing::warn!(
                old_path = %old_path.display(),
                new_path = %new_path.display(),
                "failed to remap harvest file identity after move: {error}"
            );
        }
    }

    fn remap_harvest_file_prefix_for_folder_move(
        &self,
        source_root: &Path,
        old_prefix: &Path,
        new_prefix: &Path,
    ) {
        let old_path = source_root.join(old_prefix);
        let new_path = source_root.join(new_prefix);
        let Some(old_key) = self.harvest_key_for_path(&old_path) else {
            return;
        };
        let Some(new_key) = self.harvest_key_for_path(&new_path) else {
            return;
        };
        if old_key.source_id != new_key.source_id {
            tracing::warn!(
                old_path = %old_path.display(),
                new_path = %new_path.display(),
                "skipped harvest folder remap across source boundary"
            );
            return;
        }
        if let Err(error) = wavecrate::sample_sources::library::remap_harvest_file_prefix(
            &old_key.source_id,
            &old_key.relative_path,
            &new_key.relative_path,
        ) {
            tracing::warn!(
                old_path = %old_path.display(),
                new_path = %new_path.display(),
                "failed to remap harvest folder identity after move: {error}"
            );
        }
    }
}
