use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

use wavecrate::sample_sources::{ExistingFileMetadataUpdate, SourceDatabase};

use crate::native_app::sample_library::folder_browser::view_contract::MissingCollectionFile;

use super::command::{CollectionOperation, CollectionUpdate};

pub(super) fn group_updates_by_source(
    updates: &[CollectionUpdate],
) -> BTreeMap<(PathBuf, PathBuf), Vec<CollectionUpdate>> {
    let mut by_source: BTreeMap<(PathBuf, PathBuf), Vec<CollectionUpdate>> = BTreeMap::new();
    for update in updates {
        by_source
            .entry((update.root.clone(), update.database_root.clone()))
            .or_default()
            .push(update.clone());
    }
    by_source
}

pub(super) fn persist_collection_updates(
    root: &Path,
    database_root: &Path,
    updates: &[CollectionUpdate],
) -> Result<(), String> {
    let db = SourceDatabase::open_for_user_metadata_write_with_database_root(root, database_root)
        .map_err(|err| err.to_string())?;
    let mut batch = db.write_batch().map_err(|err| err.to_string())?;
    for update in updates {
        if matches!(
            batch
                .ensure_existing_live_file(&update.relative_path)
                .map_err(|err| err.to_string())?,
            ExistingFileMetadataUpdate::Missing
        ) {
            return Err(format!(
                "collection persistence deferred until source row exists: {}",
                update.relative_path.display()
            ));
        }
        match update.operation {
            CollectionOperation::Add => batch
                .add_collection(&update.relative_path, update.collection)
                .map_err(|err| err.to_string())?,
            CollectionOperation::Remove => batch
                .remove_collection(&update.relative_path, update.collection)
                .map_err(|err| err.to_string())?,
        }
    }
    batch
        .commit_auxiliary_state()
        .map_err(|err| err.to_string())
}

pub(super) fn group_missing_collection_files_by_source(
    files: &[MissingCollectionFile],
) -> BTreeMap<(PathBuf, PathBuf), Vec<MissingCollectionFile>> {
    let mut by_source: BTreeMap<(PathBuf, PathBuf), Vec<MissingCollectionFile>> = BTreeMap::new();
    for file in files {
        by_source
            .entry((file.root.clone(), file.database_root.clone()))
            .or_default()
            .push(file.clone());
    }
    by_source
}

pub(super) fn persist_missing_collection_cleanup(
    root: &Path,
    database_root: &Path,
    files: &[MissingCollectionFile],
) -> Result<(), String> {
    let db = SourceDatabase::open_for_user_metadata_write_with_database_root(root, database_root)
        .map_err(|err| err.to_string())?;
    let mut batch = db.write_batch().map_err(|err| err.to_string())?;
    for file in files {
        batch
            .remove_collection(&file.relative_path, file.collection)
            .map_err(|err| err.to_string())?;
    }
    batch
        .commit_auxiliary_state()
        .map_err(|err| err.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use wavecrate::sample_sources::SampleCollection;

    fn update(root: &str, relative_path: &str) -> CollectionUpdate {
        CollectionUpdate {
            root: PathBuf::from(root),
            database_root: PathBuf::from(root),
            relative_path: PathBuf::from(relative_path),
            absolute_path: PathBuf::from(root).join(relative_path),
            collection: SampleCollection::new(0).expect("collection"),
            operation: CollectionOperation::Add,
        }
    }

    #[test]
    fn group_updates_by_source_preserves_per_source_order() {
        let updates = vec![
            update("C:/one", "a.wav"),
            update("C:/two", "b.wav"),
            update("C:/one", "c.wav"),
        ];

        let grouped = group_updates_by_source(&updates);

        assert_eq!(
            grouped
                .get(&(PathBuf::from("C:/one"), PathBuf::from("C:/one")))
                .expect("first source")
                .iter()
                .map(|update| update.relative_path.as_path())
                .collect::<Vec<_>>(),
            vec![Path::new("a.wav"), Path::new("c.wav")]
        );
        assert_eq!(
            grouped
                .get(&(PathBuf::from("C:/two"), PathBuf::from("C:/two")))
                .expect("second source")
                .iter()
                .map(|update| update.relative_path.as_path())
                .collect::<Vec<_>>(),
            vec![Path::new("b.wav")]
        );
    }
}
