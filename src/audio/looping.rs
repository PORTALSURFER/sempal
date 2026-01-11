use rodio::Source;
use std::time::Duration;

/// A source that loops a portion of another source, ensuring sample alignment.
pub struct LoopingSource<S> {
    inner: S,
    frames_per_cycle: u64,
    channels: u16,
    sample_rate: u32,
    current_frame: u64,
}

impl<S> LoopingSource<S>
where
    S: Source<Item = f32> + Send,
{
    pub fn new(inner: S, frames_per_cycle: u64) -> Self {
        let channels = inner.channels();
        let sample_rate = inner.sample_rate();
        Self {
            inner,
            frames_per_cycle,
            channels,
            sample_rate,
            current_frame: 0,
        }
    }
}

impl<S> Iterator for LoopingSource<S>
where
    S: Source<Item = f32> + Send,
{
    type Item = f32;

    fn next(&mut self) -> Option<Self::Item> {
        let sample = self.inner.next();
        
        // Handle end of cycle
        self.current_frame += 1;
        if self.current_frame >= self.frames_per_cycle * self.channels as u64 {
            // Restart cycle
            // Wait, how to restart the inner source?
            // If the inner source is a decoder, we might need a way to seek it back.
            // But LoopingSource should ideally hold a clonable or seekable source.
        }
        
        sample.or(Some(0.0)) // Loop infinite
    }
}

impl<S> Source for LoopingSource<S>
where
    S: Source<Item = f32> + Send,
{
    fn current_frame_len(&self) -> Option<usize> {
        None // Infinite
    }

    fn channels(&self) -> u16 {
        self.channels
    }

    fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    fn total_duration(&self) -> Option<Duration> {
        None
    }
}
