use super::{DecodedWaveform, WaveformChannelView, WaveformRenderer};
use crate::selection::SelectionRange;
use crate::waveform::zoom_cache::CachedColumns;
use egui::ColorImage;

impl WaveformRenderer {
    pub(crate) fn cached_view_window(
        &self,
        decoded: &DecodedWaveform,
        view_start: f32,
        view_end: f32,
        width: u32,
    ) -> Option<(f32, f32)> {
        let frame_count = decoded.frame_count();
        if frame_count == 0 {
            return None;
        }
        let fraction = (view_end - view_start).max(0.000_001);
        let full_width = self.cached_full_width(width, fraction, frame_count);
        let (start_col, end_col) = self.columns_window(view_start, full_width, width)?;
        let full_width = full_width as f32;
        let start = (start_col as f32 / full_width).clamp(0.0, 1.0);
        let end = (end_col as f32 / full_width).clamp(start, 1.0);
        Some((start, end))
    }

    pub(super) fn render_cached_view(
        &self,
        decoded: &DecodedWaveform,
        view_start: f32,
        view_end: f32,
        view: WaveformChannelView,
        width: u32,
        height: u32,
        edit_fade: Option<SelectionRange>,
    ) -> Option<ColorImage> {
        let frame_count = decoded.frame_count();
        let fraction = (view_end - view_start).max(0.000_001);
        let full_width = self.cached_full_width(width, fraction, frame_count);
        let (start_col, end_col) = self.columns_window(view_start, full_width, width)?;
        let cached = self.zoom_cache.get_or_compute(
            decoded.cache_token,
            &decoded.samples,
            decoded.channel_count(),
            view,
            full_width,
        );
        let frames_per_column = (frame_count as f32 / full_width as f32).max(1.0);
        let smooth_radius = Self::smoothing_radius(frames_per_column, width);
        let image = match cached {
            CachedColumns::Mono(cols) => {
                let mut cols = Self::smooth_columns(&cols[start_col..end_col], smooth_radius);
                super::apply_fade_to_columns(&mut cols, view_start, view_end, width, edit_fade);
                Self::paint_color_image_for_size_with_density(
                    &cols,
                    width,
                    height,
                    self.foreground,
                    self.background,
                    frames_per_column,
                )
            }
            CachedColumns::SplitStereo { left, right } => {
                let mut left = Self::smooth_columns(&left[start_col..end_col], smooth_radius);
                let mut right = Self::smooth_columns(&right[start_col..end_col], smooth_radius);
                super::apply_fade_to_columns(&mut left, view_start, view_end, width, edit_fade);
                super::apply_fade_to_columns(&mut right, view_start, view_end, width, edit_fade);
                Self::paint_split_color_image_with_density(
                    &left,
                    &right,
                    width,
                    height,
                    self.foreground,
                    self.background,
                    frames_per_column,
                )
            }
        };
        Some(image)
    }

    pub(super) fn cached_full_width(
        &self,
        width: u32,
        view_fraction: f32,
        frame_count: usize,
    ) -> u32 {
        const MAX_CACHED_FULL_WIDTH: u32 = 200_000;
        let desired = ((width as f32) / view_fraction).ceil().max(width as f32) as u32;
        let frame_cap = frame_count.min(u32::MAX as usize) as u32;
        desired.min(frame_cap).min(MAX_CACHED_FULL_WIDTH).max(width)
    }

    pub(super) fn columns_window(
        &self,
        view_start: f32,
        full_width: u32,
        width: u32,
    ) -> Option<(usize, usize)> {
        let full_width = full_width as usize;
        let width = width as usize;
        if full_width < width || width == 0 {
            return None;
        }
        let max_start = full_width.saturating_sub(width);
        let start = ((view_start * full_width as f32).floor() as usize).min(max_start);
        Some((start, start + width))
    }
}
