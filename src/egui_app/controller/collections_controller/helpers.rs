use super::*;

pub(crate) struct CollectionsController<'a> {
    controller: &'a mut EguiController,
}

impl<'a> CollectionsController<'a> {
    pub(crate) fn new(controller: &'a mut EguiController) -> Self {
        Self { controller }
    }
}

impl std::ops::Deref for CollectionsController<'_> {
    type Target = EguiController;

    fn deref(&self) -> &Self::Target {
        self.controller
    }
}

impl std::ops::DerefMut for CollectionsController<'_> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.controller
    }
}

impl CollectionsController<'_> {
    pub(super) fn collection_member_source(
        &self,
        member: &CollectionMember,
    ) -> Option<SampleSource> {
        if let Some(root) = member.clip_root.as_ref() {
            return Some(SampleSource {
                id: member.source_id.clone(),
                root: root.clone(),
            });
        }
        self.library
            .sources
            .iter()
            .find(|s| s.id == member.source_id)
            .cloned()
    }

    pub(super) fn collection_member_missing(&mut self, member: &CollectionMember) -> bool {
        if let Some(root) = member.clip_root.as_ref() {
            return !root.join(&member.relative_path).is_file();
        }
        self.sample_missing(&member.source_id, &member.relative_path)
    }

    pub(super) fn add_clip_to_collection(
        &mut self,
        collection_id: &CollectionId,
        clip_root: PathBuf,
        clip_relative_path: PathBuf,
    ) -> Result<(), String> {
        if !self.settings.feature_flags.collections_enabled {
            return Err("Collections are disabled".into());
        }
        SourceDatabase::open(&clip_root)
            .map_err(|err| format!("Failed to create clip database: {err}"))?;
        let clip_source_id =
            SourceId::from_string(format!("collection-{}", collection_id.as_str()));
        let clip_source = SampleSource {
            id: clip_source_id.clone(),
            root: clip_root.clone(),
        };
        self.ensure_sample_db_entry(&clip_source, &clip_relative_path)?;
        let new_member = CollectionMember {
            source_id: clip_source_id,
            relative_path: clip_relative_path.clone(),
            clip_root: Some(clip_root),
        };
        let mut collections = self.library.collections.clone();
        let Some(collection) = collections.iter_mut().find(|c| &c.id == collection_id) else {
            return Err("Collection not found".into());
        };
        let already_present = collection.contains(&new_member.source_id, &new_member.relative_path);
        if !already_present {
            collection.members.push(new_member.clone());
        }
        self.library.collections = collections;
        if already_present {
            self.set_status("Already in collection", StatusTone::Info);
            return Ok(());
        }
        self.finalize_collection_add(collection_id, &new_member, &new_member.relative_path)
    }

    pub(super) fn refresh_collections_ui(&mut self) {
        let selected_id = self.selection_state.ctx.selected_collection.clone();
        let mut collection_missing: Vec<bool> = Vec::with_capacity(self.library.collections.len());
        for collection_index in 0..self.library.collections.len() {
            let mut missing = false;
            let members_len = self.library.collections[collection_index].members.len();
            for member_index in 0..members_len {
                let (source_id, relative_path, clip_root) = {
                    let member = &self.library.collections[collection_index].members[member_index];
                    (
                        member.source_id.clone(),
                        member.relative_path.clone(),
                        member.clip_root.clone(),
                    )
                };
                let member_missing = if let Some(root) = clip_root.as_ref() {
                    !root.join(&relative_path).is_file()
                } else {
                    self.sample_missing(&source_id, &relative_path)
                };
                if member_missing {
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
        for member_index in 0..members_len {
            let (source_id, relative_path, clip_root) = {
                let member = &self.library.collections[selected_index].members[member_index];
                (
                    member.source_id.clone(),
                    member.relative_path.clone(),
                    member.clip_root.clone(),
                )
            };
            let missing = if let Some(root) = clip_root.as_ref() {
                !root.join(&relative_path).is_file()
            } else {
                self.sample_missing(&source_id, &relative_path)
            };
            let source_label = if clip_root.is_some() {
                "Collection clip".to_string()
            } else {
                source_label(&self.library.sources, &source_id)
            };
            let tag = if let Some(root) = clip_root.as_ref() {
                let source = SampleSource {
                    id: source_id.clone(),
                    root: root.clone(),
                };
                match self.sample_tag_for(&source, &relative_path) {
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
                    .find(|s| s.id == source_id)
                    .cloned();
                match source {
                    Some(source) => match self.sample_tag_for(&source, &relative_path) {
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
                                relative_path.display()
                            ));
                        }
                        SampleTag::Neutral
                    }
                }
            };
            samples.push(crate::egui_app::state::CollectionSampleView {
                source_id,
                source: source_label,
                path: relative_path.to_path_buf(),
                label: view_model::sample_display_label(&relative_path),
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

    pub(super) fn ensure_sample_db_entry(
        &mut self,
        source: &SampleSource,
        relative_path: &Path,
    ) -> Result<(), String> {
        let full_path = source.root.join(relative_path);
        let metadata = fs::metadata(&full_path)
            .map_err(|err| format!("Missing file for collection: {err}"))?;
        let modified_ns = metadata
            .modified()
            .map_err(|err| format!("Missing mtime for collection: {err}"))?
            .duration_since(SystemTime::UNIX_EPOCH)
            .map_err(|_| "File modified time is before epoch".to_string())?
            .as_nanos() as i64;
        let file_size = metadata.len();
        let db = self
            .database_for(source)
            .map_err(|err| format!("Database unavailable: {err}"))?;
        db.upsert_file(relative_path, file_size, modified_ns)
            .map_err(|err| format!("Failed to sync collection entry: {err}"))
    }

    pub(super) fn current_collection(&self) -> Option<Collection> {
        let selected = self.selection_state.ctx.selected_collection.as_ref()?;
        self.library
            .collections
            .iter()
            .find(|c| &c.id == selected)
            .cloned()
    }

    pub(super) fn add_sample_to_collection_inner(
        &mut self,
        collection_id: &CollectionId,
        source: &SampleSource,
        relative_path: &Path,
    ) -> Result<(), String> {
        self.ensure_sample_db_entry(source, relative_path)?;
        let mut collections = self.library.collections.clone();
        let Some(collection) = collections.iter_mut().find(|c| &c.id == collection_id) else {
            return Err("Collection not found".into());
        };
        let new_member = CollectionMember {
            source_id: source.id.clone(),
            relative_path: relative_path.to_path_buf(),
            clip_root: None,
        };
        let added = collection.add_member(
            new_member.source_id.clone(),
            new_member.relative_path.clone(),
        );
        self.library.collections = collections;
        if !added {
            self.set_status("Already in collection", StatusTone::Info);
            return Ok(());
        }
        self.finalize_collection_add(collection_id, &new_member, relative_path)
    }

    fn finalize_collection_add(
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

    pub(super) fn next_collection_name(&self) -> String {
        let base = "Collection";
        let mut index = self.library.collections.len() + 1;
        loop {
            let candidate = format!("{base} {index}");
            if !self.library.collections.iter().any(|c| c.name == candidate) {
                return candidate;
            }
            index += 1;
        }
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
