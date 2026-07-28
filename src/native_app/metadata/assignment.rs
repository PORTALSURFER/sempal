use super::persistence::{
    persist_metadata_rating_assignment, persist_metadata_tag_assignment,
    persist_metadata_tag_assignments,
};
use super::playback_type_tags::{
    playback_type_replacement_present, replace_other_playback_type_tags,
};
use super::types::{
    MetadataRatingPersistRequest, MetadataRatingPersistResult, MetadataTagPersistRequest,
    MetadataTagPersistResult,
};
use super::{GuiMessage, MetadataMessage, NativeAppState};
use crate::native_app::app::ExtractedFilePlaybackType;
use radiant::prelude as ui;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use wavecrate::sample_sources::Rating;

const EXTRACTED_FILE_RATING: Rating = Rating::KEEP_1;
const EXTRACTED_FILE_LOCKED: bool = false;

struct MetadataTagTarget {
    file_id: String,
    absolute_path: PathBuf,
    source_root: PathBuf,
    source_database_root: PathBuf,
    relative_path: PathBuf,
}

impl NativeAppState {
    pub(in crate::native_app) fn add_metadata_tags(
        &mut self,
        tags: Vec<String>,
        context: &mut ui::UiUpdateContext<GuiMessage>,
    ) {
        let targets = match self.selected_metadata_tag_targets("adding") {
            Ok(targets) => targets,
            Err(status) => {
                self.ui.status.sample = status;
                return;
            }
        };
        self.add_metadata_tags_to_targets(tags, targets, context);
    }

    pub(in crate::native_app) fn assign_extracted_file_metadata(
        &mut self,
        absolute_path: &Path,
        playback_type: ExtractedFilePlaybackType,
        context: &mut ui::UiUpdateContext<GuiMessage>,
    ) -> Result<(), String> {
        let Some((source_root, source_database_root, relative_path)) = self
            .library
            .folder_browser
            .source_database_relative_file_path(absolute_path)
        else {
            return Err(String::from(
                "extracted file is not inside a configured source",
            ));
        };
        self.library.folder_browser.set_file_rating_state(
            absolute_path,
            EXTRACTED_FILE_RATING,
            EXTRACTED_FILE_LOCKED,
        );
        enqueue_metadata_rating_persist_request(
            MetadataRatingPersistRequest {
                absolute_path: absolute_path.to_path_buf(),
                source_root: source_root.clone(),
                source_database_root: source_database_root.clone(),
                relative_path: relative_path.clone(),
                rating: EXTRACTED_FILE_RATING,
                locked: EXTRACTED_FILE_LOCKED,
            },
            context,
        );

        self.assign_extracted_file_playback_type(
            absolute_path,
            source_root,
            source_database_root,
            relative_path,
            playback_type,
            context,
        );

        Ok(())
    }

    fn assign_extracted_file_playback_type(
        &mut self,
        absolute_path: &Path,
        source_root: PathBuf,
        source_database_root: PathBuf,
        relative_path: PathBuf,
        playback_type: ExtractedFilePlaybackType,
        context: &mut ui::UiUpdateContext<GuiMessage>,
    ) {
        let tag = playback_type.tag().to_string();
        let file_id = absolute_path.to_string_lossy().to_string();
        let mut file_tags = self
            .metadata
            .tags_by_file
            .get(&file_id)
            .cloned()
            .unwrap_or_default();
        let mut added = Vec::new();
        let removed_conflicting =
            replace_other_playback_type_tags(&mut file_tags, tag.as_str(), &mut added);
        if !file_tags.iter().any(|existing| existing == &tag) {
            file_tags.push(tag.clone());
            push_unique(&mut added, tag);
        }
        if added.is_empty() && removed_conflicting.is_empty() {
            return;
        }

        self.metadata
            .tags_by_file
            .insert(file_id.clone(), file_tags);
        self.library
            .folder_browser
            .set_file_last_curated_at(absolute_path, epoch_seconds());
        self.library
            .folder_browser
            .invalidate_visible_sample_projection_cache();
        self.reconcile_playback_mode_after_metadata_tag_change(file_id.as_str());

        let mut requests = Vec::new();
        if !removed_conflicting.is_empty() {
            requests.push(MetadataTagPersistRequest {
                absolute_path: absolute_path.to_path_buf(),
                source_root: source_root.clone(),
                source_database_root: source_database_root.clone(),
                relative_path: relative_path.clone(),
                tags: removed_conflicting,
                assigned: false,
            });
        }
        if !added.is_empty() {
            requests.push(MetadataTagPersistRequest {
                absolute_path: absolute_path.to_path_buf(),
                source_root,
                source_database_root,
                relative_path,
                tags: added,
                assigned: true,
            });
        }
        enqueue_metadata_tag_persist_requests(requests, context);
    }

