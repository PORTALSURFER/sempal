use std::path::Path;

use rusqlite::{Connection, OpenFlags};

use super::paths;
use crate::sample_sources::db::{
    DB_FILE_NAME, SourceDatabase, SourceDatabaseConnectionRole, SourceDbError, telemetry,
};

pub(super) fn open_read_only_source_database(
    root: &Path,
    database_root: &Path,
    role: SourceDatabaseConnectionRole,
) -> Result<SourceDatabase, SourceDbError> {
    let open_started = std::time::Instant::now();
    if !root.is_dir() {
        return Err(SourceDbError::InvalidRoot(root.to_path_buf()));
    }

    let db_path = paths::read_only_db_path(database_root)?
        .ok_or_else(|| SourceDbError::ReadOnlyDatabaseMissing(database_root.join(DB_FILE_NAME)))?;

    let connect_started = std::time::Instant::now();
    let connection = match Connection::open_with_flags(&db_path, OpenFlags::SQLITE_OPEN_READ_ONLY) {
        Ok(connection) => {
            telemetry::record_open_phase(
                root,
                &db_path,
                role.label(),
                "connect",
                true,
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
                role.label(),
                "connect",
                true,
                connect_started.elapsed(),
                Err(&err),
            );
            telemetry::record_open_total(
                root,
                &db_path,
                role.label(),
                true,
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
        telemetry_label: role.label(),
        database_root_binding: None,
    };
    let pragmas_started = std::time::Instant::now();
    if let Err(err) = db.apply_read_only_pragmas(role) {
        telemetry::record_open_phase(
            root,
            &db_path,
            role.label(),
            "pragmas",
            true,
            pragmas_started.elapsed(),
            Err(&err),
        );
        telemetry::record_open_total(
            root,
            &db_path,
            role.label(),
            true,
            open_started.elapsed(),
            Err(&err),
        );
        return Err(err);
    }
    telemetry::record_open_phase(
        root,
        &db_path,
        role.label(),
        "pragmas",
        true,
        pragmas_started.elapsed(),
        Ok(()),
    );
    telemetry::record_open_total(
        root,
        &db_path,
        role.label(),
        true,
        open_started.elapsed(),
        Ok(()),
    );
    Ok(db)
}

#[cfg(test)]
mod tests {
    use rusqlite::MAIN_DB;
    use tempfile::tempdir;

    use super::*;

    #[test]
    fn read_only_open_stays_read_only_for_writable_roles() {
        let directory = tempdir().expect("temporary source");
        let writable = SourceDatabase::open_for_source_write(directory.path())
            .expect("create source database");
        writable
            .set_metadata("read_only_probe", "before")
            .expect("seed metadata");
        drop(writable);

        let read_only = open_read_only_source_database(
            directory.path(),
            directory.path(),
            SourceDatabaseConnectionRole::JobWorker,
        )
        .expect("open job-worker role read-only");

        assert!(
            read_only.connection.is_readonly(MAIN_DB).unwrap(),
            "the read-only entrypoint must override writable role flags"
        );
        assert!(
            read_only
                .connection
                .execute(
                    "INSERT OR REPLACE INTO metadata (key, value) VALUES ('read_only_probe', 'after')",
                    [],
                )
                .is_err(),
            "read-only source connections must reject writes"
        );
    }
}
