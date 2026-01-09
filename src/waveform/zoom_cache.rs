use super::{WaveformChannelView, WaveformColumnView, WaveformRenderer};
use std::{
    collections::{HashMap, VecDeque},
    hash::{Hash, Hasher},
    sync::Mutex,
};

pub(super) struct WaveformZoomCache {
    inner: Mutex<CacheInner>,
}

impl WaveformZoomCache {
    pub(super) fn new() -> Self {
        Self {
            inner: Mutex::new(CacheInner::new()),
        }
    }

    pub(super) fn get_or_compute(
        &self,
        cache_token: u64,
        samples: &[f32],
        channels: usize,
        view: WaveformChannelView,
        width: u32,
    ) -> CachedColumns {
        let key = CacheKey::new(cache_token, samples, channels, view, width);
        {
            let mut inner = self.inner.lock().expect("waveform zoom cache lock");
            if let Some(hit) = inner.map.get(&key).cloned() {
                inner.touch(key);
                return hit;
            }
        }

        let computed =
            match WaveformRenderer::sample_columns_for_width(samples, channels, width, view) {
                WaveformColumnView::Mono(cols) => CachedColumns::Mono(cols.into()),
                WaveformColumnView::SplitStereo { left, right } => CachedColumns::SplitStereo {
                    left: left.into(),
                    right: right.into(),
                },
            };
        let mut inner = self.inner.lock().expect("waveform zoom cache lock");
        if let Some(hit) = inner.map.get(&key).cloned() {
            inner.touch(key);
            return hit;
        }
        inner.insert(key, computed.clone());
        computed
    }
}

#[derive(Clone)]
pub(super) enum CachedColumns {
    Mono(std::sync::Arc<[(f32, f32)]>),
    SplitStereo {
        left: std::sync::Arc<[(f32, f32)]>,
        right: std::sync::Arc<[(f32, f32)]>,
    },
}

#[derive(Clone, Copy, Debug, Eq)]
struct CacheKey {
    cache_token: u64,
    samples_len: usize,
    channels: u16,
    view: WaveformChannelView,
    width: u32,
}

impl CacheKey {
    fn new(
        cache_token: u64,
        samples: &[f32],
        channels: usize,
        view: WaveformChannelView,
        width: u32,
    ) -> Self {
        Self {
            cache_token,
            samples_len: samples.len(),
            channels: channels.min(u16::MAX as usize) as u16,
            view,
            width,
        }
    }
}

impl PartialEq for CacheKey {
    fn eq(&self, other: &Self) -> bool {
        self.cache_token == other.cache_token
            && self.samples_len == other.samples_len
            && self.channels == other.channels
            && self.view == other.view
            && self.width == other.width
    }
}

impl Hash for CacheKey {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.cache_token.hash(state);
        self.samples_len.hash(state);
        self.channels.hash(state);
        self.view.hash(state);
        self.width.hash(state);
    }
}

struct CacheInner {
    map: HashMap<CacheKey, CachedColumns>,
    order: VecDeque<CacheKey>,
    max_entries: usize,
}

impl CacheInner {
    fn new() -> Self {
        Self {
            map: HashMap::new(),
            order: VecDeque::new(),
            max_entries: 12,
        }
    }

    fn touch(&mut self, key: CacheKey) {
        self.order.retain(|existing| existing != &key);
        self.order.push_back(key);
    }

    fn insert(&mut self, key: CacheKey, value: CachedColumns) {
        self.map.insert(key, value);
        self.touch(key);
        self.evict();
    }

    fn evict(&mut self) {
        while self.map.len() > self.max_entries {
            let Some(key) = self.order.pop_front() else {
                break;
            };
            if self.map.remove(&key).is_some() {
                break;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        sync::{mpsc, Arc, Barrier},
        thread,
        time::Duration,
    };

    fn first_mono_column(columns: &CachedColumns) -> (f32, f32) {
        match columns {
            CachedColumns::Mono(cols) => cols[0],
            CachedColumns::SplitStereo { .. } => panic!("expected mono columns"),
        }
    }

    #[test]
    fn cache_token_prevents_stale_hits_when_memory_is_reused() {
        let cache = WaveformZoomCache::new();
        let mut samples = vec![0.0_f32, 1.0, 0.0, 1.0];

        let initial = cache.get_or_compute(1, &samples, 1, WaveformChannelView::Mono, 1);
        samples.fill(1.0);
        let changed = cache.get_or_compute(2, &samples, 1, WaveformChannelView::Mono, 1);

        assert_ne!(first_mono_column(&initial), first_mono_column(&changed));
    }

    #[test]
    fn cache_order_stays_bounded_for_repeated_touch() {
        let mut inner = CacheInner::new();
        let samples = vec![0.0_f32, 1.0];
        let key = CacheKey::new(1, &samples, 1, WaveformChannelView::Mono, 10);
        let value = CachedColumns::Mono(std::sync::Arc::from([(0.0, 1.0)]));

        inner.insert(key, value);
        for _ in 0..10 {
            inner.touch(key);
        }

        assert_eq!(inner.order.len(), inner.map.len());
        assert_eq!(inner.order.len(), 1);
    }

    #[test]
    fn get_or_compute_allows_parallel_requests() {
        let cache = Arc::new(WaveformZoomCache::new());
        let samples = Arc::new(vec![0.0_f32, 1.0, 0.0, 1.0]);
        let threads = 8;
        let barrier = Arc::new(Barrier::new(threads));
        let (tx, rx) = mpsc::channel();
        let mut handles = Vec::with_capacity(threads);

        for _ in 0..threads {
            let cache = Arc::clone(&cache);
            let samples = Arc::clone(&samples);
            let barrier = Arc::clone(&barrier);
            let tx = tx.clone();
            handles.push(thread::spawn(move || {
                barrier.wait();
                let columns =
                    cache.get_or_compute(1, &samples, 1, WaveformChannelView::Mono, 32);
                tx.send(first_mono_column(&columns))
                    .expect("send waveform column");
            }));
        }
        drop(tx);

        let mut results = Vec::with_capacity(threads);
        for _ in 0..threads {
            results.push(
                rx.recv_timeout(Duration::from_secs(2))
                    .expect("receive waveform column"),
            );
        }
        for handle in handles {
            handle.join().expect("join waveform thread");
        }

        for result in results.iter().skip(1) {
            assert_eq!(*result, results[0]);
        }
    }
}
