use super::{DecodedWaveform, WaveformChannelView, WaveformColumnView, WaveformRenderer};
use egui::{Color32, ColorImage};

impl WaveformRenderer {
    /// Produce an empty waveform as an egui color image.
    pub fn empty_color_image(&self) -> ColorImage {
        self.render_color_image_with_size(
            &[],
            1,
            WaveformChannelView::Mono,
            self.width,
            self.height,
        )
    }

    /// Render an egui color image for a decoded waveform in the given channel view.
    pub fn render_color_image_for_mode(
        &self,
        decoded: &DecodedWaveform,
        view: WaveformChannelView,
    ) -> ColorImage {
        if decoded.samples.is_empty() {
            return self.render_color_image_for_view_with_size(
                decoded,
                0.0,
                1.0,
                view,
                self.width,
                self.height,
            );
        }
        self.render_color_image_with_size(
            &decoded.samples,
            decoded.channel_count(),
            view,
            self.width,
            self.height,
        )
    }

    /// Render an egui color image for a decoded waveform over a normalized view window.
    ///
    /// Uses a cached full-width column envelope keyed by zoom (view fraction) to reduce work
    /// during panning at a constant zoom level.
    pub fn render_color_image_for_view_with_size(
        &self,
        decoded: &DecodedWaveform,
        view_start: f32,
        view_end: f32,
        view: WaveformChannelView,
        width: u32,
        height: u32,
    ) -> ColorImage {
        let width = width.max(1);
        let height = height.max(1);
        let channels = decoded.channel_count();
        let frame_count = decoded.frame_count();
        if frame_count == 0 {
            return self.render_color_image_with_size(
                &[],
                1,
                WaveformChannelView::Mono,
                width,
                height,
            );
        }

        let start = view_start.clamp(0.0, 1.0);
        let end = view_end.clamp(start, 1.0);
        let fraction = (end - start).max(0.000_001);

        if decoded.samples.is_empty() {
            if let Some(peaks) = decoded.peaks.as_deref() {
                let columns = peaks.sample_columns_for_view(start, end, width, view);
                let frames_per_column = (frame_count as f32 * fraction / width as f32).max(1.0);
                let smooth_radius = Self::smoothing_radius(frames_per_column, width);
                return match columns {
                    WaveformColumnView::Mono(cols) => {
                        let cols = Self::smooth_columns(&cols, smooth_radius);
                        Self::paint_color_image_for_size_with_density(
                            &cols,
                            width,
                            height,
                            self.foreground,
                            self.background,
                            frames_per_column,
                        )
                    }
                    WaveformColumnView::SplitStereo { left, right } => {
                        let left = Self::smooth_columns(&left, smooth_radius);
                        let right = Self::smooth_columns(&right, smooth_radius);
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
            }
            return self.render_color_image_with_size(
                &[],
                1,
                WaveformChannelView::Mono,
                width,
                height,
            );
        }

        let full_width = self.cached_full_width(width, fraction, frame_count);
        if let Some((start_col, end_col)) = self.columns_window(start, full_width, width) {
            let cached = self.zoom_cache.get_or_compute(
                decoded.cache_token,
                &decoded.samples,
                channels,
                view,
                full_width,
            );
            let frames_per_column = (frame_count as f32 / full_width as f32).max(1.0);
            let smooth_radius = Self::smoothing_radius(frames_per_column, width);
            return match cached {
                super::zoom_cache::CachedColumns::Mono(cols) => {
                    let cols = Self::smooth_columns(&cols[start_col..end_col], smooth_radius);
                    Self::paint_color_image_for_size_with_density(
                        &cols,
                        width,
                        height,
                        self.foreground,
                        self.background,
                        frames_per_column,
                    )
                }
                super::zoom_cache::CachedColumns::SplitStereo { left, right } => {
                    let left = Self::smooth_columns(&left[start_col..end_col], smooth_radius);
                    let right = Self::smooth_columns(&right[start_col..end_col], smooth_radius);
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
        }

        // Fallback: sample only the visible frames directly.
        let start_frame =
            ((start * frame_count as f32).floor() as usize).min(frame_count.saturating_sub(1));
        let mut end_frame =
            ((end * frame_count as f32).ceil() as usize).clamp(start_frame + 1, frame_count);
        if end_frame <= start_frame {
            end_frame = (start_frame + 1).min(frame_count);
        }
        let start_idx = start_frame.saturating_mul(channels);
        let end_idx = end_frame
            .saturating_mul(channels)
            .min(decoded.samples.len());
        self.render_color_image_with_size(
            &decoded.samples[start_idx..end_idx],
            channels,
            view,
            width,
            height,
        )
    }

    /// Render an egui color image at an explicit size.
    pub fn render_color_image_with_size(
        &self,
        samples: &[f32],
        channels: usize,
        view: WaveformChannelView,
        width: u32,
        height: u32,
    ) -> ColorImage {
        let width = width.max(1);
        let height = height.max(1);
        let frame_count = samples.len() / channels.max(1);
        let frames_per_column = (frame_count as f32 / width as f32).max(1.0);
        if frames_per_column <= 1.2 {
            return match view {
                WaveformChannelView::Mono => Self::paint_line_image(
                    samples,
                    channels,
                    width,
                    height,
                    self.foreground,
                    self.background,
                    None,
                ),
                WaveformChannelView::SplitStereo => Self::paint_split_line_image(
                    samples,
                    channels,
                    width,
                    height,
                    self.foreground,
                    self.background,
                ),
            };
        }
        let columns = Self::sample_columns_for_width(samples, channels, width, view);
        let smooth_radius = Self::smoothing_radius(frames_per_column, width);
        match columns {
            WaveformColumnView::Mono(cols) => {
                let cols = Self::smooth_columns(&cols, smooth_radius);
                Self::paint_color_image_for_size_with_density(
                    &cols,
                    width,
                    height,
                    self.foreground,
                    self.background,
                    frames_per_column,
                )
            }
            WaveformColumnView::SplitStereo { left, right } => {
                let left = Self::smooth_columns(&left, smooth_radius);
                let right = Self::smooth_columns(&right, smooth_radius);
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
        }
    }

    fn cached_full_width(&self, width: u32, view_fraction: f32, frame_count: usize) -> u32 {
        const MAX_CACHED_FULL_WIDTH: u32 = 200_000;
        let desired = ((width as f32) / view_fraction).ceil().max(width as f32) as u32;
        let frame_cap = frame_count.min(u32::MAX as usize) as u32;
        desired.min(frame_cap).min(MAX_CACHED_FULL_WIDTH).max(width)
    }

    fn smoothing_radius(frames_per_column: f32, width: u32) -> usize {
        if width < 3 {
            return 0;
        }
        if frames_per_column > 8.0 {
            2
        } else if frames_per_column > 2.0 {
            1
        } else {
            0
        }
    }

    fn smooth_columns(columns: &[(f32, f32)], radius: usize) -> Vec<(f32, f32)> {
        if radius == 0 || columns.len() < 2 {
            return columns.to_vec();
        }
        let mut smoothed = Vec::with_capacity(columns.len());
        let len = columns.len();
        for idx in 0..len {
            let start = idx.saturating_sub(radius);
            let end = (idx + radius + 1).min(len);
            let mut min_sum = 0.0_f32;
            let mut max_sum = 0.0_f32;
            let mut weight_sum = 0.0_f32;
            for i in start..end {
                let dist = idx.abs_diff(i) as f32;
                let weight = (radius as f32 + 1.0 - dist).max(0.0);
                let (min, max) = columns[i];
                min_sum += min * weight;
                max_sum += max * weight;
                weight_sum += weight;
            }
            let denom = weight_sum.max(1.0);
            smoothed.push((min_sum / denom, max_sum / denom));
        }
        smoothed
    }

    fn paint_line_image(
        samples: &[f32],
        channels: usize,
        width: u32,
        height: u32,
        foreground: Color32,
        background: Color32,
        channel_index: Option<usize>,
    ) -> ColorImage {
        let fill =
            Color32::from_rgba_unmultiplied(background.r(), background.g(), background.b(), 0);
        let mut image = ColorImage::new(
            [width as usize, height as usize],
            vec![fill; (width as usize) * (height as usize)],
        );
        let stride = width as usize;
        let channels = channels.max(1);
        let frame_count = samples.len() / channels;
        if frame_count == 0 || width == 0 || height == 0 {
            return image;
        }
        let mid = (height.saturating_sub(1)) as f32 / 2.0;
        let half_height = mid.max(1.0);
        let fg = (
            foreground.r(),
            foreground.g(),
            foreground.b(),
            foreground.a(),
        );
        let to_y = |sample: f32| -> f32 { (mid - sample * half_height).clamp(0.0, mid * 2.0) };

        let mut prev_y = None;
        for x in 0..width as usize {
            let sample = Self::supersampled_frame(
                samples,
                channels,
                frame_count,
                x,
                width as usize,
                channel_index,
            );
            let y = to_y(sample);
            if let Some(prev) = prev_y {
                Self::draw_line_aa(
                    &mut image,
                    stride,
                    width as usize,
                    height as usize,
                    (x as f32) - 1.0,
                    prev,
                    x as f32,
                    y,
                    fg,
                );
            } else {
                Self::blend_pixel(&mut image, stride, x, y.round() as usize, fg, 1.0);
            }
            prev_y = Some(y);
        }
        image
    }

    fn paint_split_line_image(
        samples: &[f32],
        channels: usize,
        width: u32,
        height: u32,
        foreground: Color32,
        background: Color32,
    ) -> ColorImage {
        let gap = if height >= 3 { 2 } else { 0 };
        let split_height = height.saturating_sub(gap);
        let top_height = (split_height / 2).max(1);
        let bottom_height = split_height.saturating_sub(top_height).max(1);

        let top = Self::paint_line_image(
            samples,
            channels,
            width,
            top_height,
            foreground,
            background,
            Some(0),
        );
        let bottom = Self::paint_line_image(
            samples,
            channels,
            width,
            bottom_height,
            foreground,
            background,
            Some(1),
        );
        let fill =
            Color32::from_rgba_unmultiplied(background.r(), background.g(), background.b(), 0);
        let mut image = ColorImage::new(
            [width as usize, height as usize],
            vec![fill; (width as usize) * (height as usize)],
        );
        Self::blit_image(&mut image, &top, 0);
        let bottom_offset = top_height as usize + gap as usize;
        let clamped_offset = bottom_offset.min(image.size[1]);
        Self::blit_image(&mut image, &bottom, clamped_offset);
        image
    }

    fn sample_at_frame(
        samples: &[f32],
        channels: usize,
        frame_pos: f32,
        channel_index: Option<usize>,
    ) -> f32 {
        let frame_count = samples.len() / channels.max(1);
        if frame_count == 0 {
            return 0.0;
        }
        let frame_pos = frame_pos.clamp(0.0, (frame_count - 1) as f32);
        let i0 = frame_pos.floor() as usize;
        let i1 = (i0 + 1).min(frame_count - 1);
        let t = frame_pos - i0 as f32;
        let sample_at = |frame: usize| -> f32 {
            let base = frame * channels;
            match channel_index {
                Some(ch) => samples
                    .get(base + ch.min(channels.saturating_sub(1)))
                    .copied()
                    .unwrap_or(0.0),
                None => {
                    let mut sum = 0.0_f32;
                    let mut count = 0usize;
                    for ch in 0..channels {
                        if let Some(sample) = samples.get(base + ch) {
                            sum += *sample;
                            count += 1;
                        }
                    }
                    if count == 0 {
                        0.0
                    } else {
                        sum / count as f32
                    }
                }
            }
        };
        if i0 >= 1 && i1 + 1 < frame_count {
            let p0 = sample_at(i0 - 1);
            let p1 = sample_at(i0);
            let p2 = sample_at(i1);
            let p3 = sample_at(i1 + 1);
            return Self::catmull_rom(p0, p1, p2, p3, t);
        }
        let a = sample_at(i0);
        let b = sample_at(i1);
        a + (b - a) * t
    }

    fn supersampled_frame(
        samples: &[f32],
        channels: usize,
        frame_count: usize,
        x: usize,
        width: usize,
        channel_index: Option<usize>,
    ) -> f32 {
        if width <= 1 || frame_count == 0 {
            return Self::sample_at_frame(samples, channels, 0.0, channel_index);
        }
        let sub_samples = 4;
        let mut sum = 0.0_f32;
        for i in 0..sub_samples {
            let offset = (i as f32 + 0.5) / sub_samples as f32;
            let t = (x as f32 + offset) / (width as f32 - 1.0);
            let frame_pos = t * (frame_count.saturating_sub(1)) as f32;
            sum += Self::sample_at_frame(samples, channels, frame_pos, channel_index);
        }
        sum / sub_samples as f32
    }

    fn catmull_rom(p0: f32, p1: f32, p2: f32, p3: f32, t: f32) -> f32 {
        let t2 = t * t;
        let t3 = t2 * t;
        0.5
            * (2.0 * p1
                + (-p0 + p2) * t
                + (2.0 * p0 - 5.0 * p1 + 4.0 * p2 - p3) * t2
                + (-p0 + 3.0 * p1 - 3.0 * p2 + p3) * t3)
    }

    fn blend_pixel(
        image: &mut ColorImage,
        stride: usize,
        x: usize,
        y: usize,
        fg: (u8, u8, u8, u8),
        coverage: f32,
    ) {
        if coverage <= 0.0 {
            return;
        }
        let idx = y * stride + x;
        if let Some(pixel) = image.pixels.get_mut(idx) {
            let alpha = (fg.3 as f32 * coverage.clamp(0.0, 1.0)).round() as u8;
            let existing = pixel.a();
            let blended = existing.max(alpha);
            *pixel = Color32::from_rgba_unmultiplied(fg.0, fg.1, fg.2, blended);
        }
    }

    fn draw_line_aa(
        image: &mut ColorImage,
        stride: usize,
        width: usize,
        height: usize,
        mut x0: f32,
        mut y0: f32,
        mut x1: f32,
        mut y1: f32,
        fg: (u8, u8, u8, u8),
    ) {
        let steep = (y1 - y0).abs() > (x1 - x0).abs();
        if steep {
            std::mem::swap(&mut x0, &mut y0);
            std::mem::swap(&mut x1, &mut y1);
        }
        if x0 > x1 {
            std::mem::swap(&mut x0, &mut x1);
            std::mem::swap(&mut y0, &mut y1);
        }
        let dx = x1 - x0;
        let dy = y1 - y0;
        if dx.abs() < f32::EPSILON {
            let x = x0.round() as isize;
            let y = y0.round() as isize;
            if steep {
                if x >= 0 && (x as usize) < height && y >= 0 && (y as usize) < width {
                    Self::blend_pixel(image, stride, y as usize, x as usize, fg, 1.0);
                }
            } else if x >= 0 && (x as usize) < width && y >= 0 && (y as usize) < height {
                Self::blend_pixel(image, stride, x as usize, y as usize, fg, 1.0);
            }
            return;
        }
        let gradient = dy / dx;

        let xend = x0.round();
        let yend = y0 + gradient * (xend - x0);
        let xgap = 1.0 - ((x0 + 0.5).fract());
        let xpxl1 = xend as isize;
        let ypxl1 = yend.floor() as isize;
        if steep {
            Self::plot_aa(image, stride, width, height, ypxl1, xpxl1, fg, (1.0 - (yend.fract())) * xgap);
            Self::plot_aa(image, stride, width, height, ypxl1 + 1, xpxl1, fg, yend.fract() * xgap);
        } else {
            Self::plot_aa(image, stride, width, height, xpxl1, ypxl1, fg, (1.0 - (yend.fract())) * xgap);
            Self::plot_aa(image, stride, width, height, xpxl1, ypxl1 + 1, fg, yend.fract() * xgap);
        }
        let mut intery = yend + gradient;

        let xend = x1.round();
        let yend = y1 + gradient * (xend - x1);
        let xgap = (x1 + 0.5).fract();
        let xpxl2 = xend as isize;
        let ypxl2 = yend.floor() as isize;

        for x in (xpxl1 + 1)..xpxl2 {
            let y = intery.floor() as isize;
            let frac = intery.fract();
            if steep {
                Self::plot_aa(image, stride, width, height, y, x, fg, 1.0 - frac);
                Self::plot_aa(image, stride, width, height, y + 1, x, fg, frac);
            } else {
                Self::plot_aa(image, stride, width, height, x, y, fg, 1.0 - frac);
                Self::plot_aa(image, stride, width, height, x, y + 1, fg, frac);
            }
            intery += gradient;
        }

        if steep {
            Self::plot_aa(image, stride, width, height, ypxl2, xpxl2, fg, (1.0 - (yend.fract())) * xgap);
            Self::plot_aa(image, stride, width, height, ypxl2 + 1, xpxl2, fg, yend.fract() * xgap);
        } else {
            Self::plot_aa(image, stride, width, height, xpxl2, ypxl2, fg, (1.0 - (yend.fract())) * xgap);
            Self::plot_aa(image, stride, width, height, xpxl2, ypxl2 + 1, fg, yend.fract() * xgap);
        }
    }

    fn plot_aa(
        image: &mut ColorImage,
        stride: usize,
        width: usize,
        height: usize,
        x: isize,
        y: isize,
        fg: (u8, u8, u8, u8),
        coverage: f32,
    ) {
        if coverage <= 0.0 {
            return;
        }
        if x < 0 || y < 0 {
            return;
        }
        let x = x as usize;
        let y = y as usize;
        if x >= width || y >= height {
            return;
        }
        Self::blend_pixel(image, stride, x, y, fg, coverage);
    }

    fn columns_window(
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

    fn paint_color_image_for_size_with_density(
        columns: &[(f32, f32)],
        width: u32,
        height: u32,
        foreground: Color32,
        background: Color32,
        frames_per_column: f32,
    ) -> ColorImage {
        let fill =
            Color32::from_rgba_unmultiplied(background.r(), background.g(), background.b(), 0);
        let mut image = ColorImage::new(
            [width as usize, height as usize],
            vec![fill; (width as usize) * (height as usize)],
        );
        let stride = width as usize;
        let half_height = (height.saturating_sub(1)) as f32 / 2.0;
        let mid = half_height;
        let limit = height.saturating_sub(1) as f32;
        let thickness = Self::band_thickness(frames_per_column, height);
        let density_boost = Self::density_alpha_boost(frames_per_column);
        let fg = (
            foreground.r(),
            foreground.g(),
            foreground.b(),
            foreground.a(),
        );

        for (x, (min, max)) in columns.iter().enumerate() {
            let top = (mid - max * half_height).clamp(0.0, limit);
            let bottom = (mid - min * half_height).clamp(0.0, limit);
            let amp_span = (max - min).abs();
            let amp_scale = (amp_span * 12.0).clamp(0.0, 1.0);
            let column_thickness = 0.8 + (thickness - 0.8) * amp_scale;
            let band_min = top.min(bottom) - column_thickness * 0.5;
            let band_max = top.max(bottom) + column_thickness * 0.5;
            let span = (band_max - band_min).max(column_thickness);
            let start_y = band_min.floor().clamp(0.0, limit) as u32;
            let end_y = band_max.ceil().clamp(0.0, limit) as u32;
            for y in start_y..=end_y {
                let pixel_min = y as f32;
                let pixel_max = pixel_min + 1.0;
                let overlap = (band_max.min(pixel_max) - band_min.max(pixel_min)).max(0.0);
                if overlap <= 0.0 {
                    continue;
                }
                let coverage = (overlap / span).clamp(0.0, 1.0);
                let boosted = (coverage.sqrt() + density_boost).clamp(0.45, 1.0);
                let alpha = ((fg.3 as f32) * boosted).round() as u8;
                let idx = y as usize * stride + x;
                if let Some(pixel) = image.pixels.get_mut(idx) {
                    *pixel = Color32::from_rgba_unmultiplied(fg.0, fg.1, fg.2, alpha);
                }
            }
        }
        image
    }

    fn paint_split_color_image_with_density(
        left: &[(f32, f32)],
        right: &[(f32, f32)],
        width: u32,
        height: u32,
        foreground: Color32,
        background: Color32,
        frames_per_column: f32,
    ) -> ColorImage {
        let gap = if height >= 3 { 2 } else { 0 };
        let split_height = height.saturating_sub(gap);
        let top_height = (split_height / 2).max(1);
        let bottom_height = split_height.saturating_sub(top_height).max(1);

        let top = Self::paint_color_image_for_size_with_density(
            left,
            width,
            top_height,
            foreground,
            background,
            frames_per_column,
        );
        let bottom = Self::paint_color_image_for_size_with_density(
            right,
            width,
            bottom_height,
            foreground,
            background,
            frames_per_column,
        );

        let fill =
            Color32::from_rgba_unmultiplied(background.r(), background.g(), background.b(), 0);
        let mut image = ColorImage::new(
            [width as usize, height as usize],
            vec![fill; (width as usize) * (height as usize)],
        );
        Self::blit_image(&mut image, &top, 0);
        let bottom_offset = top_height as usize + gap as usize;
        let clamped_offset = bottom_offset.min(image.size[1]);
        Self::blit_image(&mut image, &bottom, clamped_offset);
        image
    }

    fn band_thickness(frames_per_column: f32, height: u32) -> f32 {
        if !frames_per_column.is_finite() || frames_per_column <= 1.0 {
            return 2.2;
        }
        let boost = (frames_per_column.log2().max(0.0) * 1.8).min(10.0);
        let max_thickness = (height as f32 * 0.78).max(2.2);
        (2.2 + boost).min(max_thickness)
    }

    fn density_alpha_boost(frames_per_column: f32) -> f32 {
        if !frames_per_column.is_finite() || frames_per_column <= 1.0 {
            return 0.0;
        }
        (frames_per_column.log2().max(0.0) * 0.12).min(0.5)
    }

    fn blit_image(target: &mut ColorImage, source: &ColorImage, y_offset: usize) {
        let width = target.size[0].min(source.size[0]);
        for y in 0..source.size[1] {
            let dest_y = y + y_offset;
            if dest_y >= target.size[1] {
                break;
            }
            let dest_offset = dest_y * target.size[0];
            let src_offset = y * source.size[0];
            let len = width.min(target.size[0]).min(source.size[0]);
            if let (Some(dest), Some(src)) = (
                target.pixels.get_mut(dest_offset..dest_offset + len),
                source.pixels.get(src_offset..src_offset + len),
            ) {
                dest.copy_from_slice(src);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn render_color_image_respects_requested_size() {
        let renderer = WaveformRenderer::new(2, 2);
        let image =
            renderer.render_color_image_with_size(&[0.0, 0.5], 1, WaveformChannelView::Mono, 4, 6);
        assert_eq!(image.size, [4, 6]);
    }

    #[test]
    fn render_color_image_for_view_respects_requested_size() {
        let renderer = WaveformRenderer::new(2, 2);
        let decoded = DecodedWaveform {
            cache_token: 1,
            samples: std::sync::Arc::from(vec![0.0, 0.5, -0.25, 0.25]),
            peaks: None,
            duration_seconds: 1.0,
            sample_rate: 48_000,
            channels: 1,
        };
        let image = renderer.render_color_image_for_view_with_size(
            &decoded,
            0.25,
            0.75,
            WaveformChannelView::Mono,
            5,
            3,
        );
        assert_eq!(image.size, [5, 3]);
    }
}
