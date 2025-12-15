use super::style;
use super::*;
use eframe::egui::{self, Color32, Stroke};
use std::time::{Duration, Instant};

fn to_screen_x_unclamped(
    position: f32,
    rect: egui::Rect,
    view: crate::egui_app::state::WaveformView,
    view_width: f32,
) -> f32 {
    rect.left() + rect.width() * ((position - view.start) / view_width)
}

fn playhead_trail_mesh(
    rect: egui::Rect,
    stops: &[(f32, u8)],
    color: Color32,
) -> Option<egui::epaint::Mesh> {
    if stops.len() < 2 || stops.iter().all(|(_, alpha)| *alpha == 0) {
        return None;
    }
    let uv = egui::pos2(0.0, 0.0);
    let mut mesh = egui::epaint::Mesh::default();

    for &(x, alpha) in stops {
        let stop_color = style::with_alpha(color, alpha);
        mesh.vertices.push(egui::epaint::Vertex {
            pos: egui::pos2(x, rect.top()),
            uv,
            color: stop_color,
        });
        mesh.vertices.push(egui::epaint::Vertex {
            pos: egui::pos2(x, rect.bottom()),
            uv,
            color: stop_color,
        });
    }

    for i in 0..stops.len().saturating_sub(1) {
        let idx = (i * 2) as u32;
        mesh.indices
            .extend_from_slice(&[idx, idx + 2, idx + 3, idx, idx + 3, idx + 1]);
    }
    Some(mesh)
}

fn paint_playhead_trail_mesh(ui: &mut egui::Ui, rect: egui::Rect, stops: &[(f32, u8)], color: Color32) {
    const MONOTONIC_EPS_PX: f32 = 0.25;

    if stops.len() < 2 {
        return;
    }

    let mut start = 0usize;
    while start + 1 < stops.len() {
        let mut end = start + 1;
        while end < stops.len() && stops[end].0 + MONOTONIC_EPS_PX >= stops[end - 1].0 {
            end += 1;
        }

        let chunk = &stops[start..end];
        if let Some(mesh) = playhead_trail_mesh(rect, chunk, color) {
            ui.painter().add(egui::Shape::mesh(mesh));
        }

        start = end;
    }
}

fn trail_samples_in_window(
    trail: &std::collections::VecDeque<crate::egui_app::state::PlayheadTrailSample>,
    cutoff: Instant,
) -> Vec<crate::egui_app::state::PlayheadTrailSample> {
    let mut window = Vec::new();
    let mut prev: Option<crate::egui_app::state::PlayheadTrailSample> = None;
    for sample in trail.iter().copied() {
        if sample.time >= cutoff {
            if let Some(prev) = prev && prev.time < cutoff {
                let Some(span) = sample.time.checked_duration_since(prev.time) else {
                    continue;
                };
                let Some(elapsed) = cutoff.checked_duration_since(prev.time) else {
                    continue;
                };
                let span_s = span.as_secs_f32().max(1e-6);
                let t = (elapsed.as_secs_f32() / span_s).clamp(0.0, 1.0);
                window.push(crate::egui_app::state::PlayheadTrailSample {
                    position: prev.position + (sample.position - prev.position) * t,
                    time: cutoff,
                });
            }
            window.push(sample);
        }
        prev = Some(sample);
    }
    window
}

