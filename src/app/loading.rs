use super::*;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::mpsc::{Receiver, Sender};

/// Work item describing which waveform to load and render off the UI thread.
pub(super) struct WaveformJob {
    pub source_id: SourceId,
    pub root: PathBuf,
    pub relative_path: PathBuf,
}

/// Successful decode payload returned from the loader thread.
pub(super) struct WaveformJobPayload {
    pub samples: Vec<f32>,
    pub audio_bytes: Vec<u8>,
    pub duration_seconds: f32,
}

/// Result message sent back to the UI thread after attempting a load.
pub(super) struct WaveformJobResult {
    pub source_id: SourceId,
    pub relative_path: PathBuf,
    pub result: Result<WaveformJobPayload, String>,
}

/// Continuously processes waveform jobs on a background thread.
pub(super) fn run_waveform_worker(
    renderer: WaveformRenderer,
    jobs: Receiver<WaveformJob>,
    results: Sender<WaveformJobResult>,
) {
    while let Ok(job) = jobs.recv() {
        let result = load_waveform_job(&renderer, &job);
        let _ = results.send(WaveformJobResult {
            source_id: job.source_id,
            relative_path: job.relative_path,
            result,
        });
    }
}

fn load_waveform_job(
    renderer: &WaveformRenderer,
    job: &WaveformJob,
) -> Result<WaveformJobPayload, String> {
    let full_path = job.root.join(&job.relative_path);
    let bytes = std::fs::read(&full_path)
        .map_err(|error| format!("Failed to read {}: {error}", full_path.display()))?;
    let decoded = renderer.decode_from_bytes(&bytes)?;
    Ok(WaveformJobPayload {
        samples: decoded.samples,
        audio_bytes: bytes,
        duration_seconds: decoded.duration_seconds,
    })
}

struct CacheEntry {
    source_id: SourceId,
    relative_path: PathBuf,
    waveform: Rc<LoadedWaveform>,
    last_used: u64,
}

/// Small LRU cache for recently rendered waveforms to avoid repeat decodes.
pub(super) struct WaveformCache {
    entries: Vec<CacheEntry>,
    capacity: usize,
    tick: u64,
}

impl WaveformCache {
    /// Create a cache that keeps up to `capacity` waveforms.
    pub fn new(capacity: usize) -> Self {
        Self {
            entries: Vec::new(),
            capacity: capacity.max(1),
            tick: 0,
        }
    }

    /// Retrieve a cached waveform for a path, updating LRU usage.
    pub fn get(
        &mut self,
        source_id: &SourceId,
        relative_path: &Path,
    ) -> Option<Rc<LoadedWaveform>> {
        self.tick = self.tick.saturating_add(1);
        let entry = self
            .entries
            .iter_mut()
            .find(|entry| entry.source_id == *source_id && entry.relative_path == relative_path)?;
        entry.last_used = self.tick;
        Some(entry.waveform.clone())
    }

    /// Store a waveform and evict the least-recently-used item when full.
    pub fn insert(
        &mut self,
        source_id: SourceId,
        relative_path: PathBuf,
        waveform: Rc<LoadedWaveform>,
    ) {
        self.tick = self.tick.saturating_add(1);
        if let Some(existing) = self
            .entries
            .iter_mut()
            .find(|entry| entry.source_id == source_id && entry.relative_path == relative_path)
        {
            existing.waveform = waveform;
            existing.last_used = self.tick;
            return;
        }
        self.entries.push(CacheEntry {
            source_id,
            relative_path,
            waveform,
            last_used: self.tick,
        });
        if self.entries.len() > self.capacity {
            self.evict_lru();
        }
    }

    fn evict_lru(&mut self) {
        if let Some((index, _)) = self
            .entries
            .iter()
            .enumerate()
            .min_by_key(|(_, entry)| entry.last_used)
        {
            self.entries.swap_remove(index);
        }
    }
}

impl DropHandler {
    /// Start polling for completed waveform loads.
    pub(super) fn start_waveform_polling(&self) {
        if *self.shutting_down.borrow() {
            return;
        }
        let poller = self.clone();
        self.waveform_poll_timer.start(
            slint::TimerMode::Repeated,
            Duration::from_millis(50),
            move || poller.process_waveform_queue(),
        );
    }

