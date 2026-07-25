use std::path::PathBuf;

use rusqlite::{Transaction, TransactionBehavior, params};

use super::super::util::{map_sql_error, normalize_relative_path, parse_relative_path_from_db};
use super::super::{META_LAST_MANIFEST_AUDIT_AT, SourceDatabase, SourceDbError, SourceWriteBatch};

impl SourceDatabase {
    /// Start a durable manifest-audit cycle or recover the paths already checked by an
    /// interrupted cycle.
    ///
    /// Returned paths are resumable traversal candidates, not proof that the current filesystem
    /// state is still represented by the manifest. The scanner reopens every returned path before
    /// allowing the resumed cycle to complete.
    ///
    /// Audit bookkeeping deliberately does not advance the source revision: it is private scan
    /// progress, not a committed manifest mutation.
    pub fn begin_or_resume_manifest_audit(
        &self,
        started_at: i64,
    ) -> Result<Vec<PathBuf>, SourceDbError> {
        self.begin_or_resume_manifest_audit_batch(started_at, usize::MAX)
            .map(|(paths, _)| paths)
    }

    /// Start or resume an audit and load one bounded slice of paths that still need
    /// checkpoint revalidation.
    pub fn begin_or_resume_manifest_audit_batch(
        &self,
        started_at: i64,
        limit: usize,
    ) -> Result<(Vec<PathBuf>, usize), SourceDbError> {
        let transaction =
            Transaction::new_unchecked(&self.connection, TransactionBehavior::Immediate)
                .map_err(map_sql_error)?;
        transaction
            .execute(
                "INSERT OR IGNORE INTO source_manifest_audit_state (
                    singleton, started_at, checked_files
                 ) VALUES (1, ?1, 0)",
                [started_at],
            )
            .map_err(map_sql_error)?;
        let raw_paths = {
            let mut statement = transaction
                .prepare(
                    "SELECT path FROM source_manifest_audit_seen
                     ORDER BY path ASC LIMIT ?1",
                )
                .map_err(map_sql_error)?;
            statement
                .query_map([i64::try_from(limit).unwrap_or(i64::MAX)], |row| {
                    row.get::<_, String>(0)
                })
                .map_err(map_sql_error)?
                .collect::<Result<Vec<_>, _>>()
                .map_err(map_sql_error)?
        };
        let checked_files = transaction
            .query_row(
                "SELECT checked_files FROM source_manifest_audit_state WHERE singleton = 1",
                [],
                |row| row.get::<_, i64>(0),
            )
            .map_err(map_sql_error)?;
        transaction.commit().map_err(map_sql_error)?;
        let paths = raw_paths
            .into_iter()
            .filter_map(|path| match parse_relative_path_from_db(&path) {
                Ok(path) => Some(path),
                Err(error) => {
                    tracing::warn!(
                        %error,
                        path,
                        "Skipping invalid durable manifest-audit checkpoint path"
                    );
                    None
                }
            })
            .collect();
        Ok((paths, usize::try_from(checked_files).unwrap_or(0)))
    }

    /// Durably checkpoint paths completed by the current manifest-audit cycle.
    pub fn checkpoint_manifest_audit_paths(&self, paths: &[PathBuf]) -> Result<(), SourceDbError> {
        self.checkpoint_manifest_audit_paths_with_count(paths)
            .map(|_| ())
    }

    /// Durably checkpoint paths and return the number that were new to the current audit state.
    pub fn checkpoint_manifest_audit_paths_with_count(
        &self,
        paths: &[PathBuf],
    ) -> Result<usize, SourceDbError> {
        if paths.is_empty() {
            return Ok(0);
        }
        let transaction =
            Transaction::new_unchecked(&self.connection, TransactionBehavior::Immediate)
                .map_err(map_sql_error)?;
        let mut inserted = 0;
        {
            let mut insert = transaction
                .prepare("INSERT OR IGNORE INTO source_manifest_audit_seen (path) VALUES (?1)")
                .map_err(map_sql_error)?;
            for path in paths {
                let normalized = normalize_relative_path(path)?;
                inserted += usize::try_from(insert.execute([normalized]).map_err(map_sql_error)?)
                    .unwrap_or(0);
            }
        }
        transaction
            .execute(
                "UPDATE source_manifest_audit_state
                 SET checked_files = (
                     SELECT COUNT(*) FROM source_manifest_audit_seen
                 )
                 WHERE singleton = 1",
                [],
            )
            .map_err(map_sql_error)?;
        transaction.commit().map_err(map_sql_error)?;
        Ok(inserted)
    }

    /// Remove paths whose resumed checkpoint revalidation committed successfully.
    pub fn clear_manifest_audit_paths(&self, paths: &[PathBuf]) -> Result<(), SourceDbError> {
        if paths.is_empty() {
            return Ok(());
        }
        let transaction =
            Transaction::new_unchecked(&self.connection, TransactionBehavior::Immediate)
                .map_err(map_sql_error)?;
        {
            let mut delete = transaction
                .prepare("DELETE FROM source_manifest_audit_seen WHERE path = ?1")
                .map_err(map_sql_error)?;
            for path in paths {
                let normalized = normalize_relative_path(path)?;
                delete.execute([normalized]).map_err(map_sql_error)?;
            }
        }
        transaction
            .execute(
                "UPDATE source_manifest_audit_state
                 SET checked_files = (
                     SELECT COUNT(*) FROM source_manifest_audit_seen
                 )
                 WHERE singleton = 1",
                [],
            )
            .map_err(map_sql_error)?;
        transaction.commit().map_err(map_sql_error)
    }
}

