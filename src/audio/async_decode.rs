use std::marker::PhantomData;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use ringbuf::{traits::*, HeapRb};

use crate::audio::Source;

const DEFAULT_BUFFER_SECONDS: f32 = 1.0;
const PRODUCER_BACKOFF: Duration = Duration::from_millis(1);
const INITIAL_SAMPLE_WAIT: Duration = Duration::from_millis(10);

/// Streams samples from a source on a background thread into a lock-free ring buffer.
pub(crate) struct AsyncSource<S> {
    consumer: ringbuf::HeapCons<f32>,
    sample_rate: u32,
    channels: u16,
    total_duration: Option<Duration>,
    done: Arc<AtomicBool>,
    stop: Arc<AtomicBool>,
    has_produced: Arc<AtomicBool>,
    last_error: Arc<Mutex<Option<String>>>,
    _marker: PhantomData<S>,
}

impl<S> AsyncSource<S>
where
    S: Source + 'static,
{
    /// Spawn a decoder thread with a 1-second buffer sized to the source format.
    pub(crate) fn new(source: S) -> Self {
        Self::with_buffer_seconds(source, DEFAULT_BUFFER_SECONDS)
    }

    /// Spawn a decoder thread with a buffer sized to `buffer_seconds`.
    pub(crate) fn with_buffer_seconds(source: S, buffer_seconds: f32) -> Self {
        let sample_rate = source.sample_rate();
        let channels = source.channels();
        let total_duration = source.total_duration();
        let buffer_samples = buffer_samples(sample_rate, channels, buffer_seconds);

        let rb = HeapRb::<f32>::new(buffer_samples.max(1));
        let (mut producer, consumer) = rb.split();

        let done = Arc::new(AtomicBool::new(false));
        let stop = Arc::new(AtomicBool::new(false));
        let last_error = Arc::new(Mutex::new(None));
        let has_produced = Arc::new(AtomicBool::new(false));

        let thread_done = Arc::clone(&done);
        let thread_stop = Arc::clone(&stop);
        let thread_error = Arc::clone(&last_error);
        let thread_has_produced = Arc::clone(&has_produced);

        let spawn_result = thread::Builder::new()
            .name("audio-decode".to_string())
            .spawn(move || {
                let mut source = source;
                loop {
                    if thread_stop.load(Ordering::Relaxed) {
                        break;
                    }
                    match source.next() {
                        Some(sample) => {
                            let mut sample = sample;
                            loop {
                                if thread_stop.load(Ordering::Relaxed) {
                                    break;
                                }
                                match producer.try_push(sample) {
                                    Ok(()) => {
                                        thread_has_produced.store(true, Ordering::Relaxed);
                                        break;
                                    }
                                    Err(returned) => {
                                        sample = returned;
                                        thread::sleep(PRODUCER_BACKOFF);
                                    }
                                }
                            }
                        }
                        None => {
                            if let Some(err) = source.last_error() {
                                if let Ok(mut slot) = thread_error.lock() {
                                    *slot = Some(err);
                                }
                            }
                            break;
                        }
                    }
                }
                thread_done.store(true, Ordering::Relaxed);
            });

        if let Err(err) = spawn_result {
            if let Ok(mut slot) = last_error.lock() {
                *slot = Some(format!("Async decode thread failed to start: {err}"));
            }
            done.store(true, Ordering::Relaxed);
        }

        Self {
            consumer,
            sample_rate,
            channels,
            total_duration,
            done,
            stop,
            has_produced,
            last_error,
            _marker: PhantomData,
        }
    }
}

impl<S> Iterator for AsyncSource<S>
where
    S: Source + 'static,
{
    type Item = f32;

    fn next(&mut self) -> Option<Self::Item> {
        if let Some(sample) = self.consumer.try_pop() {
            return Some(sample);
        }
        if self.done.load(Ordering::Relaxed) {
            return None;
        }
        if !self.has_produced.load(Ordering::Relaxed) {
            let deadline = Instant::now() + INITIAL_SAMPLE_WAIT;
            while Instant::now() < deadline {
                if let Some(sample) = self.consumer.try_pop() {
                    return Some(sample);
                }
                if self.done.load(Ordering::Relaxed) {
                    return None;
                }
                thread::sleep(PRODUCER_BACKOFF);
            }
            if let Some(sample) = self.consumer.try_pop() {
                return Some(sample);
            }
            if self.done.load(Ordering::Relaxed) {
                return None;
            }
        }
        Some(0.0)
    }
}

