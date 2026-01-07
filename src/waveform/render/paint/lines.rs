use super::WaveformRenderer;
use egui::{Color32, ColorImage};

impl WaveformRenderer {
    pub(in crate::waveform::render) fn paint_line_image(
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

    pub(in crate::waveform::render) fn paint_split_line_image(
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
        let sample_at_channel = |frame: usize, channel: usize| -> f32 {
            let base = frame * channels;
            samples
                .get(base + channel.min(channels.saturating_sub(1)))
                .copied()
                .unwrap_or(0.0)
        };
        let interpolated_for_channel = |channel: usize| -> f32 {
            if i0 >= 1 && i1 + 1 < frame_count {
                let p0 = sample_at_channel(i0 - 1, channel);
                let p1 = sample_at_channel(i0, channel);
                let p2 = sample_at_channel(i1, channel);
                let p3 = sample_at_channel(i1 + 1, channel);
                return Self::catmull_rom(p0, p1, p2, p3, t);
            }
            let a = sample_at_channel(i0, channel);
            let b = sample_at_channel(i1, channel);
            a + (b - a) * t
        };
        match channel_index {
            Some(channel) => interpolated_for_channel(channel),
            None => {
                let mut chosen = 0.0_f32;
                let mut best = -1.0_f32;
                for channel in 0..channels.max(1) {
                    let sample = interpolated_for_channel(channel);
                    let score = sample.abs();
                    if score > best {
                        best = score;
                        chosen = sample;
                    }
                }
                chosen
            }
        }
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
        0.5 * (2.0 * p1
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
            Self::plot_aa(
                image,
                stride,
                width,
                height,
                ypxl1,
                xpxl1,
                fg,
                (1.0 - (yend.fract())) * xgap,
            );
            Self::plot_aa(
                image,
                stride,
                width,
                height,
                ypxl1 + 1,
                xpxl1,
                fg,
                yend.fract() * xgap,
            );
        } else {
            Self::plot_aa(
                image,
                stride,
                width,
                height,
                xpxl1,
                ypxl1,
                fg,
                (1.0 - (yend.fract())) * xgap,
            );
            Self::plot_aa(
                image,
                stride,
                width,
                height,
                xpxl1,
                ypxl1 + 1,
                fg,
                yend.fract() * xgap,
            );
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
            Self::plot_aa(
                image,
                stride,
                width,
                height,
                ypxl2,
                xpxl2,
                fg,
                (1.0 - (yend.fract())) * xgap,
            );
            Self::plot_aa(
                image,
                stride,
                width,
                height,
                ypxl2 + 1,
                xpxl2,
                fg,
                yend.fract() * xgap,
            );
        } else {
            Self::plot_aa(
                image,
                stride,
                width,
                height,
                xpxl2,
                ypxl2,
                fg,
                (1.0 - (yend.fract())) * xgap,
            );
            Self::plot_aa(
                image,
                stride,
                width,
                height,
                xpxl2,
                ypxl2 + 1,
                fg,
                yend.fract() * xgap,
            );
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
}
