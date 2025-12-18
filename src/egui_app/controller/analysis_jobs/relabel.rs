use super::db;
use super::open_library_db;
use super::weak_labels::{SampleWeakLabels, WeakLabelInsert};
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Clone, Copy, Debug, Default)]
pub(in crate::egui_app::controller) struct RelabelOutcome {
    pub(in crate::egui_app::controller) processed: usize,
    pub(in crate::egui_app::controller) skipped: usize,
}

pub(in crate::egui_app::controller) fn recompute_weak_labels_for_source(
    source: &crate::sample_sources::SampleSource,
) -> Result<RelabelOutcome, String> {
    recompute_weak_labels_for_sources(std::slice::from_ref(source))
}

pub(in crate::egui_app::controller) fn recompute_weak_labels_for_sources(
    sources: &[crate::sample_sources::SampleSource],
) -> Result<RelabelOutcome, String> {
    let db_path = crate::app_dirs::app_root_dir()
        .map_err(|err| err.to_string())?
        .join(crate::sample_sources::library::LIBRARY_DB_FILE_NAME);
    let mut conn = open_library_db(&db_path)?;

    let mut outcome = RelabelOutcome::default();
    for source in sources {
        let per_source = recompute_weak_labels_for_source_with_conn(&mut conn, source)?;
        outcome.processed += per_source.processed;
        outcome.skipped += per_source.skipped;
    }
    Ok(outcome)
}

fn recompute_weak_labels_for_source_with_conn(
    conn: &mut rusqlite::Connection,
    source: &crate::sample_sources::SampleSource,
) -> Result<RelabelOutcome, String> {
    let source_db =
        crate::sample_sources::SourceDatabase::open(&source.root).map_err(|err| err.to_string())?;
    let mut entries = source_db.list_files().map_err(|err| err.to_string())?;
    entries.retain(|entry| !entry.missing);
    if entries.is_empty() {
        return Ok(RelabelOutcome::default());
    }

    let mut sample_metadata: Vec<db::SampleMetadata> = Vec::with_capacity(entries.len());
    let mut weak_labels: Vec<SampleWeakLabels> = Vec::with_capacity(entries.len());
    let mut skipped = 0usize;
    for entry in entries {
        let sample_id = db::build_sample_id(source.id.as_str(), &entry.relative_path);
        let content_hash = match entry.content_hash {
            Some(hash) if !hash.trim().is_empty() => hash,
            _ => {
                let absolute = source.root.join(&entry.relative_path);
                let Ok(hash) = compute_content_hash(&absolute) else {
                    skipped += 1;
                    continue;
                };
                hash
            }
        };

        sample_metadata.push(db::SampleMetadata {
            sample_id: sample_id.clone(),
            content_hash: content_hash.clone(),
            size: entry.file_size,
            mtime_ns: entry.modified_ns,
        });

        let labels = crate::labeling::weak::weak_labels_for_relative_path(&entry.relative_path)
            .into_iter()
            .map(|label| WeakLabelInsert {
                class_id: label.class_id.to_string(),
                confidence: label.confidence,
                rule_id: label.rule_id.to_string(),
            })
            .collect();
        weak_labels.push(SampleWeakLabels { sample_id, labels });
    }

    db::upsert_samples(conn, &sample_metadata)?;

    let created_at = now_epoch_seconds();
    super::weak_labels::replace_weak_labels_for_samples(
        conn,
        &weak_labels,
        crate::labeling::weak::WEAK_LABEL_RULESET_VERSION,
        created_at,
    )?;

    Ok(RelabelOutcome {
        processed: weak_labels.len(),
        skipped,
    })
}

fn now_epoch_seconds() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

fn compute_content_hash(path: &std::path::Path) -> Result<String, String> {
    use std::io::Read;
    let mut file = std::fs::File::open(path)
        .map_err(|err| format!("Open for hashing failed ({}): {err}", path.display()))?;
    let mut hasher = blake3::Hasher::new();
    let mut buffer = [0u8; 64 * 1024];
    loop {
        let read = file
            .read(&mut buffer)
            .map_err(|err| format!("Read for hashing failed ({}): {err}", path.display()))?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(hasher.finalize().to_hex().to_string())
}
