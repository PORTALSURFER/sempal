use std::sync::{Arc, RwLock};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;
use crate::audio::Source;
use crate::selection::{FadeParams, SelectionRange, fade_gain_at_position};

/// Snapshot of the current edit fade configuration.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct EditFadeSnapshot {
    pub active: bool,
    /// Absolute start time of the selection in seconds
    pub start_seconds: f32,
    /// Absolute end time of the selection in seconds
    pub end_seconds: f32,
    /// Gain applied across the selection.
    pub gain: f32,
    pub fade_in: Option<FadeParams>,
    pub fade_out: Option<FadeParams>,
}

#[derive(Debug, Default)]
struct EditFadeShared {
    state: RwLock<EditFadeSnapshot>,
    version: AtomicU64,
}

#[derive(Clone, Debug)]
pub(crate) struct EditFadeHandle {
    shared: Arc<EditFadeShared>,
}

impl EditFadeHandle {
    pub(crate) fn new() -> Self {
        Self {
            shared: Arc::new(EditFadeShared::default()),
        }
    }

    pub(crate) fn update(&self, range: Option<SelectionRange>, total_duration_secs: f32) {
        if let Ok(mut state) = self.shared.state.write() {
            if let Some(range) = range {
                state.active = true;
                state.start_seconds = range.start() * total_duration_secs;
                state.end_seconds = range.end() * total_duration_secs;
                state.gain = range.gain();
                state.fade_in = range.fade_in();
                state.fade_out = range.fade_out();
            } else {
                state.active = false;
                state.gain = 1.0;
                state.fade_in = None;
                state.fade_out = None;
            }
            self.shared.version.fetch_add(1, Ordering::Release);
        }
    }

    /// Return the current edit-fade snapshot version.
    pub(crate) fn version(&self) -> u64 {
        self.shared.version.load(Ordering::Acquire)
    }

    /// Return a copy of the latest edit-fade snapshot.
    pub(crate) fn snapshot(&self) -> EditFadeSnapshot {
        self.shared
            .state
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }
}

/// A Source that applies fades based on the live EditFadeState.
#[derive(Clone)]
pub(crate) struct EditFadeSource<S> {
    inner: S,
    handle: EditFadeHandle,
    /// The global timestamp (relative to track start) where this source segment begins.
    global_start_secs: f32,
    /// Optional number of frames in a loop, used to repeat fades each cycle.
    loop_frames: Option<u64>,
    /// Initial frame offset applied before looping.
    frame_offset: u64,
    sample_rate: u32,
    channels: u16,
    samples_emitted: u64,
    cached_state: EditFadeSnapshot,
    cached_version: u64,
}

impl<S> EditFadeSource<S>
where
    S: Source,
{
    pub(crate) fn new(inner: S, handle: EditFadeHandle, global_start_secs: f32) -> Self {
        Self::with_loop(inner, handle, global_start_secs, None, 0)
    }

    /// Create a source that repeats edit fades for each loop iteration.
    pub(crate) fn new_looped(
        inner: S,
        handle: EditFadeHandle,
        global_start_secs: f32,
        loop_frames: u64,
        frame_offset: u64,
    ) -> Self {
        let loop_frames = if loop_frames > 0 { Some(loop_frames) } else { None };
        Self::with_loop(inner, handle, global_start_secs, loop_frames, frame_offset)
    }

    fn with_loop(
        inner: S,
        handle: EditFadeHandle,
        global_start_secs: f32,
        loop_frames: Option<u64>,
        frame_offset: u64,
    ) -> Self {
        let sample_rate = inner.sample_rate();
        let channels = inner.channels();
        let cached_version = handle.version();
        let cached_state = handle.snapshot();
        Self {
            inner,
            handle,
            global_start_secs,
            loop_frames,
            frame_offset,
            sample_rate,
            channels,
            samples_emitted: 0,
            cached_state,
            cached_version,
        }
    }

    fn refresh_state_if_needed(&mut self) {
        let version = self.handle.version();
        if version == self.cached_version {
            return;
        }
        self.cached_state = self.handle.snapshot();
        self.cached_version = version;
    }
}

impl<S> Iterator for EditFadeSource<S>
where
    S: Source<Item = f32>,
{
    type Item = f32;

    fn next(&mut self) -> Option<Self::Item> {
        let sample = self.inner.next()?;

        self.refresh_state_if_needed();
        let state = self.cached_state;

        if !state.active {
            self.samples_emitted += 1;
            return Some(sample);
        }

        let frame_index = self.samples_emitted / self.channels.max(1) as u64;
        let frame_index = frame_index.saturating_add(self.frame_offset);
        let frame_index = if let Some(loop_frames) = self.loop_frames {
            frame_index % loop_frames.max(1)
        } else {
            frame_index
        };
        let local_time = if self.sample_rate > 0 {
            frame_index as f32 / self.sample_rate as f32
        } else {
            0.0
        };
        let current_time = self.global_start_secs + local_time;

        self.samples_emitted += 1;

        let gain = fade_gain_at_position(
            current_time,
            state.start_seconds,
            state.end_seconds,
            state.gain,
            state.fade_in,
            state.fade_out,
        );
        Some(sample * gain)
    }
}

impl<S> Source for EditFadeSource<S>
where
    S: Source<Item = f32>,
{
    #[inline]
    fn current_frame_len(&self) -> Option<usize> {
        self.inner.current_frame_len()
    }

    #[inline]
    fn channels(&self) -> u16 {
        self.inner.channels()
    }

    #[inline]
    fn sample_rate(&self) -> u32 {
        self.inner.sample_rate()
    }

    #[inline]
    fn total_duration(&self) -> Option<Duration> {
        self.inner.total_duration()
    }
}
