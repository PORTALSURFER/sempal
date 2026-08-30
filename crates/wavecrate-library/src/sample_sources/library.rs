//! Global SQLite storage for sources that should not live in the config file.

use std::path::Path;
use std::sync::{LazyLock, Mutex};
use std::time::Instant;

use rusqlite::Connection;
use tracing::warn;

mod connection;
mod error;
mod harvest;
mod migrations;
mod owner;
mod schema_checks;
mod schema_defs;
mod sources;
mod telemetry;

#[cfg(test)]
mod tests;

#[cfg(test)]
mod telemetry_tests;

use super::{SampleSource, SourceId};
use connection::LibraryDatabase;
use error::map_sql_error;
use telemetry::record_library_db_event;

pub use error::LibraryError;
pub use harvest::{
    HarvestDerivationOperation, HarvestDerivationRecord, HarvestFileIdentity, HarvestFileKey,
    HarvestFileRecord, HarvestMetadataSnapshot, HarvestSourceRange, HarvestState,
    NewHarvestDerivation,
};
pub use owner::{
    GLOBAL_LIBRARY_WRITER_COMMAND_CAPACITY, GlobalLibraryWriter, GlobalLibraryWriterQueueError,
    GlobalLibraryWriterStartError, GlobalLibraryWriterStatus, GlobalLibraryWriterUnavailable,
    SourceRegistrySnapshot,
};

#[cfg(test)]
use crate::sample_sources::normalize_path;

/// Filename for the global library database stored under the user app directory.
pub const LIBRARY_DB_FILE_NAME: &str = "library.db";

/// Aggregate state loaded from or written to the library database.
#[derive(Debug, Clone, Default)]
pub struct LibraryState {
    /// All configured sample sources.
    pub sources: Vec<SampleSource>,
}

const KNOWN_SOURCES_KEY: &str = "known_sources_v1";

static LIBRARY_LOCK: LazyLock<Mutex<()>> = LazyLock::new(|| Mutex::new(()));

/// Load all sources from the global library database, creating it if missing.
pub fn load() -> Result<LibraryState, LibraryError> {
    let started_at = Instant::now();
    let _guard = lock_library();
    let db = LibraryDatabase::open()?;
    let result = db.load_state();
    record_library_db_event("library.load", started_at, result.as_ref().map(|_| ()));
    result
}

/// Persist sources to the global library database, replacing existing rows.
pub fn save(state: &LibraryState) -> Result<(), LibraryError> {
    let started_at = Instant::now();
    let _guard = lock_library();
    let mut db = LibraryDatabase::open()?;
    let result = db.replace_state(state);
    record_library_db_event("library.save", started_at, result.as_ref().map(|_| ()));
    result
}

/// Open a connection to the library DB with schema + migrations applied.
pub fn open_connection() -> Result<Connection, LibraryError> {
    let started_at = Instant::now();
    let _guard = lock_library();
    let db = LibraryDatabase::open()?;
    let result = Ok(db.into_connection());
    record_library_db_event(
        "library.open_connection",
        started_at,
        result.as_ref().map(|_| ()),
    );
    result
}

/// Attempt to reuse a historical source id for the given root folder.
///
/// This allows removing and re-adding a source without creating a new `source_id::...` namespace
/// (and therefore avoids re-analysis when files are unchanged).
pub fn lookup_source_id_for_root(root: &Path) -> Result<Option<SourceId>, LibraryError> {
    let started_at = Instant::now();
    let _guard = lock_library();
    let db = LibraryDatabase::open()?;
    let result = db.lookup_known_source_id(root);
    record_library_db_event(
        "library.lookup_source_id_for_root",
        started_at,
        result.as_ref().map(|_| ()),
    );
    result
}

/// Rehydrate the last configured descriptor for a previously known source root.
///
/// Removed sources remain in the bounded known-source registry so re-adding one can reuse its
/// stable id and, importantly, its protected/AppData metadata location. The source is not active
/// merely because it is retained here; callers must still add it to [`LibraryState::sources`].
pub fn lookup_retained_source_for_root(root: &Path) -> Result<Option<SampleSource>, LibraryError> {
    let started_at = Instant::now();
    let _guard = lock_library();
    let db = LibraryDatabase::open()?;
    let result = db.lookup_retained_source(root);
    record_library_db_event(
        "library.lookup_retained_source_for_root",
        started_at,
        result.as_ref().map(|_| ()),
    );
    result
}

