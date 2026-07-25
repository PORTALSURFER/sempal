//! Explicit transaction stages for one cross-source sample move.

use super::super::super::move_transaction::{
    PreparedStagedMove, SampleMoveMetadata, load_sample_move_metadata, prepare_staged_move,
    remove_move_journal_entry, report_staged_move_failure,
};
use crate::app::controller::jobs::{SourceMoveRequest, SourceMoveSuccess};
use crate::sample_sources::SourceDatabase;
use crate::sample_sources::db::file_ops_journal;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

mod rollback;

/// Prepared cross-source move that can advance through target-db, source-db, and finalize stages.
pub(super) struct SourceMoveTransaction<'a> {
    request: SourceMoveRequest,
    source_db: &'a mut SourceDatabase,
    target_db: &'a SourceDatabase,
    target_relative: PathBuf,
    metadata: SampleMoveMetadata,
    prepared: PreparedStagedMove,
}

/// Validate one request, stage its file, and return the prepared transaction state.
pub(super) fn prepare_source_move_transaction<'a>(
    target_root: &Path,
    target_db: &'a SourceDatabase,
    source_dbs: &'a mut HashMap<PathBuf, SourceDatabase>,
    request: SourceMoveRequest,
) -> Result<SourceMoveTransaction<'a>, String> {
    let absolute = request.source_root.join(&request.relative_path);
    if !absolute.is_file() {
        return Err(format!("File missing: {}", request.relative_path.display()));
    }
    let target_relative = unique_destination_path(target_root, &request.relative_path)?;
    ensure_target_parent(target_root, &target_relative)?;
    let source_db = source_db_for_request(source_dbs, &request.source_root)?;
    let metadata = load_sample_move_metadata(source_db, &request.relative_path)?;
    let prepared = prepare_staged_move(
        target_db,
        &request.source_root,
        &request.relative_path,
        target_root,
        &target_relative,
        metadata.clone(),
    )?;
    Ok(SourceMoveTransaction {
        request,
        source_db,
        target_db,
        target_relative,
        metadata,
        prepared,
    })
}

