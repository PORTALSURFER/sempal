use super::telemetry::record_source_lifecycle_event;
use super::validation::{nested_source_conflict_error, source_roots_match};
use super::*;
#[cfg(not(test))]
use crate::app::controller::jobs::JobMessage;
use crate::app::controller::jobs::{SourceRemapJob, SourceRemapPreparedResult};
#[cfg(not(test))]
use crate::app::controller::state::runtime::PendingSourceRemap;
use wavecrate_library::timestamps::system_time_to_unix_nanos;

struct RemappedSource {
    index: usize,
    root: PathBuf,
    previous_root: PathBuf,
    id: SourceId,
    started_at: Instant,
}

static NEXT_STAGED_DATABASE_NONCE: std::sync::atomic::AtomicU64 =
    std::sync::atomic::AtomicU64::new(1);

impl AppController {
    /// Remap a source root via folder picker.
    pub fn remap_source_via_dialog(&mut self, index: usize) {
        let Some(path) = FileDialog::new().pick_folder() else {
            return;
        };
        if let Err(error) = self.remap_source_to(index, path) {
            self.set_status(error, StatusTone::Error);
        }
    }

    /// Remap a source to a new root path, preserving the source id and tags.
    pub fn remap_source_to(&mut self, index: usize, new_root: PathBuf) -> Result<(), String> {
        let started_at = Instant::now();
        let Some(existing_source) = self.library.sources.get(index) else {
            let error = String::from("Source not found");
            record_source_lifecycle_event("sources.remap", None, "error", started_at, Some(&error));
            return Err(error);
        };
        let source_id = existing_source.id.clone();
        let normalized = crate::sample_sources::config::normalize_path(new_root.as_path());
        if let Err(error) = validate_remap_source_root(&self.library.sources, index, &normalized) {
            record_source_lifecycle_event(
                "sources.remap",
                Some(source_id.as_str()),
                "error",
                started_at,
                Some(&error),
            );
            return Err(error);
        }
        if self.runtime.source_lane.pending_remap.is_some() {
            return Err(String::from("Source remap already in progress"));
        }
        if self
            .runtime
            .source_lane
            .mutations
            .source_has_pending_metadata(&source_id)
            || self.source_has_pending_file_mutations(&source_id)
        {
            return Err(String::from(
                "Cannot remap a source while file or metadata changes are pending",
            ));
        }
        if self.runtime.jobs.source_scan_in_progress(&source_id) {
            return Err(String::from(
                "Cannot remap a source while it is being scanned",
            ));
        }
        if self.runtime.jobs.trash_move_in_progress() {
            return Err(String::from(
                "Cannot remap a source while samples are being moved to trash",
            ));
        }
        if self.runtime.jobs.file_ops_in_progress() {
            return Err(String::from(
                "Cannot remap a source while file operations are running",
            ));
        }
        if self
            .runtime
            .jobs
            .selection_export_in_progress_for(&source_id)
        {
            return Err(String::from(
                "Cannot remap a source while selection exports are running",
            ));
        }
        if self
            .runtime
            .jobs
            .source_db_maintenance_in_progress_for(&source_id)
        {
            return Err(String::from(
                "Cannot remap a source while database maintenance is running",
            ));
        }
        if crate::app::controller::library::analysis_jobs::source_has_pending_or_running_jobs(
            existing_source,
        )
        .map_err(|error| format!("Failed to inspect analysis jobs before remapping: {error}"))?
        {
            return Err(String::from(
                "Cannot remap a source while analysis jobs are pending or running",
            ));
        }
        if self
            .runtime
            .source_lane
            .pending_adds
            .values()
            .any(|pending| roots_overlap(&pending.source.root, &normalized))
        {
            return Err(String::from(
                "Cannot remap to a source folder while it is being added",
            ));
        }
        let write_fence =
            std::sync::Arc::new(crate::app::controller::jobs::SourceRemapWriteFence::default());
        let job = SourceRemapJob {
            request_id: self.runtime.jobs.next_source_remap_request_id(),
            source: existing_source.clone(),
            new_root: normalized,
            write_fence,
        };
        #[cfg(test)]
        {
            let prepared = run_source_remap_prepare(job);
            prepared.result?;
            let publication = publish_prepared_database(
                &prepared.new_root,
                prepared.staged_database.as_deref(),
                &prepared.destination_current_database_identity,
                &prepared.destination_legacy_database_identity,
            )?;
            let result = self.commit_remapped_source(RemappedSource {
                index,
                root: prepared.new_root.clone(),
                previous_root: existing_source.root.clone(),
                id: source_id,
                started_at,
            });
            if result.is_err() {
                publication.rollback();
            }
            result
        }
        #[cfg(not(test))]
        {
            self.queue_source_remap_prepare(job, started_at);
            Ok(())
        }
    }

