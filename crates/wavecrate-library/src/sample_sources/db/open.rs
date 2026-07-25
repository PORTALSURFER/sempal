use std::path::Path;

use rusqlite::{Connection, OpenFlags};

mod paths;
mod read_only;

use super::{
    SOURCE_DB_READ_ONLY_ENV, SourceDatabase, SourceDatabaseConnectionRole, SourceDbError,
    telemetry, util,
};
use read_only::open_read_only_source_database;

const DEFAULT_WRITABLE_BUSY_TIMEOUT_MS: u64 = 5_000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SourceDatabaseOpenMode {
    Fast,
    Full,
}

pub(super) fn open_source_database_for_role(
    root: &Path,
    role: SourceDatabaseConnectionRole,
) -> Result<SourceDatabase, SourceDbError> {
    open_source_database_for_role_with_database_root(root, root, role)
}

pub(super) fn open_source_database_for_role_with_database_root(
    root: &Path,
    database_root: &Path,
    role: SourceDatabaseConnectionRole,
) -> Result<SourceDatabase, SourceDbError> {
    if role.uses_read_only_connection() || should_open_source_db_read_only() {
        return open_read_only_source_database(root, database_root, role);
    }
    open_source_database_with_flags(
        root,
        database_root,
        role.open_flags(),
        role.open_mode(),
        role.label(),
        role.busy_timeout_ms(),
    )
}

pub(crate) fn open_source_database(
    root: &Path,
    read_only: bool,
    mode: SourceDatabaseOpenMode,
) -> Result<SourceDatabase, SourceDbError> {
    open_source_database_with_database_root(root, root, read_only, mode)
}

pub(crate) fn open_source_database_with_database_root(
    root: &Path,
    database_root: &Path,
    read_only: bool,
    mode: SourceDatabaseOpenMode,
) -> Result<SourceDatabase, SourceDbError> {
    if read_only {
        return open_read_only_source_database(
            root,
            database_root,
            SourceDatabaseConnectionRole::UiRead,
        );
    }
    open_source_database_with_flags(
        root,
        database_root,
        OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_CREATE,
        mode,
        mode.label(),
        DEFAULT_WRITABLE_BUSY_TIMEOUT_MS,
    )
}

fn open_source_database_with_flags(
    root: &Path,
    database_root: &Path,
    open_flags: OpenFlags,
    mode: SourceDatabaseOpenMode,
    telemetry_label: &'static str,
    busy_timeout_ms: u64,
) -> Result<SourceDatabase, SourceDbError> {
    let open_started = std::time::Instant::now();
    if !root.is_dir() {
        return Err(SourceDbError::InvalidRoot(root.to_path_buf()));
    }

    let database_is_embedded = database_root == root;
    let db_path = if database_is_embedded {
        paths::prepare_writable_db_path_in_existing_root(database_root)?
    } else {
        paths::prepare_writable_db_path(database_root)?
    };
    if !database_is_embedded {
        util::create_parent_if_needed(&db_path)?;
    }
    let connect_started = std::time::Instant::now();
    let connection = match Connection::open_with_flags(&db_path, open_flags) {
        Ok(connection) => {
            telemetry::record_open_phase(
                root,
                &db_path,
                telemetry_label,
                "connect",
                false,
                connect_started.elapsed(),
                Ok(()),
            );
            connection
        }
        Err(err) => {
            let err = SourceDbError::from(err);
            telemetry::record_open_phase(
                root,
                &db_path,
                telemetry_label,
                "connect",
                false,
                connect_started.elapsed(),
                Err(&err),
            );
            telemetry::record_open_total(
                root,
                &db_path,
                telemetry_label,
                false,
                open_started.elapsed(),
                Err(&err),
            );
            return Err(err);
        }
    };
    let db = SourceDatabase {
        connection,
        db_path: db_path.clone(),
        root: root.to_path_buf(),
        telemetry_label,
    };
    let pragmas_started = std::time::Instant::now();
    if let Err(err) = db.apply_pragmas(busy_timeout_ms) {
        telemetry::record_open_phase(
            root,
            &db_path,
            telemetry_label,
            "pragmas",
            false,
            pragmas_started.elapsed(),
            Err(&err),
        );
        telemetry::record_open_total(
            root,
            &db_path,
            telemetry_label,
            false,
            open_started.elapsed(),
            Err(&err),
        );
        return Err(err);
    }
    telemetry::record_open_phase(
        root,
        &db_path,
        telemetry_label,
        "pragmas",
        false,
        pragmas_started.elapsed(),
        Ok(()),
    );
    let schema_started = std::time::Instant::now();
    let schema_result = match mode {
        SourceDatabaseOpenMode::Fast => db.apply_schema_fast(),
        SourceDatabaseOpenMode::Full => db.apply_schema(),
    };
    match schema_result {
        Ok(()) => {
            telemetry::record_open_phase(
                root,
                &db_path,
                telemetry_label,
                "schema",
                false,
                schema_started.elapsed(),
                Ok(()),
            );
        }
        Err(err) => {
            telemetry::record_open_phase(
                root,
                &db_path,
                telemetry_label,
                "schema",
                false,
                schema_started.elapsed(),
                Err(&err),
            );
            telemetry::record_open_total(
                root,
                &db_path,
                telemetry_label,
                false,
                open_started.elapsed(),
                Err(&err),
            );
            return Err(err);
        }
    }
    telemetry::record_open_total(
        root,
        &db_path,
        telemetry_label,
        false,
        open_started.elapsed(),
        Ok(()),
    );
    Ok(db)
}

impl SourceDatabaseOpenMode {
    fn label(self) -> &'static str {
        match self {
            Self::Fast => "fast",
            Self::Full => "full",
        }
    }
}

/// Owned scope for one source DB open counter used by regression tests.
#[cfg(debug_assertions)]
pub struct TestSourceDbOpenCountGuard {
    root: std::path::PathBuf,
}

#[cfg(debug_assertions)]
impl Drop for TestSourceDbOpenCountGuard {
    fn drop(&mut self) {
        telemetry::release_open_total_count_scope(&self.root);
    }
}

#[cfg(debug_assertions)]
impl TestSourceDbOpenCountGuard {
    /// Clear observations accumulated so far while retaining scope ownership.
    pub fn reset(&self) {
        telemetry::reset_open_total_count(&self.root);
    }
}

/// Clear one source DB open counter for a scope and clear it again on drop.
#[cfg(debug_assertions)]
pub fn test_scope_source_db_open_total_count(root: &Path) -> TestSourceDbOpenCountGuard {
    telemetry::acquire_open_total_count_scope(root);
    TestSourceDbOpenCountGuard {
        root: root.to_path_buf(),
    }
}

/// Return the debug-only source DB open count used by regression tests.
#[cfg(debug_assertions)]
pub fn test_source_db_open_total_count(root: &Path) -> usize {
    telemetry::open_total_count(root)
}

pub(super) fn should_open_source_db_read_only() -> bool {
    crate::env_flags::env_var_truthy(SOURCE_DB_READ_ONLY_ENV)
}
