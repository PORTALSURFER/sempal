//! UI-thread completion handlers for finished selection exports.

use super::*;
use crate::sample_sources::{
    HarvestDerivationOperation, HarvestFileIdentity, HarvestFileKey, HarvestMetadataSnapshot,
    HarvestSourceRange, NewHarvestDerivation, SourceDatabase, SourceId, WavEntry,
};
use crate::selection::SelectionRange;
use std::path::{Path, PathBuf};
#[cfg(any(target_os = "windows", target_os = "macos"))]
use tracing::{info, warn};
use wavecrate_library::timestamps::system_time_to_unix_nanos;

impl AppController {
    /// Apply one completed selection clip export on the UI thread.
    pub(crate) fn apply_selection_clip_export_success(
        &mut self,
        success: SelectionClipExportSuccess,
    ) {
        let history_key =
            crate::app::controller::history::PendingHistoryTransactionKey::SelectionExport {
                request_id: success.request_id,
            };
        let history_source_id = success.source_id.clone();
        let history_relative_path = success.entry.relative_path.clone();
        let history_absolute_path = success.absolute_path.clone();
        let history_tag = success.entry.tag;
        let history_looped = success.entry.looped;
        let history_last_played_at = success.entry.last_played_at;
        let history_backup = success.backup.clone();
        self.record_selection_export_timings("clip", &success.entry.relative_path, success.timings);
        let source =
            SampleSource::new_with_id(success.source_id.clone(), success.source_root.clone());
        self.insert_cached_entry(&source, success.entry.clone());
        self.trigger_analysis_for_added_sample(
            &source,
            &success.entry.relative_path,
            success.entry.file_size,
            success.entry.modified_ns,
        );
        self.record_selection_clip_export_harvest_derivation(&success);
        match success.destination {
            SelectionClipDestination::Browser {
                keep_source_focused,
                ..
            }
            | SelectionClipDestination::Folder {
                keep_source_focused,
                ..
            } => {
                if !keep_source_focused {
                    self.ui.browser.selection.autoscroll = true;
                    self.selection_state.suppress_autoplay_once = true;
                    self.select_from_browser(&success.entry.relative_path);
                }
                self.set_status(
                    format!("Saved clip {}", success.entry.relative_path.display()),
                    StatusTone::Info,
                );
            }
            SelectionClipDestination::ExternalDrag => {
                self.finish_external_selection_drag_export(success);
            }
        }
        if let Err(err) = self.finish_pending_sample_creation_transaction(
            &history_key,
            history_source_id,
            history_relative_path,
            history_absolute_path,
            history_tag,
            history_looped,
            history_last_played_at,
            history_backup,
            None,
        ) {
            self.set_status(
                format!("Selection export undo failed: {err}"),
                StatusTone::Error,
            );
        }
    }

    /// Apply one completed crop-to-new-sample export on the UI thread.
    pub(crate) fn apply_selection_crop_export_success(
        &mut self,
        success: SelectionCropExportSuccess,
    ) {
        let history_key =
            crate::app::controller::history::PendingHistoryTransactionKey::SelectionExport {
                request_id: success.request_id,
            };
        let history_source_id = success.source_id.clone();
        let history_relative_path = success.entry.relative_path.clone();
        let history_absolute_path = success.absolute_path.clone();
        let history_tag = success.tag;
        let history_looped = success.entry.looped;
        let history_last_played_at = success.entry.last_played_at;
        let history_backup = success.backup.clone();
        self.record_selection_export_timings(
            "crop_new_sample",
            &success.entry.relative_path,
            success.timings,
        );
        let source =
            SampleSource::new_with_id(success.source_id.clone(), success.source_root.clone());
        self.insert_cached_entry(&source, success.entry.clone());
        self.trigger_analysis_for_added_sample(
            &source,
            &success.entry.relative_path,
            success.entry.file_size,
            success.entry.modified_ns,
        );
        self.record_selection_crop_export_harvest_derivation(&success);
        self.ui.browser.selection.autoscroll = true;
        self.selection_state.suppress_autoplay_once = true;
        self.select_wav_by_path(&success.entry.relative_path);
        if success.playback.was_playing {
            self.runtime
                .jobs
                .set_pending_playback(Some(PendingPlayback {
                    source_id: source.id.clone(),
                    relative_path: success.entry.relative_path.clone(),
                    looped: success.playback.was_looping,
                    start_override: success.playback.start_override,
                    force_loaded_audio: false,
                }));
        }
        self.focus_waveform();
        self.set_status(
            format!(
                "Cropped to new sample {}",
                success.entry.relative_path.display()
            ),
            StatusTone::Info,
        );
        if let Err(err) = self.finish_pending_sample_creation_transaction(
            &history_key,
            history_source_id,
            history_relative_path.clone(),
            history_absolute_path,
            history_tag,
            history_looped,
            history_last_played_at,
            history_backup,
            Some(format!(
                "Cropped to new sample {}",
                history_relative_path.display()
            )),
        ) {
            self.set_status(format!("Crop export undo failed: {err}"), StatusTone::Error);
        }
    }

