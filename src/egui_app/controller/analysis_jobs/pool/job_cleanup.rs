use super::db;

#[cfg_attr(test, allow(dead_code))]
pub(super) fn reset_running_jobs() -> Result<(), String> {
    let db_path = super::library_db_path()?;
    let conn = db::open_library_db(&db_path)?;
    let _ = db::prune_jobs_for_missing_sources(&conn)?;
    let _ = db::reset_running_to_pending(&conn)?;
    Ok(())
}