    #[cfg(not(test))]
    fn queue_source_remap_prepare(&mut self, job: SourceRemapJob, started_at: Instant) {
        self.runtime.source_lane.pending_remap = Some(PendingSourceRemap {
            request_id: job.request_id,
            source: job.source.clone(),
            new_root: job.new_root.clone(),
            queued_at: started_at,
            canceled: false,
            write_fence: std::sync::Arc::clone(&job.write_fence),
        });
        self.set_status(
            format!("Remapping source to {}", job.new_root.display()),
            StatusTone::Info,
        );
        self.runtime.jobs.spawn_one_shot_job(
            true,
            move || run_source_remap_prepare(job),
            JobMessage::SourceRemapPrepared,
        );
    }

    pub(crate) fn handle_source_remap_prepared_message(
        &mut self,
        message: SourceRemapPreparedResult,
    ) {
        let is_current = self
            .runtime
            .source_lane
            .pending_remap
            .as_ref()
            .is_some_and(|pending| {
                pending.request_id == message.request_id
                    && pending.source.id == message.source.id
                    && pending.new_root == message.new_root
            });
        if !is_current {
            remove_staged_database(message.staged_database.as_deref());
            return;
        }
        let pending = self
            .runtime
            .source_lane
            .pending_remap
            .take()
            .expect("current source remap pending entry");
        if pending.canceled {
            remove_staged_database(message.staged_database.as_deref());
            let active_status = format!("Remapping source to {}", pending.new_root.display());
            if self.ui.status.text == active_status {
                self.set_status("Source remap canceled", StatusTone::Info);
            }
            return;
        }
        let Some(index) = self.library.sources.iter().position(|source| {
            source.id == message.source.id && source.root == message.source.root
        }) else {
            remove_staged_database(message.staged_database.as_deref());
            return;
        };
        match message.result {
            Ok(()) => {
                let mut publication = None;
                let result =
                    validate_remap_source_root(&self.library.sources, index, &message.new_root)
                        .and_then(|()| {
                            publish_prepared_database(
                                &message.new_root,
                                message.staged_database.as_deref(),
                                &message.destination_current_database_identity,
                                &message.destination_legacy_database_identity,
                            )
                        })
                        .and_then(|published| {
                            publication = Some(published);
                            self.commit_remapped_source(RemappedSource {
                                index,
                                root: message.new_root.clone(),
                                previous_root: message.source.root.clone(),
                                id: message.source.id.clone(),
                                started_at: pending.queued_at,
                            })
                        });
                if let Err(error) = result {
                    remove_staged_database(message.staged_database.as_deref());
                    if let Some(publication) = publication {
                        publication.rollback();
                    }
                    self.set_status(error, StatusTone::Error);
                }
            }
            Err(error) => {
                remove_staged_database(message.staged_database.as_deref());
                self.set_status(error, StatusTone::Error);
            }
        }
    }

    fn commit_remapped_source(&mut self, remap: RemappedSource) -> Result<(), String> {
        self.library.sources[remap.index].root = remap.root;
        if let Err(err) = self.persist_config("Failed to save config after remapping source") {
            self.library.sources[remap.index].root = remap.previous_root;
            record_source_lifecycle_event(
                "sources.remap",
                Some(remap.id.as_str()),
                "error",
                remap.started_at,
                Some(&err),
            );
            return Err(err);
        }
        self.library.missing.sources.remove(&remap.id);
        let mut invalidator = source_cache_invalidator::SourceCacheInvalidator::new_from_state(
            &mut self.cache,
            &mut self.ui_cache,
            &mut self.library.missing,
        );
        invalidator.invalidate_db_cache(&remap.id);
        invalidator.invalidate_wav_related(&remap.id);
        if self.selection_state.ctx.selected_source.as_ref() == Some(&remap.id) {
            self.clear_wavs();
            self.selection_state.ctx.selected_source = Some(remap.id.clone());
        }
        self.refresh_sources_ui();
        self.queue_wav_load();
        self.set_status("Source remapped", StatusTone::Info);
        record_source_lifecycle_event(
            "sources.remap",
            Some(remap.id.as_str()),
            "success",
            remap.started_at,
            None,
        );
        Ok(())
    }
}

fn roots_overlap(first: &PathBuf, second: &PathBuf) -> bool {
    source_roots_match(first, second)
        || nested_source_conflict_error(first, second).is_some()
        || nested_source_conflict_error(second, first).is_some()
}

fn validate_remap_source_root(
    sources: &[SampleSource],
    index: usize,
    normalized: &PathBuf,
) -> Result<(), String> {
    if !normalized.is_dir() {
        return Err(String::from("Please select a directory"));
    }
    if sources
        .iter()
        .enumerate()
        .any(|(i, source)| i != index && source_roots_match(&source.root, normalized))
    {
        return Err(String::from("Source already added"));
    }
    if let Some(error) = sources
        .iter()
        .enumerate()
        .filter(|(i, _)| *i != index)
        .find_map(|(_, source)| nested_source_conflict_error(&source.root, normalized))
    {
        return Err(error);
    }
    Ok(())
}