    #[cfg(test)]
    fn add_metadata_tags_to_file_ids(
        &mut self,
        tags: Vec<String>,
        file_ids: Vec<String>,
        context: &mut ui::UiUpdateContext<GuiMessage>,
    ) {
        let targets = match self.metadata_tag_targets_for_file_ids(&file_ids, "adding") {
            Ok(targets) => targets,
            Err(status) => {
                self.ui.status.sample = status;
                return;
            }
        };
        self.add_metadata_tags_to_targets(tags, targets, context);
    }

    fn add_metadata_tags_to_targets(
        &mut self,
        tags: Vec<String>,
        targets: Vec<MetadataTagTarget>,
        context: &mut ui::UiUpdateContext<GuiMessage>,
    ) {
        let previous_visible_ids = self
            .library
            .folder_browser
            .selected_audio_file_ids_matching_tags(&self.metadata.tags_by_file);
        let mut requests = Vec::new();
        let mut changed_files = Vec::new();
        for target in targets {
            let mut file_tags = self
                .metadata
                .tags_by_file
                .get(&target.file_id)
                .cloned()
                .unwrap_or_default();
            let mut added = Vec::new();
            let mut removed_conflicting = Vec::new();
            for tag in &tags {
                let removed = replace_other_playback_type_tags(&mut file_tags, tag, &mut added);
                extend_unique(&mut removed_conflicting, removed);
                if file_tags.iter().any(|existing| existing == tag) {
                    continue;
                }
                file_tags.push(tag.clone());
                push_unique(&mut added, tag.clone());
            }
            if added.is_empty() && removed_conflicting.is_empty() {
                continue;
            }
            self.metadata
                .tags_by_file
                .insert(target.file_id.clone(), file_tags);
            if self
                .metadata
                .selected_tag
                .as_ref()
                .is_some_and(|selected| removed_conflicting.iter().any(|tag| tag == selected))
            {
                self.metadata.selected_tag = None;
            }
            self.reconcile_playback_mode_after_metadata_tag_change(target.file_id.as_str());
            changed_files.push(target.file_id.clone());
            if !removed_conflicting.is_empty() {
                requests.push(MetadataTagPersistRequest {
                    absolute_path: target.absolute_path.clone(),
                    source_root: target.source_root.clone(),
                    source_database_root: target.source_database_root.clone(),
                    relative_path: target.relative_path.clone(),
                    tags: removed_conflicting,
                    assigned: false,
                });
            }
            if !added.is_empty() {
                requests.push(MetadataTagPersistRequest {
                    absolute_path: target.absolute_path,
                    source_root: target.source_root,
                    source_database_root: target.source_database_root,
                    relative_path: target.relative_path,
                    tags: added,
                    assigned: true,
                });
            }
        }
        if requests.is_empty() {
            return;
        }
        let touched_paths = changed_files.iter().map(PathBuf::from).collect::<Vec<_>>();
        self.schedule_harvest_touched_for_paths(&touched_paths, context);
        self.finish_metadata_tag_curation_change(&changed_files, previous_visible_ids);
        let status_tags = added_metadata_tag_status_tags(&requests, &tags);
        self.ui.status.sample = metadata_tag_added_status(&status_tags, changed_files.len());
        enqueue_metadata_tag_persist_requests(requests, context);
    }

    pub(in crate::native_app) fn toggle_metadata_tag(
        &mut self,
        tag: String,
        context: &mut ui::UiUpdateContext<GuiMessage>,
    ) {
        if self.metadata_tag_selection_state(&tag).is_all()
            && !self.selected_metadata_has_replaced_playback_type_tag(&tag)
        {
            self.remove_metadata_tag(tag, context);
        } else {
            self.add_metadata_tags(vec![tag], context);
        }
    }

    #[cfg(test)]
    pub(in crate::native_app) fn toggle_metadata_tag_for_file_ids(
        &mut self,
        tag: String,
        file_ids: Vec<String>,
        context: &mut ui::UiUpdateContext<GuiMessage>,
    ) {
        if self
            .metadata_tag_selection_state_for_file_ids(&tag, &file_ids)
            .is_all()
        {
            self.remove_metadata_tag_from_file_ids(tag, file_ids, context);
        } else {
            self.add_metadata_tags_to_file_ids(vec![tag], file_ids, context);
        }
    }

