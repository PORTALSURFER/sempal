use super::playback_type_tags::sanitize_playback_type_tags;
use super::types::{MetadataTagPersistRequest, MetadataTagPersistResult};
use crate::native_app::audio::playback::tagged_playback_mode_for_tag;
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
};
use wavecrate::sample_sources::{
    ExistingFileMetadataUpdate, SourceDatabase, SourceDbError, db::SourceWriteBatch,
};

pub(super) fn persist_metadata_tag_assignment(
    request: MetadataTagPersistRequest,
) -> MetadataTagPersistResult {
    let result = persist_metadata_tag_assignment_inner(&request);
    MetadataTagPersistResult {
        tags: request.tags,
        assigned: request.assigned,
        result,
    }
}

pub(super) fn persist_metadata_tag_assignments(
    requests: Vec<MetadataTagPersistRequest>,
) -> MetadataTagPersistResult {
    let tags = unique_request_tags(&requests);
    let result = requests
        .iter()
        .try_for_each(persist_metadata_tag_assignment_inner);
    MetadataTagPersistResult {
        tags,
        assigned: true,
        result,
    }
}

pub(super) fn persist_metadata_tag_deletions(
    requests: Vec<MetadataTagPersistRequest>,
) -> MetadataTagPersistResult {
    let tags = requests
        .first()
        .map(|request| request.tags.clone())
        .unwrap_or_default();
    let result = requests
        .iter()
        .try_for_each(persist_metadata_tag_assignment_inner);
    MetadataTagPersistResult {
        tags,
        assigned: false,
        result,
    }
}

fn unique_request_tags(requests: &[MetadataTagPersistRequest]) -> Vec<String> {
    let mut tags = Vec::new();
    for request in requests {
        for tag in &request.tags {
            if !tags.iter().any(|existing| existing == tag) {
                tags.push(tag.clone());
            }
        }
    }
    tags
}

fn persist_metadata_tag_assignment_inner(
    request: &MetadataTagPersistRequest,
) -> Result<(), String> {
    let db = SourceDatabase::open_for_user_metadata_write_with_database_root(
        &request.source_root,
        &request.source_database_root,
    )
    .map_err(|err| err.to_string())?;
    let mut batch = db.write_batch().map_err(|err| err.to_string())?;
    if matches!(
        batch
            .ensure_existing_live_file(&request.relative_path)
            .map_err(|err| err.to_string())?,
        ExistingFileMetadataUpdate::Missing
    ) {
        return Err(format!(
            "metadata tag persistence deferred until source row exists: {} ({})",
            request.relative_path.display(),
            request.absolute_path.display()
        ));
    }
    for tag in &request.tags {
        if request.assigned {
            remove_conflicting_persisted_playback_tags(&mut batch, &request.relative_path, tag)
                .map_err(|err| err.to_string())?;
            batch
                .assign_tag_to_path(&request.relative_path, tag)
                .map(|_| ())
        } else {
            batch
                .remove_tag_from_path(&request.relative_path, tag)
                .map(|_| ())
        }
        .map_err(|err| err.to_string())?;
    }
    batch
        .commit_auxiliary_state()
        .map_err(|err| err.to_string())
}

fn remove_conflicting_persisted_playback_tags(
    batch: &mut SourceWriteBatch<'_>,
    relative_path: &Path,
    incoming: &str,
) -> Result<(), SourceDbError> {
    let Some(incoming_mode) = tagged_playback_mode_for_tag(incoming) else {
        return Ok(());
    };
    let existing_tags = batch.tag_labels_for_path(relative_path)?;
    for existing in existing_tags {
        if tagged_playback_mode_for_tag(&existing)
            .is_some_and(|existing_mode| existing_mode != incoming_mode)
        {
            batch.remove_tag_from_path(relative_path, &existing)?;
        }
    }
    Ok(())
}

#[cfg(test)]
pub(in crate::native_app) fn persist_metadata_tag_additions_for_tests(
    absolute_path: PathBuf,
    source_root: PathBuf,
    relative_path: PathBuf,
    tags: Vec<String>,
) -> Result<(), String> {
    persist_metadata_tag_assignment_inner(&MetadataTagPersistRequest {
        absolute_path,
        source_database_root: source_root.clone(),
        source_root,
        relative_path,
        tags,
        assigned: true,
    })
}

#[cfg(test)]
pub(in crate::native_app) fn persist_metadata_tag_removals_for_tests(
    absolute_path: PathBuf,
    source_root: PathBuf,
    relative_path: PathBuf,
    tags: Vec<String>,
) -> Result<(), String> {
    persist_metadata_tag_assignment_inner(&MetadataTagPersistRequest {
        absolute_path,
        source_database_root: source_root.clone(),
        source_root,
        relative_path,
        tags,
        assigned: false,
    })
}

#[cfg(test)]
pub(super) fn load_persisted_metadata_tags_for_source(
    source_root: &Path,
    source_database_root: &Path,
    tags_by_file: &mut HashMap<String, Vec<String>>,
) -> Result<(), String> {
    tags_by_file.extend(load_persisted_metadata_tag_map_for_source(
        source_root,
        source_database_root,
    )?);
    Ok(())
}