fn run_source_remap_prepare(job: SourceRemapJob) -> SourceRemapPreparedResult {
    let destination = crate::sample_sources::database_path_for(&job.new_root);
    let legacy_destination = job
        .new_root
        .join(crate::sample_sources::db::LEGACY_DB_FILE_NAME);
    let legacy_migration = LegacyDatabaseMigrationSnapshot::new(&job.new_root);
    let initial_current_database_identity = database_identity(&destination);
    let initial_legacy_database_identity = database_identity(&legacy_destination);
    let destination_database_preexisting = initial_current_database_identity
        .as_ref()
        .is_ok_and(|identity| identity.has_artifacts())
        || initial_legacy_database_identity
            .as_ref()
            .is_ok_and(|identity| identity.has_artifacts());
    let mut staged_database = None;
    let mut destination_open_attempted = false;
    let mut result = (|| {
        let initial_current_database_identity = initial_current_database_identity
            .as_ref()
            .map_err(|error| error.clone())?;
        let initial_legacy_database_identity = initial_legacy_database_identity
            .as_ref()
            .map_err(|error| error.clone())?;
        if job.write_fence.is_canceled() {
            return Err(String::from("Source remap canceled"));
        }
        if !job.new_root.is_dir() {
            return Err(String::from("Remap destination is no longer available"));
        }
        let source_database = crate::sample_sources::database_path_for(&job.source.root);
        if !destination_database_preexisting {
            if source_database.exists() {
                let source =
                    SourceDatabase::open_for_source_write(&job.source.root).map_err(|error| {
                        format!("Failed to open source database for snapshot: {error}")
                    })?;
                let staged = staged_database_path(&destination, job.request_id);
                staged_database = Some(staged.clone());
                source
                    .snapshot_to_path_with_write_fence_install(&staged, |fence| {
                        job.write_fence.install(fence)
                    })
                    .map_err(|error| format!("Failed to snapshot source database: {error}"))?;
                if job.write_fence.is_canceled() {
                    return Err(String::from("Source remap canceled"));
                }
            }
            return Ok(());
        }
        if !database_artifact_owner_matches(
            initial_current_database_identity,
            &database_identity(&destination)?,
        ) || !database_artifact_owner_matches(
            initial_legacy_database_identity,
            &database_identity(&legacy_destination)?,
        ) {
            return Err(String::from(
                "Destination database changed while the remap was running",
            ));
        }
        if (initial_current_database_identity.has_artifacts()
            && initial_current_database_identity.database.is_none())
            || (initial_legacy_database_identity.has_artifacts()
                && initial_legacy_database_identity.database.is_none())
        {
            return Err(String::from(
                "Destination contains SQLite sidecars without their database",
            ));
        }
        // Ownership is fixed by the exact identity check above. The writable open may then
        // migrate the schema or journal mode; its resulting identities become the publish gate.
        destination_open_attempted = true;
        SourceDatabase::open_for_source_write(&job.new_root)
            .map(|_| ())
            .map_err(|error| format!("Failed to prepare database: {error}"))
    })();
    legacy_migration.restore_original_names(destination_open_attempted);
    let mut destination_current_database_identity =
        crate::app::controller::jobs::SourceRemapDatabaseIdentity::default();
    let mut destination_legacy_database_identity =
        crate::app::controller::jobs::SourceRemapDatabaseIdentity::default();
    if result.is_ok() {
        match (
            database_identity(&destination),
            database_identity(&legacy_destination),
        ) {
            (Ok(current), Ok(legacy)) => {
                destination_current_database_identity = current;
                destination_legacy_database_identity = legacy;
            }
            (Err(error), _) | (_, Err(error)) => result = Err(error),
        }
    }
    if result.is_err() {
        remove_staged_database(staged_database.as_deref());
        staged_database = None;
        job.write_fence.release();
    }
    SourceRemapPreparedResult {
        request_id: job.request_id,
        source: job.source,
        new_root: job.new_root,
        staged_database,
        destination_current_database_identity,
        destination_legacy_database_identity,
        write_fence: job.write_fence,
        result,
    }
}

#[derive(Debug)]
struct LegacyDatabaseMigrationSnapshot {
    artifacts: Vec<(PathBuf, PathBuf, bool, bool)>,
}

impl LegacyDatabaseMigrationSnapshot {
    fn new(root: &Path) -> Self {
        let current_database = crate::sample_sources::database_path_for(root);
        let legacy_database = root.join(crate::sample_sources::db::LEGACY_DB_FILE_NAME);
        let artifacts = ["", "-wal", "-shm"]
            .into_iter()
            .map(|suffix| {
                let legacy = path_with_suffix(&legacy_database, suffix);
                let current = path_with_suffix(&current_database, suffix);
                let legacy_existed = database_artifact_present(&legacy);
                let current_existed = database_artifact_present(&current);
                (legacy, current, legacy_existed, current_existed)
            })
            .collect();
        Self { artifacts }
    }

    fn restore_original_names(&self, migration_attempted: bool) {
        if !migration_attempted {
            return;
        }
        for (legacy, current, legacy_existed, current_existed) in &self.artifacts {
            if *legacy_existed
                && !*current_existed
                && !database_artifact_present(legacy)
                && database_artifact_present(current)
                && let Err(error) = fs::rename(current, legacy)
            {
                tracing::warn!(
                    from = %current.display(),
                    to = %legacy.display(),
                    error = %error,
                    "Failed to restore legacy source database artifact after remap rollback"
                );
            }
        }
    }
}

