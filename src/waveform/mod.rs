mod decode;
mod render;
mod sampling;

use egui::Color32;
use egui::ColorImage;
use serde::{Deserialize, Serialize};
use std::path::Path;

/// Waveform pixels and audio payload loaded from disk.
pub struct LoadedWaveform {
    pub image: ColorImage,
    pub audio_bytes: Vec<u8>,
    pub duration_seconds: f32,
}

/// Raw audio data decoded from a wav file, ready to render or play.
#[derive(Clone)]
pub struct DecodedWaveform {
    pub samples: Vec<f32>,
    pub duration_seconds: f32,
    pub sample_rate: u32,
    pub channels: u16,
}

impl DecodedWaveform {
    pub fn channel_count(&self) -> usize {
        self.channels.max(1) as usize
    }

    pub fn frame_count(&self) -> usize {
        let channels = self.channel_count();
        if channels == 0 {
            0
        } else {
            self.samples.len() / channels
        }
    }
}

/// Visual presentation mode for multi-channel audio.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
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

/// Renders averaged waveforms from wav samples.
#[derive(Clone)]
pub struct WaveformRenderer {
    pub(crate) width: u32,
    pub(crate) height: u32,
    pub(crate) background: Color32,
    pub(crate) foreground: Color32,
}

impl WaveformRenderer {
    /// Create a renderer with the target image size and colors.
    pub fn new(width: u32, height: u32) -> Self {
        Self {
            width,
            height,
            background: Color32::from_rgb(18, 16, 14),
            foreground: Color32::from_rgb(250, 246, 240),
        }
    }

    /// Current render target dimensions.
    pub fn dimensions(&self) -> (u32, u32) {
        (self.width, self.height)
    }

    /// Load a wav file from disk and return its pixels, raw bytes, and duration.
    pub fn load_waveform(&self, path: &Path) -> Result<LoadedWaveform, String> {
        let bytes = std::fs::read(path)
            .map_err(|error| format!("Failed to read {}: {error}", path.display()))?;
        let decoded = self.decode_from_bytes(&bytes)?;
        let image = self.render_color_image_for_mode(&decoded, WaveformChannelView::Mono);
        Ok(LoadedWaveform {
            image,
            audio_bytes: bytes,
            duration_seconds: decoded.duration_seconds,
        })
    }
}
