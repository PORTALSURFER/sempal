use super::playback_type_tags::sanitize_playback_type_tags;
use super::types::{MetadataTagPersistRequest, MetadataTagPersistResult};
use crate::native_app::audio::playback::tagged_playback_mode_for_tag;
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
};
use wavecrate::sample_sources::{SourceDatabase, SourceDbError, db::SourceWriteBatch};
use wavecrate_library::timestamps::system_time_to_unix_nanos;

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
    let (file_size, modified_ns) = file_metadata(&request.absolute_path)?;
    let db = SourceDatabase::open_for_user_metadata_write_with_database_root(
        &request.source_root,
        &request.source_database_root,
    )
    .map_err(|err| err.to_string())?;
    let mut batch = db.write_batch().map_err(|err| err.to_string())?;
    batch
        .upsert_file(&request.relative_path, file_size, modified_ns)
        .map_err(|err| err.to_string())?;
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
    batch.commit().map_err(|err| err.to_string())
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
    batch.commit().map_err(|err| err.to_string())
}

fn file_metadata(path: &Path) -> Result<(u64, i64), String> {
    let metadata = std::fs::metadata(path)
        .map_err(|err| format!("Sample metadata unavailable for {}: {err}", path.display()))?;
    let modified_ns = metadata
        .modified()
        .map(system_time_to_unix_nanos)
        .unwrap_or_default();
    Ok((metadata.len(), modified_ns))
}
