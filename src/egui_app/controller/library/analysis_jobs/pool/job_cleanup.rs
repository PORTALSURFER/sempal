use crate::egui_app::controller::library::analysis_jobs::db;

#[cfg_attr(test, allow(dead_code))]
pub(crate) fn reset_running_jobs() -> Result<(), String> {
    let state = crate::sample_sources::library::load().map_err(|err| err.to_string())?;
    for source in state.sources {
        if !source.root.is_dir() {
            continue;
        }
        let conn = db::open_source_db(&source.root)?;
        let _ = db::prune_jobs_for_missing_sources(&conn)?;
        let _ = db::reset_running_to_pending(&conn)?;
    }
    Ok(())
}
