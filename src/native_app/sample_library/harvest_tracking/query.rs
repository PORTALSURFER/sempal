use super::{
    HarvestFamilySummary, HarvestFileIdentity, HarvestFileKey, HarvestMetadataSnapshot,
    HarvestSeenPersistRequest, HarvestState, NativeAppState, Path, PathBuf, SampleSource, WavEntry,
    tagged_playback_mode_for_tag,
};
use wavecrate::sample_sources::HarvestTouchedPersistRequest;

impl NativeAppState {
    pub(in crate::native_app) fn selected_harvest_family_summary(
        &self,
    ) -> Option<HarvestFamilySummary> {
        let selected_path = PathBuf::from(self.library.folder_browser.selected_file_id()?);
        let identity = self.harvest_identity_for_path(&selected_path)?;
        let record = match wavecrate::sample_sources::library::harvest_file(&identity.key) {
            Ok(record) => record,
            Err(error) => {
                tracing::warn!(
                    path = %selected_path.display(),
                    "failed to load selected harvest file record: {error}"
                );
                None
            }
        };
        let parents =
            match wavecrate::sample_sources::library::harvest_parents_for_child(&identity.key) {
                Ok(parents) => parents,
                Err(error) => {
                    tracing::warn!(
                        path = %selected_path.display(),
                        "failed to load selected harvest origin records: {error}"
                    );
                    Vec::new()
                }
            };
        let derivatives =
            match wavecrate::sample_sources::library::harvest_derivations_for_parent(&identity.key)
            {
                Ok(derivatives) => derivatives,
                Err(error) => {
                    tracing::warn!(
                        path = %selected_path.display(),
                        "failed to load selected harvest derivative records: {error}"
                    );
                    Vec::new()
                }
            };
        if record.is_none()
            && parents.is_empty()
            && derivatives.is_empty()
            && self.library.folder_browser.harvest_filter().is_none()
        {
            return None;
        }

        let origin_paths = parents
            .iter()
            .map(|edge| self.harvest_path_for_key(&edge.parent.key))
            .collect::<Vec<_>>();
        let derivative_paths = derivatives
            .iter()
            .map(|edge| self.harvest_path_for_key(&edge.child.key))
            .collect::<Vec<_>>();
        Some(HarvestFamilySummary {
            state_label: harvest_state_display_label(
                record
                    .as_ref()
                    .map(|record| record.state)
                    .unwrap_or(HarvestState::New),
            )
            .to_owned(),
            origin_count: parents.len(),
            derivative_count: derivatives.len(),
            missing_origin_count: missing_related_harvest_paths(&origin_paths),
            missing_derivative_count: missing_related_harvest_paths(&derivative_paths),
            first_origin_label: origin_paths
                .iter()
                .find_map(|path| path.as_ref().map(|path| harvest_family_path_label(path))),
            first_derivative_label: derivative_paths
                .iter()
                .find_map(|path| path.as_ref().map(|path| harvest_family_path_label(path))),
        })
    }

    pub(in crate::native_app) fn selected_harvest_family_available(&self) -> bool {
        self.library
            .folder_browser
            .selected_file_id()
            .map(PathBuf::from)
            .and_then(|path| self.harvest_key_for_path(&path))
            .is_some()
    }

    pub(super) fn harvest_identity_for_path(&self, path: &Path) -> Option<HarvestFileIdentity> {
        let (source, relative_path) = self
            .library
            .folder_browser
            .sample_source_for_file_path(path)?;
        let (file_size, modified_ns) = file_identity_metadata(path);
        let entry = source_db_entry_for_path(&source, &relative_path);
        Some(HarvestFileIdentity {
            key: HarvestFileKey::new(source.id, relative_path),
            file_size: file_size.or_else(|| entry.as_ref().map(|entry| entry.file_size)),
            modified_ns: modified_ns.or_else(|| entry.as_ref().map(|entry| entry.modified_ns)),
            content_hash: entry.and_then(|entry| entry.content_hash),
        })
    }

    pub(super) fn harvest_key_for_path(&self, path: &Path) -> Option<HarvestFileKey> {
        let (source, relative_path) = self
            .library
            .folder_browser
            .sample_source_for_file_path(path)?;
        Some(HarvestFileKey::new(source.id, relative_path))
    }

    pub(super) fn harvest_seen_persist_request_for_path(
        &self,
        path: &Path,
    ) -> Option<HarvestSeenPersistRequest> {
        let (source, relative_path) = self
            .library
            .folder_browser
            .sample_source_for_file_path(path)?;
        let source_database_root = source.database_root().ok()?;
        Some(HarvestSeenPersistRequest {
            file_id: path.display().to_string(),
            source_id: source.id,
            source_root: source.root,
            source_database_root,
            relative_path,
        })
    }

    pub(super) fn harvest_touched_persist_request_for_path(
        &self,
        path: &Path,
    ) -> Option<HarvestTouchedPersistRequest> {
        let (source, relative_path) = self
            .library
            .folder_browser
            .sample_source_for_file_path(path)?;
        Some(HarvestTouchedPersistRequest {
            file_id: path.display().to_string(),
            source: source.clone(),
            relative_path,
        })
    }

    pub(super) fn harvest_metadata_snapshot_for_path(
        &self,
        path: &Path,
    ) -> HarvestMetadataSnapshot {
        let file_id = path.to_string_lossy().to_string();
        let tags = self
            .metadata
            .tags_by_file
            .get(&file_id)
            .cloned()
            .unwrap_or_default();
        let playback_type = tags
            .iter()
            .find(|tag| tagged_playback_mode_for_tag(tag).is_some())
            .cloned();
        let rating = self
            .library
            .folder_browser
            .sample_source_for_file_path(path)
            .and_then(|(source, relative)| source_db_entry_for_path(&source, &relative))
            .map(|entry| entry.tag.as_i64());
        HarvestMetadataSnapshot {
            rating,
            tags,
            playback_type,
        }
    }

    pub(super) fn harvest_path_for_key(&self, key: &HarvestFileKey) -> Option<PathBuf> {
        self.library
            .folder_browser
            .source_root_path(key.source_id.as_str())
            .map(|root| root.join(&key.relative_path))
    }
}

fn missing_related_harvest_paths(paths: &[Option<PathBuf>]) -> usize {
    paths
        .iter()
        .filter(|path| {
            path.as_ref()
                .is_none_or(|path| !wavecrate::sample_sources::harvest_file_ops::path_exists(path))
        })
        .count()
}

fn harvest_state_display_label(state: HarvestState) -> &'static str {
    match state {
        HarvestState::New => "New",
        HarvestState::Seen => "Seen",
        HarvestState::Touched => "Touched",
        HarvestState::Done => "Done",
        HarvestState::Ignored => "Ignored",
    }
}

fn harvest_family_path_label(path: &Path) -> String {
    path.file_name()
        .and_then(|name| name.to_str())
        .filter(|name| !name.trim().is_empty())
        .map(str::to_owned)
        .unwrap_or_else(|| path.display().to_string())
}

fn source_db_entry_for_path(source: &SampleSource, relative_path: &Path) -> Option<WavEntry> {
    source
        .open_db()
        .ok()
        .and_then(|db| db.entry_for_path(relative_path).ok().flatten())
}

fn file_identity_metadata(path: &Path) -> (Option<u64>, Option<i64>) {
    wavecrate::sample_sources::harvest_file_ops::file_identity_metadata(path)
}