fn gradient_stops_from_trail_window(
    window: &[crate::egui_app::state::PlayheadTrailSample],
    rect: egui::Rect,
    view: crate::egui_app::state::WaveformView,
    view_width: f32,
    alpha_for_time: impl Fn(Instant) -> u8,
) -> Vec<(f32, u8)> {
    if window.len() < 2 {
        return Vec::new();
    }

    const MAX_STOP_SPACING_PX: f32 = 1.0;
    const MAX_STOPS_PER_WINDOW: usize = 4096;
    const CLIP_MARGIN_PX: f32 = 2.0;

    let clip_left = rect.left() - CLIP_MARGIN_PX;
    let clip_right = rect.right() + CLIP_MARGIN_PX;

    #[derive(Clone, Copy)]
    struct Segment {
        a_time: Instant,
        a_x: f32,
        delta_x: f32,
        delta_t: Duration,
        t0: f32,
        t1: f32,
        visible_len: f32,
    }

    let mut segments = Vec::<Segment>::new();
    let mut total_len = 0.0f32;
    for pair in window.windows(2) {
        let a = pair[0];
        let b = pair[1];
        let a_x = to_screen_x_unclamped(a.position, rect, view, view_width);
        let b_x = to_screen_x_unclamped(b.position, rect, view, view_width);
        let delta_x = b_x - a_x;
        let delta_t = if b.time >= a.time {
            b.time.duration_since(a.time)
        } else {
            Duration::ZERO
        };

        if delta_x.abs() < 1e-6 {
            if a_x >= clip_left && a_x <= clip_right {
                segments.push(Segment {
                    a_time: a.time,
                    a_x,
                    delta_x,
                    delta_t,
                    t0: 0.0,
                    t1: 1.0,
                    visible_len: 0.0,
                });
            }
            continue;
        }

        let t_left = (clip_left - a_x) / delta_x;
        let t_right = (clip_right - a_x) / delta_x;
        let t0 = t_left.min(t_right).max(0.0);
        let t1 = t_left.max(t_right).min(1.0);
        if t0 > t1 {
            continue;
        }
        let x0 = a_x + delta_x * t0;
        let x1 = a_x + delta_x * t1;
        let visible_len = (x1 - x0).abs().max(0.0);
        total_len += visible_len;
        segments.push(Segment {
            a_time: a.time,
            a_x,
            delta_x,
            delta_t,
            t0,
            t1,
            visible_len,
        });
    }

    if segments.is_empty() {
        return Vec::new();
    }

    let budget_intervals = MAX_STOPS_PER_WINDOW.saturating_sub(1).max(1) as isize;
    let spacing_px = (total_len / budget_intervals as f32).max(MAX_STOP_SPACING_PX);
    let mut remaining_intervals = budget_intervals;

    let mut stops = Vec::new();
    for (index, segment) in segments.iter().enumerate() {
        if remaining_intervals <= 0 {
            break;
        }
        let segments_left = (segments.len() - index - 1) as isize;
        let reserve = segments_left.max(0);
        let available = (remaining_intervals - reserve).max(1);
        let desired = ((segment.visible_len / spacing_px).ceil() as isize).max(1);
        let intervals = desired.min(available);

        for step in 0..=intervals {
            if stops.len() >= MAX_STOPS_PER_WINDOW {
                break;
            }
            if index > 0 && step == 0 {
                continue;
            }
            let u = step as f32 / intervals as f32;
            let t = segment.t0 + (segment.t1 - segment.t0) * u;
            let x = segment.a_x + segment.delta_x * t;
            let time = segment.a_time + segment.delta_t.mul_f32(t);
            stops.push((x, alpha_for_time(time)));
        }
        remaining_intervals -= intervals;
    }

    stops
}

