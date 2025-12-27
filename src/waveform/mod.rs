mod decode;
mod error;
mod render;
mod sampling;
pub(crate) mod transients;
mod zoom_cache;

use egui::Color32;
use egui::ColorImage;
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::sync::Arc;

pub use error::{WaveformDecodeError, WaveformLoadError};

/// Waveform pixels and audio payload loaded from disk.
pub struct LoadedWaveform {
    pub image: ColorImage,
    pub audio_bytes: Vec<u8>,
    pub duration_seconds: f32,
}

/// Raw audio data decoded from a wav file, ready to render or play.
#[derive(Clone)]
pub struct DecodedWaveform {
    /// Cache token that uniquely identifies this decoded sample payload for render caching.
    ///
    /// Render caches should key off this token rather than the sample slice pointer to avoid
    /// stale cache hits when memory addresses are reused.
    pub cache_token: u64,
    /// Interleaved `[-1.0, 1.0]` samples for the full file.
    ///
    /// For very long files this may be empty and `peaks` will be populated instead.
    pub samples: Arc<[f32]>,
    /// Decimated min/max envelope for very long files to avoid holding every sample in memory.
    pub peaks: Option<Arc<WaveformPeaks>>,
    pub duration_seconds: f32,
    pub sample_rate: u32,
    pub channels: u16,
}

/// Decimated min/max envelope of a waveform, used when retaining full samples is too expensive.
#[derive(Clone)]
pub struct WaveformPeaks {
    pub total_frames: usize,
    pub channels: u16,
    pub bucket_size_frames: usize,
    pub mono: Vec<(f32, f32)>,
    pub left: Option<Vec<(f32, f32)>>,
    pub right: Option<Vec<(f32, f32)>>,
}

impl DecodedWaveform {
    pub fn channel_count(&self) -> usize {
        self.channels.max(1) as usize
    }

    pub fn frame_count(&self) -> usize {
        if let Some(peaks) = self.peaks.as_deref() {
            return peaks.total_frames;
        }
        let channels = self.channel_count();
        if channels == 0 {
            0
        } else {
            self.samples.len() / channels
        }
    }
}

/// Visual presentation mode for multi-channel audio.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum WaveformChannelView {
    /// Collapse all channels into one envelope using per-frame min/max to avoid phase cancellation.
    #[default]
    Mono,
    /// Render the first two channels separately in a stacked stereo view.
    SplitStereo,
}

#[derive(Clone, Debug, PartialEq)]
pub enum WaveformColumnView {
    Mono(Vec<(f32, f32)>),
    SplitStereo {
        left: Vec<(f32, f32)>,
        right: Vec<(f32, f32)>,
    },
}

impl WaveformPeaks {
    pub fn sample_columns_for_view(
        &self,
        view_start: f32,
        view_end: f32,
        width: u32,
        view: WaveformChannelView,
    ) -> WaveformColumnView {
        let width = width.max(1) as usize;
        let total_frames = self.total_frames.max(1);
        let start = view_start.clamp(0.0, 1.0);
        let end = view_end.clamp(start, 1.0);

        let start_frame =
            ((start * total_frames as f32).floor() as usize).min(total_frames.saturating_sub(1));
        let mut end_frame =
            ((end * total_frames as f32).ceil() as usize).clamp(start_frame + 1, total_frames);
        if end_frame <= start_frame {
            end_frame = (start_frame + 1).min(total_frames);
        }
        let frames_in_view = end_frame.saturating_sub(start_frame).max(1);

        match view {
            WaveformChannelView::Mono => WaveformColumnView::Mono(self.sample_peak_columns(
                &self.mono,
                start_frame,
                frames_in_view,
                width,
            )),
            WaveformChannelView::SplitStereo => {
                let left_src = self.left.as_ref().unwrap_or(&self.mono);
                let right_src = self.right.as_ref().unwrap_or(&self.mono);
                WaveformColumnView::SplitStereo {
                    left: self.sample_peak_columns(left_src, start_frame, frames_in_view, width),
                    right: self.sample_peak_columns(right_src, start_frame, frames_in_view, width),
                }
            }
        }
    }

