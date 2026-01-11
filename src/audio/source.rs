use std::time::Duration;

/// Trait for audio sources that can provide samples.
pub trait Source: Iterator<Item = f32> + Send {
    /// Returns the number of samples in the current frame, if known.
    fn current_frame_len(&self) -> Option<usize>;
    
    /// Returns the number of channels.
    fn channels(&self) -> u16;
    
    /// Returns the sample rate.
    fn sample_rate(&self) -> u32;
    
    /// Returns the total duration of the source, if known.
    fn total_duration(&self) -> Option<Duration>;

    /// Returns the last error encountered by the source, if any.
    fn last_error(&self) -> Option<String> {
        None
    }

    /// Limits the duration of the source.
    fn take_duration(self, duration: Duration) -> TakeDuration<Self>
    where
        Self: Sized,
    {
        TakeDuration {
            inner: self,
            remaining_samples: None,
            duration,
        }
    }

    /// Repeats the source infinitely.
    fn repeat_infinite(self) -> RepeatInfinite<Self>
    where
        Self: Sized + Clone,
    {
        RepeatInfinite {
            inner: self.clone(),
            source: self,
        }
    }

    /// Buffers the source into memory.
    fn buffered(self) -> Buffered<Self>
    where
        Self: Sized,
    {
        Buffered {
            inner: self,
            buffer: Vec::new(),
            pos: 0,
            finished: false,
        }
    }

    /// Skips a certain duration from the beginning of the source.
    fn skip_duration(mut self, duration: Duration) -> Self
    where
        Self: Sized,
    {
        let sample_rate = self.sample_rate();
        let channels = self.channels();
        let samples_to_skip = (duration.as_secs_f64() * sample_rate as f64 * channels as f64).round() as usize;
        for _ in 0..samples_to_skip {
            if self.next().is_none() {
                break;
            }
        }
        self
    }

    /// Fades in the source.
    fn fade_in(self, duration: Duration) -> FadeIn<Self>
    where
        Self: Sized,
    {
        FadeIn {
            inner: self,
            fade_duration: duration,
            samples_emitted: 0,
        }
    }
}

impl Source for Box<dyn Source + Send> {
    fn current_frame_len(&self) -> Option<usize> {
        (**self).current_frame_len()
    }

    fn channels(&self) -> u16 {
        (**self).channels()
    }

    fn sample_rate(&self) -> u32 {
        (**self).sample_rate()
    }

    fn total_duration(&self) -> Option<Duration> {
        (**self).total_duration()
    }

    fn last_error(&self) -> Option<String> {
        (**self).last_error()
    }
}

/// A simple buffer of samples implementing Source.
pub struct SamplesBuffer {
    samples: Vec<f32>,
    channels: u16,
    sample_rate: u32,
    pos: usize,
}

impl SamplesBuffer {
    pub fn new(channels: u16, sample_rate: u32, samples: Vec<f32>) -> Self {
        Self {
            samples,
            channels,
            sample_rate,
            pos: 0,
        }
    }
}

impl Iterator for SamplesBuffer {
    type Item = f32;

    fn next(&mut self) -> Option<Self::Item> {
        if self.pos < self.samples.len() {
            let sample = self.samples[self.pos];
            self.pos += 1;
            Some(sample)
        } else {
            None
        }
    }
}

impl Source for SamplesBuffer {
    fn current_frame_len(&self) -> Option<usize> {
        Some(self.samples.len().saturating_sub(self.pos))
    }

    fn channels(&self) -> u16 {
        self.channels
    }

    fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    fn total_duration(&self) -> Option<Duration> {
        let frames = self.samples.len() as u64 / self.channels as u64;
        Some(Duration::from_nanos((frames * 1_000_000_000) / self.sample_rate as u64))
    }
}

impl Clone for SamplesBuffer {
    fn clone(&self) -> Self {
        Self {
            samples: self.samples.clone(),
            channels: self.channels,
            sample_rate: self.sample_rate,
            pos: 0,
        }
    }
}

/// Source that limits duration.
pub struct TakeDuration<S> {
    inner: S,
    remaining_samples: Option<usize>,
    duration: Duration,
}

