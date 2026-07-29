use crate::native_app::app::GuiMessage;
use crate::native_app::app::MetadataMessage;
use crate::native_app::app::NativeAppState;
use radiant::prelude as ui;
use radiant::widgets::{TextInputMessage, TextInputMessageKind};
use std::{
    path::{Path, PathBuf},
    time::Instant,
};

pub(super) use metrics::{metadata_tag_input_width_policy, metadata_tag_pill_width};
pub(in crate::native_app) use style::{
    metadata_tag_category_is_pinned, metadata_tag_pill_selection_style, metadata_tag_pill_style,
};
#[cfg(test)]
pub(super) use types::MetadataTagCommit;
pub(super) use types::{
    MetadataTagCategoryGroup, MetadataTagCompletionOption, MetadataTagDisplayCategory,
    MetadataTagInputMode, MetadataTagLoadResult, MetadataTagPersistResult,
    MetadataTagSelectionState,
};
pub(super) use vocabulary::{
    commit_metadata_tag_text, inferred_metadata_tag_category_id_for_name,
    metadata_tag_category_order,
};
#[cfg(test)]
pub(super) use vocabulary::{
    metadata_tag_category_id, metadata_tag_completion, normalize_metadata_tag,
};

mod assignment;
mod completion;
mod library;
mod metrics;
mod persistence;
mod playback_type_tags;
mod style;
mod types;
mod vocabulary;

use persistence::load_persisted_metadata_tag_map_for_source;
#[cfg(test)]
use persistence::load_persisted_metadata_tags_for_source;
#[cfg(test)]
pub(super) use persistence::{
    persist_metadata_tag_additions_for_tests, persist_metadata_tag_removals_for_tests,
};
impl NativeAppState {
    pub(in crate::native_app) fn remap_metadata_tags_for_moved_files(
        &mut self,
        moved_paths: &[(PathBuf, PathBuf)],
    ) {
        if moved_paths.is_empty() {
            return;
        }

        let mut changed = false;
        for (old_path, new_path) in moved_paths {
            if old_path == new_path {
                continue;
            }
            let remaps = self
                .metadata
                .tags_by_file
                .iter()
                .filter_map(|(file_id, tags)| {
                    remapped_metadata_tag_file_id(file_id, old_path, new_path)
                        .map(|new_id| (file_id.clone(), new_id, tags.clone()))
                })
                .collect::<Vec<_>>();
            if remaps.is_empty() {
                changed |= self
                    .metadata
                    .tags_by_file
                    .remove(new_path.to_string_lossy().as_ref())
                    .is_some();
                continue;
            }
            for (old_id, _, _) in &remaps {
                self.metadata.tags_by_file.remove(old_id);
            }
            for (_, new_id, tags) in remaps {
                self.metadata.tags_by_file.insert(new_id, tags);
                changed = true;
            }
        }

        if changed {
            self.library
                .folder_browser
                .invalidate_visible_sample_projection_cache();
        }
    }

    #[cfg(test)]
    pub(super) fn refresh_persisted_metadata_tags_for_source(&mut self, source_id: &str) {
        let Some((root, database_root)) = self.library.folder_browser.source_roots(source_id)
        else {
            return;
        };
        match load_persisted_metadata_tags_for_source(
            &root,
            &database_root,
            &mut self.metadata.tags_by_file,
        ) {
            Ok(()) => {
                self.metadata
                    .persisted_tag_sources_loaded
                    .insert(source_id.to_owned());
                self.library
                    .folder_browser
                    .invalidate_visible_sample_projection_cache();
            }
            Err(error) => {
                self.ui.status.sample = format!("Tags not loaded: {error}");
            }
        }
    }

    pub(super) fn schedule_persisted_metadata_tags_refresh_for_source(
        &mut self,
        source_id: &str,
        force: bool,
        context: &mut ui::UiUpdateContext<GuiMessage>,
    ) {
        if !force
            && self
                .metadata
                .persisted_tag_sources_loaded
                .contains(source_id)
        {
            return;
        }
        if self
            .metadata
            .persisted_tag_sources_pending
            .contains(source_id)
        {
            return;
        }
        let Some((root, database_root)) = self.library.folder_browser.source_roots(source_id)
        else {
            return;
        };
        let source_id = source_id.to_owned();
        self.metadata
            .persisted_tag_sources_pending
            .insert(source_id.clone());
        context
            .business()
            .background("gui-metadata-tag-refresh")
            .run(
                {
                    let source_root = root.clone();
                    move |_| MetadataTagLoadResult {
                        source_id,
                        source_root,
                        result: load_persisted_metadata_tag_map_for_source(&root, &database_root),
                    }
                },
                |result| GuiMessage::Metadata(MetadataMessage::MetadataTagsLoaded(result)),
            );
    }

