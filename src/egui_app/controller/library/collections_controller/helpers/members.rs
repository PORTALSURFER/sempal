use super::super::*;
use super::{CollectionMemberView, CollectionsController, collection_member_view};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

impl CollectionsController<'_> {
    pub(crate) fn collection_member_source(
        &self,
        member: &CollectionMember,
    ) -> Option<SampleSource> {
        let view = collection_member_view(member);
        if let Some(root) = view.clip_root {
            return Some(SampleSource {
                id: view.source_id.clone(),
                root: root.clone(),
            });
        }
        self.library
            .sources
            .iter()
            .find(|s| &s.id == view.source_id)
            .cloned()
    }

    pub(crate) fn collection_member_missing(
        &mut self,
        member: &CollectionMember,
    ) -> bool {
        self.collection_member_missing_view(&collection_member_view(member))
    }

    pub(crate) fn collection_member_missing_view(
        &mut self,
        member: &CollectionMemberView<'_>,
    ) -> bool {
        if let Some(root) = member.clip_root {
            return !root.join(member.relative_path).is_file();
        }
        self.sample_missing(member.source_id, member.relative_path)
    }

    pub(crate) fn add_clip_to_collection(
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

    pub(crate) fn ensure_sample_db_entry(
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

    pub(crate) fn current_collection(
        &self,
    ) -> Option<Collection> {
        let selected = self.selection_state.ctx.selected_collection.as_ref()?;
        self.library
            .collections
            .iter()
            .find(|c| &c.id == selected)
            .cloned()
    }

    pub(crate) fn add_sample_to_collection_inner(
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


    pub(crate) fn normalize_collection_hotkey(
        &self,
        hotkey: Option<u8>,
    ) -> Result<Option<u8>, String> {
        match hotkey {
            Some(slot) if (1..=9).contains(&slot) => Ok(Some(slot)),
            Some(_) => Err("Hotkey must be between 1 and 9".into()),
            None => Ok(None),
        }
    }

    pub(crate) fn apply_collection_hotkey_binding(
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

    pub(crate) fn next_collection_name(
        &self,
    ) -> String {
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

pub(crate) struct BrowserSampleContext {
    pub(crate) source: SampleSource,
    pub(crate) entry: WavEntry,
}