/// Load descriptors for every configured or historically retained source.
///
/// This is used by bounded cache garbage collection to avoid deleting payloads referenced by a
/// source that is temporarily removed from the active configuration.
pub fn retained_sources() -> Result<Vec<SampleSource>, LibraryError> {
    let started_at = Instant::now();
    let _guard = lock_library();
    let db = LibraryDatabase::open()?;
    let result = db.load_retained_sources();
    record_library_db_event(
        "library.load_retained_sources",
        started_at,
        result.as_ref().map(|_| ()),
    );
    result
}

/// Insert or refresh a harvest file row without changing its workflow state.
pub fn upsert_harvest_file(
    identity: &HarvestFileIdentity,
) -> Result<HarvestFileRecord, LibraryError> {
    let started_at = Instant::now();
    let _guard = lock_library();
    let db = LibraryDatabase::open()?;
    let result = db.upsert_harvest_file(identity);
    record_library_db_event(
        "library.harvest.upsert_file",
        started_at,
        result.as_ref().map(|_| ()),
    );
    result
}

/// Insert or refresh multiple harvest rows in one transaction without changing workflow state.
pub fn upsert_harvest_files(identities: &[HarvestFileIdentity]) -> Result<(), LibraryError> {
    if identities.is_empty() {
        return Ok(());
    }
    let started_at = Instant::now();
    let _guard = lock_library();
    let mut db = LibraryDatabase::open()?;
    let result = db.upsert_harvest_files(identities);
    record_library_db_event(
        "library.harvest.upsert_files",
        started_at,
        result.as_ref().map(|_| ()),
    );
    result
}

/// Mark a harvest file as seen unless it is already in a later/manual state.
pub fn mark_harvest_seen(
    identity: &HarvestFileIdentity,
) -> Result<HarvestFileRecord, LibraryError> {
    let started_at = Instant::now();
    let _guard = lock_library();
    let db = LibraryDatabase::open()?;
    let result = db.advance_harvest_state(identity, HarvestState::Seen);
    record_library_db_event(
        "library.harvest.mark_seen",
        started_at,
        result.as_ref().map(|_| ()),
    );
    result
}

/// Mark a harvest file as touched unless it is already done or ignored.
pub fn mark_harvest_touched(
    identity: &HarvestFileIdentity,
) -> Result<HarvestFileRecord, LibraryError> {
    mark_harvest_touched_if_current(identity, || true)
        .map(|record| record.expect("unconditional harvest touch must not be skipped"))
}

/// Mark a harvest file as touched when the caller's revision is still current.
///
/// The predicate is evaluated while the global library lock is held and immediately before the
/// state update, allowing UI-owned revision fences to serialize correctly with manual harvest
/// mutations.
pub fn mark_harvest_touched_if_current(
    identity: &HarvestFileIdentity,
    current: impl Fn() -> bool,
) -> Result<Option<HarvestFileRecord>, LibraryError> {
    let started_at = Instant::now();
    let _guard = lock_library();
    if !current() {
        return Ok(None);
    }
    let db = LibraryDatabase::open()?;
    // Re-check after opening the database so a superseding request that arrives while opening it
    // cannot proceed to the serialized state mutation.
    if !current() {
        return Ok(None);
    }
    let result = db.advance_harvest_state(identity, HarvestState::Touched);
    record_library_db_event(
        "library.harvest.mark_touched",
        started_at,
        result.as_ref().map(|_| ()),
    );
    result.map(Some)
}

/// Manually set a harvest state, including explicit reset to `New`.
pub fn set_harvest_state(
    key: &HarvestFileKey,
    state: HarvestState,
) -> Result<HarvestFileRecord, LibraryError> {
    let started_at = Instant::now();
    let _guard = lock_library();
    let db = LibraryDatabase::open()?;
    let result = db.set_harvest_state(key, state);
    record_library_db_event(
        "library.harvest.set_state",
        started_at,
        result.as_ref().map(|_| ()),
    );
    result
}

/// Load one harvest file row.
pub fn harvest_file(key: &HarvestFileKey) -> Result<Option<HarvestFileRecord>, LibraryError> {
    let started_at = Instant::now();
    let _guard = lock_library();
    let db = LibraryDatabase::open()?;
    let result = db.harvest_file(key);
    record_library_db_event(
        "library.harvest.file",
        started_at,
        result.as_ref().map(|_| ()),
    );
    result
}