    pub(super) fn finish_persisted_metadata_tags_load(&mut self, result: MetadataTagLoadResult) {
        self.metadata
            .persisted_tag_sources_pending
            .remove(&result.source_id);
        match result.result {
            Ok(tags_by_file) => {
                self.metadata
                    .tags_by_file
                    .retain(|file_id, _| !Path::new(file_id).starts_with(&result.source_root));
                let tag_count = tags_by_file.len();
                self.metadata.tags_by_file.extend(tags_by_file);
                self.metadata
                    .persisted_tag_sources_loaded
                    .insert(result.source_id.clone());
                self.library
                    .folder_browser
                    .invalidate_visible_sample_projection_cache();
                tracing::info!(
                    target: "wavecrate::debug::metadata",
                    event = "metadata.tags.load.applied",
                    source_id = result.source_id.as_str(),
                    tag_count,
                    "applied persisted metadata tags"
                );
            }
            Err(error) => {
                self.ui.status.sample = format!("Tags not loaded: {error}");
                tracing::warn!(
                    target: "wavecrate::debug::metadata",
                    event = "metadata.tags.load.failed",
                    source_id = result.source_id.as_str(),
                    error = error.as_str(),
                    "failed to load persisted metadata tags"
                );
            }
        }
    }

    pub(super) fn retain_visible_file_selection_after_metadata_tag_change(
        &mut self,
        previous_visible_ids: Vec<String>,
    ) {
        self.library
            .folder_browser
            .invalidate_visible_sample_projection_cache();
        self.library
            .folder_browser
            .reconcile_visible_file_selection_after_tag_filter(
                previous_visible_ids,
                &self.metadata.tags_by_file,
            );
    }

    pub(in crate::native_app) fn metadata_tag_selection_state(
        &self,
        tag: &str,
    ) -> MetadataTagSelectionState {
        let file_ids = self.selected_metadata_file_ids();
        self.metadata_tag_selection_state_for_file_ids(tag, &file_ids)
    }

    pub(in crate::native_app) fn metadata_tag_selection_state_for_file_ids(
        &self,
        tag: &str,
        file_ids: &[String],
    ) -> MetadataTagSelectionState {
        if file_ids.is_empty() {
            return MetadataTagSelectionState::None;
        }
        let assigned_count = file_ids
            .iter()
            .filter(|file_id| {
                self.metadata
                    .tags_by_file
                    .get(file_id.as_str())
                    .is_some_and(|tags| tags.iter().any(|existing| existing == tag))
            })
            .count();
        match assigned_count {
            0 => MetadataTagSelectionState::None,
            count if count == file_ids.len() => MetadataTagSelectionState::All,
            _ => MetadataTagSelectionState::Mixed,
        }
    }

    pub(super) fn selected_metadata_tags_for_display(&self) -> Vec<String> {
        let mut tags = Vec::new();
        for file_id in self.selected_metadata_file_ids() {
            let Some(file_tags) = self.metadata.tags_by_file.get(&file_id) else {
                continue;
            };
            for tag in file_tags {
                if !tags.iter().any(|existing| existing == tag) {
                    tags.push(tag.clone());
                }
            }
        }
        tags
    }

    pub(super) fn mixed_selected_metadata_tags_for_display(&self) -> Vec<String> {
        self.selected_metadata_tags_for_display()
            .into_iter()
            .filter(|tag| self.metadata_tag_selection_state(tag).is_mixed())
            .collect()
    }

    pub(super) fn selected_metadata_tag_display_categories(
        &self,
    ) -> Vec<MetadataTagDisplayCategory> {
        self.selected_metadata_tags_for_display()
            .iter()
            .map(|tag| MetadataTagDisplayCategory {
                tag: tag.clone(),
                category_id: self.metadata_tag_category_id(tag),
            })
            .collect()
    }

    pub(super) fn select_metadata_tag(&mut self, tag: String) {
        if self.metadata_tag_selection_state(&tag).is_assigned() {
            self.metadata.selected_tag = Some(tag);
        }
    }

