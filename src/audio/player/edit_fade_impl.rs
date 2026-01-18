use std::sync::{Arc, RwLock};
use std::time::Duration;
use crate::audio::Source;
use crate::selection::{SelectionRange, FadeParams};

/// State shared between the controller and the audio thread.
#[derive(Clone, Default, Debug)]
pub(crate) struct EditFadeState {
    pub active: bool,
    /// Absolute start time of the selection in seconds
    pub start_seconds: f32,
    /// Absolute end time of the selection in seconds
    pub end_seconds: f32,
    pub fade_in: Option<FadeParams>,
    pub fade_out: Option<FadeParams>,
}

#[derive(Clone, Debug)]
pub(crate) struct EditFadeHandle {
    state: Arc<RwLock<EditFadeState>>,
}

impl EditFadeHandle {
    pub(crate) fn new() -> Self {
        Self {
            state: Arc::new(RwLock::new(EditFadeState::default())),
        }
    }

    pub(crate) fn update(&self, range: Option<SelectionRange>, total_duration_secs: f32) {
        if let Ok(mut state) = self.state.write() {
            if let Some(range) = range {
                state.active = true;
                state.start_seconds = range.start() * total_duration_secs;
                state.end_seconds = range.end() * total_duration_secs;
                state.fade_in = range.fade_in();
                state.fade_out = range.fade_out();
            } else {
                state.active = false;
            }
        }
    }

    pub(crate) fn get_state(&self) -> EditFadeState {
        self.state.read().unwrap_or_else(|e| e.into_inner()).clone()
    }
}

/// A Source that applies fades based on the live EditFadeState.
#[derive(Clone)]
pub(crate) struct EditFadeSource<S> {
    inner: S,
    handle: EditFadeHandle,
    /// The global timestamp (relative to track start) where this source segment begins.
    global_start_secs: f32,
    sample_rate: u32,
    channels: u16,
    samples_emitted: u64,
}

impl<S> EditFadeSource<S>
where
    S: Source,
{
    pub(crate) fn new(inner: S, handle: EditFadeHandle, global_start_secs: f32) -> Self {
        let sample_rate = inner.sample_rate();
        let channels = inner.channels();
        Self {
            inner,
            handle,
            global_start_secs,
            sample_rate,
            channels,
            samples_emitted: 0,
        }
    }

    fn apply_s_curve(t: f32, curve: f32) -> f32 {
        if curve <= 0.0 {
            return t;
        }
        // smootherstep: 6t^5 - 15t^4 + 10t^3
        let t2 = t * t;
        let t3 = t2 * t;
        let smootherstep = t3 * (t * (t * 6.0 - 15.0) + 10.0);
        
        t * (1.0 - curve) + smootherstep * curve
    }
}

impl<S> Iterator for EditFadeSource<S>
where
    S: Source<Item = f32>,
{
    type Item = f32;

    fn next(&mut self) -> Option<Self::Item> {
        let sample = self.inner.next()?;
        
        // Calculate current time in seconds relative to the whole track
        let local_time = if self.sample_rate > 0 && self.channels > 0 {
            (self.samples_emitted / self.channels as u64) as f32 / self.sample_rate as f32
        } else {
            0.0
        };
        let current_time = self.global_start_secs + local_time;
        
        self.samples_emitted += 1;

        // Get fade state (locking briefly)
        let state = self.handle.get_state();

        if !state.active {
            return Some(sample);
        }

        // Check if inside selection
        if current_time < state.start_seconds || current_time > state.end_seconds {
            return Some(sample);
        }

        let selection_width = state.end_seconds - state.start_seconds;
        if selection_width <= 0.0001 {
            return Some(sample);
        }

        let mut gain = 1.0;

        // Apply Fade In
        if let Some(fade_in) = state.fade_in {
            let fade_len_secs = selection_width * fade_in.length;
             // relative to start
            let time_in_sel = current_time - state.start_seconds;
            if time_in_sel < fade_len_secs {
                let t = (time_in_sel / fade_len_secs).clamp(0.0, 1.0);
                gain *= Self::apply_s_curve(t, fade_in.curve);
            }
        }

        // Apply Fade Out
        if let Some(fade_out) = state.fade_out {
            let fade_len_secs = selection_width * fade_out.length;
             // relative to end
            let time_until_end = state.end_seconds - current_time;
            if time_until_end < fade_len_secs {
                let t = (time_until_end / fade_len_secs).clamp(0.0, 1.0);
                gain *= Self::apply_s_curve(t, fade_out.curve);
            }
        }

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
