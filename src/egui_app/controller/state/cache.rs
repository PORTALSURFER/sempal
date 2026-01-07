//! Cached data for the controller, including databases and UI caches.

use super::super::{
    SampleSource, SourceDatabase, SourceDbError, SourceId, WavEntry, source_folders, wavs,
};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::rc::Rc;

pub(in crate::egui_app::controller) struct WavCacheState {
    pub(in crate::egui_app::controller) entries: HashMap<SourceId, WavEntriesState>,
}

impl WavCacheState {
    pub(in crate::egui_app::controller) fn new() -> Self {
        Self {
            entries: HashMap::new(),
        }
    }

    pub(in crate::egui_app::controller) fn insert_page(
        &mut self,
        source_id: SourceId,
        total: usize,
        page_size: usize,
        page_index: usize,
        entries: Vec<WavEntry>,
    ) {
        let cache = self
            .entries
            .entry(source_id)
            .or_insert_with(|| WavEntriesState::new(total, page_size));
        cache.total = total;
        cache.page_size = page_size;
        cache.insert_page(page_index, entries);
    }
}

pub(in crate::egui_app::controller) struct LibraryCacheState {
    pub(in crate::egui_app::controller) db: HashMap<SourceId, Rc<SourceDatabase>>,
    pub(in crate::egui_app::controller) wav: WavCacheState,
}

impl LibraryCacheState {
    pub(in crate::egui_app::controller) fn new() -> Self {
        Self {
            db: HashMap::new(),
            wav: WavCacheState::new(),
        }
    }

    /// Resolve or open the database for `source`, caching the handle.
    pub(in crate::egui_app::controller) fn database_for(
        &mut self,
        source: &SampleSource,
    ) -> Result<Rc<SourceDatabase>, SourceDbError> {
        if let Some(existing) = self.db.get(&source.id) {
            return Ok(existing.clone());
        }
        let db = Rc::new(SourceDatabase::open(&source.root)?);
        self.db.insert(source.id.clone(), db.clone());
        Ok(db)
    }
}

pub(in crate::egui_app::controller) struct BrowserCacheState {
    pub(in crate::egui_app::controller) labels: HashMap<SourceId, Vec<String>>,
    pub(in crate::egui_app::controller) analysis_failures:
        HashMap<SourceId, HashMap<PathBuf, String>>,
    pub(in crate::egui_app::controller) analysis_failures_pending: HashSet<SourceId>,
    pub(in crate::egui_app::controller) search: wavs::BrowserSearchCache,
    pub(in crate::egui_app::controller) features: HashMap<SourceId, FeatureCache>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum AnalysisJobStatus {
    Pending,
    Running,
    Done,
    Failed,
    Canceled,
}

#[allow(dead_code)]
#[derive(Clone, Debug)]
pub(crate) struct FeatureStatus {
    pub(crate) has_features_v1: bool,
    pub(crate) has_embedding: bool,
    pub(crate) duration_seconds: Option<f32>,
    pub(crate) sr_used: Option<i64>,
    pub(crate) analysis_status: Option<AnalysisJobStatus>,
}

pub(crate) struct FeatureCache {
    pub(crate) rows: Vec<Option<FeatureStatus>>,
}

pub(in crate::egui_app::controller) struct FolderBrowsersState {
    pub(in crate::egui_app::controller) models:
        HashMap<SourceId, source_folders::FolderBrowserModel>,
}

pub(in crate::egui_app::controller) struct ControllerUiCacheState {
    pub(in crate::egui_app::controller) browser: BrowserCacheState,
    pub(in crate::egui_app::controller) folders: FolderBrowsersState,
}

impl ControllerUiCacheState {
    pub(in crate::egui_app::controller) fn new() -> Self {
        Self {
            browser: BrowserCacheState {
                labels: HashMap::new(),
                analysis_failures: HashMap::new(),
                analysis_failures_pending: HashSet::new(),
                search: wavs::BrowserSearchCache::default(),
                features: HashMap::new(),
            },
            folders: FolderBrowsersState {
                models: HashMap::new(),
            },
        }
    }
}

pub(in crate::egui_app::controller) struct WavEntriesState {
    pub(in crate::egui_app::controller) total: usize,
    pub(in crate::egui_app::controller) page_size: usize,
    pub(in crate::egui_app::controller) pages: HashMap<usize, Vec<WavEntry>>,
    pub(in crate::egui_app::controller) lookup: HashMap<PathBuf, usize>,
}

impl WavEntriesState {
    pub(in crate::egui_app::controller) fn new(total: usize, page_size: usize) -> Self {
        Self {
            total,
            page_size: page_size.max(1),
            pages: HashMap::new(),
            lookup: HashMap::new(),
        }
    }

