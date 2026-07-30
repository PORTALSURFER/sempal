use std::{
    fs,
    path::{Path, PathBuf},
    time::Instant,
};

use radiant::prelude as ui;
use radiant::runtime::{NativeFileDrop, NativeFileDropPhase};
use wavecrate::sample_sources::{capture_source_file_evidence, SourceFileEvidence};

use crate::native_app::app::{GuiMessage, NativeAppState, NativeFileDropHover, emit_gui_action};
use crate::native_app::sample_library::committed_file_mutations::{
    FileMutationChange, FileMutationOperation, FileMutationProjection,
};
use crate::native_app::sample_library::exclusive_file_transfer::copy_file_to_unique_destination_with;

/// Immutable worker-owned evidence for one externally imported file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(in crate::native_app) struct PreparedFileMutationChange {
    pub(in crate::native_app) path: PathBuf,
    pub(in crate::native_app) evidence: SourceFileEvidence,
}

impl NativeAppState {
    pub(in crate::native_app) fn apply_native_file_drop(
        &mut self,
        drop: NativeFileDrop,
        context: &mut ui::UiUpdateContext<GuiMessage>,
    ) {
        if self.library.folder_browser.drag_active() {
            self.apply_native_file_drop_during_browser_drag(drop, context);
            return;
        }
        if self.cancel_pending_internal_file_drag_drop(&drop, context) {
            return;
        }
        match drop.phase {
            NativeFileDropPhase::Hover => self.track_native_file_hover(drop.path),
            NativeFileDropPhase::Cancel => {
                self.ui.browser_interaction.native_file_drop_hover = None;
            }
            NativeFileDropPhase::Drop => {
                self.ui.browser_interaction.native_file_drop_hover = None;
                let Some(path) = drop.path else {
                    return;
                };
                self.drop_external_file_on_waveform(path, context);
            }
        }
    }

    fn apply_native_file_drop_during_browser_drag(
        &mut self,
        drop: NativeFileDrop,
        context: &mut ui::UiUpdateContext<GuiMessage>,
    ) {
        self.ui.browser_interaction.native_file_drop_hover = None;
        match drop.phase {
            NativeFileDropPhase::Hover => {}
            NativeFileDropPhase::Cancel | NativeFileDropPhase::Drop => {
                self.library.folder_browser.clear_drag();
                self.clear_pending_internal_file_drag_paths();
                context.end_drag_session();
                self.ui.status.sample = String::from("Drag cancelled");
            }
        }
    }

    fn cancel_pending_internal_file_drag_drop(
        &mut self,
        drop: &NativeFileDrop,
        context: &mut ui::UiUpdateContext<GuiMessage>,
    ) -> bool {
        let should_cancel = match drop.phase {
            NativeFileDropPhase::Hover => drop
                .path
                .as_deref()
                .is_some_and(|path| self.is_pending_internal_file_drag_path(path)),
            NativeFileDropPhase::Cancel => !self
                .ui
                .browser_interaction
                .pending_internal_file_drag_paths
                .is_empty(),
            NativeFileDropPhase::Drop => drop
                .path
                .as_deref()
                .is_some_and(|path| self.is_pending_internal_file_drag_path(path)),
        };
        if !should_cancel {
            return false;
        }
        self.ui.browser_interaction.native_file_drop_hover = None;
        if !matches!(drop.phase, NativeFileDropPhase::Hover) {
            self.clear_pending_internal_file_drag_paths();
        }
        context.end_drag_session();
        self.ui.status.sample = String::from("Drag cancelled");
        true
    }

    fn track_native_file_hover(&mut self, path: Option<PathBuf>) {
        let Some(path) = path else {
            self.ui.browser_interaction.native_file_drop_hover = None;
            return;
        };
        self.ui.browser_interaction.native_file_drop_hover = Some(NativeFileDropHover {
            supported: supported_waveform_drop_file(&path),
            path,
        });
    }

