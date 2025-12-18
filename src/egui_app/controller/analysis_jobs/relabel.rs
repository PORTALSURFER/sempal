use super::open_library_db;
use super::weak_labels::{SampleWeakLabels, WeakLabelInsert};
use rusqlite::params;
use std::time::{SystemTime, UNIX_EPOCH};

pub(in crate::egui_app::controller) fn recompute_weak_labels_for_source(
    source_id: &crate::sample_sources::SourceId,
) -> Result<usize, String> {
    recompute_weak_labels_for_sources(std::slice::from_ref(source_id))
}

pub(in crate::egui_app::controller) fn recompute_weak_labels_for_sources(
    source_ids: &[crate::sample_sources::SourceId],
) -> Result<usize, String> {
    let db_path = crate::app_dirs::app_root_dir()
        .map_err(|err| err.to_string())?
        .join(crate::sample_sources::library::LIBRARY_DB_FILE_NAME);
    let mut conn = open_library_db(&db_path)?;

    let mut total_updated = 0usize;
    for source_id in source_ids {
        total_updated += recompute_weak_labels_for_source_with_conn(&mut conn, source_id)?;
    }
    Ok(total_updated)
}

fn recompute_weak_labels_for_source_with_conn(
    conn: &mut rusqlite::Connection,
    source_id: &crate::sample_sources::SourceId,
) -> Result<usize, String> {
    let prefix = format!("{}::", source_id.as_str());
    let prefix_end = format!("{prefix}\u{10FFFF}");

    let sample_ids: Vec<String> = {
        let mut stmt = conn
            .prepare("SELECT sample_id FROM samples WHERE sample_id >= ?1 AND sample_id < ?2")
            .map_err(|err| format!("Prepare sample_id query failed: {err}"))?;
        let mut rows = stmt
            .query(params![prefix, prefix_end])
            .map_err(|err| format!("Query sample_ids failed: {err}"))?;
        let mut sample_ids = Vec::new();
        while let Some(row) = rows
            .next()
            .map_err(|err| format!("Query sample_ids failed: {err}"))?
        {
            let sample_id: String = row.get(0).map_err(|err| err.to_string())?;
            sample_ids.push(sample_id);
        }
        sample_ids
    };

    let mut samples: Vec<SampleWeakLabels> = Vec::with_capacity(sample_ids.len());
    for sample_id in sample_ids {
        let Some((_, relative_path)) = sample_id.split_once("::") else {
            continue;
        };
        let relative_path = std::path::PathBuf::from(relative_path);
        let labels = crate::labeling::weak::weak_labels_for_relative_path(&relative_path)
            .into_iter()
            .map(|label| WeakLabelInsert {
                class_id: label.class_id.to_string(),
                confidence: label.confidence,
                rule_id: label.rule_id.to_string(),
            })
            .collect();
        samples.push(SampleWeakLabels { sample_id, labels });
    }

    let created_at = now_epoch_seconds();
    super::weak_labels::replace_weak_labels_for_samples(
        conn,
        &samples,
        crate::labeling::weak::WEAK_LABEL_RULESET_VERSION,
        created_at,
    )?;

    Ok(samples.len())
}

fn now_epoch_seconds() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}