pub(super) fn render_overlays(
    app: &mut EguiApp,
    ui: &mut egui::Ui,
    rect: egui::Rect,
    view: crate::egui_app::state::WaveformView,
    view_width: f32,
    highlight: Color32,
    start_marker_color: Color32,
    to_screen_x: &impl Fn(f32, egui::Rect) -> f32,
) {
    if let Some(marker_pos) = app.controller.ui.waveform.last_start_marker
        && marker_pos >= view.start
        && marker_pos <= view.end
    {
        let x = to_screen_x(marker_pos, rect);
        let stroke = Stroke::new(1.5, style::with_alpha(start_marker_color, 230));
        let mut y = rect.top();
        let bottom = rect.bottom();
        let dash = 6.0;
        let gap = 4.0;
        while y < bottom {
            let end = (y + dash).min(bottom);
            ui.painter()
                .line_segment([egui::pos2(x, y), egui::pos2(x, end)], stroke);
            y += dash + gap;
        }
    }

    let loop_bar_alpha = if app.controller.ui.waveform.loop_enabled {
        180
    } else {
        25
    };
    if loop_bar_alpha > 0 {
        let (loop_start, loop_end) = app
            .controller
            .ui
            .waveform
            .selection
            .map(|range| (range.start(), range.end()))
            .unwrap_or((0.0, 1.0));
        let clamped_start = loop_start.clamp(0.0, 1.0);
        let clamped_end = loop_end.clamp(clamped_start, 1.0);
        let start_norm = ((clamped_start - view.start) / view_width).clamp(0.0, 1.0);
        let end_norm = ((clamped_end - view.start) / view_width).clamp(0.0, 1.0);
        let width = (end_norm - start_norm).max(0.0) * rect.width();
        let bar_rect = egui::Rect::from_min_size(
            egui::pos2(rect.left() + rect.width() * start_norm, rect.top()),
            egui::vec2(width.max(2.0), 6.0),
        );
        ui.painter()
            .rect_filled(bar_rect, 0.0, style::with_alpha(highlight, loop_bar_alpha));
    }

    let playhead = &mut app.controller.ui.waveform.playhead;
    let now = Instant::now();
    const TRAIL_DURATION: Duration = Duration::from_millis(1250);
    const TRAIL_FADE: Duration = Duration::from_millis(450);

    for fading in playhead.fading_trails.iter() {
        let age = now.saturating_duration_since(fading.started_at);
        if age >= TRAIL_FADE {
            continue;
        }
        let fade_t = 1.0 - (age.as_secs_f32() / TRAIL_FADE.as_secs_f32()).clamp(0.0, 1.0);
        let fade_strength = fade_t * fade_t;
        let Some(last_time) = fading.samples.back().map(|sample| sample.time) else {
            continue;
        };
        let cutoff = last_time
            .checked_sub(TRAIL_DURATION)
            .unwrap_or(last_time);
        let window = trail_samples_in_window(&fading.samples, cutoff);
        if window.len() < 2 {
            continue;
        }
        let stops = gradient_stops_from_trail_window(&window, rect, view, view_width, |time| {
            let base_age = last_time.saturating_duration_since(time);
            let t =
                1.0 - (base_age.as_secs_f32() / TRAIL_DURATION.as_secs_f32()).clamp(0.0, 1.0);
            ((t * t) * 105.0 * fade_strength)
                .round()
                .clamp(0.0, 255.0) as u8
        });
        paint_playhead_trail_mesh(ui, rect, &stops, highlight);
    }

    if playhead.visible && playhead.trail.len() >= 2 {
        let cutoff = now.checked_sub(TRAIL_DURATION).unwrap_or(now);
        let window = trail_samples_in_window(&playhead.trail, cutoff);
        if window.len() >= 2 {
            let stops = gradient_stops_from_trail_window(&window, rect, view, view_width, |time| {
                let age = now.saturating_duration_since(time);
                let t = 1.0 - (age.as_secs_f32() / TRAIL_DURATION.as_secs_f32()).clamp(0.0, 1.0);
                ((t * t) * 119.0).round().clamp(0.0, 255.0) as u8
            });
            paint_playhead_trail_mesh(ui, rect, &stops, highlight);
        }
    }

    if playhead.visible {
        let position = playhead.position.clamp(0.0, 1.0);
        let x = to_screen_x(position, rect);
        ui.painter().line_segment(
            [egui::pos2(x, rect.top()), egui::pos2(x, rect.bottom())],
            Stroke::new(2.0, highlight),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::{gradient_stops_from_trail_window, trail_samples_in_window};
    use crate::egui_app::state::PlayheadTrailSample;
    use crate::egui_app::state::WaveformView;
    use eframe::egui;
    use std::collections::VecDeque;
    use std::time::{Duration, Instant};

    #[test]
    fn trail_samples_in_window_includes_cutoff_interpolation() {
        let base = Instant::now();
        let mut trail = VecDeque::new();
        trail.push_back(PlayheadTrailSample {
            position: 0.1,
            time: base,
        });
        trail.push_back(PlayheadTrailSample {
            position: 0.3,
            time: base + Duration::from_secs(1),
        });
        let window = trail_samples_in_window(&trail, base + Duration::from_millis(500));
        assert_eq!(window.len(), 2);
        assert!((window[0].position - 0.2).abs() < 1e-6);
        assert_eq!(window[0].time, base + Duration::from_millis(500));
        assert!((window[1].position - 0.3).abs() < 1e-6);
    }

    #[test]
    fn gradient_stops_from_trail_window_densifies_large_gaps() {
        let rect = egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(100.0, 10.0));
        let view = WaveformView { start: 0.0, end: 1.0 };
        let base = Instant::now();
        let window = vec![
            PlayheadTrailSample {
                position: 0.0,
                time: base,
            },
            PlayheadTrailSample {
                position: 1.0,
                time: base + Duration::from_secs(1),
            },
        ];
        let stops = gradient_stops_from_trail_window(&window, rect, view, 1.0, |_| 128);
        assert!(stops.len() > 10);
    }
}