impl<S> Source for AsyncSource<S>
where
    S: Source + 'static,
{
    fn current_frame_len(&self) -> Option<usize> {
        if self.done.load(Ordering::Relaxed) {
            Some(self.consumer.occupied_len())
        } else {
            None
        }
    }

    fn channels(&self) -> u16 {
        self.channels
    }

    fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    fn total_duration(&self) -> Option<Duration> {
        self.total_duration
    }

    fn last_error(&self) -> Option<String> {
        self.last_error
            .lock()
            .ok()
            .and_then(|slot| slot.clone())
    }
}

impl<S> Drop for AsyncSource<S> {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

fn buffer_samples(sample_rate: u32, channels: u16, buffer_seconds: f32) -> usize {
    let channels = channels.max(1) as f32;
    let sample_rate = sample_rate.max(1) as f32;
    let buffer_seconds = if buffer_seconds.is_finite() {
        buffer_seconds.max(0.01)
    } else {
        DEFAULT_BUFFER_SECONDS
    };
    (sample_rate * channels * buffer_seconds).ceil() as usize
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Clone)]
    struct TestSource {
        samples: Vec<f32>,
        pos: usize,
        sample_rate: u32,
        channels: u16,
        delay: Duration,
        error: Option<String>,
        start_barrier: Option<Arc<std::sync::Barrier>>,
        start_barrier_waited: bool,
    }

    impl Iterator for TestSource {
        type Item = f32;

        fn next(&mut self) -> Option<Self::Item> {
            if !self.start_barrier_waited {
                if let Some(barrier) = self.start_barrier.as_ref() {
                    self.start_barrier_waited = true;
                    barrier.wait();
                }
            }
            if self.delay > Duration::ZERO {
                thread::sleep(self.delay);
            }
            if self.pos < self.samples.len() {
                let sample = self.samples[self.pos];
                self.pos += 1;
                Some(sample)
            } else {
                None
            }
        }
    }

    impl Source for TestSource {
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
            None
        }

        fn last_error(&self) -> Option<String> {
            self.error.clone()
        }
    }

    #[test]
    fn async_source_emits_samples_after_decode() {
        let source = TestSource {
            samples: vec![0.1, 0.2, 0.3],
            pos: 0,
            sample_rate: 10,
            channels: 1,
            delay: Duration::ZERO,
            error: None,
            start_barrier: None,
            start_barrier_waited: false,
        };
        let mut async_source = AsyncSource::with_buffer_seconds(source, 1.0);
        thread::sleep(Duration::from_millis(20));
        let mut collected = Vec::new();
        while let Some(sample) = async_source.next() {
            collected.push(sample);
        }
        assert_eq!(collected, vec![0.1, 0.2, 0.3]);
    }

    #[test]
    fn async_source_returns_silence_on_underrun() {
        let source = TestSource {
            samples: vec![0.5],
            pos: 0,
            sample_rate: 10,
            channels: 1,
            delay: Duration::from_millis(30),
            error: None,
            start_barrier: Some(Arc::new(std::sync::Barrier::new(2))),
            start_barrier_waited: false,
        };
        let start_barrier = source.start_barrier.clone().expect("barrier present");
        let mut async_source = AsyncSource::with_buffer_seconds(source, 0.1);
        let first = async_source.next().unwrap();
        assert_eq!(first, 0.0);
        start_barrier.wait();
        let mut second = 0.0;
        for _ in 0..10 {
            if let Some(sample) = async_source.next() {
                if sample != 0.0 {
                    second = sample;
                    break;
                }
            }
            thread::sleep(Duration::from_millis(5));
        }
        assert_eq!(second, 0.5);
    }

    #[test]
    fn async_source_waits_for_consumer_when_buffer_full() {
        let source = TestSource {
            samples: vec![0.1, 0.2],
            pos: 0,
            sample_rate: 1,
            channels: 1,
            delay: Duration::ZERO,
            error: None,
            start_barrier: None,
            start_barrier_waited: false,
        };
        let mut async_source = AsyncSource::with_buffer_seconds(source, 0.1);
        thread::sleep(Duration::from_millis(20));
        let first = async_source.next().unwrap();
        assert_eq!(first, 0.1);
        thread::sleep(Duration::from_millis(20));
        let second = async_source.next().unwrap();
        assert_eq!(second, 0.2);
    }

    #[test]
    fn async_source_propagates_errors() {
        let source = TestSource {
            samples: vec![0.7],
            pos: 0,
            sample_rate: 10,
            channels: 1,
            delay: Duration::ZERO,
            error: Some("decode failed".to_string()),
            start_barrier: None,
            start_barrier_waited: false,
        };
        let mut async_source = AsyncSource::with_buffer_seconds(source, 1.0);
        thread::sleep(Duration::from_millis(20));
        while async_source.next().is_some() {}
        assert_eq!(
            async_source.last_error(),
            Some("decode failed".to_string())
        );
    }
}