    fn sample_peak_columns(
        &self,
        peaks: &[(f32, f32)],
        start_frame: usize,
        frames_in_view: usize,
        width: usize,
    ) -> Vec<(f32, f32)> {
        let bucket_size = self.bucket_size_frames.max(1);
        let bucket_count = peaks.len().max(1);
        let total = frames_in_view as f32;
        let mut columns = vec![(0.0_f32, 0.0_f32); width.max(1)];
        for (x, col) in columns.iter_mut().enumerate() {
            let rel_start = ((x as f32 * total) / width as f32).floor() as usize;
            let rel_end = (((x as f32 + 1.0) * total) / width as f32)
                .ceil()
                .max((rel_start + 1) as f32) as usize;
            let abs_start = start_frame.saturating_add(rel_start);
            let abs_end = start_frame
                .saturating_add(rel_end)
                .min(start_frame.saturating_add(frames_in_view))
                .max(abs_start + 1);
            let start_bucket = (abs_start / bucket_size).min(bucket_count - 1);
            let end_bucket = ((abs_end - 1) / bucket_size)
                .min(bucket_count.saturating_sub(1))
                .max(start_bucket);

            let mut min_v: f32 = 1.0;
            let mut max_v: f32 = -1.0;
            for i in start_bucket..=end_bucket {
                let (lo, hi) = peaks.get(i).copied().unwrap_or((0.0, 0.0));
                min_v = min_v.min(lo);
                max_v = max_v.max(hi);
            }
            if min_v > max_v {
                min_v = 0.0;
                max_v = 0.0;
            }
            *col = (min_v.clamp(-1.0, 1.0), max_v.clamp(-1.0, 1.0));
        }
        columns
    }
}

#[cfg(test)]
mod peaks_tests {
    use super::*;

    #[test]
    fn peaks_sampling_returns_expected_width() {
        let peaks = WaveformPeaks {
            total_frames: 100,
            channels: 1,
            bucket_size_frames: 10,
            mono: (0..10)
                .map(|i| (-(i as f32) / 10.0, i as f32 / 10.0))
                .collect(),
            left: None,
            right: None,
        };
        let columns = peaks.sample_columns_for_view(0.0, 1.0, 7, WaveformChannelView::Mono);
        let WaveformColumnView::Mono(cols) = columns else {
            panic!("expected mono columns");
        };
        assert_eq!(cols.len(), 7);
        assert!(cols.iter().all(|(min, max)| min <= max));
    }
}

/// Renders averaged waveforms from wav samples.
#[derive(Clone)]
pub struct WaveformRenderer {
    pub(crate) width: u32,
    pub(crate) height: u32,
    pub(crate) background: Color32,
    pub(crate) foreground: Color32,
    zoom_cache: std::sync::Arc<zoom_cache::WaveformZoomCache>,
    decode_cache: std::sync::Arc<std::sync::Mutex<decode::DecodeCache>>,
}

impl WaveformRenderer {
    /// Create a renderer with the target image size and colors.
    pub fn new(width: u32, height: u32) -> Self {
        Self {
            width,
            height,
            background: Color32::from_rgb(18, 16, 14),
            foreground: Color32::from_rgb(250, 246, 240),
            zoom_cache: std::sync::Arc::new(zoom_cache::WaveformZoomCache::new()),
            decode_cache: std::sync::Arc::new(decode::default_decode_cache()),
        }
    }

    /// Current render target dimensions.
    pub fn dimensions(&self) -> (u32, u32) {
        (self.width, self.height)
    }

    /// Load a wav file from disk and return its pixels, raw bytes, and duration.
    pub fn load_waveform(&self, path: &Path) -> Result<LoadedWaveform, WaveformLoadError> {
        let bytes = std::fs::read(path).map_err(|source| WaveformLoadError::Read {
            path: path.to_path_buf(),
            source,
        })?;
        let decoded = self.decode_from_bytes(&bytes)?;
        let image = self.render_color_image_for_mode(&decoded, WaveformChannelView::Mono);
        Ok(LoadedWaveform {
            image,
            audio_bytes: bytes,
            duration_seconds: decoded.duration_seconds,
        })
    }
}