    /// Queue a waveform load for the worker thread.
    pub(super) fn enqueue_waveform_load(
        &self,
        source_id: &SourceId,
        root: &Path,
        relative_path: &Path,
    ) {
        if *self.shutting_down.borrow() {
            return;
        }
        let job = WaveformJob {
            source_id: source_id.clone(),
            root: root.to_path_buf(),
            relative_path: relative_path.to_path_buf(),
        };
        let _ = self.waveform_tx.send(job);
    }

    /// Process any pending waveform worker results.
    pub(super) fn process_waveform_queue(&self) {
        let Some(app) = self.app() else {
            return;
        };
        while let Ok(message) = self.waveform_rx.borrow().try_recv() {
            self.handle_waveform_result(&app, message);
        }
    }

    fn handle_waveform_result(&self, app: &HelloWorld, message: WaveformJobResult) {
        let relative_path = message.relative_path;
        match message.result {
            Ok(payload) => {
                self.apply_waveform_payload(app, message.source_id, relative_path, payload);
            }
            Err(error) => {
                if self.is_current_selection(&message.source_id, &relative_path) {
                    self.set_status(app, error, StatusState::Error);
                }
            }
        }
    }

    fn apply_waveform_payload(
        &self,
        app: &HelloWorld,
        source_id: SourceId,
        relative_path: PathBuf,
        payload: WaveformJobPayload,
    ) {
        let image = self.renderer.render_from_samples(&payload.samples);
        let loaded = Rc::new(LoadedWaveform {
            image,
            audio_bytes: payload.audio_bytes,
            duration_seconds: payload.duration_seconds,
        });
        self.waveform_cache.borrow_mut().insert(
            source_id.clone(),
            relative_path.clone(),
            loaded.clone(),
        );
        if !self.is_current_selection(&source_id, &relative_path) {
            return;
        }
        self.loaded_wav.borrow_mut().replace(relative_path.clone());
        self.update_wav_view(app, false);
        self.apply_loaded_waveform(app, &loaded);
        self.set_status(
            app,
            format!("Loaded {}", relative_path.display()),
            StatusState::Info,
        );
        let _ = self.play_audio(*self.loop_enabled.borrow());
    }

    fn is_current_selection(&self, source_id: &SourceId, relative_path: &Path) -> bool {
        let matches_source = self
            .selected_source
            .borrow()
            .as_ref()
            .is_some_and(|id| id == source_id);
        let matches_path = self
            .selected_wav
            .borrow()
            .as_ref()
            .is_some_and(|path| path == relative_path);
        matches_source && matches_path
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dummy_waveform() -> Rc<LoadedWaveform> {
        let renderer = WaveformRenderer::new(4, 2);
        Rc::new(LoadedWaveform {
            image: renderer.empty_image(),
            audio_bytes: vec![],
            duration_seconds: 1.0,
        })
    }

    #[test]
    fn cache_keeps_most_recent_entries() {
        let mut cache = WaveformCache::new(1);
        let source = SourceId::new();
        cache.insert(source.clone(), PathBuf::from("one.wav"), dummy_waveform());
        cache.insert(source.clone(), PathBuf::from("two.wav"), dummy_waveform());
        assert!(cache.get(&source, Path::new("one.wav")).is_none());
        assert!(cache.get(&source, Path::new("two.wav")).is_some());
    }

    #[test]
    fn cache_updates_usage_on_get() {
        let mut cache = WaveformCache::new(2);
        let source = SourceId::new();
        cache.insert(source.clone(), PathBuf::from("one.wav"), dummy_waveform());
        cache.insert(source.clone(), PathBuf::from("two.wav"), dummy_waveform());
        assert!(cache.get(&source, Path::new("one.wav")).is_some());
        cache.insert(source.clone(), PathBuf::from("three.wav"), dummy_waveform());
        assert!(cache.get(&source, Path::new("one.wav")).is_some());
        assert!(cache.get(&source, Path::new("two.wav")).is_none());
    }
}
