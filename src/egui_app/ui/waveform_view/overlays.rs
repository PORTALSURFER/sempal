use super::style;
use super::*;
use eframe::egui::{self, Color32, Stroke};

fn paint_playhead_trail_gradient(
    ui: &mut egui::Ui,
    rect: egui::Rect,
    left_x: f32,
    right_x: f32,
    highlight: Color32,
    alpha: u8,
) {
    if (right_x - left_x).abs() < 1.0 || alpha == 0 {
        return;
    }
    let y_top = rect.top();
    let y_bottom = rect.bottom();

    let mut mesh = egui::epaint::Mesh::default();
    let uv = egui::pos2(0.0, 0.0);
    let left_color = style::with_alpha(highlight, 0);
    let right_color = style::with_alpha(highlight, alpha);

    let idx0 = mesh.vertices.len() as u32;
    mesh.vertices.push(egui::epaint::Vertex {
        pos: egui::pos2(left_x, y_top),
        uv,
        color: left_color,
    });
    mesh.vertices.push(egui::epaint::Vertex {
        pos: egui::pos2(right_x, y_top),
        uv,
        color: right_color,
    });
    mesh.vertices.push(egui::epaint::Vertex {
        pos: egui::pos2(right_x, y_bottom),
        uv,
        color: right_color,
    });
    mesh.vertices.push(egui::epaint::Vertex {
        pos: egui::pos2(left_x, y_bottom),
        uv,
        color: left_color,
    });
    mesh.indices
        .extend_from_slice(&[idx0, idx0 + 1, idx0 + 2, idx0, idx0 + 2, idx0 + 3]);
    ui.painter().add(egui::Shape::mesh(mesh));
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

    if app.controller.ui.waveform.playhead.visible {
        let playhead = &mut app.controller.ui.waveform.playhead;
        let now = ui.input(|i| i.time);
        let position = playhead.position.clamp(0.0, 1.0);
        const TRAIL_DURATION_SECS: f64 = 1.25;
        const TRAIL_FADE_SECS: f64 = 0.45;
        const MAX_TRAIL_SAMPLES: usize = 384;
        const POSITION_EPS: f32 = 0.0005;
        const JUMP_THRESHOLD: f32 = 0.02;
        const MIN_SAMPLE_DT: f64 = 1.0 / 120.0;

        if let Some(fading) = playhead.fading_trail {
            let age = (now - fading.started_at).max(0.0);
            if age < TRAIL_FADE_SECS {
                let t = (1.0 - (age / TRAIL_FADE_SECS)).clamp(0.0, 1.0) as f32;
                let alpha = ((t * t) * 140.0).round() as u8;
                let start_pos = fading.start.clamp(view.start, view.end);
                let end_pos = fading.end.clamp(view.start, view.end);
                let start_x = to_screen_x(start_pos, rect);
                let end_x = to_screen_x(end_pos, rect);
                paint_playhead_trail_gradient(
                    ui,
                    rect,
                    start_x.min(end_x),
                    start_x.max(end_x),
                    highlight,
                    alpha,
                );
            } else {
                playhead.fading_trail = None;
            }
        }

        let mut should_fade_and_clear = false;
        if let Some(last) = playhead.trail.back() {
            let delta = (position - last.position).abs();
            if delta > JUMP_THRESHOLD || position + POSITION_EPS < last.position {
                should_fade_and_clear = true;
            }
        };

        if should_fade_and_clear && !playhead.trail.is_empty() {
            if let (Some(start), Some(end)) = (playhead.trail.front(), playhead.trail.back()) {
                playhead.fading_trail = Some(crate::egui_app::state::FadingPlayheadTrail {
                    start: start.position,
                    end: end.position,
                    started_at: now,
                });
            }
            playhead.trail.clear();
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

        if let Some(end) = playhead.trail.back() {
            let cutoff = now - TRAIL_DURATION_SECS;
            let mut start_pos = end.position;
            let mut prev: Option<crate::egui_app::state::PlayheadTrailSample> = None;
            for sample in playhead.trail.iter() {
                if sample.time >= cutoff {
                    if let Some(prev) = prev {
                        let span = (sample.time - prev.time).max(1e-6);
                        let t = ((cutoff - prev.time) / span).clamp(0.0, 1.0) as f32;
                        start_pos = prev.position + (sample.position - prev.position) * t;
                    } else {
                        start_pos = sample.position;
                    }
                    break;
                }
                prev = Some(*sample);
            }

            let start_pos = start_pos.clamp(view.start, view.end);
            let end_pos = end.position.clamp(view.start, view.end);
            let start_x = to_screen_x(start_pos, rect);
            let end_x = to_screen_x(end_pos, rect);
            paint_playhead_trail_gradient(
                ui,
                rect,
                start_x.min(end_x),
                start_x.max(end_x),
                highlight,
                150,
            );
        }

        let x = to_screen_x(position, rect);
        ui.painter().line_segment(
            [egui::pos2(x, rect.top()), egui::pos2(x, rect.bottom())],
            Stroke::new(2.0, highlight),
        );
    } else {
        let playhead = &mut app.controller.ui.waveform.playhead;
        if let (Some(start), Some(end)) = (playhead.trail.front(), playhead.trail.back())
            && !playhead.trail.is_empty()
        {
            let now = ui.input(|i| i.time);
            playhead.fading_trail = Some(crate::egui_app::state::FadingPlayheadTrail {
                start: start.position,
                end: end.position,
                started_at: now,
            });
        }
        playhead.trail.clear();
    }
}