fn publish_prepared_database(
    root: &Path,
    staged_database: Option<&Path>,
    destination_current_database_identity: &crate::app::controller::jobs::SourceRemapDatabaseIdentity,
    destination_legacy_database_identity: &crate::app::controller::jobs::SourceRemapDatabaseIdentity,
) -> Result<PublishedRemapDatabase, String> {
    if !root.is_dir() {
        return Err(String::from("Remap destination is no longer available"));
    }
    let artifact_snapshot = DatabaseArtifactSnapshot::new(root);
    let destination = crate::sample_sources::database_path_for(root);
    let legacy_destination = root.join(crate::sample_sources::db::LEGACY_DB_FILE_NAME);
    if let Some(staged) = staged_database {
        if database_identity(&destination)?.has_artifacts()
            || database_identity(&legacy_destination)?.has_artifacts()
        {
            return Err(String::from(
                "Destination database changed while the remap was running",
            ));
        }
        publish_staged_database_without_replace(staged, &destination)
            .map_err(|error| format!("Failed to publish source database snapshot: {error}"))?;
        let published_identity = database_identity(&destination)?;
        if let Err(error) = SourceDatabase::open_for_source_write(root) {
            artifact_snapshot.remove_created_if_unchanged(&published_identity);
            return Err(format!("Failed to prepare database: {error}"));
        }
        let initialized_identity = database_identity(&destination)?;
        return Ok(PublishedRemapDatabase {
            artifact_snapshot,
            legacy_migration: LegacyDatabaseMigrationSnapshot::new(root),
            created_database_identity: Some(initialized_identity),
        });
    }
    if database_identity(&destination)? != *destination_current_database_identity
        || database_identity(&legacy_destination)? != *destination_legacy_database_identity
    {
        return Err(String::from(
            "Destination database changed while the remap was running",
        ));
    }
    let destination_was_empty = !destination_current_database_identity.has_artifacts()
        && !destination_legacy_database_identity.has_artifacts();
    let legacy_migration = LegacyDatabaseMigrationSnapshot::new(root);
    let empty_claim = destination_was_empty
        .then(|| reserve_empty_database_path(&destination))
        .transpose()
        .map_err(|error| format!("Failed to claim remap destination database: {error}"))?;
    if let Err(error) = SourceDatabase::open_for_source_write(root) {
        legacy_migration.restore_original_names(true);
        if let Some(claim) = &empty_claim {
            artifact_snapshot.remove_created_if_unchanged(
                &crate::app::controller::jobs::SourceRemapDatabaseIdentity {
                    database: Some(claim.clone()),
                    ..Default::default()
                },
            );
        }
        return Err(format!("Failed to prepare database: {error}"));
    }
    if let Some(claim) = &empty_claim {
        let initialized = database_artifact_identity(&destination)?
            .ok_or_else(|| String::from("Remap destination database disappeared"))?;
        if !database_artifact_is_same_file(claim, &initialized) {
            return Err(String::from(
                "Destination database changed while the remap was running",
            ));
        }
    }
    let created_database_identity = if destination_was_empty {
        Some(database_identity(&destination)?)
    } else {
        None
    };
    Ok(PublishedRemapDatabase {
        artifact_snapshot,
        legacy_migration,
        created_database_identity,
    })
}

#[derive(Debug)]
struct PublishedRemapDatabase {
    artifact_snapshot: DatabaseArtifactSnapshot,
    legacy_migration: LegacyDatabaseMigrationSnapshot,
    created_database_identity: Option<crate::app::controller::jobs::SourceRemapDatabaseIdentity>,
}

impl PublishedRemapDatabase {
    fn rollback(self) {
        self.legacy_migration.restore_original_names(true);
        if let Some(identity) = &self.created_database_identity {
            self.artifact_snapshot.remove_created_if_unchanged(identity);
        }
    }
}

#[derive(Debug)]
struct DatabaseArtifactSnapshot {
    database_preexisting: bool,
    artifacts: Vec<(PathBuf, bool)>,
}

impl DatabaseArtifactSnapshot {
    fn new(root: &Path) -> Self {
        let database = crate::sample_sources::database_path_for(root);
        let database_preexisting = database_artifact_present(&database);
        let artifacts = [
            database.clone(),
            path_with_suffix(&database, "-wal"),
            path_with_suffix(&database, "-shm"),
            path_with_suffix(&database, "-journal"),
        ]
        .into_iter()
        .map(|path| {
            let existed = database_artifact_present(&path);
            (path, existed)
        })
        .collect();
        Self {
            database_preexisting,
            artifacts,
        }
    }

    fn remove_created(&self) {
        if self.database_preexisting {
            return;
        }
        for (path, existed) in &self.artifacts {
            if *existed {
                continue;
            }
            match fs::remove_file(path) {
                Ok(()) => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => tracing::warn!(
                    path = %path.display(),
                    error = %error,
                    "Failed to remove remap-created database artifact"
                ),
            }
        }
    }

