use super::*;
use std::fs;
use std::io::ErrorKind;
use std::time::SystemTime;

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

    pub(super) fn primary_visible_row_for_browser_selection(&mut self) -> Option<usize> {
        let selected_index = self.selected_row_index()?;
        let path = self
            .wav_entry(selected_index)
            .map(|entry| entry.relative_path.clone())?;
        self.visible_row_for_path(&path)
    }

    pub(super) fn add_browser_rows_to_collection(
        &mut self,
        collection_id: &CollectionId,
        rows: &[usize],
    ) {
        if !self.settings.feature_flags.collections_enabled {
            self.set_status("Collections are disabled", StatusTone::Warning);
            return;
        }
        let Some(collection_index) = self
            .library
            .collections
            .iter()
            .position(|collection| &collection.id == collection_id)
        else {
            self.set_status("Collection not found", StatusTone::Error);
            return;
        };
        let collection_name = self.library.collections[collection_index].name.clone();
        let (contexts, last_error) = self.collect_browser_contexts(rows);
        let (added, already, new_members, last_error) =
            self.add_contexts_to_collection(collection_index, contexts, last_error);
        self.finalize_browser_collection_add(
            collection_id, &collection_name, added, already, new_members, last_error,
        );
    }

    pub(super) fn browser_selection_rows_for_move(&mut self) -> Vec<usize> {
        let mut rows: Vec<usize> = self
            .ui
            .browser
            .selected_paths
            .clone()
            .iter()
            .filter_map(|path| self.visible_row_for_path(path))
            .collect();
        if rows.is_empty() {
            if let Some(row) = self
                .focused_browser_row()
                .or_else(|| self.primary_visible_row_for_browser_selection())
            {
                rows.push(row);
            }
        }
        rows.sort_unstable();
        rows.dedup();
        rows
    }

    pub(super) fn next_browser_focus_path_after_move(
        &mut self,
        rows: &[usize],
    ) -> Option<PathBuf> {
        if rows.is_empty() || self.ui.browser.visible.len() == 0 {
            return None;
        }
        let mut sorted = rows.to_vec();
        sorted.sort_unstable();
        let highest = sorted.last().copied()?;
        let first = sorted.first().copied().unwrap_or(highest);
        let after = highest
            .checked_add(1)
            .and_then(|idx| self.ui.browser.visible.get(idx))
            .and_then(|entry_idx| self.wav_entry(entry_idx))
            .map(|entry| entry.relative_path.clone());
        if after.is_some() {
            return after;
        }
        first
            .checked_sub(1)
            .and_then(|idx| self.ui.browser.visible.get(idx))
            .and_then(|entry_idx| self.wav_entry(entry_idx))
            .map(|entry| entry.relative_path.clone())
    }

    pub(super) fn normalize_collection_hotkey(
        &self,
        hotkey: Option<u8>,
    ) -> Result<Option<u8>, String> {
        match hotkey {
            Some(slot) if (1..=9).contains(&slot) => Ok(Some(slot)),
            Some(_) => Err("Hotkey must be between 1 and 9".into()),
            None => Ok(None),
        }
    }

    pub(super) fn apply_collection_hotkey_binding(
        &mut self,
        collection_id: &CollectionId,
        hotkey: Option<u8>,
    ) -> Result<String, String> {
        if let Some(slot) = hotkey {
            for collection in self.library.collections.iter_mut() {
                if collection.hotkey == Some(slot) && &collection.id != collection_id {
                    collection.hotkey = None;
                }
            }
        }
        let target = self
            .library
            .collections
            .iter_mut()
            .find(|collection| &collection.id == collection_id)
            .ok_or_else(|| "Collection not found".to_string())?;
        target.hotkey = hotkey;
        let name = target.name.clone();
        self.persist_config("Failed to save collection hotkey")?;
        self.refresh_collections_ui();
        Ok(name)
    }

    pub(super) fn move_browser_rows_to_collection(
        &mut self,
        collection_id: &CollectionId,
        rows: &[usize],
    ) {
        if !self.settings.feature_flags.collections_enabled {
            self.set_status("Collections are disabled", StatusTone::Warning);
            return;
        }
        let clip_root = match self.resolve_collection_clip_root(collection_id) {
            Ok(root) => root,
            Err(err) => {
                self.set_status(err, StatusTone::Error);
                return;
            }
        };
        let Some(collection_index) = self
            .library
            .collections
            .iter()
            .position(|collection| &collection.id == collection_id)
        else {
            self.set_status("Collection not found", StatusTone::Error);
            return;
        };
        let collection_name = self.library.collections[collection_index].name.clone();
        let (contexts, mut last_error) = self.collect_browser_contexts(rows);
        let mut moved = 0usize;
        for ctx in contexts {
            let source = ctx.source.clone();
            let relative_path = ctx.entry.relative_path.clone();
            let absolute = source.root.join(&relative_path);
            if !absolute.is_file() {
                last_error = Some(format!("File missing: {}", relative_path.display()));
                continue;
            }
            let clip_relative = match unique_destination_name(&clip_root, &relative_path) {
                Ok(path) => path,
                Err(err) => {
                    last_error = Some(err);
                    continue;
                }
            };
            let clip_absolute = clip_root.join(&clip_relative);
            if let Err(err) = move_sample_file(&absolute, &clip_absolute) {
                last_error = Some(err);
                continue;
            }
            if let Err(err) = self.add_clip_to_collection(
                collection_id,
                clip_root.clone(),
                clip_relative.clone(),
            ) {
                let _ = fs::rename(&clip_absolute, &absolute);
                last_error = Some(err);
                continue;
            }
            if let Err(err) = self.remove_source_sample(&source, &relative_path) {
                last_error = Some(err);
                continue;
            }
            moved += 1;
        }
        if moved > 0 {
            self.set_status(
                format!("Moved {moved} sample(s) to '{collection_name}'"),
                StatusTone::Info,
            );
        } else if let Some(err) = last_error {
            self.set_status(err, StatusTone::Error);
        }
    }

    fn collect_browser_contexts(
        &mut self,
        rows: &[usize],
    ) -> (Vec<BrowserSampleContext>, Option<String>) {
        let mut contexts = Vec::new();
        let mut seen = std::collections::HashSet::new();
        let mut last_error = None;
        for row in rows {
            match self.resolve_browser_sample(*row) {
                Ok(ctx) => {
                    if seen.insert(ctx.entry.relative_path.clone()) {
                        contexts.push(BrowserSampleContext {
                            source: ctx.source,
                            entry: ctx.entry,
                        });
                    }
                }
                Err(err) => last_error = Some(err),
            }
        }
        (contexts, last_error)
    }

    fn resolve_collection_clip_root(&self, collection_id: &CollectionId) -> Result<PathBuf, String> {
        let preferred = self
            .library
            .collections
            .iter()
            .find(|collection| &collection.id == collection_id)
            .and_then(|collection| {
                collection_export::resolved_export_dir(
                    collection,
                    self.settings.collection_export_root.as_deref(),
                )
            });
        if let Some(path) = preferred {
            if path.exists() && !path.is_dir() {
                return Err(format!(
                    "Collection export path is not a directory: {}",
                    path.display()
                ));
            }
            std::fs::create_dir_all(&path).map_err(|err| {
                format!(
                    "Failed to create collection export path {}: {err}",
                    path.display()
                )
            })?;
            return Ok(path);
        }
        let fallback = crate::app_dirs::app_root_dir()
            .map_err(|err| err.to_string())?
            .join("collection_clips")
            .join(collection_id.as_str());
        std::fs::create_dir_all(&fallback)
            .map_err(|err| format!("Failed to create collection clip folder: {err}"))?;
        Ok(fallback)
    }

    fn add_contexts_to_collection(
        &mut self,
        collection_index: usize,
        contexts: Vec<BrowserSampleContext>,
        mut last_error: Option<String>,
    ) -> (usize, usize, Vec<CollectionMember>, Option<String>) {
        let mut added = 0;
        let mut already = 0;
        let mut new_members = Vec::new();
        for ctx in contexts {
            if let Err(err) = self.ensure_sample_db_entry(&ctx.source, &ctx.entry.relative_path) {
                last_error = Some(err);
                continue;
            }
            let contains = self.library.collections[collection_index]
                .contains(&ctx.source.id, &ctx.entry.relative_path);
            if contains {
                already += 1;
                continue;
            }
            let member = CollectionMember {
                source_id: ctx.source.id.clone(),
                relative_path: ctx.entry.relative_path.clone(),
                clip_root: None,
            };
            self.library.collections[collection_index]
                .members
                .push(member.clone());
            new_members.push(member);
            added += 1;
        }
        (added, already, new_members, last_error)
    }

    fn remove_source_sample(
        &mut self,
        source: &SampleSource,
        relative_path: &Path,
    ) -> Result<(), String> {
        let db = self
            .database_for(source)
            .map_err(|err| format!("Database unavailable: {err}"))?;
        db.remove_file(relative_path)
            .map_err(|err| format!("Failed to drop database row: {err}"))?;
        self.prune_cached_sample(source, relative_path);
        let collections_changed = self.remove_sample_from_collections(&source.id, relative_path);
        if collections_changed {
            self.persist_config("Failed to save collection after move")?;
        }
        Ok(())
    }

    fn finalize_browser_collection_add(
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

struct BrowserSampleContext {
    source: SampleSource,
    entry: WavEntry,
}

fn unique_destination_name(root: &Path, path: &Path) -> Result<PathBuf, String> {
    let file_name = path
        .file_name()
        .ok_or_else(|| "Sample has no file name".to_string())?;
    let candidate = PathBuf::from(file_name);
    if !root.join(&candidate).exists() {
        return Ok(candidate);
    }
    let stem = path
        .file_stem()
        .map(|name| name.to_string_lossy().to_string())
        .unwrap_or_else(|| "sample".to_string());
    let extension = path
        .extension()
        .map(|ext| ext.to_string_lossy().to_string());
    for index in 1..=999 {
        let suffix = format!("{stem}_move{index:03}");
        let file_name = if let Some(ext) = &extension {
            format!("{suffix}.{ext}")
        } else {
            suffix
        };
        let candidate = PathBuf::from(file_name);
        if !root.join(&candidate).exists() {
            return Ok(candidate);
        }
    }
    Err("Failed to find destination file name".into())
}

fn move_sample_file(source: &Path, destination: &Path) -> Result<(), String> {
    match fs::rename(source, destination) {
        Ok(()) => Ok(()),
        Err(err) if is_cross_device_link(&err) => {
            fs::copy(source, destination)
                .map_err(|err| format!("Failed to move file: {err}"))?;
            fs::remove_file(source).map_err(|err| format!("Failed to remove file: {err}"))?;
            Ok(())
        }
        Err(err) => Err(format!("Failed to move file: {err}")),
    }
}

fn is_cross_device_link(err: &std::io::Error) -> bool {
    #[cfg(unix)]
    {
        err.kind() == ErrorKind::CrossDeviceLink
    }
    #[cfg(not(unix))]
    {
        let _ = err;
        false
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
