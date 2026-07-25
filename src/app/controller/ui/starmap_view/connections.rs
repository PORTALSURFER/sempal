//! Connection helpers for map-view queries and background layout jobs.

use crate::app::controller::library::{analysis_jobs, source_write_priority};
use rusqlite::Connection;

use super::*;

pub(super) enum StarmapWriteSession {
    Ready(analysis_jobs::AnalysisJobSession),
    DeferredForFileOp,
}

/// Return a cached per-source map-query connection, opening it on first use.
pub(super) fn open_cached_source_db<'a>(
    controller: &'a mut AppController,
    source_id: Option<&SourceId>,
) -> Result<&'a mut Connection, String> {
    let source_id = source_id
        .ok_or_else(|| "No source selected".to_string())?
        .clone();
    let source_root = controller
        .library
        .sources
        .iter()
        .find(|source| source.id == source_id)
        .map(|source| source.root.clone())
        .ok_or_else(|| "Source not found".to_string())?;
    if !controller
        .runtime
        .map
        .query_connections
        .contains_key(&source_id)
    {
        let conn = analysis_jobs::open_source_db_ui_read(&source_root)?;
        controller
            .runtime
            .map
            .query_connections
            .insert(source_id.clone(), conn);
    }
    controller
        .runtime
        .map
        .query_connections
        .get_mut(&source_id)
        .map(|session| &mut **session)
        .ok_or_else(|| "Map query connection missing after open".to_string())
}

pub(super) fn open_source_db_for_id(source_id: &SourceId) -> Result<StarmapWriteSession, String> {
    if source_write_priority::file_op_write_priority_active(source_id) {
        return Ok(StarmapWriteSession::DeferredForFileOp);
    }
    let state = crate::sample_sources::library::load().map_err(|err| err.to_string())?;
    let source = state
        .sources
        .iter()
        .find(|source| &source.id == source_id)
        .ok_or_else(|| "Source not found".to_string())?;
    analysis_jobs::open_source_db(&source.root).map(StarmapWriteSession::Ready)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::controller::library::source_write_priority::FileOpWritePriorityGuard;
    use tempfile::TempDir;

    #[test]
    fn writable_starmap_session_defers_during_same_source_file_op() {
        let config_dir = TempDir::new().expect("create config dir");
        let _config_guard = crate::app_dirs::ConfigBaseGuard::set(config_dir.path().to_path_buf());
        let source_dir = TempDir::new().expect("create source dir");
        let source = crate::sample_sources::SampleSource::new(source_dir.path().to_path_buf());
        crate::sample_sources::SourceDatabase::open_for_test_fixture_source_write(&source.root)
            .expect("seed source db");
        crate::sample_sources::library::save(&crate::sample_sources::library::LibraryState {
            sources: vec![source.clone()],
        })
        .expect("save source library");
        let _open_count_scope =
            crate::sample_sources::db::test_scope_source_db_open_total_count(&source.root);

        {
            let _guard = FileOpWritePriorityGuard::new(&source.id);
            let session = open_source_db_for_id(&source.id)
                .expect("Starmap file-op deferral should not be reported as a failure");

            assert!(matches!(session, StarmapWriteSession::DeferredForFileOp));
            assert_eq!(
                crate::sample_sources::db::test_source_db_open_total_count(&source.root),
                0,
                "Starmap writer must not open the source DB during file-op priority"
            );
        }

        let session = open_source_db_for_id(&source.id)
            .expect("Starmap writer should open after file-op priority clears");
        assert!(matches!(session, StarmapWriteSession::Ready(_)));
        assert_eq!(
            crate::sample_sources::db::test_source_db_open_total_count(&source.root),
            1
        );
    }
}