    pub(in crate::native_app) fn remove_selected_metadata_tag(
        &mut self,
        context: &mut ui::UiUpdateContext<GuiMessage>,
    ) {
        let Some(tag) = self.metadata.selected_tag.clone() else {
            return;
        };
        self.remove_metadata_tag(tag, context);
    }

    pub(in crate::native_app) fn remove_metadata_tag(
        &mut self,
        tag: String,
        context: &mut ui::UiUpdateContext<GuiMessage>,
    ) {
        let targets = match self.selected_metadata_tag_targets("removing") {
            Ok(targets) => targets,
            Err(status) => {
                self.ui.status.sample = status;
                return;
            }
        };
        self.remove_metadata_tag_from_targets(tag, targets, context);
    }

    #[cfg(test)]
    fn remove_metadata_tag_from_file_ids(
        &mut self,
        tag: String,
        file_ids: Vec<String>,
        context: &mut ui::UiUpdateContext<GuiMessage>,
    ) {
        let targets = match self.metadata_tag_targets_for_file_ids(&file_ids, "removing") {
            Ok(targets) => targets,
            Err(status) => {
                self.ui.status.sample = status;
                return;
            }
        };
        self.remove_metadata_tag_from_targets(tag, targets, context);
    }

    fn remove_metadata_tag_from_targets(
        &mut self,
        tag: String,
        targets: Vec<MetadataTagTarget>,
        context: &mut ui::UiUpdateContext<GuiMessage>,
    ) {
        let previous_visible_ids = self
            .library
            .folder_browser
            .selected_audio_file_ids_matching_tags(&self.metadata.tags_by_file);
        let mut requests = Vec::new();
        let mut changed_files = Vec::new();
        for target in targets {
            let Some(file_tags) = self.metadata.tags_by_file.get_mut(&target.file_id) else {
                continue;
            };
            let before_len = file_tags.len();
            file_tags.retain(|existing| existing != &tag);
            if file_tags.len() == before_len {
                continue;
            }
            if file_tags.is_empty() {
                self.metadata.tags_by_file.remove(&target.file_id);
            }
            self.reconcile_playback_mode_after_metadata_tag_change(target.file_id.as_str());
            changed_files.push(target.file_id.clone());
            requests.push(MetadataTagPersistRequest {
                absolute_path: target.absolute_path,
                source_root: target.source_root,
                source_database_root: target.source_database_root,
                relative_path: target.relative_path,
                tags: vec![tag.clone()],
                assigned: false,
            });
        }
        if requests.is_empty() {
            return;
        }
        if self.metadata.selected_tag.as_deref() == Some(tag.as_str()) {
            self.metadata.selected_tag = None;
        }
        let touched_paths = changed_files.iter().map(PathBuf::from).collect::<Vec<_>>();
        self.schedule_harvest_touched_for_paths(&touched_paths, context);
        self.finish_metadata_tag_curation_change(&changed_files, previous_visible_ids);
        self.ui.status.sample = metadata_tag_removed_status(&tag, changed_files.len());
        if requests.len() == 1 {
            let request = requests.remove(0);
            context
                .business()
                .background("gui-metadata-tag-persist")
                .run(
                    move |_| persist_metadata_tag_assignment(request),
                    |result| GuiMessage::Metadata(MetadataMessage::MetadataTagsPersisted(result)),
                );
        } else {
            context
                .business()
                .background("gui-metadata-tag-persist")
                .run(
                    move |_| super::persistence::persist_metadata_tag_deletions(requests),
                    |result| GuiMessage::Metadata(MetadataMessage::MetadataTagsPersisted(result)),
                );
        }
    }

    fn finish_metadata_tag_curation_change(
        &mut self,
        file_ids: &[String],
        previous_visible_ids: Vec<String>,
    ) {
        self.library.folder_browser.clear_curation_focus_override();
        self.mark_file_ids_curated(file_ids);
        self.retain_visible_file_selection_after_metadata_tag_change(previous_visible_ids);
    }

    fn mark_file_ids_curated(&mut self, file_ids: &[String]) {
        let curated_at = epoch_seconds();
        self.library
            .folder_browser
            .set_file_ids_last_curated_at(file_ids, curated_at);
    }

    pub(in crate::native_app) fn finish_metadata_tag_persist(
        &mut self,
        result: MetadataTagPersistResult,
    ) {
        if let Err(error) = result.result {
            self.ui.status.sample = match result.tags.as_slice() {
                [tag] if result.assigned => format!("Tag {tag} not saved: {error}"),
                [tag] => format!("Tag {tag} not removed: {error}"),
                tags if result.assigned => format!("{} tags not saved: {error}", tags.len()),
                tags => format!("{} tags not removed: {error}", tags.len()),
            };
        }
    }