    pub(in crate::native_app) fn selected_metadata_file_ids(&self) -> Vec<String> {
        self.library
            .folder_browser
            .selected_file_paths()
            .into_iter()
            .map(|path| path.to_string_lossy().to_string())
            .collect()
    }

    pub(super) fn apply_metadata_tag_input(
        &mut self,
        message: TextInputMessage,
        context: &mut ui::UiUpdateContext<GuiMessage>,
    ) {
        let parts = message.parts();
        match parts.kind {
            TextInputMessageKind::Submitted => {
                self.submit_metadata_tag_input(parts.value.to_owned(), context);
            }
            TextInputMessageKind::Changed => {
                self.metadata.tag_draft = parts.value.to_owned();
                self.reset_metadata_tag_completion_cycle();
            }
            TextInputMessageKind::CompletionRequested => {
                self.metadata.tag_draft = parts.value.to_owned();
                self.activate_metadata_tag_completion();
            }
        }
    }

    pub(super) fn focus_metadata_tag_input(
        &mut self,
        context: &mut ui::UiUpdateContext<GuiMessage>,
    ) {
        self.cancel_metadata_tag_entry();
        context.focus(crate::native_app::ui::ids::METADATA_TAG_INPUT_ID);
    }

    fn submit_metadata_tag_input(
        &mut self,
        value: String,
        context: &mut ui::UiUpdateContext<GuiMessage>,
    ) {
        if matches!(
            self.metadata.tag_input_mode,
            MetadataTagInputMode::Category { .. }
        ) {
            self.submit_metadata_tag_category(value, context);
        } else {
            self.submit_metadata_tag_value(value, context);
        }
    }

    pub(super) fn submit_selected_metadata_tag_completion(
        &mut self,
        value: String,
        context: &mut ui::UiUpdateContext<GuiMessage>,
    ) {
        if matches!(
            self.metadata.tag_input_mode,
            MetadataTagInputMode::Category { .. }
        ) {
            self.submit_metadata_tag_category_value(value, context);
        } else {
            self.submit_metadata_tag_input(value, context);
        }
    }

    fn submit_metadata_tag_value(
        &mut self,
        value: String,
        context: &mut ui::UiUpdateContext<GuiMessage>,
    ) {
        let mut commit = commit_metadata_tag_text(&value);
        let mut tags = std::mem::take(&mut self.metadata.tag_tokens);
        if tags.is_empty()
            && commit.tags.len() <= 1
            && let Some(tag) = self.explicit_metadata_tag_completion()
        {
            tags.push(tag);
            commit.tags.clear();
        }
        tags.append(&mut commit.tags);
        if tags.len() == 1 && self.is_new_metadata_tag(tags[0].as_str()) {
            let tag = tags.remove(0);
            self.metadata.tag_input_mode = MetadataTagInputMode::Category {
                pending_tag: tag.clone(),
            };
            self.metadata.tag_draft.clear();
            self.reset_metadata_tag_completion_cycle();
            self.ui.status.sample = format!("Choose a category for {tag}");
            return;
        }
        self.metadata.tag_draft.clear();
        self.reset_metadata_tag_completion_cycle();
        self.add_metadata_tags(tags, context);
    }

    fn submit_metadata_tag_category(
        &mut self,
        value: String,
        context: &mut ui::UiUpdateContext<GuiMessage>,
    ) {
        let MetadataTagInputMode::Category { pending_tag } = self.metadata.tag_input_mode.clone()
        else {
            return;
        };
        let Some(category_id) = self.selected_metadata_tag_category(value.as_str()) else {
            self.ui.status.sample = format!("Choose a category for {pending_tag}");
            return;
        };
        self.commit_metadata_tag_category(pending_tag, category_id, context);
    }

    fn submit_metadata_tag_category_value(
        &mut self,
        value: String,
        context: &mut ui::UiUpdateContext<GuiMessage>,
    ) {
        let MetadataTagInputMode::Category { pending_tag } = self.metadata.tag_input_mode.clone()
        else {
            return;
        };
        let Some(category_id) = self.metadata_tag_category_for_value(value.as_str()) else {
            self.ui.status.sample = format!("Choose a category for {pending_tag}");
            return;
        };
        self.commit_metadata_tag_category(pending_tag, category_id, context);
    }