impl<S> Iterator for TakeDuration<S>
where
    S: Source,
{
    type Item = f32;

    fn next(&mut self) -> Option<Self::Item> {
        if self.remaining_samples.is_none() {
            let sample_rate = self.inner.sample_rate();
            let channels = self.inner.channels();
            let samples = (self.duration.as_secs_f64() * sample_rate as f64 * channels as f64).round() as usize;
            self.remaining_samples = Some(samples);
        }

        let remaining = self.remaining_samples.as_mut().unwrap();
        if *remaining == 0 {
            return None;
        }

        *remaining -= 1;
        self.inner.next()
    }
}

impl<S> Source for TakeDuration<S>
where
    S: Source,
{
    fn current_frame_len(&self) -> Option<usize> {
        self.inner.current_frame_len()
            .map(|l| l.min(self.remaining_samples.unwrap_or(usize::MAX)))
    }

    fn channels(&self) -> u16 {
        self.inner.channels()
    }

    fn sample_rate(&self) -> u32 {
        self.inner.sample_rate()
    }

    fn total_duration(&self) -> Option<Duration> {
        Some(self.duration)
    }
}

/// Source that repeats infinitely.
pub struct RepeatInfinite<S> {
    inner: S,
    source: S,
}

impl<S> Iterator for RepeatInfinite<S>
where
    S: Source + Clone,
{
    type Item = f32;

    fn next(&mut self) -> Option<Self::Item> {
        if let Some(sample) = self.source.next() {
            Some(sample)
        } else {
            self.source = self.inner.clone();
            self.source.next()
        }
    }
}

impl<S> Source for RepeatInfinite<S>
where
    S: Source + Clone,
{
    fn current_frame_len(&self) -> Option<usize> {
        None
    }

    fn channels(&self) -> u16 {
        self.inner.channels()
    }

    fn sample_rate(&self) -> u32 {
        self.inner.sample_rate()
    }

    fn total_duration(&self) -> Option<Duration> {
        None
    }
}

/// Source that is buffered into memory.
pub struct Buffered<S> {
    inner: S,
    buffer: Vec<f32>,
    pos: usize,
    finished: bool,
}

impl<S> Iterator for Buffered<S>
where
    S: Source,
{
    type Item = f32;

    fn next(&mut self) -> Option<Self::Item> {
        if self.pos < self.buffer.len() {
            let sample = self.buffer[self.pos];
            self.pos += 1;
            return Some(sample);
        }

        if self.finished {
            return None;
        }

        if let Some(sample) = self.inner.next() {
            self.buffer.push(sample);
            self.pos += 1;
            Some(sample)
        } else {
            self.finished = true;
            None
        }
    }
}

impl<S> Source for Buffered<S>
where
    S: Source,
{
    fn current_frame_len(&self) -> Option<usize> {
        if self.finished {
            Some(self.buffer.len().saturating_sub(self.pos))
        } else {
            None
        }
    }

    fn channels(&self) -> u16 {
        self.inner.channels()
    }

    fn sample_rate(&self) -> u32 {
        self.inner.sample_rate()
    }

    fn total_duration(&self) -> Option<Duration> {
        self.inner.total_duration()
    }
}

/// Source that fades in.
pub struct FadeIn<S> {
    inner: S,
    fade_duration: Duration,
    samples_emitted: u64,
}

impl<S> Iterator for FadeIn<S>
where
    S: Source,
{
    type Item = f32;

    fn next(&mut self) -> Option<Self::Item> {
        let sample = self.inner.next()?;
        let sample_rate = self.inner.sample_rate();
        let channels = self.inner.channels();
        let fade_samples = (self.fade_duration.as_secs_f64() * sample_rate as f64 * channels as f64) as u64;
        
        let factor = if fade_samples == 0 {
            1.0
        } else {
            (self.samples_emitted as f32 / fade_samples as f32).min(1.0)
        };
        
        self.samples_emitted = self.samples_emitted.saturating_add(1);
        Some(sample * factor)
    }
}

impl<S> Source for FadeIn<S>
where
    S: Source,
{
    fn current_frame_len(&self) -> Option<usize> {
        self.inner.current_frame_len()
    }

    fn channels(&self) -> u16 {
        self.inner.channels()
    }

    fn sample_rate(&self) -> u32 {
        self.inner.sample_rate()
    }

    fn total_duration(&self) -> Option<Duration> {
        self.inner.total_duration()
    }
}
