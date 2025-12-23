use std::sync::Arc;
use std::time::Duration;

use rodio::Source;

pub(crate) fn fade_duration(span_seconds: f32, max_fade: Duration) -> Duration {
    if span_seconds <= 0.0 || max_fade.is_zero() {
        return Duration::ZERO;
    }
    let max_fade_secs = max_fade.as_secs_f32();
    let clamped = max_fade_secs.min(span_seconds * 0.5);
    duration_from_secs_f32(clamped)
}

pub(crate) fn fade_frames_for_duration(sample_rate: u32, fade: Duration) -> u32 {
    if fade.is_zero() || sample_rate == 0 {
        return 1;
    }
    let frames = (fade.as_secs_f64() * sample_rate as f64).ceil();
    frames.clamp(1.0, u32::MAX as f64) as u32
}

fn duration_from_secs_f32(seconds: f32) -> Duration {
    if !seconds.is_finite() || seconds <= 0.0 {
        return Duration::ZERO;
    }
    Duration::from_secs_f64(seconds as f64)
}

#[derive(Clone)]
pub(crate) struct EdgeFade<S> {
    inner: S,
    fade_secs: f32,
    total_secs: Option<f32>,
    fade_out_start: Option<f32>,
    sample_rate: u32,
    channels: u16,
    samples_emitted: u64,
}

impl<S> EdgeFade<S> {
    pub(crate) fn new(inner: S, fade: Duration) -> Self
    where
        S: Source<Item = f32> + Clone,
    {
        let fade_secs = fade.as_secs_f32();
        let total_secs = inner.total_duration().map(|d| d.as_secs_f32());
        let fade_out_start = total_secs.and_then(|total| {
            if fade_secs <= 0.0 || fade_secs >= total {
                None
            } else {
                Some(total - fade_secs)
            }
        });
        let sample_rate = inner.sample_rate();
        let channels = inner.channels();
        Self {
            inner,
            fade_secs,
            total_secs,
            fade_out_start,
            sample_rate,
            channels,
            samples_emitted: 0,
        }
    }
}

impl<S> Iterator for EdgeFade<S>
where
    S: Source<Item = f32>,
{
    type Item = f32;

    fn next(&mut self) -> Option<Self::Item> {
        let sample = self.inner.next()?;
        let time = if self.sample_rate > 0 && self.channels > 0 {
            self.samples_emitted as f32 / (self.sample_rate as f32 * self.channels as f32)
        } else {
            0.0
        };
        self.samples_emitted = self.samples_emitted.saturating_add(1);
        if self.fade_secs <= 0.0 {
            return Some(sample);
        }
        let mut factor = 1.0;
        if time < self.fade_secs {
            factor *= (time / self.fade_secs).clamp(0.0, 1.0);
        }
        if let (Some(total), Some(start)) = (self.total_secs, self.fade_out_start)
            && time > start
        {
            factor *= ((total - time) / self.fade_secs).clamp(0.0, 1.0);
        }
        Some(sample * factor)
    }
}

impl<S> Source for EdgeFade<S>
where
    S: Source<Item = f32>,
{
    #[inline]
    fn current_span_len(&self) -> Option<usize> {
        self.inner.current_span_len()
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

#[derive(Clone)]
pub(crate) struct FadeOutHandle {
    requested_frames: Arc<std::sync::atomic::AtomicU32>,
}

impl FadeOutHandle {
    pub(crate) fn new() -> Self {
        Self {
            requested_frames: Arc::new(std::sync::atomic::AtomicU32::new(0)),
        }
    }

    pub(crate) fn request_fade_out_frames(&self, frames: u32) {
        self.requested_frames
            .store(frames.max(1), std::sync::atomic::Ordering::Release);
    }

    fn take_requested_frames(&self) -> Option<u32> {
        let frames = self
            .requested_frames
            .swap(0, std::sync::atomic::Ordering::AcqRel);
        if frames == 0 { None } else { Some(frames) }
    }
}

pub(crate) struct FadeOutOnRequest<S> {
    inner: S,
    handle: FadeOutHandle,
    sample_rate: u32,
    channels: u16,
    samples_emitted: u64,
    pending_frames: Option<u32>,
    fade_start_frame: u64,
    fade_total_frames: u32,
    fading: bool,
}

impl<S> FadeOutOnRequest<S>
where
    S: Source<Item = f32>,
{
    pub(crate) fn new(inner: S, handle: FadeOutHandle) -> Self {
        let sample_rate = inner.sample_rate();
        let channels = inner.channels();
        Self {
            inner,
            handle,
            sample_rate,
            channels,
            samples_emitted: 0,
            pending_frames: None,
            fade_start_frame: 0,
            fade_total_frames: 0,
            fading: false,
        }
    }

    fn current_frame(&self) -> u64 {
        let channels = self.channels.max(1) as u64;
        self.samples_emitted / channels
    }

    fn is_frame_boundary(&self) -> bool {
        let channels = self.channels.max(1) as u64;
        self.samples_emitted % channels == 0
    }

    fn start_fade_if_ready(&mut self) {
        if self.fading {
            return;
        }
        if self.pending_frames.is_none() {
            self.pending_frames = self.handle.take_requested_frames();
        }
        if self.pending_frames.is_none() || !self.is_frame_boundary() {
            return;
        }
        let Some(frames) = self.pending_frames.take() else {
            return;
        };
        self.fading = true;
        self.fade_start_frame = self.current_frame();
        self.fade_total_frames = frames.max(1);
    }

    fn fade_factor(&self) -> f32 {
        if !self.fading {
            return 1.0;
        }
        let current_frame = self.current_frame();
        let offset = current_frame.saturating_sub(self.fade_start_frame);
        let total = self.fade_total_frames.max(1) as u64;
        if offset >= total {
            return 0.0;
        }
        if total <= 1 {
            return 0.0;
        }
        let denom = (total - 1) as f32;
        let progress = (offset as f32 / denom).clamp(0.0, 1.0);
        (1.0 - progress).clamp(0.0, 1.0)
    }
}

impl<S> Iterator for FadeOutOnRequest<S>
where
    S: Source<Item = f32>,
{
    type Item = f32;

    fn next(&mut self) -> Option<Self::Item> {
        self.start_fade_if_ready();
        if self.fading && self.is_frame_boundary() {
            let current_frame = self.current_frame();
            let offset = current_frame.saturating_sub(self.fade_start_frame);
            if offset >= self.fade_total_frames as u64 {
                return None;
            }
        }

        let sample = self.inner.next()?;
        let factor = self.fade_factor();
        self.samples_emitted = self.samples_emitted.saturating_add(1);
        Some(sample * factor)
    }
}

impl<S> Source for FadeOutOnRequest<S>
where
    S: Source<Item = f32>,
{
    #[inline]
    fn current_span_len(&self) -> Option<usize> {
        self.inner.current_span_len()
    }

    #[inline]
    fn channels(&self) -> u16 {
        self.channels
    }

    #[inline]
    fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    #[inline]
    fn total_duration(&self) -> Option<Duration> {
        None
    }
}
