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
    pub(super) fn collection_member_source(&self, member: &CollectionMember) -> Option<SampleSource> {
        if let Some(root) = member.clip_root.as_ref() {
            return Some(SampleSource {
                id: member.source_id.clone(),
                root: root.clone(),
            });
        }
        self.sources
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
        if !self.feature_flags.collections_enabled {
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
        let mut collections = self.collections.clone();
        let Some(collection) = collections.iter_mut().find(|c| &c.id == collection_id) else {
            return Err("Collection not found".into());
        };
        let already_present = collection.contains(
            &new_member.source_id,
            &new_member.relative_path,
        );
        if !already_present {
            collection.members.push(new_member.clone());
        }
        self.collections = collections;
        if already_present {
            self.set_status("Already in collection", StatusTone::Info);
            return Ok(());
        }
        self.finalize_collection_add(collection_id, &new_member, &new_member.relative_path)
    }

    pub(super) fn refresh_collections_ui(&mut self) {
        let selected_id = self.selected_collection.clone();
        let collections_snapshot = self.collections.clone();
        let collection_missing: Vec<bool> = collections_snapshot
            .iter()
            .map(|collection| {
                collection
                    .members
                    .iter()
                    .any(|member| self.collection_member_missing(member))
            })
            .collect();
        self.ui.collections.rows = view_model::collection_rows(
            &self.collections,
            selected_id.as_ref(),
            &collection_missing,
            self.collection_export_root.as_deref(),
        );
        self.ui.collections.selected = selected_id
            .as_ref()
            .and_then(|id| self.collections.iter().position(|c| &c.id == id));
        self.refresh_collection_samples();
    }

    pub(super) fn refresh_collection_samples(&mut self) {
        let selected = self
            .selected_collection
            .as_ref()
            .and_then(|id| self.collections.iter().find(|c| &c.id == id))
            .cloned();
        let sources = self.sources.clone();
        let mut tag_error: Option<String> = None;
        let sample_missing_flags = selected.as_ref().map(|collection| {
            collection
                .members
                .iter()
                .map(|member| self.collection_member_missing(member))
                .collect::<Vec<bool>>()
        });
        let missing_slice = sample_missing_flags.as_deref();
        self.ui.collections.samples =
            view_model::collection_samples(selected.as_ref(), &sources, missing_slice, |member| {
                match self.tag_for_collection_member(member) {
                    Ok(tag) => tag,
                    Err(err) => {
                        if tag_error.is_none() {
                            tag_error = Some(err);
                        }
                        SampleTag::Neutral
                    }
                }
            });
        if let Some(err) = tag_error {
            self.set_status(err, StatusTone::Warning);
        }
        let len = self.ui.collections.samples.len();
        if len == 0 {
            self.ui.collections.selected_sample = None;
            self.clear_collection_focus_context();
        } else if let Some(selected) = self.ui.collections.selected_sample
            && selected >= len
        {
            self.ui.collections.selected_sample = Some(len.saturating_sub(1));
            self.focus_collection_context();
        }
    }

    fn tag_for_collection_member(
        &mut self,
        member: &CollectionMember,
    ) -> Result<SampleTag, String> {
        let source = self.collection_member_source(member).ok_or_else(|| {
            format!(
                "Source not available for {}",
                member.relative_path.display()
            )
        })?;
        self.sample_tag_for(&source, &member.relative_path)
    }

    pub(super) fn ensure_collection_selection(&mut self) {
        if self.selected_collection.is_some() {
            return;
        }
        if let Some(first) = self.collections.first().cloned() {
            self.selected_collection = Some(first.id);
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
        let selected = self.selected_collection.as_ref()?;
        self.collections.iter().find(|c| &c.id == selected).cloned()
    }

    pub(super) fn add_sample_to_collection_inner(
        &mut self,
        collection_id: &CollectionId,
        source: &SampleSource,
        relative_path: &Path,
    ) -> Result<(), String> {
        self.ensure_sample_db_entry(source, relative_path)?;
        let mut collections = self.collections.clone();
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
        self.collections = collections;
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
        let mut index = self.collections.len() + 1;
        loop {
            let candidate = format!("{base} {index}");
            if !self.collections.iter().any(|c| c.name == candidate) {
                return candidate;
            }
            index += 1;
        }
    }
}