    pub(in crate::native_app) fn finish_metadata_rating_persist(
        &mut self,
        result: MetadataRatingPersistResult,
    ) {
        if let Err(error) = result.result {
            let label = crate::native_app::app::sample_path_label(&result.absolute_path);
            self.ui.status.sample = format!("Rating for {label} not saved: {error}");
        }
    }

    fn selected_metadata_tag_targets(
        &self,
        action: &'static str,
    ) -> Result<Vec<MetadataTagTarget>, String> {
        let file_ids = self.selected_metadata_file_ids();
        self.metadata_tag_targets_for_file_ids(&file_ids, action)
    }

    fn metadata_tag_targets_for_file_ids(
        &self,
        file_ids: &[String],
        action: &'static str,
    ) -> Result<Vec<MetadataTagTarget>, String> {
        let paths = file_ids.iter().map(PathBuf::from).collect::<Vec<_>>();
        if paths.is_empty() {
            return Err(format!("Select a sample before {action} tags"));
        }
        paths
            .into_iter()
            .map(|absolute_path| {
                let Some((source_root, source_database_root, relative_path)) = self
                    .library
                    .folder_browser
                    .source_database_relative_file_path(&absolute_path)
                else {
                    return Err(String::from(
                        "Selected sample is not inside a configured source",
                    ));
                };
                Ok(MetadataTagTarget {
                    file_id: absolute_path.to_string_lossy().to_string(),
                    absolute_path,
                    source_root,
                    source_database_root,
                    relative_path,
                })
            })
            .collect()
    }

    fn selected_metadata_has_replaced_playback_type_tag(&self, incoming: &str) -> bool {
        self.selected_metadata_file_ids().iter().any(|file_id| {
            self.metadata
                .tags_by_file
                .get(file_id)
                .is_some_and(|tags| playback_type_replacement_present(tags, incoming))
        })
    }
}

fn epoch_seconds() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or(Duration::ZERO)
        .as_secs() as i64
}

fn push_unique(tags: &mut Vec<String>, tag: String) {
    if !tags.iter().any(|existing| existing == &tag) {
        tags.push(tag);
    }
}

fn extend_unique(tags: &mut Vec<String>, incoming: Vec<String>) {
    for tag in incoming {
        push_unique(tags, tag);
    }
}

fn added_metadata_tag_status_tags(
    requests: &[MetadataTagPersistRequest],
    requested_tags: &[String],
) -> Vec<String> {
    let mut tags = Vec::new();
    for request in requests.iter().filter(|request| request.assigned) {
        extend_unique(&mut tags, request.tags.clone());
    }
    if tags.is_empty() {
        extend_unique(&mut tags, requested_tags.to_vec());
    }
    tags
}

fn metadata_tag_added_status(tags: &[String], changed_file_count: usize) -> String {
    match (tags, changed_file_count) {
        ([tag], 1) => format!("Added tag {tag}"),
        ([tag], count) => format!("Added tag {tag} to {count} samples"),
        (tags, 1) => format!("Added {} tags", tags.len()),
        (tags, count) => format!("Added {} tags to {count} samples", tags.len()),
    }
}

fn metadata_tag_removed_status(tag: &str, changed_file_count: usize) -> String {
    if changed_file_count == 1 {
        format!("Removed tag {tag}")
    } else {
        format!("Removed tag {tag} from {changed_file_count} samples")
    }
}

fn enqueue_metadata_tag_persist_requests(
    mut requests: Vec<MetadataTagPersistRequest>,
    context: &mut ui::UiUpdateContext<GuiMessage>,
) {
    if requests.is_empty() {
        return;
    }
    if requests.len() == 1 {
        let request = requests.remove(0);
        context
            .business()
            .background("gui-metadata-tag-persist")
            .run(
                move |_| persist_metadata_tag_assignment(request),
                |result| GuiMessage::Metadata(MetadataMessage::MetadataTagsPersisted(result)),
            );
    } else {
        context
            .business()
            .background("gui-metadata-tag-persist")
            .run(
                move |_| persist_metadata_tag_assignments(requests),
                |result| GuiMessage::Metadata(MetadataMessage::MetadataTagsPersisted(result)),
            );
    }
}

fn enqueue_metadata_rating_persist_request(
    request: MetadataRatingPersistRequest,
    context: &mut ui::UiUpdateContext<GuiMessage>,
) {
    context
        .business()
        .background("gui-metadata-rating-persist")
        .run(
            move |_| persist_metadata_rating_assignment(request),
            |result| GuiMessage::Metadata(MetadataMessage::MetadataRatingPersisted(result)),
        );
}
