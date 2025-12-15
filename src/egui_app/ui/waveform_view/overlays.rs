use super::style;
use super::*;
use eframe::egui::{self, Color32, Stroke};

fn playhead_trail_mesh(rect: egui::Rect, stops: &[(f32, u8)], color: Color32) -> Option<egui::epaint::Mesh> {
    if stops.len() < 2 || stops.iter().all(|(_, alpha)| *alpha == 0) {
        return None;
    }
    let uv = egui::pos2(0.0, 0.0);
    let mut mesh = egui::epaint::Mesh::default();

    for (x, alpha) in stops {
        let x = x.clamp(rect.left(), rect.right());
        let stop_color = style::with_alpha(color, *alpha);
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
    let Some(mesh) = playhead_trail_mesh(rect, stops, color) else {
        return;
    };
    ui.painter().add(egui::Shape::mesh(mesh));
}

fn paint_playhead_trail_mesh_runs(
    ui: &mut egui::Ui,
    rect: egui::Rect,
    stops: &[(f32, u8)],
    color: Color32,
) {
    const DIRECTION_TOLERANCE_PX: f32 = 0.5;
    const DUPLICATE_TOLERANCE_PX: f32 = 0.1;

    let mut run: Vec<(f32, u8)> = Vec::new();
    let mut direction: Option<i8> = None;

    let flush = |ui: &mut egui::Ui, rect: egui::Rect, run: &[(f32, u8)], color: Color32| {
        if run.len() >= 2 {
            paint_playhead_trail_mesh(ui, rect, run, color);
        }
    };

    for &(x, alpha) in stops {
        if let Some((prev_x, prev_alpha)) = run.last().copied() {
            if (x - prev_x).abs() <= DUPLICATE_TOLERANCE_PX && alpha == prev_alpha {
                continue;
            }

            let dx = x - prev_x;
            match direction {
                None => {
                    if dx.abs() > DIRECTION_TOLERANCE_PX {
                        direction = Some(if dx.is_sign_negative() { -1 } else { 1 });
                    }
                }
                Some(1) => {
                    if dx < -DIRECTION_TOLERANCE_PX {
                        flush(ui, rect, &run, color);
                        run.clear();
                        run.push((prev_x, prev_alpha));
                        direction = Some(-1);
                    }
                }
                Some(-1) => {
                    if dx > DIRECTION_TOLERANCE_PX {
                        flush(ui, rect, &run, color);
                        run.clear();
                        run.push((prev_x, prev_alpha));
                        direction = Some(1);
                    }
                }
                _ => {}
            }
        }
        run.push((x, alpha));
    }

    flush(ui, rect, &run, color);
}

fn trail_samples_in_window(
    trail: &std::collections::VecDeque<crate::egui_app::state::PlayheadTrailSample>,
    cutoff: f64,
) -> Vec<crate::egui_app::state::PlayheadTrailSample> {
    let mut window = Vec::new();
    let mut prev: Option<crate::egui_app::state::PlayheadTrailSample> = None;
    for sample in trail.iter().copied() {
        if sample.time >= cutoff {
            if let Some(prev) = prev
                && prev.time < cutoff
            {
                let span = (sample.time - prev.time).max(1e-6);
                let t = ((cutoff - prev.time) / span).clamp(0.0, 1.0) as f32;
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

fn clip_trail_window_to_view(
    window: &[crate::egui_app::state::PlayheadTrailSample],
    view: crate::egui_app::state::WaveformView,
) -> Vec<crate::egui_app::state::PlayheadTrailSample> {
    if window.len() < 2 {
        return Vec::new();
    }

    let clamp_bounds = |mut t0: f32, mut t1: f32| {
        if t0 > t1 {
            std::mem::swap(&mut t0, &mut t1);
        }
        let t0 = t0.clamp(0.0, 1.0);
        let t1 = t1.clamp(0.0, 1.0);
        if t0 <= t1 { Some((t0, t1)) } else { None }
    };

    let lerp = |a: crate::egui_app::state::PlayheadTrailSample,
                b: crate::egui_app::state::PlayheadTrailSample,
                t: f32| {
        crate::egui_app::state::PlayheadTrailSample {
            position: a.position + (b.position - a.position) * t,
            time: a.time + (b.time - a.time) * t as f64,
        }
    };

    let mut clipped = Vec::new();
    for segment in window.windows(2) {
        let a = segment[0];
        let b = segment[1];
        let delta = b.position - a.position;

        if delta.abs() < 1e-9 {
            if (a.position >= view.start && a.position <= view.end)
                && (b.position >= view.start && b.position <= view.end)
            {
                if clipped.is_empty() {
                    clipped.push(a);
                }
                clipped.push(b);
            }
            continue;
        }

        let mut t0 = 0.0f32;
        let mut t1 = 1.0f32;

        for bound in [view.start, view.end] {
            let t = (bound - a.position) / delta;
            if delta.is_sign_positive() {
                if bound == view.start {
                    t0 = t0.max(t);
                } else {
                    t1 = t1.min(t);
                }
            } else {
                if bound == view.start {
                    t1 = t1.min(t);
                } else {
                    t0 = t0.max(t);
                }
            }
        }

        let Some((t0, t1)) = clamp_bounds(t0, t1) else {
            continue;
        };

        let start = lerp(a, b, t0);
        let end = lerp(a, b, t1);
        let start_inside = start.position >= view.start && start.position <= view.end;
        let end_inside = end.position >= view.start && end.position <= view.end;
        if !start_inside || !end_inside {
            continue;
        }

        if clipped.is_empty() {
            clipped.push(start);
        }
        clipped.push(end);
    }

    clipped
}

fn gradient_stops_from_trail_window(
    window: &[crate::egui_app::state::PlayheadTrailSample],
    rect: egui::Rect,
    view: crate::egui_app::state::WaveformView,
    to_screen_x: &impl Fn(f32, egui::Rect) -> f32,
    alpha_for_time: impl Fn(f64) -> u8,
) -> Vec<(f32, u8)> {
    if window.len() < 2 {
        return Vec::new();
    }

    const MAX_STOP_SPACING_PX: f32 = 1.0;
    const MAX_STOPS_PER_WINDOW: usize = 4096;

    let window = clip_trail_window_to_view(window, view);
    if window.len() < 2 {
        return Vec::new();
    }

    let mut stops = Vec::new();
    for segment in window.windows(2) {
        let a = segment[0];
        let b = segment[1];
        let a_x = to_screen_x(a.position, rect);
        let b_x = to_screen_x(b.position, rect);
        let dx = (b_x - a_x).abs();
        let steps = ((dx / MAX_STOP_SPACING_PX).ceil() as usize).max(1);

        for step in 0..steps {
            if stops.len() >= MAX_STOPS_PER_WINDOW {
                break;
            }
            let t = step as f32 / steps as f32;
            let time = a.time + (b.time - a.time) * t as f64;
            let x = a_x + (b_x - a_x) * t;
            stops.push((x, alpha_for_time(time)));
        }
        if stops.len() >= MAX_STOPS_PER_WINDOW {
            break;
        }
    }

    if stops.len() < MAX_STOPS_PER_WINDOW
        && let Some(last) = window.last()
    {
        stops.push((to_screen_x(last.position, rect), alpha_for_time(last.time)));
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
    let now = ui.input(|i| i.time);
    const TRAIL_DURATION_SECS: f64 = 1.25;
    const TRAIL_FADE_SECS: f64 = 0.45;
    const MAX_TRAIL_SAMPLES: usize = 384;
    const POSITION_EPS: f32 = 0.0005;
    const JUMP_THRESHOLD: f32 = 0.02;
    const MIN_SAMPLE_DT: f64 = 1.0 / 120.0;
    const MAX_FADING_TRAILS: usize = 2;

    playhead
        .fading_trails
        .retain(|fading| (now - fading.started_at).max(0.0) < TRAIL_FADE_SECS);
    for fading in playhead.fading_trails.iter() {
        let age = (now - fading.started_at).max(0.0);
        let fade_t = (1.0 - (age / TRAIL_FADE_SECS)).clamp(0.0, 1.0) as f32;
        let fade_strength = fade_t * fade_t;
        let Some(last_time) = fading.samples.back().map(|sample| sample.time) else {
            continue;
        };
        let cutoff = last_time - TRAIL_DURATION_SECS;
        let window = trail_samples_in_window(&fading.samples, cutoff);
        if window.len() < 2 {
            continue;
        }
        let stops = gradient_stops_from_trail_window(&window, rect, view, to_screen_x, |time| {
            let base_age = (last_time - time).max(0.0);
            let t = (1.0 - (base_age / TRAIL_DURATION_SECS)).clamp(0.0, 1.0) as f32;
            ((t * t) * 150.0 * fade_strength)
                .round()
                .clamp(0.0, 255.0) as u8
        });
        paint_playhead_trail_mesh_runs(ui, rect, &stops, highlight);
    }

    if playhead.visible {
        let position = playhead.position.clamp(0.0, 1.0);

        let mut should_fade_and_clear = false;
        if let Some(last) = playhead.trail.back() {
            let delta = (position - last.position).abs();
            if delta > JUMP_THRESHOLD || position + POSITION_EPS < last.position {
                should_fade_and_clear = true;
            }
        };

        if should_fade_and_clear && !playhead.trail.is_empty() {
            let samples = std::mem::take(&mut playhead.trail);
            playhead
                .fading_trails
                .push(crate::egui_app::state::FadingPlayheadTrail {
                    started_at: now,
                    samples,
                });
            while playhead.fading_trails.len() > MAX_FADING_TRAILS {
                playhead.fading_trails.remove(0);
            }
        }

        let should_push = match playhead.trail.back() {
            Some(last) => {
                (position - last.position).abs() > POSITION_EPS
                    || (now - last.time) >= MIN_SAMPLE_DT
            }
            None => true,
        };
        if should_push {
            playhead
                .trail
                .push_back(crate::egui_app::state::PlayheadTrailSample {
                    position,
                    time: now,
                });
        }
        let prune_cutoff = now - (TRAIL_DURATION_SECS * 2.0);
        while let Some(front) = playhead.trail.front()
            && front.time < prune_cutoff
        {
            playhead.trail.pop_front();
        }
        while playhead.trail.len() > MAX_TRAIL_SAMPLES {
            playhead.trail.pop_front();
        }

    let cutoff = now - TRAIL_DURATION_SECS;
    let window = trail_samples_in_window(&playhead.trail, cutoff);
    if window.len() >= 2 {
        let stops = gradient_stops_from_trail_window(&window, rect, view, to_screen_x, |time| {
            let age = (now - time).max(0.0);
            let t = (1.0 - (age / TRAIL_DURATION_SECS)).clamp(0.0, 1.0) as f32;
            ((t * t) * 170.0).round().clamp(0.0, 255.0) as u8
        });
        paint_playhead_trail_mesh_runs(ui, rect, &stops, highlight);
    }

        let x = to_screen_x(position, rect);
        ui.painter().line_segment(
            [egui::pos2(x, rect.top()), egui::pos2(x, rect.bottom())],
            Stroke::new(2.0, highlight),
        );
    } else {
        if !playhead.trail.is_empty() {
            let samples = std::mem::take(&mut playhead.trail);
            playhead
                .fading_trails
                .push(crate::egui_app::state::FadingPlayheadTrail {
                    started_at: now,
                    samples,
                });
            while playhead.fading_trails.len() > MAX_FADING_TRAILS {
                playhead.fading_trails.remove(0);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{clip_trail_window_to_view, gradient_stops_from_trail_window, trail_samples_in_window};
    use crate::egui_app::state::PlayheadTrailSample;
    use crate::egui_app::state::WaveformView;
    use eframe::egui;
    use std::collections::VecDeque;

    #[test]
    fn trail_samples_in_window_includes_cutoff_interpolation() {
        let mut trail = VecDeque::new();
        trail.push_back(PlayheadTrailSample {
            position: 0.1,
            time: 0.0,
        });
        trail.push_back(PlayheadTrailSample {
            position: 0.3,
            time: 1.0,
        });
        let window = trail_samples_in_window(&trail, 0.5);
        assert_eq!(window.len(), 2);
        assert!((window[0].position - 0.2).abs() < 1e-6);
        assert!((window[0].time - 0.5).abs() < 1e-12);
        assert!((window[1].position - 0.3).abs() < 1e-6);
    }

    #[test]
    fn gradient_stops_from_trail_window_densifies_large_gaps() {
        let rect = egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(100.0, 10.0));
        let view = WaveformView { start: 0.0, end: 1.0 };
        let to_screen_x = |pos: f32, rect: egui::Rect| rect.left() + rect.width() * pos;
        let window = vec![
            PlayheadTrailSample {
                position: 0.0,
                time: 0.0,
            },
            PlayheadTrailSample {
                position: 1.0,
                time: 1.0,
            },
        ];
        let stops = gradient_stops_from_trail_window(&window, rect, view, &to_screen_x, |_| 128);
        assert!(stops.len() > 10);
    }

    #[test]
    fn clip_trail_window_to_view_trims_to_bounds() {
        let view = WaveformView { start: 0.25, end: 0.75 };
        let window = vec![
            PlayheadTrailSample {
                position: 0.0,
                time: 0.0,
            },
            PlayheadTrailSample {
                position: 1.0,
                time: 1.0,
            },
        ];
        let clipped = clip_trail_window_to_view(&window, view);
        assert!(clipped.first().is_some_and(|s| (s.position - 0.25).abs() < 1e-6));
        assert!(clipped.last().is_some_and(|s| (s.position - 0.75).abs() < 1e-6));
    }
}
