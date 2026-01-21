use crate::egui_app::controller::library::analysis_jobs::db;

/// Rebuild the ANN index for the source database and clear any dirty marker.
pub(crate) fn run_rebuild_index_job(
    conn: &rusqlite::Connection,
    _job: &db::ClaimedJob,
) -> Result<(), String> {
    crate::analysis::rebuild_ann_index(conn)?;
    db::clear_ann_index_dirty(conn)?;
    Ok(())
}