impl SourceWriteBatch<'_> {
    /// Finish a complete manifest-audit cycle in the same transaction that publishes its
    /// completion timestamp.
    pub fn complete_manifest_audit(&mut self, completed_at: i64) -> Result<(), SourceDbError> {
        self.set_metadata(META_LAST_MANIFEST_AUDIT_AT, &completed_at.to_string())?;
        self.tx
            .execute("DELETE FROM source_manifest_audit_seen", [])
            .map_err(map_sql_error)?;
        self.tx
            .execute(
                "DELETE FROM source_manifest_audit_state WHERE singleton = ?1",
                params![1],
            )
            .map_err(map_sql_error)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn interrupted_manifest_audit_recovers_only_durable_checked_paths() {
        let directory = tempfile::tempdir().expect("source");
        let database = SourceDatabase::open_for_source_write(directory.path()).expect("database");

        assert!(
            database
                .begin_or_resume_manifest_audit(10)
                .expect("begin audit")
                .is_empty()
        );
        database
            .checkpoint_manifest_audit_paths(&[
                Path::new("a.wav").to_path_buf(),
                Path::new("nested/b.wav").to_path_buf(),
            ])
            .expect("checkpoint audit");

        assert_eq!(
            database
                .begin_or_resume_manifest_audit(20)
                .expect("resume audit"),
            vec![PathBuf::from("a.wav"), PathBuf::from("nested/b.wav")]
        );
        assert_eq!(
            database
                .begin_or_resume_manifest_audit_batch(20, 1)
                .expect("bounded resume batch"),
            (vec![PathBuf::from("a.wav")], 2)
        );
        database
            .clear_manifest_audit_paths(&[PathBuf::from("a.wav")])
            .expect("clear revalidated checkpoint");
        assert_eq!(
            database
                .begin_or_resume_manifest_audit_batch(20, 1)
                .expect("next bounded resume batch"),
            (vec![PathBuf::from("nested/b.wav")], 1)
        );

        let mut batch = database.write_batch().expect("completion batch");
        batch.complete_manifest_audit(30).expect("complete audit");
        batch.commit().expect("commit completion");

        assert!(
            database
                .begin_or_resume_manifest_audit(40)
                .expect("new audit")
                .is_empty()
        );
        assert_eq!(
            database
                .get_metadata(META_LAST_MANIFEST_AUDIT_AT)
                .expect("audit metadata")
                .as_deref(),
            Some("30")
        );
    }
}