    #[cfg(any(target_os = "windows", target_os = "macos"))]
    fn finish_external_selection_drag_export(&mut self, success: SelectionClipExportSuccess) {
        let Some(request_id) = self.ui.drag.pending_external_selection_request_id else {
            warn!(
                finished_request_id = success.request_id,
                "selection export: missing pending external drag request id at completion"
            );
            return;
        };
        if request_id != success.request_id {
            warn!(
                pending_request_id = request_id,
                finished_request_id = success.request_id,
                "selection export: ignoring stale external drag completion"
            );
            return;
        }
        self.ui.drag.pending_external_selection_request_id = None;
        info!(
            request_id,
            path = %success.absolute_path.display(),
            "selection export: launching external drag for exported clip"
        );
        match self
            .drag_drop()
            .start_external_drag(std::slice::from_ref(&success.absolute_path))
        {
            Ok(()) => {
                let label = format!(
                    "Drag {} to an external target",
                    success.entry.relative_path.display()
                );
                info!(
                    request_id,
                    "selection export: external drag launch succeeded"
                );
                self.drag_drop().reset_drag();
                self.set_status(label, StatusTone::Info);
            }
            Err(err) => {
                warn!(request_id, error = %err, "selection export: external drag launch failed");
                self.drag_drop().reset_drag();
                self.set_status(err, StatusTone::Error);
            }
        }
    }

    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    fn finish_external_selection_drag_export(&mut self, success: SelectionClipExportSuccess) {
        self.ui.drag.pending_external_selection_request_id = None;
        self.drag_drop().reset_drag();
        self.set_status(
            format!(
                "External drag-out is not supported on this platform for {}",
                success.entry.relative_path.display()
            ),
            StatusTone::Error,
        );
    }

    fn record_selection_clip_export_harvest_derivation(
        &self,
        success: &SelectionClipExportSuccess,
    ) {
        record_selection_export_harvest_derivation(SelectionExportHarvestDerivationInput {
            origin_source_id: success.origin_source_id.clone(),
            origin_source_root: success.origin_source_root.clone(),
            origin_relative_path: success.origin_relative_path.clone(),
            origin_bounds: success.origin_bounds,
            origin_duration_seconds: success.origin_duration_seconds,
            child_source_id: success.source_id.clone(),
            child_entry: success.entry.clone(),
            child_absolute_path: success.absolute_path.clone(),
        });
    }

    fn record_selection_crop_export_harvest_derivation(
        &self,
        success: &SelectionCropExportSuccess,
    ) {
        record_selection_export_harvest_derivation(SelectionExportHarvestDerivationInput {
            origin_source_id: success.source_id.clone(),
            origin_source_root: success.source_root.clone(),
            origin_relative_path: success.source_relative_path.clone(),
            origin_bounds: success.source_bounds,
            origin_duration_seconds: success.source_duration_seconds,
            child_source_id: success.source_id.clone(),
            child_entry: success.entry.clone(),
            child_absolute_path: success.absolute_path.clone(),
        });
    }
}

pub(in crate::app::controller::library::selection_export) struct SelectionExportHarvestDerivationInput
{
    pub(in crate::app::controller::library::selection_export) origin_source_id: SourceId,
    pub(in crate::app::controller::library::selection_export) origin_source_root: PathBuf,
    pub(in crate::app::controller::library::selection_export) origin_relative_path: PathBuf,
    pub(in crate::app::controller::library::selection_export) origin_bounds: SelectionRange,
    pub(in crate::app::controller::library::selection_export) origin_duration_seconds: f32,
    pub(in crate::app::controller::library::selection_export) child_source_id: SourceId,
    pub(in crate::app::controller::library::selection_export) child_entry: WavEntry,
    pub(in crate::app::controller::library::selection_export) child_absolute_path: PathBuf,
}

