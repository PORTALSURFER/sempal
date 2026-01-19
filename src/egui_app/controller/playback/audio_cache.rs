use crate::{sample_sources::SourceId, waveform::DecodedWaveform};
use std::{
    collections::{HashMap, VecDeque},
    path::{Path, PathBuf},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct FileMetadata {
    pub file_size: u64,
    pub modified_ns: i64,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) struct CacheKey {
    pub source_id: SourceId,
    pub relative_path: PathBuf,
}

impl CacheKey {
    pub(crate) fn new(source_id: &SourceId, relative_path: &Path) -> Self {
        Self {
            source_id: source_id.clone(),
            relative_path: relative_path.to_path_buf(),
        }
    }
}

#[derive(Clone)]
pub(crate) struct CachedAudio {
    pub metadata: FileMetadata,
    pub decoded: DecodedWaveform,
    pub bytes: Vec<u8>,
}

pub(crate) struct AudioCache {
    capacity: usize,
    history_limit: usize,
    entries: HashMap<CacheKey, CachedAudio>,
    history: VecDeque<CacheKey>,
}

impl AudioCache {
    pub(crate) fn new(capacity: usize, history_limit: usize) -> Self {
        Self {
            capacity: capacity.max(1),
            history_limit: history_limit.max(1),
            entries: HashMap::new(),
            history: VecDeque::new(),
        }
    }

    pub(crate) fn get(&mut self, key: &CacheKey, metadata: FileMetadata) -> Option<CachedAudio> {
        if let Some(entry) = self.entries.get(key) {
            if entry.metadata == metadata {
                let hit = entry.clone();
                self.touch_history(key);
                return Some(hit);
            }
            self.invalidate(key);
        }
        None
    }

    pub(crate) fn insert(
        &mut self,
        key: CacheKey,
        metadata: FileMetadata,
        decoded: DecodedWaveform,
        bytes: Vec<u8>,
    ) {
        self.entries.insert(
            key.clone(),
            CachedAudio {
                metadata,
                decoded,
                bytes,
            },
        );
        self.touch_history(&key);
        self.enforce_limits();
    }

    pub(crate) fn invalidate(&mut self, key: &CacheKey) {
        self.entries.remove(key);
        self.history.retain(|existing| existing != key);
    }

    fn touch_history(&mut self, key: &CacheKey) {
        self.history.retain(|existing| existing != key);
        self.history.push_front(key.clone());
    }

    fn enforce_limits(&mut self) {
        while self.history.len() > self.history_limit {
            if let Some(removed) = self.history.pop_back() {
                self.entries.remove(&removed);
            }
        }
        while self.entries.len() > self.capacity {
            if let Some(removed) = self.history.pop_back() {
                self.entries.remove(&removed);
            } else {
                break;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn build_metadata(modified_ns: i64) -> FileMetadata {
        FileMetadata {
            file_size: 1,
            modified_ns,
        }
    }

    fn sample_key() -> CacheKey {
        CacheKey::new(&SourceId::from_string("a"), Path::new("one.wav"))
    }

    fn decoded() -> DecodedWaveform {
        DecodedWaveform {
            cache_token: 1,
            samples: std::sync::Arc::from(vec![0.1, 0.2]),
            analysis_samples: std::sync::Arc::from(Vec::new()),
            analysis_sample_rate: 0,
            analysis_stride: 1,
            peaks: None,
            duration_seconds: 1.0,
            sample_rate: 44_100,
            channels: 1,
        }
    }

    #[test]
    fn returns_hit_when_metadata_matches() {
        let mut cache = AudioCache::new(4, 4);
        let key = sample_key();
        cache.insert(key.clone(), build_metadata(1), decoded(), vec![1, 2]);

        let hit = cache.get(&key, build_metadata(1));

        assert!(hit.is_some());
    }

    #[test]
    fn evicts_on_metadata_mismatch() {
        let mut cache = AudioCache::new(4, 4);
        let key = sample_key();
        cache.insert(key.clone(), build_metadata(1), decoded(), vec![1, 2]);

        let miss = cache.get(&key, build_metadata(2));

        assert!(miss.is_none());
        assert!(cache.get(&key, build_metadata(1)).is_none());
    }

    #[test]
    fn evicts_least_recent_when_over_capacity() {
        let mut cache = AudioCache::new(2, 2);
        let key_a = CacheKey::new(&SourceId::from_string("a"), Path::new("a.wav"));
        let key_b = CacheKey::new(&SourceId::from_string("a"), Path::new("b.wav"));
        let key_c = CacheKey::new(&SourceId::from_string("a"), Path::new("c.wav"));

        cache.insert(key_a.clone(), build_metadata(1), decoded(), vec![]);
        cache.insert(key_b.clone(), build_metadata(1), decoded(), vec![]);
        cache.insert(key_c.clone(), build_metadata(1), decoded(), vec![]);

        assert!(cache.get(&key_a, build_metadata(1)).is_none());
        assert!(cache.get(&key_b, build_metadata(1)).is_some());
        assert!(cache.get(&key_c, build_metadata(1)).is_some());
    }
}
