mod cache;
mod decode;
mod normalize;
mod resample;

use std::sync::Arc;
use std::sync::Mutex;

use crate::waveform::{DecodedWaveform, WaveformDecodeError, WaveformRenderer};

const DEFAULT_DECODE_CACHE_LIMIT: usize = 8;

impl WaveformRenderer {
    /// Decode wav bytes into samples and duration without rendering.
    pub fn decode_from_bytes(&self, bytes: &[u8]) -> Result<DecodedWaveform, WaveformDecodeError> {
        let key = cache::hash_bytes(bytes);
        if let Ok(mut cache) = self.decode_cache.lock() {
            if let Some(cached) = cache.get(&key) {
                return Ok((*cached).clone());
            }
        }

        let decoded = self.load_decoded(bytes)?;
        if let Ok(mut cache) = self.decode_cache.lock() {
            cache.insert(key, Arc::new(decoded.clone()));
        }
        Ok(decoded)
    }
}

pub(super) fn default_decode_cache() -> Mutex<cache::DecodeCache> {
    Mutex::new(cache::DecodeCache::new(DEFAULT_DECODE_CACHE_LIMIT))
}

pub(super) use cache::DecodeCache;