pub(in crate::app::controller::library::selection_export) fn record_selection_export_harvest_derivation(
    input: SelectionExportHarvestDerivationInput,
) {
    let parent_entry = source_db_entry(&input.origin_source_root, &input.origin_relative_path);
    let parent_identity = harvest_identity_for_export_origin(
        input.origin_source_id,
        &input.origin_source_root,
        input.origin_relative_path.clone(),
        parent_entry.as_ref(),
    );
    let child_identity =
        harvest_identity_for_export_child(input.child_source_id, input.child_entry.clone());
    let source_range = export_source_range(input.origin_bounds, input.origin_duration_seconds);
    let edge = NewHarvestDerivation {
        parent: parent_identity,
        child: child_identity,
        operation: HarvestDerivationOperation::Export,
        source_range: Some(source_range),
        output_duration_seconds: Some(
            (source_range.end_seconds - source_range.start_seconds).max(0.0),
        ),
        destination_folder: input.child_absolute_path.parent().map(Path::to_path_buf),
        inherited_metadata: harvest_metadata_snapshot_from_entry(parent_entry.as_ref()),
        tool_version: format!("wavecrate-{}", env!("CARGO_PKG_VERSION")),
    };
    if let Err(error) = crate::sample_sources::library::record_harvest_derivation(&edge) {
        tracing::warn!(
            origin = %input.origin_relative_path.display(),
            child = %input.child_entry.relative_path.display(),
            "failed to record selection export harvest derivation: {error}"
        );
    }
}

fn harvest_identity_for_export_origin(
    source_id: SourceId,
    source_root: &Path,
    relative_path: PathBuf,
    entry: Option<&WavEntry>,
) -> HarvestFileIdentity {
    let metadata = file_metadata_identity(&source_root.join(&relative_path));
    HarvestFileIdentity {
        key: HarvestFileKey::new(source_id, relative_path),
        file_size: entry
            .map(|entry| entry.file_size)
            .or(metadata.map(|metadata| metadata.0)),
        modified_ns: entry
            .map(|entry| entry.modified_ns)
            .or(metadata.map(|metadata| metadata.1)),
        content_hash: entry.and_then(|entry| entry.content_hash.clone()),
    }
}

fn harvest_identity_for_export_child(source_id: SourceId, entry: WavEntry) -> HarvestFileIdentity {
    HarvestFileIdentity {
        key: HarvestFileKey::new(source_id, entry.relative_path),
        file_size: Some(entry.file_size),
        modified_ns: Some(entry.modified_ns),
        content_hash: entry.content_hash,
    }
}

fn export_source_range(bounds: SelectionRange, duration_seconds: f32) -> HarvestSourceRange {
    let duration = f64::from(duration_seconds.max(0.0));
    HarvestSourceRange {
        start_seconds: f64::from(bounds.start()) * duration,
        end_seconds: f64::from(bounds.end()) * duration,
    }
}

fn harvest_metadata_snapshot_from_entry(entry: Option<&WavEntry>) -> HarvestMetadataSnapshot {
    let Some(entry) = entry else {
        return HarvestMetadataSnapshot::default();
    };
    let mut tags = entry.normal_tags.clone();
    if let Some(user_tag) = entry.user_tag.as_ref()
        && !user_tag.is_empty()
    {
        tags.push(user_tag.clone());
    }
    HarvestMetadataSnapshot {
        rating: Some(entry.tag.as_i64()),
        tags,
        playback_type: entry.looped.then(|| String::from("loop")),
    }
}

fn source_db_entry(source_root: &Path, relative_path: &Path) -> Option<WavEntry> {
    let db = SourceDatabase::open_for_ui_read(source_root.to_path_buf()).ok()?;
    db.list_files()
        .ok()?
        .into_iter()
        .find(|entry| entry.relative_path == relative_path)
}

fn file_metadata_identity(path: &Path) -> Option<(u64, i64)> {
    let metadata = std::fs::metadata(path).ok()?;
    let modified_ns = system_time_to_unix_nanos(metadata.modified().ok()?);
    Some((metadata.len(), modified_ns))
}
