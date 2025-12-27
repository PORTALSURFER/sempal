use super::audio_cache::FileMetadata;
use super::*;
use crate::egui_app::state::WaveformView;
use crate::waveform::DecodedWaveform;
use std::fs;
use std::path::Path;

const MIN_VIEW_WIDTH_BASE: f32 = 0.001;
const MIN_SAMPLES_PER_PIXEL: f32 = 1.0;
const MAX_ZOOM_MULTIPLIER: f32 = 64.0;
// Cap oversampling to avoid subpixel waveform columns that shimmer when downscaled.
const MAX_COLUMNS_PER_PIXEL: f32 = 1.0;
const DEFAULT_TRANSIENT_SENSITIVITY: f32 = 0.6;

fn min_view_width_for_frames(frame_count: usize, width_px: u32) -> f32 {
    if frame_count == 0 {
        return 1.0;
    }
    let samples = frame_count as f32;
    let pixels = width_px.max(1) as f32;
    (pixels * MIN_SAMPLES_PER_PIXEL / samples).clamp(MIN_VIEW_WIDTH_BASE, 1.0)
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(in crate::egui_app::controller) struct WaveformRenderMeta {
    pub view_start: f32,
    pub view_end: f32,
    pub size: [u32; 2],
    pub samples_len: usize,
    pub texture_width: u32,
    pub channel_view: crate::waveform::WaveformChannelView,
    pub channels: u16,
}

impl WaveformRenderMeta {
    /// Check whether two render targets describe the same view and layout.
    pub(in crate::egui_app::controller) fn matches(&self, other: &WaveformRenderMeta) -> bool {
        let width = (self.view_end - self.view_start)
            .abs()
            .max((other.view_end - other.view_start).abs())
            .max(1e-6);
        let pixels = self.size[0].max(1) as f32;
        let eps = (width / pixels).max(1e-6);
        self.samples_len == other.samples_len
            && self.size == other.size
            && self.texture_width == other.texture_width
            && self.channel_view == other.channel_view
            && self.channels == other.channels
            && (self.view_start - other.view_start).abs() < eps
            && (self.view_end - other.view_end).abs() < eps
    }
}

impl EguiController {
    pub(in crate::egui_app::controller) fn min_view_width(&self) -> f32 {
        if let Some(decoded) = self.sample_view.waveform.decoded.as_ref() {
            min_view_width_for_frames(decoded.frame_count(), self.sample_view.waveform.size[0])
        } else {
            MIN_VIEW_WIDTH_BASE
        }
    }

    #[allow(dead_code)]
    pub(super) fn apply_view_bounds_with_min(&mut self, min_width: f32) -> WaveformView {
        let mut view = self.ui.waveform.view.clamp();
        let width = view.width().max(min_width);
        view.start = view.start.min(1.0 - width);
        view.end = (view.start + width).min(1.0);
        self.ui.waveform.view = view;
        view
    }

    pub(in crate::egui_app::controller::wavs) fn apply_waveform_image(
        &mut self,
        decoded: DecodedWaveform,
    ) {
        // Force a rerender whenever decoded samples change, even if the view metadata is
        // identical to the previous render.
        self.sample_view.waveform.render_meta = None;
        self.sample_view.waveform.decoded = Some(decoded);
        self.refresh_waveform_transients();
        self.refresh_waveform_image();
    }

    /// Update the waveform render target to match the current view size.
    pub fn update_waveform_size(&mut self, width: u32, height: u32) {
        let width = width.max(1);
        let height = height.max(1);
        if self.sample_view.waveform.size == [width, height] {
            return;
        }
        self.sample_view.waveform.size = [width, height];
        self.refresh_waveform_image();
    }

    pub(crate) fn refresh_waveform_image(&mut self) {
        let Some(decoded) = self.sample_view.waveform.decoded.as_ref() else {
            return;
        };
        let [width, height] = self.sample_view.waveform.size;
        let total_frames = decoded.frame_count();
        let min_view_width = min_view_width_for_frames(total_frames, width);
        let mut view = self.ui.waveform.view.clamp();
        let width_clamped = view.width().max(min_view_width);
        view.start = view.start.min(1.0 - width_clamped);
        view.end = (view.start + width_clamped).min(1.0);
        let view = view;
        let max_zoom = (1.0 / min_view_width).min(MAX_ZOOM_MULTIPLIER);
        let zoom_scale = (1.0 / width_clamped).min(max_zoom).max(1.0);
        let max_target = (width as f32 * MAX_COLUMNS_PER_PIXEL)
            .ceil()
            .max(width as f32) as usize;
        let target = (width as f32 * zoom_scale).ceil().max(width as f32) as usize;
        let target = target.min(max_target);

        if (decoded.samples.is_empty() && decoded.peaks.is_none()) || total_frames == 0 {
            self.ui.waveform.image = None;
            return;
        }
        let start_frame = ((view.start * total_frames as f32).floor() as usize)
            .min(total_frames.saturating_sub(1));
        let mut end_frame =
            ((view.end * total_frames as f32).ceil() as usize).clamp(start_frame + 1, total_frames);
        if end_frame <= start_frame {
            end_frame = (start_frame + 1).min(total_frames);
        }
        let frames_in_view = end_frame.saturating_sub(start_frame).max(1);
        let upper_width = frames_in_view.min(super::MAX_TEXTURE_WIDTH as usize);
        let lower_bound = width.min(super::MAX_TEXTURE_WIDTH) as usize;
        let effective_width = target.min(upper_width).max(lower_bound) as u32;
        let desired_meta = WaveformRenderMeta {
            view_start: view.start,
            view_end: view.end,
            size: [width, height],
            samples_len: total_frames,
            texture_width: effective_width,
            channel_view: self.ui.waveform.channel_view,
            channels: decoded.channels,
        };
        if self
            .sample_view
            .waveform
            .render_meta
            .as_ref()
            .is_some_and(|meta| meta.matches(&desired_meta))
        {
            return;
        }
        let color_image = self
            .sample_view
            .renderer
            .render_color_image_for_view_with_size(
                decoded,
                view.start,
                view.end,
                self.ui.waveform.channel_view,
                effective_width,
                height,
            );
        self.ui.waveform.image = Some(WaveformImage {
            image: color_image,
            view_start: view.start,
            view_end: view.end,
        });
        self.ui.waveform.view = view;
        self.sample_view.waveform.render_meta = Some(desired_meta);
    }

    pub(in crate::egui_app::controller) fn refresh_waveform_transients(&mut self) {
        let Some(decoded) = self.sample_view.waveform.decoded.as_ref() else {
            self.ui.waveform.transients.clear();
            self.ui.waveform.transient_cache_token = None;
            return;
        };
        if self.ui.waveform.transient_cache_token == Some(decoded.cache_token) {
            return;
        }
        self.ui.waveform.transients = crate::waveform::transients::detect_transients(
            decoded,
            DEFAULT_TRANSIENT_SENSITIVITY,
        );
        self.ui.waveform.transient_cache_token = Some(decoded.cache_token);
    }

    pub(in crate::egui_app::controller::wavs) fn read_waveform_bytes(
        &self,
        source: &SampleSource,
        relative_path: &Path,
    ) -> Result<Vec<u8>, String> {
        let full_path = source.root.join(relative_path);
        let bytes = fs::read(&full_path)
            .map_err(|err| format!("Failed to read {}: {err}", full_path.display()))?;
        Ok(crate::wav_sanitize::sanitize_wav_bytes(bytes))
    }

    pub(in crate::egui_app::controller::wavs) fn current_file_metadata(
        &self,
        source: &SampleSource,
        relative_path: &Path,
    ) -> Result<FileMetadata, String> {
        let full_path = source.root.join(relative_path);
        let metadata = fs::metadata(&full_path)
            .map_err(|err| format!("Failed to read {}: {err}", full_path.display()))?;
        let modified_ns = metadata
            .modified()
            .map_err(|err| format!("Missing modified time for {}: {err}", full_path.display()))?
            .duration_since(std::time::SystemTime::UNIX_EPOCH)
            .map_err(|_| "File modified time is before epoch".to_string())?
            .as_nanos() as i64;
        Ok(FileMetadata {
            file_size: metadata.len(),
            modified_ns,
        })
    }
}
