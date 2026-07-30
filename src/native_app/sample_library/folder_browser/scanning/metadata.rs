use std::{
    collections::HashMap,
    fmt,
    path::{Path, PathBuf},
};

use super::super::FileEntry;
use super::{
    entry::{
        BrowserEntryKind, classify_path_without_following_with_policy, source_traversal_policy,
    },
    file_entry_metadata::file_entry_with_metadata,
};
use wavecrate::sample_sources::{
    BrowserMetadataSnapshot, Rating, SampleCollection, SourceDatabase, SourceDatabaseConnectionRole,
};

pub(in crate::native_app::sample_library::folder_browser) type SourceMetadataMap = HashMap<
    PathBuf,
    (
        Rating,
        bool,
        Vec<SampleCollection>,
        Option<i64>,
        Option<i64>,
    ),
>;

pub(crate) type RatingSnapshot = HashMap<PathBuf, (Rating, bool)>;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum SourceMetadataHydrationError {
    Open(String),
    Snapshot(String),
}

impl fmt::Display for SourceMetadataHydrationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Open(error) => write!(formatter, "open source metadata: {error}"),
            Self::Snapshot(error) => write!(formatter, "read browser metadata snapshot: {error}"),
        }
    }
}

pub(super) fn source_rating_map(
    root: &Path,
    database_root: &Path,
) -> Result<(SourceMetadataMap, u64), SourceMetadataHydrationError> {
    let db = SourceDatabase::open_for_ui_read_with_database_root(root, database_root)
        .map_err(|error| SourceMetadataHydrationError::Open(error.to_string()))?;
    let BrowserMetadataSnapshot { revision, files } = db
        .browser_metadata_snapshot()
        .map_err(|error| SourceMetadataHydrationError::Snapshot(error.to_string()))?;
    let metadata = files
        .into_iter()
        .map(|entry| {
            (
                entry.relative_path,
                (
                    entry.rating,
                    entry.locked,
                    entry.collections,
                    entry.last_played_at,
                    entry.last_curated_at,
                ),
            )
        })
        .collect();
    Ok((metadata, revision))
}

pub(super) fn source_rating_snapshot(
    root: &Path,
    database_root: &Path,
) -> Result<RatingSnapshot, SourceMetadataHydrationError> {
    let db = SourceDatabase::open_with_role_and_database_root(
        root,
        database_root,
        SourceDatabaseConnectionRole::BackgroundRead,
    )
    .map_err(|error| SourceMetadataHydrationError::Open(error.to_string()))?;
    let BrowserMetadataSnapshot { files, .. } = db
        .browser_metadata_snapshot()
        .map_err(|error| SourceMetadataHydrationError::Snapshot(error.to_string()))?;
    let ratings = files
        .into_iter()
        .map(|entry| (entry.relative_path, (entry.rating, entry.locked)))
        .collect();
    Ok(ratings)
}

pub(in crate::native_app::sample_library::folder_browser) fn file_entry_for_source_path(
    path: &PathBuf,
    source_root: &Path,
    source_database_root: &Path,
) -> FileEntry {
    let metadata = source_file_metadata(source_root, source_database_root, path).unwrap_or((
        Rating::NEUTRAL,
        false,
        Vec::new(),
        None,
        None,
    ));
    file_entry_with_metadata(
        path, metadata.0, metadata.1, metadata.2, metadata.3, metadata.4,
    )
}

pub(in crate::native_app::sample_library::folder_browser) fn refreshed_file_entries_for_paths(
    paths: &[PathBuf],
    source_root: &Path,
    source_database_root: &Path,
) -> Vec<FileEntry> {
    let ratings = source_rating_map(source_root, source_database_root)
        .map(|(ratings, _)| ratings)
        .unwrap_or_else(|error| {
            tracing::warn!(source = %source_root.display(), "{error}");
            SourceMetadataMap::new()
        });
    let policy = source_traversal_policy(source_root, source_database_root);
    paths
        .iter()
        .filter(|path| {
            classify_path_without_following_with_policy(path, policy)
                == Some(BrowserEntryKind::File)
        })
        .map(|path| rated_file_entry(path, source_root, &ratings))
        .collect()
}

pub(super) fn rated_file_entry(
    path: &PathBuf,
    source_root: &Path,
    ratings: &SourceMetadataMap,
) -> FileEntry {
    let (rating, locked, collections, last_played_at, last_curated_at) = path
        .strip_prefix(source_root)
        .ok()
        .and_then(|relative| ratings.get(relative).cloned())
        .unwrap_or((Rating::NEUTRAL, false, Vec::new(), None, None));
    file_entry_with_metadata(
        path,
        rating,
        locked,
        collections,
        last_played_at,
        last_curated_at,
    )
}

fn source_file_metadata(
    source_root: &Path,
    source_database_root: &Path,
    path: &Path,
) -> Option<(
    Rating,
    bool,
    Vec<SampleCollection>,
    Option<i64>,
    Option<i64>,
)> {
    let relative = path.strip_prefix(source_root).ok()?;
    let db = SourceDatabase::open_for_ui_read_with_database_root(source_root, source_database_root)
        .ok()?;
    let entry = db.entry_for_path(relative).ok()??;
    let collections = db.collections_for_path(relative).unwrap_or_default();
    Some((
        entry.tag,
        entry.locked,
        collections,
        entry.last_played_at,
        entry.last_curated_at,
    ))
}