/// Record a parent-to-child derivation edge and mark the parent as touched.
pub fn record_harvest_derivation(edge: &NewHarvestDerivation) -> Result<i64, LibraryError> {
    let started_at = Instant::now();
    let _guard = lock_library();
    let mut db = LibraryDatabase::open()?;
    let result = db.record_harvest_derivation(edge);
    record_library_db_event(
        "library.harvest.record_derivation",
        started_at,
        result.as_ref().map(|_| ()),
    );
    result
}

/// Remap harvest file rows and graph edge endpoints after a file move.
pub fn remap_harvest_file_key(
    old_key: &HarvestFileKey,
    new_key: &HarvestFileKey,
) -> Result<usize, LibraryError> {
    let started_at = Instant::now();
    let _guard = lock_library();
    let mut db = LibraryDatabase::open()?;
    let result = db.remap_harvest_file_key(old_key, new_key);
    record_library_db_event(
        "library.harvest.remap_file_key",
        started_at,
        result.as_ref().map(|_| ()),
    );
    result
}

/// Remap harvest file rows and graph edge endpoints after a folder move.
pub fn remap_harvest_file_prefix(
    source_id: &SourceId,
    old_prefix: &Path,
    new_prefix: &Path,
) -> Result<usize, LibraryError> {
    let started_at = Instant::now();
    let _guard = lock_library();
    let mut db = LibraryDatabase::open()?;
    let result = db.remap_harvest_file_prefix(source_id, old_prefix, new_prefix);
    record_library_db_event(
        "library.harvest.remap_file_prefix",
        started_at,
        result.as_ref().map(|_| ()),
    );
    result
}

/// Load all immediate derivatives for an origin file.
pub fn harvest_derivations_for_parent(
    key: &HarvestFileKey,
) -> Result<Vec<HarvestDerivationRecord>, LibraryError> {
    let started_at = Instant::now();
    let _guard = lock_library();
    let db = LibraryDatabase::open()?;
    let result = db.harvest_derivations_for_parent(key);
    record_library_db_event(
        "library.harvest.derivations_for_parent",
        started_at,
        result.as_ref().map(|_| ()),
    );
    result
}

/// Load immediate parents for a derived file.
pub fn harvest_parents_for_child(
    key: &HarvestFileKey,
) -> Result<Vec<HarvestDerivationRecord>, LibraryError> {
    let started_at = Instant::now();
    let _guard = lock_library();
    let db = LibraryDatabase::open()?;
    let result = db.harvest_parents_for_child(key);
    record_library_db_event(
        "library.harvest.parents_for_child",
        started_at,
        result.as_ref().map(|_| ()),
    );
    result
}

/// Count immediate derivatives for an origin file.
pub fn harvest_derivative_count(key: &HarvestFileKey) -> Result<u64, LibraryError> {
    let started_at = Instant::now();
    let _guard = lock_library();
    let db = LibraryDatabase::open()?;
    let result = db.harvest_derivative_count(key);
    record_library_db_event(
        "library.harvest.derivative_count",
        started_at,
        result.as_ref().map(|_| ()),
    );
    result
}

/// Load all harvest rows for one source.
pub fn harvest_files_for_source(
    source_id: &SourceId,
) -> Result<Vec<HarvestFileRecord>, LibraryError> {
    let started_at = Instant::now();
    let _guard = lock_library();
    let db = LibraryDatabase::open()?;
    let result = db.harvest_files_for_source(source_id);
    record_library_db_event(
        "library.harvest.files_for_source",
        started_at,
        result.as_ref().map(|_| ()),
    );
    result
}

/// Load derivative counts keyed by parent relative path for one source.
pub fn harvest_derivative_counts_for_source(
    source_id: &SourceId,
) -> Result<Vec<(std::path::PathBuf, u64)>, LibraryError> {
    let started_at = Instant::now();
    let _guard = lock_library();
    let db = LibraryDatabase::open()?;
    let result = db.harvest_derivative_counts_for_source(source_id);
    record_library_db_event(
        "library.harvest.derivative_counts_for_source",
        started_at,
        result.as_ref().map(|_| ()),
    );
    result
}

fn lock_library() -> std::sync::MutexGuard<'static, ()> {
    match LIBRARY_LOCK.lock() {
        Ok(guard) => guard,
        Err(poisoned) => {
            warn!("Library DB mutex poisoned; recovering to keep the app running.");
            poisoned.into_inner()
        }
    }
}
