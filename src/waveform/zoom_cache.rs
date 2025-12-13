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
        samples: &[f32],
        channels: usize,
        view: WaveformChannelView,
        width: u32,
    ) -> CachedColumns {
        let key = CacheKey::new(samples, channels, view, width);
        let mut inner = self.inner.lock().expect("waveform zoom cache lock");
        if let Some(hit) = inner.map.get(&key).cloned() {
            inner.touch(key);
            return hit;
        }

        let computed = match WaveformRenderer::sample_columns_for_width(samples, channels, width, view)
        {
            WaveformColumnView::Mono(cols) => CachedColumns::Mono(cols.into()),
            WaveformColumnView::SplitStereo { left, right } => CachedColumns::SplitStereo {
                left: left.into(),
                right: right.into(),
            },
        };
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
    samples_ptr: usize,
    samples_len: usize,
    channels: u16,
    view: WaveformChannelView,
    width: u32,
}

impl CacheKey {
    fn new(samples: &[f32], channels: usize, view: WaveformChannelView, width: u32) -> Self {
        Self {
            samples_ptr: samples.as_ptr() as usize,
            samples_len: samples.len(),
            channels: channels.min(u16::MAX as usize) as u16,
            view,
            width,
        }
    }
}

impl PartialEq for CacheKey {
    fn eq(&self, other: &Self) -> bool {
        self.samples_ptr == other.samples_ptr
            && self.samples_len == other.samples_len
            && self.channels == other.channels
            && self.view == other.view
            && self.width == other.width
    }
}

impl Hash for CacheKey {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.samples_ptr.hash(state);
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