    pub(in crate::egui_app::controller) fn clear(&mut self) {
        self.total = 0;
        self.pages.clear();
        self.lookup.clear();
    }

    pub(in crate::egui_app::controller) fn insert_page(
        &mut self,
        page_index: usize,
        entries: Vec<WavEntry>,
    ) {
        let offset = page_index * self.page_size;
        for (idx, entry) in entries.iter().enumerate() {
            self.insert_lookup(entry.relative_path.clone(), offset + idx);
        }
        self.pages.insert(page_index, entries);
    }

    pub(in crate::egui_app::controller) fn entry(&self, index: usize) -> Option<&WavEntry> {
        let page_index = index / self.page_size;
        let in_page = index % self.page_size;
        self.pages
            .get(&page_index)
            .and_then(|page| page.get(in_page))
    }

    pub(in crate::egui_app::controller) fn entry_mut(
        &mut self,
        index: usize,
    ) -> Option<&mut WavEntry> {
        let page_index = index / self.page_size;
        let in_page = index % self.page_size;
        self.pages
            .get_mut(&page_index)
            .and_then(|page| page.get_mut(in_page))
    }

    pub(in crate::egui_app::controller) fn update_entry(
        &mut self,
        path: &Path,
        entry: WavEntry,
    ) -> bool {
        let normalized = path.to_string_lossy().replace('\\', "/");
        let Some(index) = self.lookup.get(Path::new(&normalized)).copied() else {
            return false;
        };
        let Some(slot) = self.entry_mut(index) else {
            return false;
        };
        *slot = entry;
        true
    }

    pub(in crate::egui_app::controller) fn insert_lookup(&mut self, path: PathBuf, index: usize) {
        let normalized = path.to_string_lossy().replace('\\', "/");
        self.lookup.insert(PathBuf::from(normalized), index);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_insert_lookup_normalizes_paths() {
        let mut cache = WavEntriesState::new(10, 10);
        
        // Insert with backslash
        cache.insert_lookup(PathBuf::from("foo\\bar.wav"), 1);
        
        // Should be found with forward slash
        assert_eq!(cache.lookup.get(Path::new("foo/bar.wav")), Some(&1));
        
        // Should be found with backslash (due to normalization on lookup/insert? No, insert normalizes key. Lookup must normalize query.)
        // We haven't updated lookup accessors on WavEntriesState itself other than update_entry.
        // Wait, update_entry calls lookup.get(path). 
        // WavEntriesState::entry() accesses by index.
        
        // Let's verify internal storage is normalized (size is 1)
        assert_eq!(cache.lookup.len(), 1);
        assert!(cache.lookup.contains_key(Path::new("foo/bar.wav")));
    }

    #[test]
    fn test_update_entry_normalizes_lookup_key() {
        let mut cache = WavEntriesState::new(10, 10);
        
        // Mock entry existence
        cache.insert_page(0, vec![WavEntry {
            relative_path: PathBuf::from("foo/bar.wav"),
            file_size: 0,
            modified_ns: 0,
            content_hash: None,
            tag: crate::sample_sources::SampleTag::Neutral,
            missing: false,
        }]);
        
        let new_entry = WavEntry {
            relative_path: PathBuf::from("foo/bar.wav"),
            file_size: 100,
            modified_ns: 100,
            content_hash: None,
            tag: crate::sample_sources::SampleTag::Keep,
            missing: false,
        };
        
        // Update using backslash path
        let success = cache.update_entry(Path::new("foo\\bar.wav"), new_entry);
        assert!(success, "Should find entry even with backslash path");
        
        // Verify update happened
        let entry = cache.entry(0).unwrap();
        assert_eq!(entry.tag, crate::sample_sources::SampleTag::Keep);
    }
}