impl SourceMoveTransaction<'_> {
    /// Commit the destination database stage or roll back the staged file on failure.
    pub(super) fn commit_target_db_stage(
        &self,
        errors: &mut Vec<String>,
        hooks: &mut impl super::SourceMoveHooks,
    ) -> bool {
        if let Err(err) = hooks.before_target_db_stage() {
            report_staged_move_failure(errors, self.target_db, &self.prepared, err);
            return false;
        }
        let mut batch = match self.target_db.write_batch() {
            Ok(batch) => batch,
            Err(err) => {
                report_staged_move_failure(
                    errors,
                    self.target_db,
                    &self.prepared,
                    format!("Failed to open target DB batch: {err}"),
                );
                return false;
            }
        };
        if let Err(err) = batch.upsert_file(
            &self.target_relative,
            self.prepared.file_size,
            self.prepared.modified_ns,
        ) {
            report_staged_move_failure(
                errors,
                self.target_db,
                &self.prepared,
                format!("Failed to register file: {err}"),
            );
            return false;
        }
        if let Err(err) = batch.set_tag(&self.target_relative, self.metadata.tag) {
            report_staged_move_failure(
                errors,
                self.target_db,
                &self.prepared,
                format!("Failed to set tag: {err}"),
            );
            return false;
        }
        if let Err(err) = batch.set_looped(&self.target_relative, self.metadata.looped) {
            report_staged_move_failure(
                errors,
                self.target_db,
                &self.prepared,
                format!("Failed to set loop marker: {err}"),
            );
            return false;
        }
        if let Err(err) = batch.set_locked(&self.target_relative, self.metadata.locked) {
            report_staged_move_failure(
                errors,
                self.target_db,
                &self.prepared,
                format!("Failed to set keep lock: {err}"),
            );
            return false;
        }
        if let Some(last_played_at) = self.metadata.last_played_at
            && let Err(err) = batch.set_last_played_at(&self.target_relative, last_played_at)
        {
            report_staged_move_failure(
                errors,
                self.target_db,
                &self.prepared,
                format!("Failed to copy playback age: {err}"),
            );
            return false;
        }
        if let Some(last_curated_at) = self.metadata.last_curated_at
            && let Err(err) = batch.set_last_curated_at(&self.target_relative, last_curated_at)
        {
            report_staged_move_failure(
                errors,
                self.target_db,
                &self.prepared,
                format!("Failed to copy curation timestamp: {err}"),
            );
            return false;
        }
        if let Some(sound_type) = self.metadata.sound_type
            && let Err(err) = batch.set_sound_type(&self.target_relative, Some(sound_type))
        {
            report_staged_move_failure(
                errors,
                self.target_db,
                &self.prepared,
                format!("Failed to copy sound type: {err}"),
            );
            return false;
        }
        if let Some(user_tag) = self.metadata.user_tag.as_deref()
            && let Err(err) = batch.set_user_tag(&self.target_relative, Some(user_tag))
        {
            report_staged_move_failure(
                errors,
                self.target_db,
                &self.prepared,
                format!("Failed to copy custom tag: {err}"),
            );
            return false;
        }
        if let Err(err) =
            batch.replace_tags_for_path(&self.target_relative, &self.metadata.normal_tags)
        {
            report_staged_move_failure(
                errors,
                self.target_db,
                &self.prepared,
                format!("Failed to copy normal tags: {err}"),
            );
            return false;
        }
        if let Err(err) = batch.set_collection(&self.target_relative, self.metadata.collection) {
            report_staged_move_failure(
                errors,
                self.target_db,
                &self.prepared,
                format!("Failed to copy collection: {err}"),
            );
            return false;
        }
        if let Err(err) = batch.commit() {
            report_staged_move_failure(
                errors,
                self.target_db,
                &self.prepared,
                format!("Failed to commit target DB update: {err}"),
            );
            return false;
        }
        if let Err(err) = file_ops_journal::update_stage(
            self.target_db,
            &self.prepared.op_id,
            file_ops_journal::FileOpStage::TargetDb,
            None,
            None,
        ) {
            errors.push(format!("Failed to update move journal: {err}"));
        }
        true
    }

    /// Remove the original source DB row and advance the journal to `SourceDb`.
    pub(super) fn commit_source_db_stage(&mut self, errors: &mut Vec<String>) -> bool {
        if let Err(err) = self.source_db.remove_file(&self.request.relative_path) {
            errors.push(format!("Failed to drop database row: {err}"));
            return false;
        }
        if let Err(err) = file_ops_journal::update_stage(
            self.target_db,
            &self.prepared.op_id,
            file_ops_journal::FileOpStage::SourceDb,
            None,
            None,
        ) {
            errors.push(format!("Failed to update move journal: {err}"));
        }
        true
    }

    /// Rename the staged file into its final target location.
    pub(super) fn finalize_filesystem_stage(
        &self,
        errors: &mut Vec<String>,
        hooks: &mut impl super::SourceMoveHooks,
    ) -> bool {
        hooks.before_finalize();
        if let Err(err) = std::fs::rename(
            &self.prepared.staged_absolute,
            &self.prepared.target_absolute,
        ) {
            self.rollback_after_finalize_failure(errors, format!("Failed to finalize move: {err}"));
            return false;
        }
        true
    }

    /// Clear the journal entry and build the success payload for the completed move.
    pub(super) fn into_success(self, errors: &mut Vec<String>) -> SourceMoveSuccess {
        remove_move_journal_entry(errors, self.target_db, &self.prepared.op_id);
        SourceMoveSuccess {
            source_id: self.request.source_id,
            relative_path: self.request.relative_path,
            target_relative: self.target_relative,
            file_size: self.prepared.file_size,
            modified_ns: self.prepared.modified_ns,
            tag: self.metadata.tag,
            looped: self.metadata.looped,
            locked: self.metadata.locked,
            last_played_at: self.metadata.last_played_at,
            last_curated_at: self.metadata.last_curated_at,
            sound_type: self.metadata.sound_type,
            user_tag: self.metadata.user_tag,
            normal_tags: self.metadata.normal_tags,
            collection: self.metadata.collection,
        }
    }
}

fn unique_destination_path(root: &Path, relative: &Path) -> Result<PathBuf, String> {
    if !root.join(relative).exists() {
        return Ok(relative.to_path_buf());
    }
    let parent = relative.parent().unwrap_or_else(|| Path::new(""));
    let file_name = relative
        .file_name()
        .ok_or_else(|| "Sample has no file name".to_string())?;
    let stem = Path::new(file_name)
        .file_stem()
        .map(|name| name.to_string_lossy().to_string())
        .unwrap_or_else(|| "sample".to_string());
    let extension = Path::new(file_name)
        .extension()
        .map(|ext| ext.to_string_lossy().to_string());
    for index in 1..=999 {
        let suffix = format!("{stem}_move{index:03}");
        let file_name = if let Some(ext) = &extension {
            format!("{suffix}.{ext}")
        } else {
            suffix
        };
        let candidate = parent.join(file_name);
        if !root.join(&candidate).exists() {
            return Ok(candidate);
        }
    }
    Err("Failed to find destination file name".into())
}

fn ensure_target_parent(target_root: &Path, target_relative: &Path) -> Result<(), String> {
    if let Some(parent) = target_relative.parent() {
        let target_dir = target_root.join(parent);
        std::fs::create_dir_all(&target_dir).map_err(|err| {
            format!(
                "Failed to create target folder {}: {err}",
                target_dir.display()
            )
        })?;
    }
    Ok(())
}

fn source_db_for_request<'a>(
    source_dbs: &'a mut HashMap<PathBuf, SourceDatabase>,
    source_root: &Path,
) -> Result<&'a mut SourceDatabase, String> {
    match source_dbs.entry(source_root.to_path_buf()) {
        std::collections::hash_map::Entry::Occupied(entry) => Ok(entry.into_mut()),
        std::collections::hash_map::Entry::Vacant(entry) => {
            match SourceDatabase::open_for_source_write(source_root) {
                Ok(db) => Ok(entry.insert(db)),
                Err(err) => Err(format!("Failed to open source DB: {err}")),
            }
        }
    }
}