    fn remove_created_if_unchanged(
        &self,
        expected: &crate::app::controller::jobs::SourceRemapDatabaseIdentity,
    ) {
        if self.database_preexisting {
            return;
        }
        let Some((database, _)) = self.artifacts.first() else {
            return;
        };
        if database_identity(database).as_ref().ok() != Some(expected) {
            tracing::warn!(
                path = %database.display(),
                "Preserving remap database artifacts because ownership changed"
            );
            return;
        }
        self.remove_created();
    }
}

fn publish_staged_database_without_replace(
    staged: &Path,
    destination: &Path,
) -> std::io::Result<()> {
    #[cfg(target_os = "windows")]
    {
        use windows::{
            Win32::Storage::FileSystem::{MOVEFILE_WRITE_THROUGH, MoveFileExW},
            core::PCWSTR,
        };

        let staged = wide_path(staged);
        let destination = wide_path(destination);
        unsafe {
            MoveFileExW(
                PCWSTR(staged.as_ptr()),
                PCWSTR(destination.as_ptr()),
                MOVEFILE_WRITE_THROUGH,
            )
        }
        .map_err(|error| std::io::Error::other(error.to_string()))
    }
    #[cfg(target_os = "macos")]
    {
        use std::os::unix::ffi::OsStrExt;

        let staged = std::ffi::CString::new(staged.as_os_str().as_bytes())?;
        let destination = std::ffi::CString::new(destination.as_os_str().as_bytes())?;
        let result =
            unsafe { libc::renamex_np(staged.as_ptr(), destination.as_ptr(), libc::RENAME_EXCL) };
        if result == 0 {
            Ok(())
        } else {
            Err(std::io::Error::last_os_error())
        }
    }
    #[cfg(any(target_os = "linux", target_os = "android"))]
    {
        use std::os::unix::ffi::OsStrExt;

        let staged = std::ffi::CString::new(staged.as_os_str().as_bytes())?;
        let destination = std::ffi::CString::new(destination.as_os_str().as_bytes())?;
        let result = unsafe {
            libc::renameat2(
                libc::AT_FDCWD,
                staged.as_ptr(),
                libc::AT_FDCWD,
                destination.as_ptr(),
                libc::RENAME_NOREPLACE,
            )
        };
        if result == 0 {
            Ok(())
        } else {
            Err(std::io::Error::last_os_error())
        }
    }
    #[cfg(not(any(
        target_os = "windows",
        target_os = "macos",
        target_os = "linux",
        target_os = "android"
    )))]
    {
        fs::hard_link(staged, destination)?;
        fs::remove_file(staged)
    }
}

fn reserve_empty_database_path(
    destination: &Path,
) -> std::io::Result<crate::app::controller::jobs::SourceRemapArtifactIdentity> {
    let reservation = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(destination)?;
    let metadata = reservation.metadata()?;
    Ok(crate::app::controller::jobs::SourceRemapArtifactIdentity {
        stable_id: wavecrate_library::filesystem_identity::stable_filesystem_identity(
            destination,
            &metadata,
        ),
        len: metadata.len(),
        modified_ns: metadata.modified().ok().map(system_time_to_unix_nanos),
        is_symlink: false,
    })
}

#[cfg(target_os = "windows")]
fn wide_path(path: &Path) -> Vec<u16> {
    use std::os::windows::ffi::OsStrExt;

    path.as_os_str().encode_wide().chain(Some(0)).collect()
}

fn remove_staged_database(staged_database: Option<&Path>) {
    let Some(staged_database) = staged_database else {
        return;
    };
    for path in [
        staged_database.to_path_buf(),
        path_with_suffix(staged_database, "-wal"),
        path_with_suffix(staged_database, "-shm"),
        path_with_suffix(staged_database, "-journal"),
    ] {
        match fs::remove_file(path) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => tracing::warn!(
                error = %error,
                "Failed to remove staged remap snapshot artifact"
            ),
        }
    }
}

fn database_artifact_present(path: &Path) -> bool {
    match fs::symlink_metadata(path) {
        Ok(_) => true,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => false,
        Err(_) => true,
    }
}

fn database_artifact_identity(
    path: &Path,
) -> Result<Option<crate::app::controller::jobs::SourceRemapArtifactIdentity>, String> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => {
            return Err(format!(
                "Failed to inspect destination database {}: {error}",
                path.display()
            ));
        }
    };
    let modified_ns = metadata.modified().ok().map(system_time_to_unix_nanos);
    Ok(Some(
        crate::app::controller::jobs::SourceRemapArtifactIdentity {
            stable_id: wavecrate_library::filesystem_identity::stable_filesystem_identity(
                path, &metadata,
            ),
            len: metadata.len(),
            modified_ns,
            is_symlink: metadata.file_type().is_symlink(),
        },
    ))
}