pub(super) fn load_persisted_metadata_tag_map_for_source(
    source_root: &Path,
    source_database_root: &Path,
) -> Result<HashMap<String, Vec<String>>, String> {
    let db = match SourceDatabase::open_for_ui_read_with_database_root(
        source_root,
        source_database_root,
    ) {
        Ok(db) => db,
        Err(SourceDbError::ReadOnlyDatabaseMissing(_)) => return Ok(HashMap::new()),
        Err(err) => return Err(err.to_string()),
    };
    let mut tags_by_file = HashMap::new();
    let mut repairs = Vec::new();
    for entry in db.list_files().map_err(|err| err.to_string())? {
        let mut normal_tags = entry.normal_tags;
        if normal_tags.is_empty() {
            continue;
        }
        if sanitize_playback_type_tags(&mut normal_tags) {
            repairs.push(PersistedMetadataTagRepair {
                relative_path: entry.relative_path.clone(),
                tags: normal_tags.clone(),
            });
        }
        let absolute_path = source_root.join(entry.relative_path);
        tags_by_file.insert(absolute_path.to_string_lossy().to_string(), normal_tags);
    }
    if let Err(err) =
        repair_persisted_metadata_tag_conflicts(source_root, source_database_root, repairs)
    {
        tracing::warn!(
            "Failed to repair persisted playback-type tag conflicts for {}: {err}",
            source_root.display()
        );
    }
    Ok(tags_by_file)
}

struct PersistedMetadataTagRepair {
    relative_path: PathBuf,
    tags: Vec<String>,
}

fn repair_persisted_metadata_tag_conflicts(
    source_root: &Path,
    source_database_root: &Path,
    repairs: Vec<PersistedMetadataTagRepair>,
) -> Result<(), String> {
    if repairs.is_empty() {
        return Ok(());
    }
    let db = SourceDatabase::open_for_user_metadata_write_with_database_root(
        source_root,
        source_database_root,
    )
    .map_err(|err| err.to_string())?;
    let mut batch = db.write_batch().map_err(|err| err.to_string())?;
    for repair in repairs {
        batch
            .replace_tags_for_path(&repair.relative_path, &repair.tags)
            .map_err(|err| err.to_string())?;
    }
    batch
        .commit_auxiliary_state()
        .map_err(|err| err.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::native_app::sample_library::sample_ratings::{
        RatingPersistRequest, persist_rating_requests,
    };
    use wavecrate::sample_sources::{Rating, SourceDatabase};

    #[test]
    fn extracted_metadata_retry_after_authoritative_row_persists_rating_and_playback_tag() {
        let root = tempfile::tempdir().expect("source root");
        let relative_path = PathBuf::from("extracted.wav");
        let absolute_path = root.path().join(&relative_path);
        std::fs::write(&absolute_path, b"extracted").expect("extracted file");

        let rating_request = RatingPersistRequest {
            source_id: String::from("source"),
            lifecycle_generation: None,
            root: root.path().to_path_buf(),
            database_root: root.path().to_path_buf(),
            relative_path: relative_path.clone(),
            absolute_path: absolute_path.clone(),
            rating: Rating::KEEP_1,
            locked: false,
        };
        assert!(
            persist_rating_requests(std::slice::from_ref(&rating_request), |_| true)[0]
                .as_ref()
                .expect("missing-row result")
                .is_err()
        );
        assert!(
            persist_metadata_tag_additions_for_tests(
                absolute_path.clone(),
                root.path().to_path_buf(),
                relative_path.clone(),
                vec![String::from("loop")],
            )
            .is_err()
        );

        let database = SourceDatabase::open_for_user_metadata_write_with_database_root(
            root.path(),
            root.path(),
        )
        .expect("source database");
        let metadata = std::fs::metadata(&absolute_path).expect("file metadata");
        let mut batch = database.write_batch().expect("write batch");
        batch
            .upsert_file(
                &relative_path,
                metadata.len(),
                metadata
                    .modified()
                    .expect("modified")
                    .duration_since(std::time::UNIX_EPOCH)
                    .expect("epoch")
                    .as_nanos() as i64,
            )
            .expect("authoritative row");
        batch.commit().expect("commit source row");

        assert!(
            persist_metadata_tag_additions_for_tests(
                absolute_path.clone(),
                root.path().to_path_buf(),
                relative_path.clone(),
                vec![String::from("loop")],
            )
            .is_ok()
        );
        assert_eq!(
            persist_rating_requests(std::slice::from_ref(&rating_request), |_| true)[0],
            Some(Ok(()))
        );
        assert_eq!(
            database.tag_for_path(&relative_path).expect("rating"),
            Some(Rating::KEEP_1)
        );
        assert_eq!(
            database.tag_labels_for_path(&relative_path).expect("tags"),
            vec![String::from("loop")]
        );
    }
}
