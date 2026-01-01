use super::super::*;
use super::{collection_member_view, CollectionsController};
use std::path::{Path, PathBuf};

impl CollectionsController<'_> {
    pub(super) fn refresh_collections_ui(&mut self) {
        let selected_id = self.selection_state.ctx.selected_collection.clone();
        let mut collection_missing: Vec<bool> = Vec::with_capacity(self.library.collections.len());
        for collection in &self.library.collections {
            let mut missing = false;
            for member in &collection.members {
                if self.collection_member_missing_view(&collection_member_view(member)) {
                    missing = true;
                    break;
                }
            }
            collection_missing.push(missing);
        }
        self.ui.collections.rows = view_model::collection_rows(
            &self.library.collections,
            selected_id.as_ref(),
            &collection_missing,
            self.settings.collection_export_root.as_deref(),
        );
        self.ui.collections.selected = selected_id
            .as_ref()
            .and_then(|id| self.library.collections.iter().position(|c| &c.id == id));
        self.refresh_collection_samples();
    }

    pub(super) fn refresh_collection_selection_ui(&mut self) {
        if self.ui.collections.rows.is_empty() {
            self.refresh_collections_ui();
            return;
        }
        let selected_id = self.selection_state.ctx.selected_collection.clone();
        for row in self.ui.collections.rows.iter_mut() {
            row.selected = selected_id.as_ref().is_some_and(|id| id == &row.id);
        }
        self.ui.collections.selected = selected_id
            .as_ref()
            .and_then(|id| self.library.collections.iter().position(|c| &c.id == id));
    }

    pub(super) fn refresh_collection_samples(&mut self) {
        let selected_index = self
            .selection_state
            .ctx
            .selected_collection
            .as_ref()
            .and_then(|id| self.library.collections.iter().position(|c| &c.id == id));
        let mut tag_error: Option<String> = None;
        let Some(selected_index) = selected_index else {
            self.ui.collections.samples.clear();
            self.ui.collections.selected_sample = None;
            self.ui.collections.selected_paths.clear();
            self.ui.collections.selection_anchor = None;
            self.clear_collection_focus_context();
            return;
        };

        let members_len = self.library.collections[selected_index].members.len();
        let mut samples = Vec::with_capacity(members_len);
        for member in &self.library.collections[selected_index].members {
            let view = collection_member_view(member);
            let missing = self.collection_member_missing_view(&view);
            let source_label = if view.clip_root.is_some() {
                "Collection clip".to_string()
            } else {
                source_label(&self.library.sources, view.source_id)
            };
            let tag = if let Some(root) = view.clip_root {
                let source = SampleSource {
                    id: view.source_id.clone(),
                    root: root.clone(),
                };
                match self.sample_tag_for(&source, view.relative_path) {
                    Ok(tag) => tag,
                    Err(err) => {
                        if tag_error.is_none() {
                            tag_error = Some(err);
                        }
                        SampleTag::Neutral
                    }
                }
            } else {
                let source = self
                    .library
                    .sources
                    .iter()
                    .find(|s| &s.id == view.source_id)
                    .cloned();
                match source {
                    Some(source) => match self.sample_tag_for(&source, view.relative_path) {
                        Ok(tag) => tag,
                        Err(err) => {
                            if tag_error.is_none() {
                                tag_error = Some(err);
                            }
                            SampleTag::Neutral
                        }
                    },
                    None => {
                        if tag_error.is_none() {
                            tag_error = Some(format!(
                                "Source not available for {}",
                                view.relative_path.display()
                            ));
                        }
                        SampleTag::Neutral
                    }
                }
            };
            samples.push(crate::egui_app::state::CollectionSampleView {
                source_id: view.source_id.clone(),
                source: source_label,
                path: view.relative_path.to_path_buf(),
                label: view_model::sample_display_label(view.relative_path),
                tag,
                missing,
            });
        }
        self.ui.collections.samples = samples;
        if let Some(err) = tag_error {
            self.set_status(err, StatusTone::Warning);
        }
        if !self.ui.collections.selected_paths.is_empty() {
            let available: Vec<PathBuf> = self
                .ui
                .collections
                .samples
                .iter()
                .map(|sample| sample.path.clone())
                .collect();
            self.ui
                .collections
                .selected_paths
                .retain(|path| available.iter().any(|p| p == path));
            if self.ui.collections.selected_paths.is_empty() {
                self.ui.collections.selection_anchor = None;
            }
        }
        let len = self.ui.collections.samples.len();
        if len == 0 {
            self.ui.collections.selected_sample = None;
            self.ui.collections.scroll_to_sample = None;
            self.ui.collections.selected_paths.clear();
            self.ui.collections.selection_anchor = None;
            self.clear_collection_focus_context();
        } else if let Some(selected) = self.ui.collections.selected_sample
            && selected >= len
        {
            let clamped = len.saturating_sub(1);
            self.ui.collections.selected_sample = Some(clamped);
            self.ui.collections.scroll_to_sample = Some(clamped);
            self.focus_collection_context();
        }
    }

    pub(super) fn ensure_collection_selection(&mut self) {
        if self.selection_state.ctx.selected_collection.is_some() {
            return;
        }
        if let Some(first) = self.library.collections.first().cloned() {
            self.selection_state.ctx.selected_collection = Some(first.id);
        }
    }

    pub(super) fn finalize_browser_collection_add(
        &mut self,
        collection_id: &CollectionId,
        collection_name: &str,
        added: usize,
        already: usize,
        new_members: Vec<CollectionMember>,
        last_error: Option<String>,
    ) {
        if added > 0 {
            if let Err(err) = self.persist_config("Failed to save collection") {
                self.set_status(err, StatusTone::Error);
                return;
            }
            self.refresh_collections_ui();
            for member in &new_members {
                if let Err(err) = self.export_member_if_needed(collection_id, member) {
                    self.set_status(err, StatusTone::Warning);
                }
            }
        }
        if added > 0 {
            let mut message = format!("Added {added} sample(s) to '{collection_name}'");
            if already > 0 {
                message.push_str(&format!(" ({already} already there)"));
            }
            self.set_status(message, StatusTone::Info);
        } else if already > 0 {
            self.set_status("Samples already in collection", StatusTone::Info);
        } else if let Some(err) = last_error {
            self.set_status(err, StatusTone::Error);
        }
    }

    pub(super) fn finalize_collection_add(
        &mut self,
        collection_id: &CollectionId,
        member: &CollectionMember,
        relative_path: &Path,
    ) -> Result<(), String> {
        self.persist_config("Failed to save collection")?;
        self.refresh_collections_ui();
        if let Err(err) = self.export_member_if_needed(collection_id, member) {
            self.set_status(err, StatusTone::Warning);
        }
        self.set_status(
            format!("Added {} to collection", relative_path.display()),
            StatusTone::Info,
        );
        Ok(())
    }
}

fn source_label(sources: &[SampleSource], id: &SourceId) -> String {
    sources
        .iter()
        .find(|s| &s.id == id)
        .and_then(|source| {
            source
                .root
                .file_name()
                .and_then(|n| n.to_str())
                .map(|n| n.to_string())
        })
        .unwrap_or_else(|| "Unknown source".to_string())
}