    fn commit_metadata_tag_category(
        &mut self,
        pending_tag: String,
        category_id: &str,
        context: &mut ui::UiUpdateContext<GuiMessage>,
    ) {
        self.metadata
            .tag_dictionary
            .insert(pending_tag.clone(), category_id.to_string());
        self.metadata.tag_input_mode = MetadataTagInputMode::Tag;
        self.metadata.tag_draft.clear();
        self.reset_metadata_tag_completion_cycle();
        self.persist_user_configuration("metadata.tags.dictionary.persist", Instant::now());
        self.add_metadata_tags(vec![pending_tag], context);
    }

    pub(super) fn metadata_tag_input_placeholder(&self) -> &'static str {
        match self.metadata.tag_input_mode {
            MetadataTagInputMode::Tag => "add tag",
            MetadataTagInputMode::Category { .. } => "select group/parent tag",
        }
    }

    pub(super) fn pending_metadata_tag_category_tag(&self) -> Option<&str> {
        match &self.metadata.tag_input_mode {
            MetadataTagInputMode::Tag => None,
            MetadataTagInputMode::Category { pending_tag } => Some(pending_tag.as_str()),
        }
    }

    pub(super) fn cancel_metadata_tag_entry(&mut self) {
        self.metadata.tag_draft.clear();
        self.metadata.tag_tokens.clear();
        self.metadata.tag_input_mode = MetadataTagInputMode::Tag;
        self.reset_metadata_tag_completion_cycle();
    }
}

fn remapped_metadata_tag_file_id(
    file_id: &str,
    old_path: &Path,
    new_path: &Path,
) -> Option<String> {
    let path = Path::new(file_id);
    let suffix = path.strip_prefix(old_path).ok()?;
    Some(
        if suffix.as_os_str().is_empty() {
            new_path.to_path_buf()
        } else {
            new_path.join(suffix)
        }
        .to_string_lossy()
        .to_string(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn metadata_tags_normalize_to_single_token_values() {
        assert_eq!(
            normalize_metadata_tag("Deep Kick 01"),
            Some(String::from("deep-kick-01"))
        );
        assert_eq!(
            normalize_metadata_tag("  metal_floor  "),
            Some(String::from("metal_floor"))
        );
        assert_eq!(normalize_metadata_tag("!!!"), None);
    }

    #[test]
    fn submitted_tag_input_commits_delimited_tags() {
        assert_eq!(
            commit_metadata_tag_text("kick, warm tone"),
            MetadataTagCommit {
                tags: vec![String::from("kick"), String::from("warm-tone")],
                remainder: String::new(),
            }
        );
    }

    #[test]
    fn metadata_tag_completion_matches_known_tag_prefix() {
        assert_eq!(
            metadata_tag_completion("ki", ["warm", "kick", "kicker"].into_iter()),
            Some(String::from("kick"))
        );
        assert_eq!(metadata_tag_completion("zz", ["kick"].into_iter()), None);
    }

    #[test]
    fn metadata_tag_category_matches_target_category_vocabulary() {
        assert_eq!(metadata_tag_category_id("one-shot"), "playback-type");
        assert_eq!(metadata_tag_category_id("hat"), "sound-type");
        assert_eq!(metadata_tag_category_id("bright"), "character");
        assert_eq!(metadata_tag_category_id("prefix-artist"), "prefix");
        assert_eq!(metadata_tag_category_id("dorian"), "tuning-scale");
        assert_eq!(metadata_tag_category_id("custom-texture"), "character");
    }

    #[test]
    fn focus_metadata_tag_input_starts_with_empty_tag_entry() {
        let mut state = NativeAppState::load_default().expect("default state loads");
        state.metadata.tag_draft = String::from("§");
        state.metadata.tag_tokens = vec![String::from("kick")];
        state.metadata.tag_input_mode = MetadataTagInputMode::Category {
            pending_tag: String::from("rumble"),
        };
        state.metadata.pending_tag_completion_query = Some(String::from("ki"));

        state.focus_metadata_tag_input(&mut ui::UiUpdateContext::default());

        assert!(state.metadata.tag_draft.is_empty());
        assert!(state.metadata.tag_tokens.is_empty());
        assert_eq!(state.metadata.tag_input_mode, MetadataTagInputMode::Tag);
        assert!(state.metadata.pending_tag_completion_query.is_none());
    }
}
