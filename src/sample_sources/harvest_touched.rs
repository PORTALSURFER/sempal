use std::path::PathBuf;

use crate::sample_sources::{
    HarvestFileIdentity, HarvestFileKey, SampleSource, SourceDatabase, harvest_file_ops, library,
};

/// Result of persisting a harvest file's touched identity in the library database.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HarvestTouchedPersistResult {
    /// Absolute file id/path that the native browser scheduled for persistence.
    pub file_id: String,
    /// Persistence outcome with a user-loggable error string on failure.
    pub result: Result<(), String>,
}

/// Background request for marking a harvest file as touched.
#[derive(Clone, Debug)]
pub struct HarvestTouchedPersistRequest {
    /// Absolute file id/path used to correlate the background result.
    pub file_id: String,
    /// Owned source descriptor. Database-root resolution is deferred to the worker.
    pub source: SampleSource,
    /// Path to the sample relative to the owning source root.
    pub relative_path: PathBuf,
}

/// Persist a harvest-touched marker using file metadata and the owning source database.
pub fn persist_harvest_touched(
    request: HarvestTouchedPersistRequest,
) -> HarvestTouchedPersistResult {
    let result = match persist_harvest_touched_inner(&request, || true) {
        Ok(Some(())) => Ok(()),
        Ok(None) => Ok(()),
        Err(error) => Err(error),
    };
    HarvestTouchedPersistResult {
        file_id: request.file_id,
        result,
    }
}

/// Persist a touched marker only while the owning revision remains current.
///
/// The currentness callback is checked before filesystem/source-database work and again while the
/// global harvest library lock is held immediately before the state mutation. A superseded request
/// returns `None` without touching the filesystem or database.
pub fn persist_harvest_touched_if_current(
    request: HarvestTouchedPersistRequest,
    current: impl Fn() -> bool,
) -> Option<HarvestTouchedPersistResult> {
    let file_id = request.file_id.clone();
    match persist_harvest_touched_inner(&request, current) {
        Ok(Some(())) => Some(HarvestTouchedPersistResult {
            file_id,
            result: Ok(()),
        }),
        Ok(None) => None,
        Err(error) => Some(HarvestTouchedPersistResult {
            file_id,
            result: Err(error),
        }),
    }
}

fn persist_harvest_touched_inner(
    request: &HarvestTouchedPersistRequest,
    current: impl Fn() -> bool,
) -> Result<Option<()>, String> {
    if !current() {
        return Ok(None);
    }
    let path = request.source.root.join(&request.relative_path);
    let (file_size, modified_ns) = harvest_file_ops::file_identity_metadata(&path);
    let entry = SourceDatabase::open_for_ui_read_with_database_root(
        &request.source.root,
        &request
            .source
            .database_root()
            .map_err(|err| err.to_string())?,
    )
    .ok()
    .and_then(|db| db.entry_for_path(&request.relative_path).ok().flatten());
    let identity = HarvestFileIdentity {
        key: HarvestFileKey::new(request.source.id.clone(), request.relative_path.clone()),
        file_size: file_size.or_else(|| entry.as_ref().map(|entry| entry.file_size)),
        modified_ns: modified_ns.or_else(|| entry.as_ref().map(|entry| entry.modified_ns)),
        content_hash: entry.and_then(|entry| entry.content_hash),
    };
    library::mark_harvest_touched_if_current(&identity, current)
        .map(|record| record.map(|_| ()))
        .map_err(|err| err.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[test]
    fn stale_revision_skips_filesystem_and_library_work() {
        let checks = AtomicUsize::new(0);
        let request = HarvestTouchedPersistRequest {
            file_id: "/tmp/stale-touch.wav".to_owned(),
            source: SampleSource::new(PathBuf::from("/tmp/stale-touch-source")),
            relative_path: PathBuf::from("stale-touch.wav"),
        };
        assert!(
            persist_harvest_touched_if_current(request, || {
                checks.fetch_add(1, Ordering::Relaxed);
                false
            })
            .is_none()
        );
        assert_eq!(checks.load(Ordering::Relaxed), 1);
    }
}
