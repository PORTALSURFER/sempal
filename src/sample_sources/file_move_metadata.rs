use std::path::{Path, PathBuf};

use crate::sample_sources::{SampleCollection, SourceDatabase, WavEntry, db::SourceWriteBatch};
use wavecrate_library::timestamps::system_time_to_unix_nanos;

/// Metadata remap input for a file move that may cross configured sources.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SourcedFileMoveMetadata {
    /// Source folder that owned the moved file before the move.
    pub source_root: PathBuf,
    /// Folder that stores metadata for the source.
    pub source_database_root: PathBuf,
    /// Absolute path before the move.
    pub old_path: PathBuf,
    /// Absolute path after the move.
    pub new_path: PathBuf,
    /// Whether the old source row should be removed after writing target metadata.
    pub remove_source: bool,
}

/// Preserve Wavecrate metadata after moving files within or across sources.
pub fn persist_sourced_moved_file_metadata(
    target_source_root: &Path,
    target_database_root: &Path,
    moves: &[SourcedFileMoveMetadata],
    remove_from_collection: Option<SampleCollection>,
) -> Result<(), String> {
    let mut errors = Vec::new();
    let same_source_moves = moves
        .iter()
        .filter(|move_item| move_item.source_root == target_source_root && move_item.remove_source)
        .map(|move_item| (move_item.old_path.clone(), move_item.new_path.clone()))
        .collect::<Vec<_>>();
    if !same_source_moves.is_empty()
        && let Err(error) = persist_same_source_moved_file_metadata(
            target_source_root,
            target_database_root,
            &same_source_moves,
            remove_from_collection,
        )
    {
        errors.push(error);
    }

    for move_item in moves
        .iter()
        .filter(|move_item| move_item.source_root != target_source_root)
    {
        if let Err(error) = persist_cross_source_moved_file_metadata(
            &move_item.source_root,
            &move_item.source_database_root,
            target_source_root,
            target_database_root,
            &move_item.old_path,
            &move_item.new_path,
            move_item.remove_source,
            remove_from_collection,
        ) {
            errors.push(error);
        }
    }

    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors.join("; "))
    }
}

/// Preserve Wavecrate metadata after copying one file inside a configured source.
pub fn persist_copied_file_metadata(
    source_root: &Path,
    database_root: &Path,
    old_path: &Path,
    new_path: &Path,
) -> Result<(), String> {
    let old_relative = old_path
        .strip_prefix(source_root)
        .map_err(|_| String::from("File copy metadata update failed: source file mismatch"))?;
    let new_relative = new_path
        .strip_prefix(source_root)
        .map_err(|_| String::from("File copy metadata update failed: target file mismatch"))?;
    let db =
        SourceDatabase::open_for_user_metadata_write_with_database_root(source_root, database_root)
            .map_err(|err| format!("File copy metadata update failed: {err}"))?;
    let metadata = moved_file_metadata(&db, old_relative)?;
    write_cross_source_target_metadata(&db, new_relative, new_path, metadata.as_ref(), None)
}

fn persist_same_source_moved_file_metadata(
    source_root: &Path,
    database_root: &Path,
    moves: &[(PathBuf, PathBuf)],
    remove_from_collection: Option<SampleCollection>,
) -> Result<(), String> {
    let remaps = moves
        .iter()
        .filter_map(|(old_path, new_path)| {
            let old_relative = old_path.strip_prefix(source_root).ok()?.to_path_buf();
            let new_relative = new_path.strip_prefix(source_root).ok()?.to_path_buf();
            Some((old_relative, new_relative))
        })
        .collect::<Vec<_>>();
    if remaps.is_empty() {
        return Ok(());
    }

    let db =
        SourceDatabase::open_for_user_metadata_write_with_database_root(source_root, database_root)
            .map_err(|err| format!("File move metadata update failed: {err}"))?;
    let mut batch = db
        .write_batch()
        .map_err(|err| format!("File move metadata update failed: {err}"))?;
    for (old_relative, new_relative) in remaps {
        batch
            .remap_wav_file_path(&old_relative, &new_relative)
            .map_err(|err| format!("File move metadata update failed: {err}"))?;
        batch
            .remap_analysis_sample_identity(&old_relative, &new_relative)
            .map_err(|err| format!("File move metadata update failed: {err}"))?;
        if let Some(collection) = remove_from_collection {
            batch
                .remove_collection(&new_relative, collection)
                .map_err(|err| format!("File move metadata update failed: {err}"))?;
        }
    }
    batch
        .commit()
        .map_err(|err| format!("File move metadata update failed: {err}"))
}

fn persist_cross_source_moved_file_metadata(
    source_root: &Path,
    source_database_root: &Path,
    target_source_root: &Path,
    target_database_root: &Path,
    old_path: &Path,
    new_path: &Path,
    remove_source: bool,
    remove_from_collection: Option<SampleCollection>,
) -> Result<(), String> {
    let old_relative = old_path
        .strip_prefix(source_root)
        .map_err(|_| String::from("File move metadata update failed: source file mismatch"))?;
    let new_relative = new_path
        .strip_prefix(target_source_root)
        .map_err(|_| String::from("File move metadata update failed: target file mismatch"))?;
    let source_db = SourceDatabase::open_for_user_metadata_write_with_database_root(
        source_root,
        source_database_root,
    )
    .map_err(|err| format!("File move metadata update failed: {err}"))?;
    let target_db = SourceDatabase::open_for_user_metadata_write_with_database_root(
        target_source_root,
        target_database_root,
    )
    .map_err(|err| format!("File move metadata update failed: {err}"))?;
    let metadata = moved_file_metadata(&source_db, old_relative)?;
    write_cross_source_target_metadata(
        &target_db,
        new_relative,
        new_path,
        metadata.as_ref(),
        remove_from_collection,
    )?;

    if !remove_source {
        return Ok(());
    }
    let mut source_batch = source_db
        .write_batch()
        .map_err(|err| format!("File move metadata update failed: {err}"))?;
    source_batch
        .remove_file(old_relative)
        .map_err(|err| format!("File move metadata update failed: {err}"))?;
    source_batch
        .commit()
        .map_err(|err| format!("File move metadata update failed: {err}"))
}

