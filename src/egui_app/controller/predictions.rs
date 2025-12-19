use super::*;
use rusqlite::{Connection, OptionalExtension, params};

impl EguiController {
    pub(super) fn queue_prediction_load_for_selection(&mut self) {
        self.ui.waveform.predicted_category = None;
        let Some(source) = self.current_source() else {
            return;
        };
        let Some(relative_path) = self.sample_view.wav.selected_wav.clone() else {
            return;
        };
        let sample_id = format!("{}::{}", source.id.as_str(), relative_path.to_string_lossy());
        let preferred_model_id = self.classifier_model_id();
        let use_overrides = self.use_user_overrides_in_browser();
        let tx = self.runtime.jobs.message_sender();
        std::thread::spawn(move || {
            let db_path = match crate::app_dirs::app_root_dir() {
                Ok(dir) => dir.join(crate::sample_sources::library::LIBRARY_DB_FILE_NAME),
                Err(_) => return,
            };
            let conn = match open_db(&db_path) {
                Ok(conn) => conn,
                Err(_) => return,
            };
            let user_label: Option<String> = if use_overrides {
                conn.query_row(
                    "SELECT class_id FROM labels_user WHERE sample_id = ?1",
                    params![sample_id],
                    |row| row.get(0),
                )
                .optional()
                .ok()
                .flatten()
            } else {
                None
            };
            let (top_class, confidence) = if let Some(class_id) = user_label {
                (Some(class_id), Some(1.0))
            } else {
                let active_model_id = resolve_active_model_id(&conn, preferred_model_id);
                let row: Option<(String, f64)> = if let Some(model_id) = active_model_id.as_deref() {
                    conn.query_row(
                        "SELECT p.top_class, p.confidence
                         FROM predictions p
                         JOIN models m ON m.model_id = p.model_id
                         WHERE p.sample_id = ?1
                           AND p.model_id = ?2
                         ORDER BY m.created_at DESC, m.model_id DESC
                         LIMIT 1",
                        params![sample_id, model_id],
                        |row| Ok((row.get(0)?, row.get(1)?)),
                    )
                    .optional()
                    .ok()
                    .flatten()
                } else {
                    None
                };
                match row {
                    Some((class_id, confidence)) => (Some(class_id), Some(confidence as f32)),
                    None => (None, None),
                }
            };
            let _ = tx.send(super::jobs::JobMessage::Analysis(
                super::analysis_jobs::AnalysisJobMessage::PredictionLoaded {
                    sample_id,
                    top_class,
                    confidence,
                },
            ));
        });
    }
}

fn resolve_active_model_id(conn: &Connection, preferred: Option<String>) -> Option<String> {
    if let Some(model_id) = preferred {
        let exists: Option<String> = conn
            .query_row(
                "SELECT model_id FROM models WHERE model_id = ?1",
                params![&model_id],
                |row| row.get(0),
            )
            .optional()
            .ok()
            .flatten();
        if exists.is_some() {
            return Some(model_id);
        }
    }
    conn.query_row(
        "SELECT model_id
         FROM models
         ORDER BY created_at DESC, model_id DESC
         LIMIT 1",
        [],
        |row| row.get(0),
    )
    .optional()
    .ok()
    .flatten()
}

fn open_db(path: &std::path::Path) -> Result<Connection, rusqlite::Error> {
    let conn = Connection::open(path)?;
    conn.execute_batch(
        "PRAGMA journal_mode=WAL;
         PRAGMA synchronous = NORMAL;
         PRAGMA foreign_keys=ON;
         PRAGMA busy_timeout=5000;
         PRAGMA temp_store=MEMORY;
         PRAGMA cache_size=-64000;
         PRAGMA mmap_size=268435456;",
    )?;
    let _ = crate::sqlite_ext::try_load_optional_extension(&conn);
    Ok(conn)
}
