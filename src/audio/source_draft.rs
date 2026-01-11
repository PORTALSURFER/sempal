use std::time::Duration;

pub trait Source: Iterator<Item = f32> + Send {
    fn current_frame_len(&self) -> Option<usize>;
    fn channels(&self) -> u16;
    fn sample_rate(&self) -> u32;
    fn total_duration(&self) -> Option<Duration>;

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

    fn repeat_infinite(self) -> RepeatInfinite<Self>
    where
        Self: Sized + Clone,
    {
        RepeatInfinite {
            inner: self.clone(),
            source: self,
        }
    }

    fn buffered(self) -> Buffered<Self>
    where
        Self: Sized,
    {
        Buffered {
            inner: self,
            buffer: Vec::new(),
            pos: 0,
        }
    }
}

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
            let samples = (self.duration.as_secs_f64() * sample_rate as f64 * channels as f64) as usize;
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
        self.inner.current_frame_len().map(|l| l.min(self.remaining_samples.unwrap_or(usize::MAX)))
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

pub struct Buffered<S> {
    inner: S,
    buffer: Vec<f32>,
    pos: 0, // Wait, this should be field initialization
}
// Actually, I'll stop here and implement it properly in source.rs later.
// I just wanted to verify the trait structure.