    fn drop_external_file_on_waveform(
        &mut self,
        path: PathBuf,
        context: &mut ui::UiUpdateContext<GuiMessage>,
    ) {
        let started_at = Instant::now();
        if !supported_waveform_drop_file(&path) {
            self.ui.status.sample =
                format!("Unsupported waveform drop: {}", file_name_or_path(&path));
            emit_gui_action(
                "waveform.external_file_drop",
                Some("waveform"),
                None,
                "unsupported",
                started_at,
                Some("unsupported file type"),
            );
            return;
        }
        let Some(target_folder) = self.library.folder_browser.selected_folder_path() else {
            self.ui.status.sample = String::from("External drop failed: no selected folder");
            emit_gui_action(
                "waveform.external_file_drop",
                Some("waveform"),
                None,
                "error",
                started_at,
                Some("no selected folder"),
            );
            return;
        };
        if path.parent() == Some(target_folder.as_path()) {
            self.ui.status.sample = String::from("Drag cancelled");
            emit_gui_action(
                "waveform.external_file_drop",
                Some("waveform"),
                Some(file_name_or_path(&path).as_str()),
                "unchanged",
                started_at,
                None,
            );
            return;
        }
        if let Some(error) = self
            .library
            .folder_browser
            .folder_target_lock_error(&target_folder, "External drop")
        {
            self.ui.status.sample = error.clone();
            emit_gui_action(
                "waveform.external_file_drop",
                Some("waveform"),
                None,
                "blocked",
                started_at,
                Some(&error),
            );
            return;
        }
        self.ui.status.sample = format!("Copying {}", file_name_or_path(&path));
        let source = path.clone();
        context
            .business()
            .background("gui-external-waveform-drop")
            .run(
                move |_| execute_external_waveform_file_drop(&source, &target_folder),
                move |result| GuiMessage::ExternalWaveformFileDropFinished {
                    source: path,
                    started_at,
                    result,
                },
            );
    }

    pub(in crate::native_app) fn finish_external_waveform_file_drop(
        &mut self,
        source: PathBuf,
        started_at: Instant,
        result: Result<PreparedFileMutationChange, String>,
        context: &mut ui::UiUpdateContext<GuiMessage>,
    ) {
        match result {
            Ok(copied) => self.load_copied_external_file(copied, context, started_at),
            Err(error) => {
                self.record_failed_file_mutation(
                    FileMutationOperation::ImportDrop,
                    None,
                    error.clone(),
                    context,
                );
                self.ui.status.sample = format!("External drop failed: {error}");
                emit_gui_action(
                    "waveform.external_file_drop",
                    Some("waveform"),
                    Some(file_name_or_path(&source).as_str()),
                    "error",
                    started_at,
                    Some(&error),
                );
            }
        }
    }

    fn load_copied_external_file(
        &mut self,
        prepared: PreparedFileMutationChange,
        context: &mut ui::UiUpdateContext<GuiMessage>,
        started_at: Instant,
    ) {
        self.queue_prepared_committed_file_mutation(
            FileMutationOperation::ImportDrop,
            vec![
                FileMutationChange::created_prepared(prepared.path.clone(), prepared.evidence)
                    .with_projection(FileMutationProjection::SelectAndLoad {
                        path: prepared.path.clone(),
                    }),
            ],
            context,
        );
        emit_gui_action(
            "waveform.external_file_drop",
            Some("waveform"),
            None,
            "copied",
            started_at,
            None,
        );
    }
}

fn supported_waveform_drop_file(path: &Path) -> bool {
    wavecrate_library::sample_sources::is_supported_audio(path)
}

fn file_name_or_path(path: &Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().to_string())
        .unwrap_or_else(|| path.display().to_string())
}

fn execute_external_waveform_file_drop(
    source: &Path,
    target_folder: &Path,
) -> Result<PreparedFileMutationChange, String> {
    execute_external_waveform_file_drop_with(source, target_folder, |_, _| {})
}

