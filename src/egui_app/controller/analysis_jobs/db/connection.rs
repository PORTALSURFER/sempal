use rusqlite::Connection;
use std::path::Path;

pub(in crate::egui_app::controller) fn open_source_db(
    source_root: &Path,
) -> Result<Connection, String> {
    crate::sample_sources::SourceDatabase::open_connection(source_root)
        .map_err(|err| format!("Open source DB failed: {err}"))
}
