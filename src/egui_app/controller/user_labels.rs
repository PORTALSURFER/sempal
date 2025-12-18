use super::*;
use rusqlite::{Connection, TransactionBehavior, params};
use std::collections::HashSet;
use std::path::Path;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

impl EguiController {
    pub fn set_user_category_override_for_visible_rows(
        &mut self,
        visible_rows: &[usize],
        class_id: Option<&str>,
    ) -> Result<(), String> {
        let source_id = selected_source_id(self)?;
        let sample_ids = sample_ids_for_visible_rows(self, &source_id, visible_rows);
        if sample_ids.is_empty() {
            return Ok(());
        }
        write_user_labels(&sample_ids, class_id)?;

        self.ui_cache.browser.predictions.remove(&source_id);
        self.queue_prediction_load_for_selection();
        self.rebuild_browser_lists();
        Ok(())
    }
}

fn selected_source_id(controller: &EguiController) -> Result<SourceId, String> {
    controller
        .selection_state
        .ctx
        .selected_source
        .clone()
        .ok_or_else(|| "No source selected".to_string())
}

fn sample_ids_for_visible_rows(
    controller: &EguiController,
    source_id: &SourceId,
    visible_rows: &[usize],
) -> HashSet<String> {
    let mut sample_ids = HashSet::new();
    for &visible_row in visible_rows {
        let Some(entry_index) = controller.visible_browser_indices().get(visible_row).copied() else {
            continue;
        };
        let Some(entry) = controller.wav_entry(entry_index) else {
            continue;
        };
        sample_ids.insert(build_sample_id(source_id.as_str(), &entry.relative_path));
    }
    sample_ids
}

fn write_user_labels(sample_ids: &HashSet<String>, class_id: Option<&str>) -> Result<(), String> {
    let db_path = crate::app_dirs::app_root_dir()
        .map_err(|err| err.to_string())?
        .join(crate::sample_sources::library::LIBRARY_DB_FILE_NAME);
    let mut conn = open_library_db(&db_path)?;
    let now = now_epoch_seconds();
    let tx = conn
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(|err| format!("Start labels_user transaction failed: {err}"))?;
    if let Some(class_id) = class_id {
        upsert_user_labels(&tx, sample_ids, class_id, now)?;
    } else {
        clear_user_labels(&tx, sample_ids)?;
    }
    tx.commit()
        .map_err(|err| format!("Commit labels_user transaction failed: {err}"))?;
    Ok(())
}

fn upsert_user_labels(
    tx: &rusqlite::Transaction<'_>,
    sample_ids: &HashSet<String>,
    class_id: &str,
    now_ms: i64,
) -> Result<(), String> {
    let mut stmt = tx
        .prepare(
            "INSERT INTO labels_user (sample_id, class_id, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?3)
             ON CONFLICT(sample_id) DO UPDATE SET
                class_id = excluded.class_id,
                updated_at = excluded.updated_at",
        )
        .map_err(|err| format!("Prepare labels_user upsert failed: {err}"))?;
    for sample_id in sample_ids {
        stmt.execute(params![sample_id, class_id, now_ms])
            .map_err(|err| format!("Upsert labels_user failed: {err}"))?;
    }
    Ok(())
}

fn clear_user_labels(
    tx: &rusqlite::Transaction<'_>,
    sample_ids: &HashSet<String>,
) -> Result<(), String> {
    let mut stmt = tx
        .prepare("DELETE FROM labels_user WHERE sample_id = ?1")
        .map_err(|err| format!("Prepare labels_user delete failed: {err}"))?;
    for sample_id in sample_ids {
        stmt.execute(params![sample_id])
            .map_err(|err| format!("Delete labels_user failed: {err}"))?;
    }
    Ok(())
}

fn open_library_db(path: &Path) -> Result<Connection, String> {
    let conn = Connection::open(path).map_err(|err| format!("Open library DB failed: {err}"))?;
    conn.execute_batch(
        "PRAGMA journal_mode=WAL;
         PRAGMA synchronous = NORMAL;
         PRAGMA foreign_keys=ON;",
    )
    .map_err(|err| format!("Failed to set library DB pragmas: {err}"))?;
    Ok(conn)
}

fn build_sample_id(source_id: &str, relative_path: &Path) -> String {
    format!("{}::{}", source_id, relative_path.to_string_lossy())
}

fn now_epoch_seconds() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_else(|_| Duration::from_secs(0))
        .as_secs() as i64
}