fn database_identity(
    database: &Path,
) -> Result<crate::app::controller::jobs::SourceRemapDatabaseIdentity, String> {
    Ok(crate::app::controller::jobs::SourceRemapDatabaseIdentity {
        database: database_artifact_identity(database)?,
        wal: database_artifact_identity(&path_with_suffix(database, "-wal"))?,
        shm: database_artifact_identity(&path_with_suffix(database, "-shm"))?,
        journal: database_artifact_identity(&path_with_suffix(database, "-journal"))?,
    })
}

fn database_artifact_owner_matches(
    initial: &crate::app::controller::jobs::SourceRemapDatabaseIdentity,
    prepared: &crate::app::controller::jobs::SourceRemapDatabaseIdentity,
) -> bool {
    initial == prepared
}

fn database_artifact_is_same_file(
    claimed: &crate::app::controller::jobs::SourceRemapArtifactIdentity,
    current: &crate::app::controller::jobs::SourceRemapArtifactIdentity,
) -> bool {
    match (&claimed.stable_id, &current.stable_id) {
        (Some(claimed), Some(current)) => claimed == current,
        (None, None) => !claimed.is_symlink && !current.is_symlink,
        _ => false,
    }
}

fn staged_database_path(destination: &Path, request_id: u64) -> PathBuf {
    let unique = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or_default();
    let nonce = NEXT_STAGED_DATABASE_NONCE.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    path_with_suffix(
        destination,
        &format!(
            ".remap-{}-{request_id}-{unique}-{nonce}.staged",
            std::process::id()
        ),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn empty_database_identity() -> crate::app::controller::jobs::SourceRemapDatabaseIdentity {
        crate::app::controller::jobs::SourceRemapDatabaseIdentity::default()
    }

    #[test]
    fn staged_snapshot_paths_are_unique_across_requests_and_restarts() {
        let destination = PathBuf::from(".wavecrate.db");

        let first = staged_database_path(&destination, 1);
        let second = staged_database_path(&destination, 1);

        assert_ne!(first, second);
        assert!(first.to_string_lossy().contains(".remap-"));
    }

    #[test]
    fn failed_post_publish_validation_removes_new_snapshot_artifacts() {
        let root = tempfile::tempdir().expect("destination root");
        let destination = crate::sample_sources::database_path_for(root.path());
        let staged = staged_database_path(&destination, 1);
        fs::write(&staged, b"not a sqlite database").expect("invalid staged database");

        let empty = empty_database_identity();
        let error = publish_prepared_database(root.path(), Some(&staged), &empty, &empty)
            .expect_err("invalid snapshot must fail validation");

        assert!(error.contains("Failed to prepare database"));
        assert!(!destination.exists());
        assert!(!path_with_suffix(&destination, "-wal").exists());
        assert!(!path_with_suffix(&destination, "-shm").exists());
    }

    #[test]
    fn no_snapshot_publish_rejects_newly_appeared_destination() {
        let root = tempfile::tempdir().expect("destination root");
        let destination = crate::sample_sources::database_path_for(root.path());
        fs::write(&destination, b"other owner").expect("create competing database");

        let empty = empty_database_identity();
        let error = publish_prepared_database(root.path(), None, &empty, &empty)
            .expect_err("new destination must not be claimed");

        assert!(error.contains("changed while the remap was running"));
        assert_eq!(fs::read(destination).unwrap(), b"other owner");
    }

    #[test]
    fn empty_destination_claim_preserves_late_owner() {
        let root = tempfile::tempdir().expect("destination root");
        let destination = crate::sample_sources::database_path_for(root.path());
        fs::write(&destination, b"late owner").expect("create competing database");

        let error = reserve_empty_database_path(&destination)
            .expect_err("atomic claim must reject a late destination owner");

        assert_eq!(error.kind(), std::io::ErrorKind::AlreadyExists);
        assert_eq!(fs::read(destination).unwrap(), b"late owner");
    }

    #[cfg(any(unix, windows))]
    #[test]
    fn empty_destination_claim_detects_replacement_before_initialization() {
        let root = tempfile::tempdir().expect("destination root");
        let destination = crate::sample_sources::database_path_for(root.path());
        let claim = reserve_empty_database_path(&destination).expect("claim destination");
        fs::remove_file(&destination).expect("remove claimed file");
        fs::write(&destination, b"replacement owner").expect("replace destination");
        let replacement = database_artifact_identity(&destination)
            .expect("replacement identity")
            .expect("replacement artifact");

        assert!(!database_artifact_is_same_file(&claim, &replacement));
    }

    #[test]
    fn no_snapshot_publish_claims_and_initializes_empty_destination() {
        let root = tempfile::tempdir().expect("destination root");
        let destination = crate::sample_sources::database_path_for(root.path());
        let empty = empty_database_identity();

        let publication = publish_prepared_database(root.path(), None, &empty, &empty)
            .expect("empty destination should be claimed");

        assert!(destination.exists());
        publication.rollback();
        assert!(!destination.exists());
    }

    #[test]
    fn empty_destination_rollback_preserves_post_initialization_writes() {
        let root = tempfile::tempdir().expect("destination root");
        let destination = crate::sample_sources::database_path_for(root.path());
        let empty = empty_database_identity();
        let publication = publish_prepared_database(root.path(), None, &empty, &empty)
            .expect("empty destination should be claimed");
        fs::write(&destination, b"other process state").expect("change claimed database");

        publication.rollback();

        assert_eq!(fs::read(destination).unwrap(), b"other process state");
    }

    #[test]
    fn staged_publish_rejects_newly_appeared_legacy_destination() {
        let root = tempfile::tempdir().expect("destination root");
        let destination = crate::sample_sources::database_path_for(root.path());
        let staged = staged_database_path(&destination, 1);
        fs::write(&staged, b"snapshot").expect("stage snapshot");
        let legacy = root
            .path()
            .join(crate::sample_sources::db::LEGACY_DB_FILE_NAME);
        fs::write(&legacy, b"legacy owner").expect("create competing legacy database");

        let empty = empty_database_identity();
        let error = publish_prepared_database(root.path(), Some(&staged), &empty, &empty)
            .expect_err("new legacy destination must not be claimed");

        assert!(error.contains("changed while the remap was running"));
        assert_eq!(fs::read(legacy).unwrap(), b"legacy owner");
        assert!(!destination.exists());
    }

    #[test]
    fn no_snapshot_publish_rejects_removed_preexisting_destination() {
        let root = tempfile::tempdir().expect("destination root");
        let destination = crate::sample_sources::database_path_for(root.path());
        fs::write(&destination, b"original database").expect("original database");
        let expected = database_identity(&destination).expect("database identity");
        fs::remove_file(&destination).expect("remove original database");

        let empty = empty_database_identity();
        let error = publish_prepared_database(root.path(), None, &expected, &empty)
            .expect_err("removed destination must not be recreated");

        assert!(error.contains("changed while the remap was running"));
        assert!(!destination.exists());
    }

    #[test]
    fn no_snapshot_publish_rejects_removed_preexisting_legacy_destination() {
        let root = tempfile::tempdir().expect("destination root");
        let legacy = root
            .path()
            .join(crate::sample_sources::db::LEGACY_DB_FILE_NAME);
        fs::write(&legacy, b"original legacy database").expect("original legacy database");
        let expected = database_identity(&legacy).expect("legacy database identity");
        fs::remove_file(&legacy).expect("remove original legacy database");

        let empty = empty_database_identity();
        let error = publish_prepared_database(root.path(), None, &empty, &expected)
            .expect_err("removed legacy destination must not be replaced");

        assert!(error.contains("changed while the remap was running"));
        assert!(!legacy.exists());
        assert!(!crate::sample_sources::database_path_for(root.path()).exists());
    }

    #[test]
    fn no_snapshot_publish_rejects_replaced_preexisting_destination() {
        let root = tempfile::tempdir().expect("destination root");
        let destination = crate::sample_sources::database_path_for(root.path());
        fs::write(&destination, b"original owner").expect("original database");
        let expected = database_identity(&destination).expect("database identity");
        let replacement = root.path().join("replacement.db");
        fs::write(&replacement, b"replacement!!!").expect("replacement database");
        fs::remove_file(&destination).expect("remove original database");
        fs::rename(&replacement, &destination).expect("replace destination database");

        let empty = empty_database_identity();
        let error = publish_prepared_database(root.path(), None, &expected, &empty)
            .expect_err("replacement destination must not be claimed");

        assert!(error.contains("changed while the remap was running"));
        assert_eq!(fs::read(destination).unwrap(), b"replacement!!!");
    }

    #[test]
    fn no_snapshot_publish_rejects_changed_preexisting_wal() {
        let root = tempfile::tempdir().expect("destination root");
        let destination = crate::sample_sources::database_path_for(root.path());
        let wal = path_with_suffix(&destination, "-wal");
        fs::write(&destination, b"database owner").expect("database");
        fs::write(&wal, b"old wal rows").expect("wal");
        let expected = database_identity(&destination).expect("database identity");
        fs::write(&wal, b"new committed wal rows").expect("change wal");

        let empty = empty_database_identity();
        let error = publish_prepared_database(root.path(), None, &expected, &empty)
            .expect_err("changed WAL must invalidate destination ownership");

        assert!(error.contains("changed while the remap was running"));
        assert_eq!(fs::read(wal).unwrap(), b"new committed wal rows");
    }

    #[test]
    fn prepared_owner_check_rejects_changed_preexisting_wal() {
        let root = tempfile::tempdir().expect("destination root");
        let destination = crate::sample_sources::database_path_for(root.path());
        let wal = path_with_suffix(&destination, "-wal");
        fs::write(&destination, b"database owner").expect("database");
        fs::write(&wal, b"old wal rows").expect("wal");
        let initial = database_identity(&destination).expect("initial identity");
        fs::write(&wal, b"new committed wal rows").expect("change wal");
        let prepared = database_identity(&destination).expect("prepared identity");

        assert_eq!(
            initial.database.as_ref().unwrap().stable_id,
            prepared.database.as_ref().unwrap().stable_id
        );
        assert!(!database_artifact_owner_matches(&initial, &prepared));
    }

    #[test]
    fn prepared_owner_check_rejects_changed_preexisting_database() {
        let root = tempfile::tempdir().expect("destination root");
        let destination = crate::sample_sources::database_path_for(root.path());
        fs::write(&destination, b"database owner").expect("database");
        let initial = database_identity(&destination).expect("initial identity");
        fs::write(&destination, b"changed database owner").expect("change database");
        let prepared = database_identity(&destination).expect("prepared identity");

        assert_eq!(
            initial.database.as_ref().unwrap().stable_id,
            prepared.database.as_ref().unwrap().stable_id
        );
        assert!(!database_artifact_owner_matches(&initial, &prepared));
    }

    #[test]
    fn prepare_refreshes_identity_after_controlled_destination_migration() {
        let source_root = tempfile::tempdir().expect("source root");
        let destination_root = tempfile::tempdir().expect("destination root");
        let destination = crate::sample_sources::database_path_for(destination_root.path());
        let connection = rusqlite::Connection::open(&destination).expect("legacy database");
        connection
            .execute_batch(
                "CREATE TABLE legacy_marker (id INTEGER PRIMARY KEY); PRAGMA user_version = 0;",
            )
            .expect("stale schema");
        connection.close().expect("close stale database");
        let initial = database_identity(&destination).expect("initial identity");
        let job = SourceRemapJob {
            request_id: 1,
            source: SampleSource::new(source_root.path().to_path_buf()),
            new_root: destination_root.path().to_path_buf(),
            write_fence: std::sync::Arc::new(
                crate::app::controller::jobs::SourceRemapWriteFence::default(),
            ),
        };

        let prepared = run_source_remap_prepare(job);

        assert!(prepared.result.is_ok(), "{:?}", prepared.result);
        assert_ne!(initial, prepared.destination_current_database_identity);
        assert_eq!(
            database_identity(&destination).expect("migrated identity"),
            prepared.destination_current_database_identity
        );
    }

    #[test]
    fn legacy_restore_ignores_changes_before_destination_open() {
        let root = tempfile::tempdir().expect("destination root");
        let legacy = root
            .path()
            .join(crate::sample_sources::db::LEGACY_DB_FILE_NAME);
        let current = crate::sample_sources::database_path_for(root.path());
        fs::write(&legacy, b"legacy owner").expect("legacy database");
        let migration = LegacyDatabaseMigrationSnapshot::new(root.path());
        fs::rename(&legacy, &current).expect("external migration");

        migration.restore_original_names(false);

        assert!(!legacy.exists());
        assert_eq!(fs::read(current).unwrap(), b"legacy owner");
    }

    #[test]
    fn no_replace_publish_preserves_late_destination_owner() {
        let root = tempfile::tempdir().expect("destination root");
        let destination = crate::sample_sources::database_path_for(root.path());
        let staged = staged_database_path(&destination, 1);
        fs::write(&staged, b"snapshot").expect("stage snapshot");
        fs::write(&destination, b"late owner").expect("create late owner");

        publish_staged_database_without_replace(&staged, &destination)
            .expect_err("no-replace publish must reject a late owner");

        assert_eq!(fs::read(&destination).unwrap(), b"late owner");
        assert_eq!(fs::read(&staged).unwrap(), b"snapshot");
    }

    #[test]
    fn rollback_preserves_preexisting_sidecars_and_removes_created_artifacts() {
        let root = tempfile::tempdir().expect("destination root");
        let database = crate::sample_sources::database_path_for(root.path());
        let wal = path_with_suffix(&database, "-wal");
        let shm = path_with_suffix(&database, "-shm");
        fs::write(&wal, b"preexisting wal").expect("preexisting wal");
        let snapshot = DatabaseArtifactSnapshot::new(root.path());
        fs::write(&database, b"created database").expect("created database");
        fs::write(&shm, b"created shm").expect("created shm");

        snapshot.remove_created();

        assert!(!database.exists());
        assert!(!shm.exists());
        assert_eq!(fs::read(wal).unwrap(), b"preexisting wal");
    }

    #[test]
    fn rollback_preserves_sidecars_created_for_preexisting_database() {
        let root = tempfile::tempdir().expect("destination root");
        let database = crate::sample_sources::database_path_for(root.path());
        let wal = path_with_suffix(&database, "-wal");
        let shm = path_with_suffix(&database, "-shm");
        fs::write(&database, b"preexisting database").expect("preexisting database");
        let snapshot = DatabaseArtifactSnapshot::new(root.path());
        fs::write(&wal, b"committed wal rows").expect("created wal");
        fs::write(&shm, b"shared memory state").expect("created shm");

        snapshot.remove_created();

        assert_eq!(fs::read(database).unwrap(), b"preexisting database");
        assert_eq!(fs::read(wal).unwrap(), b"committed wal rows");
        assert_eq!(fs::read(shm).unwrap(), b"shared memory state");
    }
}

fn path_with_suffix(path: &Path, suffix: &str) -> PathBuf {
    let mut value = path.as_os_str().to_os_string();
    value.push(suffix);
    PathBuf::from(value)
}
