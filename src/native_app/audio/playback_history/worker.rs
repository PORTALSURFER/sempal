use std::{cell::RefCell, collections::HashMap, path::PathBuf};

#[cfg(test)]
use std::path::Path;

use wavecrate::sample_sources::{ExistingFileMetadataUpdate, SourceDatabase};

use crate::native_app::audio::playback_history::LastPlayedPersistResult;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(in crate::native_app) struct LastPlayedPersistRequest {
    pub(in crate::native_app) file_id: String,
    pub(in crate::native_app) source_root: PathBuf,
    pub(in crate::native_app) source_database_root: PathBuf,
    pub(in crate::native_app) relative_path: PathBuf,
    pub(in crate::native_app) played_at: i64,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct CachedSourceDbKey {
    source_root: PathBuf,
    source_database_root: PathBuf,
}

thread_local! {
    static PLAYBACK_HISTORY_SOURCE_DBS: RefCell<HashMap<CachedSourceDbKey, SourceDatabase>> =
        RefCell::new(HashMap::new());
}

pub(super) fn persist_last_played(request: LastPlayedPersistRequest) -> LastPlayedPersistResult {
    let result = persist_last_played_inner(&request);
    LastPlayedPersistResult {
        file_id: request.file_id,
        result,
    }
}

fn persist_last_played_inner(request: &LastPlayedPersistRequest) -> Result<(), String> {
    with_playback_history_source_db(request, |db| persist_last_played_with_db(db, request))
}

#[cfg(test)]
fn file_metadata(path: &Path) -> Result<(u64, i64), String> {
    let metadata = std::fs::metadata(path)
        .map_err(|err| format!("Failed to read {}: {err}", path.display()))?;
    let modified_ns = metadata
        .modified()
        .map(wavecrate_library::timestamps::system_time_to_unix_nanos)
        .map_err(|err| format!("Missing modified time for {}: {err}", path.display()))?;
    Ok((metadata.len(), modified_ns))
}

fn with_playback_history_source_db<T>(
    request: &LastPlayedPersistRequest,
    action: impl FnOnce(&SourceDatabase) -> Result<T, String>,
) -> Result<T, String> {
    let key = CachedSourceDbKey {
        source_root: request.source_root.clone(),
        source_database_root: request.source_database_root.clone(),
    };
    PLAYBACK_HISTORY_SOURCE_DBS.with(|dbs| {
        let mut dbs = dbs.borrow_mut();
        if !dbs.contains_key(&key) {
            let db = SourceDatabase::open_for_playback_history_write_with_database_root(
                &request.source_root,
                &request.source_database_root,
            )
            .map_err(|err| err.to_string())?;
            dbs.insert(key.clone(), db);
        }
        let db = dbs
            .get(&key)
            .expect("playback history source DB was inserted");
        action(db)
    })
}

fn persist_last_played_with_db(
    db: &SourceDatabase,
    request: &LastPlayedPersistRequest,
) -> Result<(), String> {
    let mut batch = db.write_batch().map_err(|err| err.to_string())?;
    if matches!(
        batch
            .ensure_existing_live_file(&request.relative_path)
            .map_err(|err| err.to_string())?,
        ExistingFileMetadataUpdate::Missing
    ) {
        return Err(format!(
            "playback persistence deferred until source row exists: {}",
            request.relative_path.display()
        ));
    }
    batch
        .set_last_played_at(&request.relative_path, request.played_at)
        .map_err(|err| err.to_string())?;
    batch
        .commit_auxiliary_state()
        .map_err(|err| err.to_string())
}

#[cfg(test)]
mod tests {
    use std::{
        fs,
        path::PathBuf,
        time::{Duration, Instant},
    };

    use super::*;
    use wavecrate::sample_sources::{
        SourceDatabase, SourceDatabaseConnectionRole, db as source_db_test,
    };

    #[test]
    fn last_played_persist_fails_fast_when_source_db_is_busy() {
        let temp = tempfile::tempdir().expect("tempdir");
        let source_root = temp.path().join("source");
        fs::create_dir_all(&source_root).expect("create source");
        let relative_path = PathBuf::from("kick.wav");
        let sample_path = source_root.join(&relative_path);
        fs::write(&sample_path, [1_u8, 2, 3, 4]).expect("write sample");

        let db = SourceDatabase::open_for_user_metadata_write_with_database_root(
            &source_root,
            &source_root,
        )
        .expect("open source db");
        let (file_size, modified_ns) = file_metadata(&sample_path).expect("file metadata");
        let mut batch = db.write_batch().expect("write batch");
        batch
            .upsert_file(&relative_path, file_size, modified_ns)
            .expect("upsert sample");
        batch.commit().expect("commit seed");
        drop(db);

        let locking_connection = SourceDatabase::open_connection_with_role_and_database_root(
            &source_root,
            &source_root,
            SourceDatabaseConnectionRole::JobWorker,
        )
        .expect("open locking connection");
        locking_connection
            .execute_batch("BEGIN IMMEDIATE")
            .expect("hold write lock");

        let request = LastPlayedPersistRequest {
            file_id: sample_path.display().to_string(),
            source_root: source_root.clone(),
            source_database_root: source_root,
            relative_path,
            played_at: 42,
        };
        let started = Instant::now();
        let result = persist_last_played_inner(&request);

        assert!(
            started.elapsed() < Duration::from_secs(1),
            "last-played persistence should skip locked databases quickly"
        );
        let error = result.expect_err("busy source DB should skip last-played persist");
        assert!(
            error.contains("Database is busy"),
            "expected busy error, got: {error}"
        );
    }

    #[test]
    fn last_played_persist_reuses_source_db_connection() {
        let temp = tempfile::tempdir().expect("tempdir");
        let source_root = temp.path().join("source");
        fs::create_dir_all(&source_root).expect("create source");
        let first_relative = PathBuf::from("kick.wav");
        let second_relative = PathBuf::from("hat.wav");
        let first = source_root.join(&first_relative);
        let second = source_root.join(&second_relative);
        fs::write(&first, [1_u8, 2, 3, 4]).expect("write first sample");
        fs::write(&second, [5_u8, 6, 7, 8]).expect("write second sample");

        let open_count_scope = source_db_test::test_scope_source_db_open_total_count(&source_root);
        let first_request = LastPlayedPersistRequest {
            file_id: first.display().to_string(),
            source_root: source_root.clone(),
            source_database_root: source_root.clone(),
            relative_path: first_relative,
            played_at: 42,
        };
        let second_request = LastPlayedPersistRequest {
            file_id: second.display().to_string(),
            source_root: source_root.clone(),
            source_database_root: source_root.clone(),
            relative_path: second_relative,
            played_at: 43,
        };

        persist_last_played_inner(&first_request).expect("persist first");
        open_count_scope.reset();
        persist_last_played_inner(&second_request).expect("persist second");

        assert_eq!(
            source_db_test::test_source_db_open_total_count(&source_root),
            0,
            "last-played persistence should reuse its source DB handle after the first write"
        );
    }
}
