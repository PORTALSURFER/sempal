use rusqlite::Connection;
use std::path::Path;

pub(in crate::egui_app::controller) fn open_library_db(
    db_path: &Path,
) -> Result<Connection, String> {
    let conn = Connection::open(db_path).map_err(|err| format!("Open library DB failed: {err}"))?;
    conn.execute_batch(
        "PRAGMA journal_mode=WAL;
         PRAGMA synchronous = NORMAL;
         PRAGMA foreign_keys=ON;
         PRAGMA busy_timeout=5000;
         PRAGMA temp_store=MEMORY;
         PRAGMA cache_size=-64000;
         PRAGMA mmap_size=268435456;",
    )
    .map_err(|err| format!("Failed to set library DB pragmas: {err}"))?;
    if let Err(err) = crate::sqlite_ext::try_load_optional_extension(&conn) {
        tracing::debug!("SQLite extension not loaded: {err}");
    }
    Ok(conn)
}
