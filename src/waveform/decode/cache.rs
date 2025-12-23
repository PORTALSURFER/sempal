use std::collections::{HashMap, VecDeque};
use std::sync::Arc;

use crate::waveform::DecodedWaveform;

pub(super) struct DecodeCache {
    entries: HashMap<String, Arc<DecodedWaveform>>,
    order: VecDeque<String>,
    max_entries: usize,
}

impl DecodeCache {
    pub(super) fn new(max_entries: usize) -> Self {
        Self {
            entries: HashMap::new(),
            order: VecDeque::new(),
            max_entries: max_entries.max(1),
        }
    }

    pub(super) fn get(&mut self, key: &str) -> Option<Arc<DecodedWaveform>> {
        let value = self.entries.get(key).cloned();
        if value.is_some() {
            self.touch(key);
        }
        value
    }

    pub(super) fn insert(&mut self, key: String, value: Arc<DecodedWaveform>) {
        self.entries.insert(key.clone(), value);
        self.touch(&key);
        self.evict_overflow();
    }

    fn touch(&mut self, key: &str) {
        self.order.retain(|existing| existing != key);
        self.order.push_front(key.to_string());
    }

    fn evict_overflow(&mut self) {
        while self.order.len() > self.max_entries {
            if let Some(removed) = self.order.pop_back() {
                self.entries.remove(&removed);
            }
        }
    }
}

pub(super) fn hash_bytes(bytes: &[u8]) -> String {
    blake3::hash(bytes).to_hex().to_string()
}