fn execute_external_waveform_file_drop_with(
    source: &Path,
    target_folder: &Path,
    before_publish: impl FnMut(usize, &Path),
) -> Result<PreparedFileMutationChange, String> {
    if !source.is_file() {
        return Err(format!("not a file: {}", source.display()));
    }
    fs::create_dir_all(target_folder).map_err(|err| {
        format!(
            "failed to create target folder {}: {err}",
            target_folder.display()
        )
    })?;
    let file_name = source
        .file_name()
        .ok_or_else(|| String::from("dropped file has no file name"))?;
    let direct_target = target_folder.join(file_name);
    if paths_refer_to_same_file(source, &direct_target) {
        return Err(String::from("drop target unchanged"));
    }
    let committed = copy_file_to_unique_destination_with(source, &direct_target, before_publish)
        .map_err(|err| {
            format!(
                "failed to copy {} into {}: {err}",
                source.display(),
                target_folder.display()
            )
        })?;
    let path = committed.path().to_path_buf();
    let evidence = capture_source_file_evidence(&path);
    Ok(PreparedFileMutationChange { path, evidence })
}

fn paths_refer_to_same_file(left: &Path, right: &Path) -> bool {
    match (left.canonicalize(), right.canonicalize()) {
        (Ok(left), Ok(right)) => left == right,
        _ => left == right,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wavecrate::sample_sources::MAX_SOURCE_FILE_EVIDENCE_HASH_BYTES;

    #[test]
    fn waveform_drop_rejects_appledouble_sidecars() {
        assert!(supported_waveform_drop_file(Path::new("kick.wav")));
        assert!(!supported_waveform_drop_file(Path::new("._kick.wav")));
        assert!(!supported_waveform_drop_file(Path::new("drums/._kick.wav")));
    }

    #[test]
    fn external_drop_captures_small_file_hash_evidence() {
        let root = tempfile::tempdir().unwrap();
        let source_folder = root.path().join("external");
        let target_folder = root.path().join("target");
        fs::create_dir(&source_folder).unwrap();
        fs::create_dir(&target_folder).unwrap();
        let source = source_folder.join("kick.wav");
        fs::write(&source, b"small source").unwrap();

        let prepared = execute_external_waveform_file_drop(&source, &target_folder).unwrap();

        assert_eq!(prepared.path, target_folder.join("kick.wav"));
        assert_eq!(
            prepared.evidence,
            SourceFileEvidence::ContentHash(*blake3::hash(b"small source").as_bytes())
        );
    }

    #[test]
    fn external_drop_captures_large_file_metadata_evidence() {
        let root = tempfile::tempdir().unwrap();
        let source_folder = root.path().join("external");
        let target_folder = root.path().join("target");
        fs::create_dir(&source_folder).unwrap();
        fs::create_dir(&target_folder).unwrap();
        let source = source_folder.join("large.wav");
        let bytes = vec![0x5a; (MAX_SOURCE_FILE_EVIDENCE_HASH_BYTES + 1) as usize];
        fs::write(&source, &bytes).unwrap();

        let prepared = execute_external_waveform_file_drop(&source, &target_folder).unwrap();

        assert_eq!(prepared.path, target_folder.join("large.wav"));
        assert!(matches!(
            prepared.evidence,
            SourceFileEvidence::Metadata {
                len,
                is_dir: false,
                ..
            } if len == bytes.len() as u64
        ));
    }

    #[test]
    fn external_drop_retries_a_destination_created_before_commit() {
        let root = tempfile::tempdir().unwrap();
        let source_folder = root.path().join("external");
        let target_folder = root.path().join("target");
        fs::create_dir(&source_folder).unwrap();
        fs::create_dir(&target_folder).unwrap();
        let source = source_folder.join("kick.wav");
        let direct_target = target_folder.join("kick.wav");
        fs::write(&source, b"source").unwrap();

        let prepared = execute_external_waveform_file_drop_with(
            &source,
            &target_folder,
            |index, candidate| {
                if index == 0 {
                    fs::write(candidate, b"late owner").unwrap();
                }
            },
        )
        .unwrap();

        assert_eq!(prepared.path, target_folder.join("kick_copy001.wav"));
        assert_eq!(
            prepared.evidence,
            SourceFileEvidence::ContentHash(*blake3::hash(b"source").as_bytes())
        );
        assert_eq!(fs::read(direct_target).unwrap(), b"late owner");
        assert_eq!(fs::read(&prepared.path).unwrap(), b"source");
        assert_eq!(fs::read(source).unwrap(), b"source");
    }
}