struct MovedFileMetadata {
    entry: WavEntry,
    normal_tags: Vec<String>,
    collections: Vec<SampleCollection>,
}

fn moved_file_metadata(
    db: &SourceDatabase,
    relative_path: &Path,
) -> Result<Option<MovedFileMetadata>, String> {
    let Some(entry) = db
        .entry_for_path(relative_path)
        .map_err(|err| format!("File move metadata update failed: {err}"))?
    else {
        return Ok(None);
    };
    let normal_tags = db
        .tag_labels_for_path(relative_path)
        .map_err(|err| format!("File move metadata update failed: {err}"))?;
    let collections = db
        .collections_for_path(relative_path)
        .map_err(|err| format!("File move metadata update failed: {err}"))?;
    Ok(Some(MovedFileMetadata {
        entry,
        normal_tags,
        collections,
    }))
}

fn write_cross_source_target_metadata(
    db: &SourceDatabase,
    relative_path: &Path,
    absolute_path: &Path,
    metadata: Option<&MovedFileMetadata>,
    remove_from_collection: Option<SampleCollection>,
) -> Result<(), String> {
    let (file_size, modified_ns) = file_metadata(absolute_path)?;
    let mut batch = db
        .write_batch()
        .map_err(|err| format!("File move metadata update failed: {err}"))?;
    match metadata.and_then(|metadata| metadata.entry.content_hash.as_deref()) {
        Some(content_hash) => batch
            .upsert_file_with_hash(relative_path, file_size, modified_ns, content_hash)
            .map_err(|err| format!("File move metadata update failed: {err}"))?,
        None => batch
            .upsert_file_without_hash(relative_path, file_size, modified_ns)
            .map_err(|err| format!("File move metadata update failed: {err}"))?,
    }
    if let Some(metadata) = metadata {
        write_cross_source_user_metadata(
            &mut batch,
            relative_path,
            metadata,
            remove_from_collection,
        )?;
    }
    batch
        .commit()
        .map_err(|err| format!("File move metadata update failed: {err}"))
}

fn write_cross_source_user_metadata(
    batch: &mut SourceWriteBatch<'_>,
    relative_path: &Path,
    metadata: &MovedFileMetadata,
    remove_from_collection: Option<SampleCollection>,
) -> Result<(), String> {
    batch
        .set_tag(relative_path, metadata.entry.tag)
        .map_err(|err| format!("File move metadata update failed: {err}"))?;
    batch
        .set_looped(relative_path, metadata.entry.looped)
        .map_err(|err| format!("File move metadata update failed: {err}"))?;
    batch
        .set_locked(relative_path, metadata.entry.locked)
        .map_err(|err| format!("File move metadata update failed: {err}"))?;
    batch
        .set_sound_type(relative_path, metadata.entry.sound_type)
        .map_err(|err| format!("File move metadata update failed: {err}"))?;
    batch
        .set_user_tag(relative_path, metadata.entry.user_tag.as_deref())
        .map_err(|err| format!("File move metadata update failed: {err}"))?;
    batch
        .set_tag_named(relative_path, metadata.entry.tag_named)
        .map_err(|err| format!("File move metadata update failed: {err}"))?;
    match metadata.entry.last_played_at {
        Some(last_played_at) => batch
            .set_last_played_at(relative_path, last_played_at)
            .map_err(|err| format!("File move metadata update failed: {err}"))?,
        None => batch
            .clear_last_played_at(relative_path)
            .map_err(|err| format!("File move metadata update failed: {err}"))?,
    }
    batch
        .replace_tags_for_path(relative_path, &metadata.normal_tags)
        .map_err(|err| format!("File move metadata update failed: {err}"))?;
    batch
        .set_collection(relative_path, None)
        .map_err(|err| format!("File move metadata update failed: {err}"))?;
    for collection in &metadata.collections {
        batch
            .add_collection(relative_path, *collection)
            .map_err(|err| format!("File move metadata update failed: {err}"))?;
    }
    if let Some(collection) = remove_from_collection {
        batch
            .remove_collection(relative_path, collection)
            .map_err(|err| format!("File move metadata update failed: {err}"))?;
    }
    match metadata.entry.last_curated_at {
        Some(last_curated_at) => batch
            .set_last_curated_at(relative_path, last_curated_at)
            .map_err(|err| format!("File move metadata update failed: {err}"))?,
        None => batch
            .clear_last_curated_at(relative_path)
            .map_err(|err| format!("File move metadata update failed: {err}"))?,
    }
    Ok(())
}

fn file_metadata(path: &Path) -> Result<(u64, i64), String> {
    let metadata = std::fs::metadata(path)
        .map_err(|err| format!("File move metadata update failed: {err}"))?;
    let modified_ns = system_time_to_unix_nanos(
        metadata
            .modified()
            .map_err(|err| format!("File move metadata update failed: {err}"))?,
    );
    Ok((metadata.len(), modified_ns))
}
