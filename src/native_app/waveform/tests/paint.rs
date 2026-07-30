use super::*;

fn runtime_overlay_plan(widget: &WaveformWidget, bounds: Rect) -> SurfacePaintPlan {
    let mut plan = SurfacePaintPlan::empty(&ThemeTokens::default());
    widget.append_runtime_overlay_paint(
        &mut plan.primitives,
        bounds,
        &radiant::layout::LayoutOutput::default(),
        &ThemeTokens::default(),
    );
    plan
}

fn composed_widget_plan(widget: &WaveformWidget, bounds: Rect) -> SurfacePaintPlan {
    cached_base_with_runtime_overlay(widget, widget, bounds)
}

fn cached_base_with_runtime_overlay(
    base_widget: &WaveformWidget,
    runtime_widget: &WaveformWidget,
    bounds: Rect,
) -> SurfacePaintPlan {
    let mut plan = base_widget.paint_plan_with_defaults(bounds);
    runtime_widget.append_runtime_overlay_paint(
        &mut plan.primitives,
        bounds,
        &radiant::layout::LayoutOutput::default(),
        &ThemeTokens::default(),
    );
    plan
}

fn playmark_label_background_count(plan: &SurfacePaintPlan) -> usize {
    plan.fill_rects()
        .filter(|fill| {
            (fill.color.r, fill.color.g, fill.color.b, fill.color.a) == (24, 27, 28, 230)
        })
        .count()
}

fn assert_single_playmark_label(plan: &SurfacePaintPlan, text: &str) {
    assert_eq!(
        plan.text_runs()
            .filter(|run| run.text.as_str() == text)
            .count(),
        1,
        "expected exactly one {text:?} text primitive"
    );
    assert_eq!(
        playmark_label_background_count(plan),
        1,
        "expected exactly one playmark label background"
    );
}

fn assert_no_white_hover_border(plan: &SurfacePaintPlan) {
    assert!(
        !plan.stroke_rects().any(|stroke| {
            (stroke.color.r, stroke.color.g, stroke.color.b) == (255, 255, 255)
                && stroke.color.a >= 200
        }),
        "handle hover should use a brighter colored fill, not a white border"
    );
}

fn fill_index(
    fills: &[&PaintFillRect],
    label: &str,
    predicate: impl Fn(&PaintFillRect) -> bool,
) -> usize {
    fills
        .iter()
        .position(|fill| predicate(fill))
        .unwrap_or_else(|| panic!("expected fill for {label}, got {fills:?}"))
}

fn beat_guide_fills<'a>(fills: &'a [&PaintFillRect]) -> Vec<&'a PaintFillRect> {
    fills
        .iter()
        .copied()
        .filter(|fill| {
            (fill.color.r, fill.color.g, fill.color.b, fill.color.a) == (231, 229, 223, 150)
        })
        .collect()
}

fn assert_beat_guides_at(fills: &[&PaintFillRect], expected_xs: &[f32]) {
    let guides = beat_guide_fills(fills);
    assert_eq!(guides.len(), expected_xs.len(), "got guides {guides:?}");
    for expected_x in expected_xs {
        assert!(
            guides.iter().any(|fill| {
                (fill.rect.center().x - expected_x).abs() < 0.01
                    && (fill.rect.min.y - 11.0).abs() < 0.01
                    && (fill.rect.max.y - 69.0).abs() < 0.01
            }),
            "expected beat guide at x={expected_x}, got {guides:?}"
        );
    }
}

#[test]
fn overlay_paint_projects_play_edit_and_playhead_markers() {
    let mut state = WaveformState::synthetic_for_tests();
    state.start_playback(0.125);
    state.set_playhead_ratio(0.25);
    state.stop_playback();
    state.set_playhead_ratio(0.25);
    state.edit_mark_ratio = Some(0.375);

    let widget = waveform_widget_for_state(&state);
    let plan = widget.paint_plan_with_defaults(Rect::from_size(400.0, 80.0));

    let fills = fill_rects(&plan);
    assert!(fills.iter().any(|fill| {
        (fill.rect.center().x / 400.0 - 0.125).abs() < 0.01
            && (fill.color.r, fill.color.g, fill.color.b, fill.color.a) == (233, 88, 67, 245)
    }));
    assert!(fills.iter().any(|fill| {
        (fill.rect.center().x / 400.0 - 0.375).abs() < 0.01
            && (fill.color.r, fill.color.g, fill.color.b) == (174, 176, 173)
            && fill.color.a == 230
    }));
    assert!(fills.iter().any(|fill| {
        (fill.rect.center().x / 400.0 - 0.25).abs() < 0.01
            && (fill.color.r, fill.color.g, fill.color.b) == (231, 229, 223)
            && fill.color.a == 210
    }));
}

#[test]
fn playhead_cursor_paints_pixel_stable_rect_when_progress_is_subpixel() {
    let mut state = WaveformState::synthetic_for_tests();
    state.set_playhead_ratio(0.12345);
    let widget = waveform_widget_for_state(&state);
    let plan = widget.paint_plan_with_defaults(Rect::from_size(400.0, 80.0));

    let playhead = fill_rects(&plan)
        .into_iter()
        .find(|fill| {
            (fill.color.r, fill.color.g, fill.color.b, fill.color.a) == (231, 229, 223, 210)
        })
        .expect("playhead fill paints");
    assert_eq!(playhead.rect.width(), 2.0);
    assert_eq!(playhead.rect.min.x.fract(), 0.0);
    assert_eq!(playhead.rect.max.x.fract(), 0.0);
}

#[test]
fn playhead_cursor_paints_for_static_small_playmark_selection() {
    let mut state = WaveformState::synthetic_for_tests();
    state.play_selection = Some(wavecrate::selection::SelectionRange::new(0.500, 0.505));
    state.set_playhead_ratio(0.503);

    let widget = waveform_widget_for_state(&state);
    let plan = widget.paint_plan_with_defaults(Rect::from_size(400.0, 80.0));

    let playhead = fill_rects(&plan)
        .into_iter()
        .find(|fill| {
            (fill.color.r, fill.color.g, fill.color.b, fill.color.a) == (231, 229, 223, 210)
        })
        .expect("static playhead fill paints");
    assert!((playhead.rect.center().x / 400.0 - 0.503).abs() < 0.01);
}

#[test]
fn playmark_label_formats_duration_and_paints_at_selection_bottom() {
    let mut subsecond = WaveformState::synthetic_for_tests();
    subsecond.play_selection = Some(wavecrate::selection::SelectionRange::new(0.125, 0.875));
    let subsecond_plan = waveform_widget_for_state(&subsecond)
        .paint_plan_with_defaults(Rect::from_size(400.0, 80.0));
    let subsecond_label = subsecond_plan
        .first_text_run("750 ms")
        .expect("subsecond playmark duration label");
    assert!((subsecond_label.rect.center().x - 200.0).abs() < 0.01);
    assert_eq!(subsecond_label.rect.max.y, 74.0);

    let mut seconds = waveform_state_with_duration_seconds(2);
    seconds.play_selection = Some(wavecrate::selection::SelectionRange::new(0.25, 1.0));
    let seconds_plan =
        waveform_widget_for_state(&seconds).paint_plan_with_defaults(Rect::from_size(400.0, 80.0));
    assert!(seconds_plan.contains_text("1.50 s"));

    let mut minutes = waveform_state_with_duration_seconds(125);
    minutes.play_selection = Some(wavecrate::selection::SelectionRange::new(0.0, 1.0));
    let minutes_plan =
        waveform_widget_for_state(&minutes).paint_plan_with_defaults(Rect::from_size(400.0, 80.0));
    assert!(minutes_plan.contains_text("2m 05.00s"));
}

#[test]
fn beat_guides_replace_playmark_duration_with_derived_bpm() {
    let mut state = waveform_state_with_duration_seconds(2);
    state.play_selection = Some(wavecrate::selection::SelectionRange::new(0.0, 1.0));
    let plan = waveform_widget_for_state_with_beat_guides(&state, true, 4)
        .paint_plan_with_defaults(Rect::from_size(400.0, 80.0));

    assert!(plan.contains_text("120 BPM"));
    assert!(!plan.contains_text("2.00 s"));
}

#[test]
fn live_playmark_preview_paints_current_duration_label() {
    let state = WaveformState::synthetic_for_tests();
    let mut widget = waveform_widget_for_state(&state);
    widget.active_drag_kind = Some(WaveformActiveDragKind::Selection(
        WaveformSelectionKind::Play,
    ));
    widget.live_selection_preview = Some(LiveSelectionPreview {
        kind: WaveformSelectionKind::Play,
        selection: wavecrate::selection::SelectionRange::new(0.2, 0.7),
    });

    let plan = composed_widget_plan(&widget, Rect::from_size(400.0, 80.0));

    assert!(plan.contains_text("500 ms"));
}

#[test]
fn narrow_playmark_labels_move_outside_selection_at_left_center_and_right() {
    let bounds = Rect::from_size(400.0, 80.0);

    let mut center = WaveformState::synthetic_for_tests();
    center.play_selection = Some(wavecrate::selection::SelectionRange::new(0.49, 0.51));
    let center_plan = composed_widget_plan(&waveform_widget_for_state(&center), bounds);
    let center_label = center_plan.text_runs().next().expect("center label paints");
    assert!(center_label.rect.min.x >= 204.0 + 6.0 - f32::EPSILON);
    assert!(center_label.rect.max.x <= bounds.max.x);
    assert_eq!(playmark_label_background_count(&center_plan), 1);

    let mut left = WaveformState::synthetic_for_tests();
    left.play_selection = Some(wavecrate::selection::SelectionRange::new(0.0, 0.02));
    let left_plan = composed_widget_plan(&waveform_widget_for_state(&left), bounds);
    let left_label = left_plan.text_runs().next().expect("left label paints");
    assert!(left_label.rect.min.x >= 8.0 + 6.0 - f32::EPSILON);
    assert!(left_label.rect.max.x <= bounds.max.x);

    let mut right = WaveformState::synthetic_for_tests();
    right.play_selection = Some(wavecrate::selection::SelectionRange::new(0.95, 1.0));
    let right_plan = composed_widget_plan(&waveform_widget_for_state(&right), bounds);
    let right_label = right_plan.text_runs().next().expect("right label paints");
    assert!(right_label.rect.max.x <= 380.0 - 6.0 + f32::EPSILON);
    assert!(right_label.rect.min.x >= bounds.min.x);

    let mut zoomed = WaveformState::synthetic_for_tests();
    let total_frames = zoomed.file.frames as i64;
    zoomed.viewport = WaveformViewport {
        start: total_frames / 4,
        end: total_frames * 3 / 4,
    };
    zoomed.play_selection = Some(wavecrate::selection::SelectionRange::new(0.495, 0.505));
    let zoomed_plan = composed_widget_plan(&waveform_widget_for_state(&zoomed), bounds);
    let zoomed_label = zoomed_plan.text_runs().next().expect("zoomed label paints");
    assert!(zoomed_label.rect.min.x >= 204.0 + 6.0 - f32::EPSILON);
    assert!(zoomed_label.rect.max.x <= bounds.max.x);
}

#[test]
fn playmark_label_has_one_owner_across_live_and_transition_states() {
    let bounds = Rect::from_size(400.0, 80.0);
    let mut state = WaveformState::synthetic_for_tests();
    state.play_selection = Some(wavecrate::selection::SelectionRange::new(0.2, 0.7));

    let steady = waveform_widget_for_state(&state);
    assert_single_playmark_label(&composed_widget_plan(&steady, bounds), "500 ms");

    let mut creation = waveform_widget_for_state(&WaveformState::synthetic_for_tests());
    creation.active_drag_kind = Some(WaveformActiveDragKind::Selection(
        WaveformSelectionKind::Play,
    ));
    creation.live_selection_preview = Some(LiveSelectionPreview {
        kind: WaveformSelectionKind::Play,
        selection: wavecrate::selection::SelectionRange::new(0.2, 0.7),
    });
    assert_single_playmark_label(&composed_widget_plan(&creation, bounds), "500 ms");

    let mut move_start = waveform_widget_for_state(&state);
    move_start.active_drag_kind = Some(WaveformActiveDragKind::SelectionMove(
        WaveformSelectionKind::Play,
    ));
    assert_single_playmark_label(&composed_widget_plan(&move_start, bounds), "500 ms");

    let mut zero_delta_move = waveform_widget_for_state(&state);
    zero_delta_move.active_drag_kind = move_start.active_drag_kind;
    zero_delta_move.live_selection_preview = Some(LiveSelectionPreview {
        kind: WaveformSelectionKind::Play,
        selection: state.play_selection.expect("committed playmark"),
    });
    assert_single_playmark_label(&composed_widget_plan(&zero_delta_move, bounds), "500 ms");

    let mut moved = waveform_widget_for_state(&state);
    moved.active_drag_kind = zero_delta_move.active_drag_kind;
    moved.live_selection_preview = Some(LiveSelectionPreview {
        kind: WaveformSelectionKind::Play,
        selection: wavecrate::selection::SelectionRange::new(0.4, 0.7),
    });
    assert!(
        !moved.prefers_pointer_move_paint_only(),
        "live playmark text must rebuild the base scene instead of composing over its stale label"
    );
    let moved_plan = composed_widget_plan(&moved, bounds);
    assert_single_playmark_label(&moved_plan, "300 ms");
    assert!(!moved_plan.contains_text("500 ms"));

    let mut resized_state = WaveformState::synthetic_for_tests();
    resized_state.play_selection = Some(wavecrate::selection::SelectionRange::new(0.2, 0.5));
    let mut resized = waveform_widget_for_state(&resized_state);
    resized.active_drag_kind = Some(WaveformActiveDragKind::SelectionResize(
        WaveformSelectionKind::Play,
        WaveformSelectionEdge::End,
    ));
    resized.live_selection_preview = Some(LiveSelectionPreview {
        kind: WaveformSelectionKind::Play,
        selection: wavecrate::selection::SelectionRange::new(0.2, 0.6),
    });
    let resized_plan = composed_widget_plan(&resized, bounds);
    assert!(
        !resized.prefers_pointer_move_paint_only(),
        "live playmark resize text must rebuild the base scene"
    );
    assert_single_playmark_label(&resized_plan, "400 ms");
    assert!(!resized_plan.contains_text("300 ms"));

    let mut stale_preview_after_release = waveform_widget_for_state(&state);
    stale_preview_after_release.live_selection_preview = Some(LiveSelectionPreview {
        kind: WaveformSelectionKind::Play,
        selection: wavecrate::selection::SelectionRange::new(0.4, 0.7),
    });
    let released_plan = composed_widget_plan(&stale_preview_after_release, bounds);
    assert_single_playmark_label(&released_plan, "500 ms");
    assert!(!released_plan.contains_text("300 ms"));
}

#[test]
fn playmark_control_occlusion_tracks_live_drag_selection() {
    let bounds = Rect::from_size(400.0, 80.0);
    let live_selection = wavecrate::selection::SelectionRange::new(0.6, 0.9);
    let mut widget = waveform_widget_for_state(&WaveformState::synthetic_for_tests());
    widget.active_drag_kind = Some(WaveformActiveDragKind::Selection(
        WaveformSelectionKind::Play,
    ));
    widget.live_selection_preview = Some(LiveSelectionPreview {
        kind: WaveformSelectionKind::Play,
        selection: live_selection,
    });

    let occlusion = widget
        .playmark_control_cluster_rect(bounds)
        .expect("live playmark control occlusion");
    let mut committed = WaveformState::synthetic_for_tests();
    committed.play_selection = Some(live_selection);
    let expected = waveform_widget_for_state(&committed)
        .playmark_control_cluster_rect(bounds)
        .expect("equivalent committed playmark control occlusion");

    assert_eq!(occlusion, expected);
    assert_eq!(occlusion.max.y, 74.0);
}

#[test]
fn steady_base_is_rebuilt_before_live_move_label_paints() {
    let bounds = Rect::from_size(400.0, 80.0);
    let mut state = WaveformState::synthetic_for_tests();
    state.play_selection = Some(wavecrate::selection::SelectionRange::new(0.2, 0.7));
    let steady = waveform_widget_for_state(&state);
    let steady_base = steady.paint_plan_with_defaults(bounds);
    assert_single_playmark_label(&steady_base, "500 ms");

    let mut moved = waveform_widget_for_state(&state);
    moved.active_drag_kind = Some(WaveformActiveDragKind::SelectionMove(
        WaveformSelectionKind::Play,
    ));
    moved.live_selection_preview = Some(LiveSelectionPreview {
        kind: WaveformSelectionKind::Play,
        selection: wavecrate::selection::SelectionRange::new(0.4, 0.7),
    });

    assert!(
        !moved.prefers_pointer_move_paint_only(),
        "Base-to-live label ownership must invalidate the cached steady scene"
    );
    let rebuilt = composed_widget_plan(&moved, bounds);
    assert_single_playmark_label(&rebuilt, "300 ms");
    assert!(!rebuilt.contains_text("500 ms"));
}

#[test]
fn rebuilt_move_widget_preserves_one_live_playmark_label_owner() {
    let bounds = Rect::from_size(400.0, 80.0);
    let mut state = WaveformState::synthetic_for_tests();
    state.play_selection = Some(wavecrate::selection::SelectionRange::new(0.2, 0.7));
    let active_drag = Some(WaveformActiveDragKind::SelectionMove(
        WaveformSelectionKind::Play,
    ));

    let mut previous = waveform_widget_for_state(&state);
    previous.active_drag_kind = active_drag;
    previous.live_selection_preview = Some(LiveSelectionPreview {
        kind: WaveformSelectionKind::Play,
        selection: wavecrate::selection::SelectionRange::new(0.4, 0.7),
    });

    let mut rebuilt = waveform_widget_for_state(&state);
    rebuilt.active_drag_kind = active_drag;
    rebuilt.synchronize_from_previous(&previous);

    assert_eq!(
        rebuilt.live_selection_preview,
        previous.live_selection_preview
    );
    let plan = composed_widget_plan(&rebuilt, bounds);
    assert_single_playmark_label(&plan, "300 ms");
    assert!(!plan.contains_text("500 ms"));
}

fn waveform_state_with_duration_seconds(seconds: usize) -> WaveformState {
    WaveformState::from_file(Arc::new(waveform_file_from_mono_samples(
        std::path::PathBuf::from(format!("duration-{seconds}.wav")),
        Arc::from([0_u8]),
        1,
        1,
        vec![0.0; seconds],
    )))
}

#[test]
fn playhead_cursor_paints_around_occlusion_rect() {
    let mut state = WaveformState::synthetic_for_tests();
    state.set_playhead_ratio(0.25);
    let props = WaveformWidgetProps::from_state_with_playhead_occlusion(
        &state,
        false,
        false,
        4,
        Some(Rect::from_min_max(
            Point::new(99.0, 20.0),
            Point::new(101.0, 60.0),
        )),
    );
    let widget = WaveformWidget::new(props);
    let plan = widget.paint_plan_with_defaults(Rect::from_size(400.0, 80.0));

    let playhead_segments = fill_rects(&plan)
        .into_iter()
        .filter(|fill| {
            (fill.color.r, fill.color.g, fill.color.b, fill.color.a) == (231, 229, 223, 210)
        })
        .collect::<Vec<_>>();

    assert_eq!(playhead_segments.len(), 2);
    assert!(
        playhead_segments
            .iter()
            .any(|fill| fill.rect.min.y == 0.0 && fill.rect.max.y == 20.0)
    );
    assert!(
        playhead_segments
            .iter()
            .any(|fill| fill.rect.min.y == 60.0 && fill.rect.max.y == 80.0)
    );
}

#[test]
fn hover_cursor_paints_thin_warm_gray_overlay_line() {
    let state = WaveformState::synthetic_for_tests();
    let mut widget = waveform_widget_for_state(&state);
    let bounds = Rect::from_size(400.0, 80.0);
    let output = widget.handle_input(bounds, WidgetInput::pointer_move(Point::new(100.0, 40.0)));
    assert!(output.is_none());

    let plan = runtime_overlay_plan(&widget, bounds);

    let cursor = fill_rects(&plan)
        .into_iter()
        .find(|fill| {
            (fill.color.r, fill.color.g, fill.color.b, fill.color.a) == (231, 229, 223, 190)
        })
        .expect("hover cursor fill paints");
    assert_eq!(cursor.rect.width(), 1.0);
    assert!((cursor.rect.center().x - 100.0).abs() < 1.0);
}

#[test]
fn playmark_slide_handle_hover_paints_bright_overlay() {
    let mut state = WaveformState::synthetic_for_tests();
    state.play_selection = Some(wavecrate::selection::SelectionRange::new(0.2, 0.6));
    state.play_mark_ratio = Some(0.2);
    let mut widget = waveform_widget_for_state(&state);
    let bounds = Rect::from_size(200.0, 80.0);

    let output = widget.handle_input(bounds, WidgetInput::pointer_move(Point::new(60.0, 3.0)));
    assert!(output.is_none());
    assert_eq!(widget.hover_cursor_ratio, None);

    let plan = runtime_overlay_plan(&widget, bounds);
    let fills = fill_rects(&plan);
    assert!(fills.iter().any(|fill| {
        (fill.rect.min.x - 49.0).abs() < 0.001
            && (fill.rect.max.x - 111.0).abs() < 0.001
            && (fill.rect.min.y - 0.0).abs() < 0.001
            && (fill.rect.max.y - 7.0).abs() < 0.001
            && (fill.color.r, fill.color.g, fill.color.b, fill.color.a) == (241, 121, 98, 255)
    }));
    assert!(!fills.iter().any(
        |fill| (fill.color.r, fill.color.g, fill.color.b, fill.color.a) == (255, 255, 255, 210)
    ));
}

#[test]
fn playmark_resize_handle_hover_paints_bright_overlay() {
    let mut state = WaveformState::synthetic_for_tests();
    state.play_selection = Some(wavecrate::selection::SelectionRange::new(0.2, 0.6));
    state.play_mark_ratio = Some(0.2);
    let mut widget = waveform_widget_for_state(&state);
    let bounds = Rect::from_size(200.0, 80.0);

    let output = widget.handle_input(bounds, WidgetInput::pointer_move(Point::new(120.0, 8.0)));
    assert!(output.is_none());
    assert_eq!(widget.hover_cursor_ratio, None);

    let plan = runtime_overlay_plan(&widget, bounds);
    let fills = fill_rects(&plan);
    assert!(fills.iter().any(|fill| {
        (fill.rect.min.x - 115.5).abs() < 0.001
            && (fill.rect.max.x - 124.5).abs() < 0.001
            && (fill.rect.min.y - 0.0).abs() < 0.001
            && (fill.rect.max.y - 22.0).abs() < 0.001
            && (fill.color.r, fill.color.g, fill.color.b, fill.color.a) == (241, 121, 98, 255)
    }));
    assert!(!fills.iter().any(|fill| {
        (fill.rect.min.x - 112.5).abs() < 0.001
            && (fill.rect.max.x - 127.5).abs() < 0.001
            && (fill.rect.min.y - 0.0).abs() < 0.001
            && (fill.rect.max.y - 22.0).abs() < 0.001
            && (fill.color.r, fill.color.g, fill.color.b, fill.color.a) == (241, 121, 98, 180)
    }));
    assert_no_white_hover_border(&plan);
}

#[test]
fn playmark_export_handle_hover_paints_bright_overlay() {
    let mut state = WaveformState::synthetic_for_tests();
    state.play_selection = Some(wavecrate::selection::SelectionRange::new(0.2, 0.6));
    state.play_mark_ratio = Some(0.2);
    let mut widget = waveform_widget_for_state(&state);
    let bounds = Rect::from_size(200.0, 80.0);

    let output = widget.handle_input(bounds, WidgetInput::pointer_move(Point::new(118.0, 76.0)));
    assert!(output.is_none());
    assert_eq!(widget.hover_cursor_ratio, None);

    let plan = runtime_overlay_plan(&widget, bounds);
    let fills = fill_rects(&plan);
    assert!(fills.iter().any(|fill| {
        (fill.rect.min.x - 104.0).abs() < 0.001
            && (fill.rect.max.x - 120.0).abs() < 0.001
            && (fill.rect.min.y - 64.0).abs() < 0.001
            && (fill.rect.max.y - 80.0).abs() < 0.001
            && (fill.color.r, fill.color.g, fill.color.b, fill.color.a) == (241, 121, 98, 255)
    }));
    assert!(!fills.iter().any(|fill| {
        (fill.rect.min.x - 100.0).abs() < 0.001
            && (fill.rect.max.x - 124.0).abs() < 0.001
            && (fill.rect.min.y - 60.0).abs() < 0.001
            && (fill.rect.max.y - 80.0).abs() < 0.001
            && (fill.color.r, fill.color.g, fill.color.b, fill.color.a) == (241, 121, 98, 180)
    }));
    assert!(!fills.iter().any(
        |fill| (fill.color.r, fill.color.g, fill.color.b, fill.color.a) == (255, 255, 255, 210)
    ));
    assert_no_white_hover_border(&plan);
}

#[test]
fn editmark_slide_handle_hover_paints_bright_overlay() {
    let mut state = WaveformState::synthetic_for_tests();
    state.edit_selection = Some(wavecrate::selection::SelectionRange::new(0.2, 0.6));
    state.edit_mark_ratio = Some(0.2);
    let mut widget = waveform_widget_for_state(&state);
    let bounds = Rect::from_size(200.0, 80.0);

    let output = widget.handle_input(bounds, WidgetInput::pointer_move(Point::new(60.0, 3.0)));
    assert!(output.is_none());
    assert_eq!(widget.hover_cursor_ratio, None);

    let plan = runtime_overlay_plan(&widget, bounds);
    let fills = fill_rects(&plan);
    assert!(fills.iter().any(|fill| {
        (fill.rect.min.x - 49.0).abs() < 0.001
            && (fill.rect.max.x - 111.0).abs() < 0.001
            && (fill.rect.min.y - 0.0).abs() < 0.001
            && (fill.rect.max.y - 7.0).abs() < 0.001
            && (fill.color.r, fill.color.g, fill.color.b, fill.color.a) == (174, 176, 173, 255)
    }));
}

#[test]
fn editmark_bottom_resize_handles_paint_on_base_edit_selection() {
    let mut state = WaveformState::synthetic_for_tests();
    state.edit_selection = Some(wavecrate::selection::SelectionRange::new(0.2, 0.6));
    let widget = waveform_widget_for_state(&state);
    let plan = widget.paint_plan_with_defaults(Rect::from_size(200.0, 80.0));
    let fills = fill_rects(&plan);

    assert!(fills.iter().any(|fill| {
        (fill.rect.min.x - 36.5).abs() < 0.001
            && (fill.rect.max.x - 43.5).abs() < 0.001
            && (fill.rect.min.y - 58.0).abs() < 0.001
            && (fill.rect.max.y - 80.0).abs() < 0.001
            && (fill.color.r, fill.color.g, fill.color.b, fill.color.a) == (174, 176, 173, 190)
    }));
    assert!(fills.iter().any(|fill| {
        (fill.rect.min.x - 116.5).abs() < 0.001
            && (fill.rect.max.x - 123.5).abs() < 0.001
            && (fill.rect.min.y - 58.0).abs() < 0.001
            && (fill.rect.max.y - 80.0).abs() < 0.001
            && (fill.color.r, fill.color.g, fill.color.b, fill.color.a) == (174, 176, 173, 190)
    }));
}

#[test]
fn playmark_resize_handles_paint_above_boundary_and_play_start_lines() {
    let mut state = WaveformState::synthetic_for_tests();
    state.play_selection = Some(wavecrate::selection::SelectionRange::new(0.2, 0.6));
    state.play_mark_ratio = Some(0.2);
    let widget = waveform_widget_for_state(&state);
    let plan = widget.paint_plan_with_defaults(Rect::from_size(200.0, 80.0));
    let fills = fill_rects(&plan);

    let left_boundary = fill_index(&fills, "left playmark boundary", |fill| {
        (fill.rect.center().x - 40.0).abs() < 1.0
            && (fill.color.r, fill.color.g, fill.color.b, fill.color.a) == (241, 121, 98, 230)
    });
    let right_boundary = fill_index(&fills, "right playmark boundary", |fill| {
        (fill.rect.center().x - 120.0).abs() < 1.0
            && (fill.color.r, fill.color.g, fill.color.b, fill.color.a) == (241, 121, 98, 230)
    });
    let play_start_marker = fill_index(&fills, "play-start marker", |fill| {
        (fill.rect.center().x - 40.0).abs() < 1.0
            && (fill.color.r, fill.color.g, fill.color.b, fill.color.a) == (233, 88, 67, 245)
    });
    let left_handle = fill_index(&fills, "left playmark resize handle", |fill| {
        (fill.rect.min.x - 35.5).abs() < 0.001
            && (fill.rect.max.x - 44.5).abs() < 0.001
            && (fill.rect.min.y - 0.0).abs() < 0.001
            && (fill.rect.max.y - 22.0).abs() < 0.001
            && (fill.color.r, fill.color.g, fill.color.b, fill.color.a) == (241, 121, 98, 220)
    });
    let right_handle = fill_index(&fills, "right playmark resize handle", |fill| {
        (fill.rect.min.x - 115.5).abs() < 0.001
            && (fill.rect.max.x - 124.5).abs() < 0.001
            && (fill.rect.min.y - 0.0).abs() < 0.001
            && (fill.rect.max.y - 22.0).abs() < 0.001
            && (fill.color.r, fill.color.g, fill.color.b, fill.color.a) == (241, 121, 98, 220)
    });

    assert!(
        left_boundary < left_handle,
        "left resize handle should paint above the playmark boundary line"
    );
    assert!(
        right_boundary < right_handle,
        "right resize handle should paint above the playmark boundary line"
    );
    assert!(
        play_start_marker < left_handle,
        "left resize handle should paint above the bright play-start marker"
    );
}

#[test]
fn editmark_resize_handles_paint_above_boundary_lines() {
    let mut state = WaveformState::synthetic_for_tests();
    state.edit_selection = Some(wavecrate::selection::SelectionRange::new(0.2, 0.6));
    let widget = waveform_widget_for_state(&state);
    let plan = widget.paint_plan_with_defaults(Rect::from_size(200.0, 80.0));
    let fills = fill_rects(&plan);

    let left_boundary = fill_index(&fills, "left editmark boundary", |fill| {
        (fill.rect.center().x - 40.0).abs() < 1.0
            && (fill.color.r, fill.color.g, fill.color.b, fill.color.a) == (174, 176, 173, 230)
    });
    let right_boundary = fill_index(&fills, "right editmark boundary", |fill| {
        (fill.rect.center().x - 120.0).abs() < 1.0
            && (fill.color.r, fill.color.g, fill.color.b, fill.color.a) == (174, 176, 173, 230)
    });
    let left_handle = fill_index(&fills, "left editmark resize handle", |fill| {
        (fill.rect.min.x - 36.5).abs() < 0.001
            && (fill.rect.max.x - 43.5).abs() < 0.001
            && (fill.rect.min.y - 58.0).abs() < 0.001
            && (fill.rect.max.y - 80.0).abs() < 0.001
            && (fill.color.r, fill.color.g, fill.color.b, fill.color.a) == (174, 176, 173, 190)
    });
    let right_handle = fill_index(&fills, "right editmark resize handle", |fill| {
        (fill.rect.min.x - 116.5).abs() < 0.001
            && (fill.rect.max.x - 123.5).abs() < 0.001
            && (fill.rect.min.y - 58.0).abs() < 0.001
            && (fill.rect.max.y - 80.0).abs() < 0.001
            && (fill.color.r, fill.color.g, fill.color.b, fill.color.a) == (174, 176, 173, 190)
    });

    assert!(
        left_boundary < left_handle,
        "left editmark resize handle should paint above the boundary line"
    );
    assert!(
        right_boundary < right_handle,
        "right editmark resize handle should paint above the boundary line"
    );
}

#[test]
fn editmark_bottom_resize_handle_hides_on_faded_side() {
    let mut state = WaveformState::synthetic_for_tests();
    state.edit_selection = Some(
        wavecrate::selection::SelectionRange::new(0.2, 0.6)
            .with_fade_in(0.25, 0.2)
            .with_fade_out(0.25, 0.7),
    );
    let widget = waveform_widget_for_state(&state);
    let plan = widget.paint_plan_with_defaults(Rect::from_size(200.0, 80.0));
    let fills = fill_rects(&plan);

    assert!(!fills.iter().any(|fill| {
        (fill.rect.min.x - 36.5).abs() < 0.001
            && (fill.rect.max.x - 43.5).abs() < 0.001
            && (fill.rect.min.y - 58.0).abs() < 0.001
            && (fill.rect.max.y - 80.0).abs() < 0.001
            && (fill.color.r, fill.color.g, fill.color.b, fill.color.a) == (174, 176, 173, 190)
    }));
    assert!(!fills.iter().any(|fill| {
        (fill.rect.min.x - 116.5).abs() < 0.001
            && (fill.rect.max.x - 123.5).abs() < 0.001
            && (fill.rect.min.y - 58.0).abs() < 0.001
            && (fill.rect.max.y - 80.0).abs() < 0.001
            && (fill.color.r, fill.color.g, fill.color.b, fill.color.a) == (174, 176, 173, 190)
    }));
}

#[test]
fn editmark_bottom_resize_handle_hover_paints_bright_overlay() {
    let mut state = WaveformState::synthetic_for_tests();
    state.edit_selection = Some(wavecrate::selection::SelectionRange::new(0.2, 0.6));
    state.edit_mark_ratio = Some(0.2);
    let mut widget = waveform_widget_for_state(&state);
    let bounds = Rect::from_size(200.0, 80.0);

    let output = widget.handle_input(bounds, WidgetInput::pointer_move(Point::new(120.0, 76.0)));
    assert!(output.is_none());
    assert_eq!(widget.hover_cursor_ratio, None);

    let plan = runtime_overlay_plan(&widget, bounds);
    let fills = fill_rects(&plan);
    assert!(fills.iter().any(|fill| {
        (fill.rect.min.x - 116.5).abs() < 0.001
            && (fill.rect.max.x - 123.5).abs() < 0.001
            && (fill.rect.min.y - 58.0).abs() < 0.001
            && (fill.rect.max.y - 80.0).abs() < 0.001
            && (fill.color.r, fill.color.g, fill.color.b, fill.color.a) == (174, 176, 173, 255)
    }));
    assert_no_white_hover_border(&plan);
}

#[test]
fn editmark_gain_handle_hover_paints_bright_center_tab() {
    let mut state = WaveformState::synthetic_for_tests();
    state.edit_selection = Some(wavecrate::selection::SelectionRange::new(0.2, 0.6));
    state.edit_mark_ratio = Some(0.2);
    let mut widget = waveform_widget_for_state(&state);
    let bounds = Rect::from_size(200.0, 80.0);

    let output = widget.handle_input(bounds, WidgetInput::pointer_move(Point::new(80.0, 5.0)));
    assert!(output.is_none());
    assert_eq!(widget.hover_cursor_ratio, None);
    assert!(widget.hovered_edit_gain_handle);
    assert_eq!(widget.hovered_selection_handle, None);

    let plan = runtime_overlay_plan(&widget, bounds);
    let fills = fill_rects(&plan);
    assert!(fills.iter().any(|fill| {
        (fill.rect.min.x - 74.0).abs() < 0.001
            && (fill.rect.max.x - 86.0).abs() < 0.001
            && (fill.rect.min.y - 0.0).abs() < 0.001
            && (fill.rect.max.y - 10.0).abs() < 0.001
            && (fill.color.r, fill.color.g, fill.color.b, fill.color.a) == (174, 176, 173, 255)
    }));
}

#[test]
fn editmark_gain_handle_paints_on_base_edit_selection() {
    let mut state = WaveformState::synthetic_for_tests();
    state.edit_selection = Some(wavecrate::selection::SelectionRange::new(0.2, 0.6));
    let widget = waveform_widget_for_state(&state);
    let plan = widget.paint_plan_with_defaults(Rect::from_size(200.0, 80.0));
    let fills = fill_rects(&plan);

    assert!(fills.iter().any(|fill| {
        (fill.rect.min.x - 74.0).abs() < 0.001
            && (fill.rect.max.x - 86.0).abs() < 0.001
            && (fill.rect.min.y - 0.0).abs() < 0.001
            && (fill.rect.max.y - 10.0).abs() < 0.001
            && (fill.color.r, fill.color.g, fill.color.b, fill.color.a) == (174, 176, 173, 225)
    }));
}

#[test]
fn edit_fade_handle_hover_paints_bright_overlay() {
    let mut state = WaveformState::synthetic_for_tests();
    state.edit_selection =
        Some(wavecrate::selection::SelectionRange::new(0.2, 0.6).with_fade_in(0.25, 0.2));
    let mut widget = waveform_widget_for_state(&state);
    let bounds = Rect::from_size(200.0, 80.0);

    let output = widget.handle_input(bounds, WidgetInput::pointer_move(Point::new(60.0, 5.0)));
    assert!(output.is_none());
    assert_eq!(widget.hover_cursor_ratio, None);

    let plan = runtime_overlay_plan(&widget, bounds);
    let fills = fill_rects(&plan);
    assert!(fills.iter().any(|fill| {
        (fill.rect.min.x - 55.0).abs() < 0.001
            && (fill.rect.max.x - 65.0).abs() < 0.001
            && (fill.rect.min.y - 0.0).abs() < 0.001
            && (fill.rect.max.y - 10.0).abs() < 0.001
            && (fill.color.r, fill.color.g, fill.color.b, fill.color.a) == (174, 176, 173, 255)
    }));
}

#[test]
fn edit_fade_outer_gain_handle_paints_at_current_gain_height() {
    let mut state = WaveformState::synthetic_for_tests();
    state.edit_selection = Some(
        wavecrate::selection::SelectionRange::new(0.2, 0.6)
            .with_fade_in(0.25, 0.2)
            .with_fade_in_mute(0.25)
            .with_fade_in_outer_gain(0.0),
    );
    let widget = waveform_widget_for_state(&state);
    let plan = widget.paint_plan_with_defaults(Rect::from_size(200.0, 80.0));
    let fills = fill_rects(&plan);

    assert!(fills.iter().any(|fill| {
        (fill.rect.min.x - 15.0).abs() < 0.001
            && (fill.rect.max.x - 25.0).abs() < 0.001
            && (fill.rect.min.y - 35.0).abs() < 0.001
            && (fill.rect.max.y - 45.0).abs() < 0.001
            && (fill.color.r, fill.color.g, fill.color.b, fill.color.a) == (174, 176, 173, 205)
    }));
}

#[test]
fn edit_fade_outer_gain_handle_hover_paints_bright_overlay() {
    let mut state = WaveformState::synthetic_for_tests();
    state.edit_selection = Some(
        wavecrate::selection::SelectionRange::new(0.2, 0.6)
            .with_fade_in(0.25, 0.2)
            .with_fade_in_mute(0.25),
    );
    let mut widget = waveform_widget_for_state(&state);
    let bounds = Rect::from_size(200.0, 80.0);

    let output = widget.handle_input(bounds, WidgetInput::pointer_move(Point::new(20.0, 5.0)));
    assert!(output.is_none());
    assert_eq!(
        widget.hovered_edit_fade_outer_gain_handle,
        Some(WaveformEditFadeOuterGainHandle::In)
    );

    let plan = runtime_overlay_plan(&widget, bounds);
    let fills = fill_rects(&plan);
    assert!(fills.iter().any(|fill| {
        (fill.rect.min.x - 15.0).abs() < 0.001
            && (fill.rect.max.x - 25.0).abs() < 0.001
            && (fill.rect.min.y - 0.0).abs() < 0.001
            && (fill.rect.max.y - 10.0).abs() < 0.001
            && (fill.color.r, fill.color.g, fill.color.b, fill.color.a) == (174, 176, 173, 255)
    }));
}

#[test]
fn play_start_marker_is_hidden_at_sample_start() {
    let mut state = WaveformState::synthetic_for_tests();
    state.start_playback(0.0);
    let widget = waveform_widget_for_state(&state);
    let plan = widget.paint_plan_with_defaults(Rect::from_size(400.0, 80.0));

    assert!(
        !fill_rects(&plan).iter().any(|fill| {
            (fill.color.r, fill.color.g, fill.color.b, fill.color.a) == (233, 88, 67, 245)
        }),
        "play-start marker should be implicit when playback starts at sample head"
    );
}

#[test]
fn play_start_marker_paints_coral_when_start_deviates_from_sample_start() {
    let mut state = WaveformState::synthetic_for_tests();
    state.start_playback(0.125);
    let widget = waveform_widget_for_state(&state);
    let plan = widget.paint_plan_with_defaults(Rect::from_size(400.0, 80.0));

    assert!(fill_rects(&plan).iter().any(|fill| {
        (fill.rect.center().x / 400.0 - 0.125).abs() < 0.01
            && (fill.color.r, fill.color.g, fill.color.b, fill.color.a) == (233, 88, 67, 245)
    }));
}

#[test]
fn play_start_marker_paints_even_when_play_selection_exists() {
    let mut state = WaveformState::synthetic_for_tests();
    state.play_selection = Some(wavecrate::selection::SelectionRange::new(0.2, 0.6));
    state.start_playback(0.4);
    let widget = waveform_widget_for_state(&state);
    let plan = widget.paint_plan_with_defaults(Rect::from_size(400.0, 80.0));

    assert!(fill_rects(&plan).iter().any(|fill| {
        (fill.rect.center().x / 400.0 - 0.4).abs() < 0.01
            && (fill.color.r, fill.color.g, fill.color.b, fill.color.a) == (233, 88, 67, 245)
    }));
}

#[test]
fn beat_guides_paint_internal_lines_inside_play_selection() {
    let mut state = WaveformState::synthetic_for_tests();
    state.play_selection = Some(wavecrate::selection::SelectionRange::new(0.2, 0.6));
    let widget = waveform_widget_for_state_with_beat_guides(&state, true, 4);
    let plan = widget.paint_plan_with_defaults(Rect::from_size(200.0, 80.0));

    let guides = fill_rects(&plan)
        .into_iter()
        .filter(|fill| {
            (fill.color.r, fill.color.g, fill.color.b, fill.color.a) == (231, 229, 223, 150)
        })
        .collect::<Vec<_>>();

    assert_eq!(guides.len(), 3);
    for expected_x in [60.0, 80.0, 100.0] {
        assert!(
            guides.iter().any(|fill| {
                (fill.rect.center().x - expected_x).abs() < 0.01
                    && (fill.rect.min.y - 11.0).abs() < 0.01
                    && (fill.rect.max.y - 69.0).abs() < 0.01
            }),
            "expected beat guide at x={expected_x}, got {guides:?}"
        );
    }
}

#[test]
fn beat_guides_paint_internal_lines_inside_edit_selection() {
    let mut state = WaveformState::synthetic_for_tests();
    state.edit_selection = Some(wavecrate::selection::SelectionRange::new(0.2, 0.6));
    let widget = waveform_widget_for_state_with_beat_guides(&state, true, 4);
    let plan = widget.paint_plan_with_defaults(Rect::from_size(200.0, 80.0));

    let guides = fill_rects(&plan)
        .into_iter()
        .filter(|fill| {
            (fill.color.r, fill.color.g, fill.color.b, fill.color.a) == (231, 229, 223, 150)
        })
        .collect::<Vec<_>>();

    assert_eq!(guides.len(), 3);
    for expected_x in [60.0, 80.0, 100.0] {
        assert!(
            guides.iter().any(|fill| {
                (fill.rect.center().x - expected_x).abs() < 0.01
                    && (fill.rect.min.y - 11.0).abs() < 0.01
                    && (fill.rect.max.y - 69.0).abs() < 0.01
            }),
            "expected beat guide at x={expected_x}, got {guides:?}"
        );
    }
}

#[test]
fn beat_guides_do_not_paint_when_toggle_is_off() {
    let mut state = WaveformState::synthetic_for_tests();
    state.play_selection = Some(wavecrate::selection::SelectionRange::new(0.2, 0.6));
    state.edit_selection = Some(wavecrate::selection::SelectionRange::new(0.3, 0.7));
    let widget = waveform_widget_for_state_with_beat_guides(&state, false, 4);
    let plan = widget.paint_plan_with_defaults(Rect::from_size(200.0, 80.0));

    assert!(!fill_rects(&plan).iter().any(|fill| {
        (fill.color.r, fill.color.g, fill.color.b, fill.color.a) == (231, 229, 223, 150)
    }));
}

#[test]
fn selection_range_projects_visible_ratios_inside_viewport() {
    let mut state = WaveformState::synthetic_for_tests();
    state.apply_interaction(WaveformInteraction::BeginSelection {
        kind: WaveformSelectionKind::Edit,
        visible_ratio: 0.25,
    });
    state.apply_interaction(WaveformInteraction::UpdateSelection {
        visible_ratio: 0.75,
    });
    state.apply_interaction(WaveformInteraction::FinishSelection {
        visible_ratio: 0.75,
    });
    let widget = waveform_widget_for_state(&state);
    let range = widget
        .visible_normalized_range_for_selection(state.edit_selection())
        .expect("selection range");

    assert!((range.start_fraction() - 0.25).abs() < 0.001);
    assert!((range.end_fraction() - 0.75).abs() < 0.001);
}

#[test]
fn selection_fill_paints_as_overlay_widget_rects() {
    let mut state = WaveformState::synthetic_for_tests();
    state.apply_interaction(WaveformInteraction::BeginSelection {
        kind: WaveformSelectionKind::Play,
        visible_ratio: 0.2,
    });
    state.apply_interaction(WaveformInteraction::UpdateSelection { visible_ratio: 0.6 });
    state.apply_interaction(WaveformInteraction::FinishSelection { visible_ratio: 0.6 });
    let widget = waveform_widget_for_state(&state);
    let plan = widget.paint_plan_with_defaults(Rect::from_size(200.0, 80.0));

    assert!(
        plan.gpu_surfaces().next().is_none(),
        "ordinary waveform overlay widget must not emit the GPU waveform"
    );
    let fills = fill_rects(&plan);
    assert!(fills.iter().any(|fill| {
        (fill.rect.min.x - 40.0).abs() < 0.001
            && (fill.rect.max.x - 120.0).abs() < 0.001
            && (fill.color.r, fill.color.g, fill.color.b, fill.color.a) == (241, 121, 98, 48)
    }));
    assert!(fills.iter().any(|fill| {
        (fill.rect.center().x - 40.0).abs() < 1.0
            && (fill.color.r, fill.color.g, fill.color.b, fill.color.a) == (241, 121, 98, 230)
    }));
    assert!(fills.iter().any(|fill| {
        (fill.rect.center().x - 120.0).abs() < 1.0
            && (fill.color.r, fill.color.g, fill.color.b, fill.color.a) == (241, 121, 98, 230)
    }));
}

#[test]
fn extracted_ranges_paint_as_gray_waveform_overlays() {
    let mut state = WaveformState::synthetic_for_tests();
    state
        .extracted_ranges
        .push(wavecrate::selection::SelectionRange::new(0.2, 0.6));
    let widget = waveform_widget_for_state(&state);
    let plan = widget.paint_plan_with_defaults(Rect::from_size(200.0, 80.0));

    let fills = fill_rects(&plan);
    assert!(fills.iter().any(|fill| {
        (fill.rect.min.x - 40.0).abs() < 0.001
            && (fill.rect.max.x - 120.0).abs() < 0.001
            && (fill.color.r, fill.color.g, fill.color.b, fill.color.a) == (156, 160, 168, 108)
    }));
    assert!(fills.iter().any(|fill| {
        (fill.rect.min.x - 40.0).abs() < 0.001
            && (fill.rect.max.x - 120.0).abs() < 0.001
            && (fill.rect.min.y - 0.0).abs() < 0.001
            && (fill.rect.max.y - 2.0).abs() < 0.001
            && (fill.color.r, fill.color.g, fill.color.b, fill.color.a) == (206, 211, 219, 225)
    }));
    assert!(fills.iter().any(|fill| {
        (fill.rect.min.x - 40.0).abs() < 0.001
            && (fill.rect.max.x - 120.0).abs() < 0.001
            && (fill.rect.min.y - 78.0).abs() < 0.001
            && (fill.rect.max.y - 80.0).abs() < 0.001
            && (fill.color.r, fill.color.g, fill.color.b, fill.color.a) == (206, 211, 219, 225)
    }));
}

#[test]
fn played_ranges_paint_as_a_subtle_thick_bottom_rail() {
    let mut state = WaveformState::synthetic_for_tests();
    state
        .played_ranges
        .push(wavecrate::selection::SelectionRange::new(0.2, 0.6));
    let widget = waveform_widget_for_state(&state);
    let plan = widget.paint_plan_with_defaults(Rect::from_size(200.0, 80.0));

    let fills = fill_rects(&plan);
    let rail = fills
        .iter()
        .find(|fill| {
            (fill.color.r, fill.color.g, fill.color.b, fill.color.a) == (98, 102, 106, 255)
        })
        .expect("played range rail");
    assert!((rail.rect.min.x - 40.0).abs() < 0.001);
    assert!((rail.rect.max.x - 120.0).abs() < 0.001);
    assert!((rail.rect.min.y - 76.0).abs() < 0.001);
    assert!((rail.rect.max.y - 80.0).abs() < 0.001);
}

#[test]
fn extracted_ranges_paint_while_playmark_selection_drag_is_active() {
    let mut state = WaveformState::synthetic_for_tests();
    state
        .extracted_ranges
        .push(wavecrate::selection::SelectionRange::new(0.2, 0.6));
    state.start_similar_sections(wavecrate::selection::SelectionRange::new(0.1, 0.2));
    state.finish_similar_sections_scan(vec![wavecrate::selection::SelectionRange::new(0.3, 0.7)]);
    state.play_selection = Some(wavecrate::selection::SelectionRange::new(0.4, 0.8));
    state.play_mark_ratio = Some(0.4);

    let mut widget = waveform_widget_for_state(&state);
    widget.active_drag_kind = Some(WaveformActiveDragKind::Selection(
        WaveformSelectionKind::Play,
    ));
    widget.live_selection_preview = Some(LiveSelectionPreview {
        kind: WaveformSelectionKind::Play,
        selection: wavecrate::selection::SelectionRange::new(0.4, 0.8),
    });
    let bounds = Rect::from_size(200.0, 80.0);
    let plan = widget.paint_plan_with_defaults(bounds);

    let fills = fill_rects(&plan);
    assert!(
        fills.iter().any(|fill| {
            (fill.rect.min.x - 40.0).abs() < 0.001
                && (fill.rect.max.x - 120.0).abs() < 0.001
                && (fill.color.r, fill.color.g, fill.color.b, fill.color.a) == (156, 160, 168, 108)
        }),
        "extracted overlays should remain visible during live playmark drags"
    );
    assert!(
        !fills.iter().any(|fill| {
            (fill.color.r, fill.color.g, fill.color.b, fill.color.a) == (114, 235, 184, 54)
        }),
        "similar-section overlays should still pause during live playmark drags"
    );
    let runtime_plan = runtime_overlay_plan(&widget, bounds);
    let runtime_fills = fill_rects(&runtime_plan);
    assert!(
        runtime_fills.iter().any(|fill| {
            (fill.rect.min.x - 80.0).abs() < 0.001
                && (fill.rect.max.x - 160.0).abs() < 0.001
                && (fill.color.r, fill.color.g, fill.color.b, fill.color.a) == (241, 121, 98, 48)
        }),
        "the live playmark selection itself should keep painting"
    );
}

#[test]
fn extracted_ranges_paint_while_editmark_selection_drag_is_active() {
    let mut state = WaveformState::synthetic_for_tests();
    state
        .extracted_ranges
        .push(wavecrate::selection::SelectionRange::new(0.2, 0.6));
    state.edit_selection = Some(wavecrate::selection::SelectionRange::new(0.4, 0.8));
    state.edit_mark_ratio = Some(0.4);

    let mut widget = waveform_widget_for_state(&state);
    widget.active_drag_kind = Some(WaveformActiveDragKind::Selection(
        WaveformSelectionKind::Edit,
    ));
    widget.live_selection_preview = Some(LiveSelectionPreview {
        kind: WaveformSelectionKind::Edit,
        selection: wavecrate::selection::SelectionRange::new(0.4, 0.8),
    });
    let bounds = Rect::from_size(200.0, 80.0);
    let plan = widget.paint_plan_with_defaults(bounds);

    let fills = fill_rects(&plan);
    assert!(
        fills.iter().any(|fill| {
            (fill.rect.min.x - 40.0).abs() < 0.001
                && (fill.rect.max.x - 120.0).abs() < 0.001
                && (fill.color.r, fill.color.g, fill.color.b, fill.color.a) == (156, 160, 168, 108)
        }),
        "extracted overlays should remain visible during live editmark drags"
    );
    let runtime_plan = runtime_overlay_plan(&widget, bounds);
    let runtime_fills = fill_rects(&runtime_plan);
    assert!(
        runtime_fills.iter().any(|fill| {
            (fill.rect.min.x - 80.0).abs() < 0.001
                && (fill.rect.max.x - 160.0).abs() < 0.001
                && (fill.color.r, fill.color.g, fill.color.b, fill.color.a) == (174, 176, 173, 46)
        }),
        "the live editmark selection itself should keep painting"
    );
}

#[test]
fn resize_selection_paints_once_from_base_layer_when_preview_is_live() {
    let mut state = WaveformState::synthetic_for_tests();
    state.play_selection = Some(wavecrate::selection::SelectionRange::new(0.2, 0.8));
    state.play_mark_ratio = Some(0.2);
    let mut widget = waveform_widget_for_state(&state);
    widget.active_drag_kind = Some(WaveformActiveDragKind::SelectionResize(
        WaveformSelectionKind::Play,
        WaveformSelectionEdge::End,
    ));
    widget.live_selection_preview = Some(LiveSelectionPreview {
        kind: WaveformSelectionKind::Play,
        selection: wavecrate::selection::SelectionRange::new(0.2, 0.8),
    });

    let plan = widget.paint_plan_with_defaults(Rect::from_size(200.0, 80.0));
    let fills = fill_rects(&plan);

    assert!(
        fills.iter().any(|fill| {
            (fill.rect.min.x - 40.0).abs() < 0.001
                && (fill.rect.max.x - 160.0).abs() < 0.001
                && (fill.color.r, fill.color.g, fill.color.b, fill.color.a) == (241, 121, 98, 48)
        }),
        "app-state play selection should paint the current resize range"
    );

    let runtime_plan = runtime_overlay_plan(&widget, Rect::from_size(200.0, 80.0));
    let runtime_fills = fill_rects(&runtime_plan);
    assert!(
        runtime_fills.iter().all(|fill| {
            !((fill.rect.min.x - 40.0).abs() < 0.001
                && (fill.rect.max.x - 160.0).abs() < 0.001
                && (fill.color.r, fill.color.g, fill.color.b, fill.color.a) == (241, 121, 98, 48))
        }),
        "runtime resize preview should not double-paint the same translucent selection"
    );
}

#[test]
fn playmark_resize_drag_ghost_paints_active_handle_without_double_painting_selection() {
    let mut state = WaveformState::synthetic_for_tests();
    state.play_selection = Some(wavecrate::selection::SelectionRange::new(0.2, 0.8));
    state.play_mark_ratio = Some(0.2);
    let mut widget = waveform_widget_for_state(&state);
    widget.active_drag_kind = Some(WaveformActiveDragKind::SelectionResize(
        WaveformSelectionKind::Play,
        WaveformSelectionEdge::End,
    ));
    widget.live_selection_preview = Some(LiveSelectionPreview {
        kind: WaveformSelectionKind::Play,
        selection: wavecrate::selection::SelectionRange::new(0.2, 0.8),
    });

    let runtime_plan = runtime_overlay_plan(&widget, Rect::from_size(200.0, 80.0));
    let runtime_fills = fill_rects(&runtime_plan);

    assert!(
        runtime_fills.iter().any(|fill| {
            (fill.rect.min.x - 155.5).abs() < 0.001
                && (fill.rect.max.x - 164.5).abs() < 0.001
                && (fill.rect.min.y - 0.0).abs() < 0.001
                && (fill.rect.max.y - 22.0).abs() < 0.001
                && (fill.color.r, fill.color.g, fill.color.b, fill.color.a) == (241, 121, 98, 178)
        }),
        "active resize handle should paint a drag ghost"
    );
    assert!(
        runtime_fills.iter().all(|fill| {
            !((fill.rect.min.x - 40.0).abs() < 0.001
                && (fill.rect.max.x - 160.0).abs() < 0.001
                && (fill.color.r, fill.color.g, fill.color.b, fill.color.a) == (241, 121, 98, 48))
        }),
        "drag ghost must not reintroduce runtime double-painting of the playmark range"
    );
}

#[test]
fn playmark_move_drag_ghost_paints_body_handle_on_live_preview() {
    let mut state = WaveformState::synthetic_for_tests();
    state.play_selection = Some(wavecrate::selection::SelectionRange::new(0.2, 0.6));
    state.play_mark_ratio = Some(0.2);
    let mut widget = waveform_widget_for_state(&state);
    widget.active_drag_kind = Some(WaveformActiveDragKind::SelectionMove(
        WaveformSelectionKind::Play,
    ));
    widget.live_selection_preview = Some(LiveSelectionPreview {
        kind: WaveformSelectionKind::Play,
        selection: wavecrate::selection::SelectionRange::new(0.25, 0.65),
    });

    let plan = runtime_overlay_plan(&widget, Rect::from_size(200.0, 80.0));
    let fills = fill_rects(&plan);

    assert!(
        fills.iter().any(|fill| {
            (fill.rect.min.x - 59.0).abs() < 0.001
                && (fill.rect.max.x - 121.0).abs() < 0.001
                && (fill.rect.min.y - 0.0).abs() < 0.001
                && (fill.rect.max.y - 7.0).abs() < 0.001
                && (fill.color.r, fill.color.g, fill.color.b, fill.color.a) == (241, 121, 98, 178)
        }),
        "active move handle should paint a drag ghost over the live preview"
    );
}

#[test]
fn committed_selection_paints_as_resize_fallback_until_preview_is_live() {
    let mut state = WaveformState::synthetic_for_tests();
    state.play_selection = Some(wavecrate::selection::SelectionRange::new(0.2, 0.6));
    state.play_mark_ratio = Some(0.2);
    let mut widget = waveform_widget_for_state(&state);
    widget.active_drag_kind = Some(WaveformActiveDragKind::SelectionResize(
        WaveformSelectionKind::Play,
        WaveformSelectionEdge::End,
    ));

    let plan = widget.paint_plan_with_defaults(Rect::from_size(200.0, 80.0));
    let fills = fill_rects(&plan);

    assert!(
        fills.iter().any(|fill| {
            (fill.rect.min.x - 40.0).abs() < 0.001
                && (fill.rect.max.x - 120.0).abs() < 0.001
                && (fill.color.r, fill.color.g, fill.color.b, fill.color.a) == (241, 121, 98, 48)
        }),
        "committed play selection should remain visible until the live resize preview paints"
    );
}

#[test]
fn beat_guides_paint_from_live_playmark_creation_preview() {
    let state = WaveformState::synthetic_for_tests();
    let mut widget = waveform_widget_for_state_with_beat_guides(&state, true, 4);
    widget.active_drag_kind = Some(WaveformActiveDragKind::Selection(
        WaveformSelectionKind::Play,
    ));
    widget.live_selection_preview = Some(LiveSelectionPreview {
        kind: WaveformSelectionKind::Play,
        selection: wavecrate::selection::SelectionRange::new(0.2, 0.6),
    });

    let plan = runtime_overlay_plan(&widget, Rect::from_size(200.0, 80.0));
    let fills = fill_rects(&plan);

    assert_beat_guides_at(&fills, &[60.0, 80.0, 100.0]);
    assert!(
        fills.iter().any(|fill| {
            (fill.rect.min.x - 40.0).abs() < 0.001
                && (fill.rect.max.x - 120.0).abs() < 0.001
                && (fill.color.r, fill.color.g, fill.color.b, fill.color.a) == (241, 121, 98, 48)
        }),
        "the live playmark creation preview should still paint"
    );
}

#[test]
fn beat_guides_paint_from_live_editmark_creation_preview() {
    let state = WaveformState::synthetic_for_tests();
    let mut widget = waveform_widget_for_state_with_beat_guides(&state, true, 4);
    widget.active_drag_kind = Some(WaveformActiveDragKind::Selection(
        WaveformSelectionKind::Edit,
    ));
    widget.live_selection_preview = Some(LiveSelectionPreview {
        kind: WaveformSelectionKind::Edit,
        selection: wavecrate::selection::SelectionRange::new(0.2, 0.6),
    });

    let plan = runtime_overlay_plan(&widget, Rect::from_size(200.0, 80.0));
    let fills = fill_rects(&plan);

    assert_beat_guides_at(&fills, &[60.0, 80.0, 100.0]);
    assert!(
        fills.iter().any(|fill| {
            (fill.rect.min.x - 40.0).abs() < 0.001
                && (fill.rect.max.x - 120.0).abs() < 0.001
                && (fill.color.r, fill.color.g, fill.color.b, fill.color.a) == (174, 176, 173, 46)
        }),
        "the live editmark creation preview should still paint"
    );
}

#[test]
fn beat_guides_update_for_wider_live_creation_preview() {
    let state = WaveformState::synthetic_for_tests();
    let mut widget = waveform_widget_for_state_with_beat_guides(&state, true, 4);
    widget.active_drag_kind = Some(WaveformActiveDragKind::Selection(
        WaveformSelectionKind::Play,
    ));
    widget.live_selection_preview = Some(LiveSelectionPreview {
        kind: WaveformSelectionKind::Play,
        selection: wavecrate::selection::SelectionRange::new(0.1, 0.7),
    });

    let plan = runtime_overlay_plan(&widget, Rect::from_size(200.0, 80.0));
    let fills = fill_rects(&plan);

    assert_beat_guides_at(&fills, &[50.0, 80.0, 110.0]);
}

#[test]
fn beat_guides_remain_hidden_for_live_creation_preview_when_disabled() {
    let state = WaveformState::synthetic_for_tests();
    let mut widget = waveform_widget_for_state_with_beat_guides(&state, false, 4);
    widget.active_drag_kind = Some(WaveformActiveDragKind::Selection(
        WaveformSelectionKind::Play,
    ));
    widget.live_selection_preview = Some(LiveSelectionPreview {
        kind: WaveformSelectionKind::Play,
        selection: wavecrate::selection::SelectionRange::new(0.2, 0.6),
    });

    let plan = runtime_overlay_plan(&widget, Rect::from_size(200.0, 80.0));
    let fills = fill_rects(&plan);

    assert!(
        beat_guide_fills(&fills).is_empty(),
        "disabled beat guides should not paint during live creation previews"
    );
}

#[test]
fn beat_guides_paint_from_live_playmark_slide_preview() {
    let mut state = WaveformState::synthetic_for_tests();
    state.play_selection = Some(wavecrate::selection::SelectionRange::new(0.2, 0.6));
    state.play_mark_ratio = Some(0.2);
    let mut widget = waveform_widget_for_state_with_beat_guides(&state, true, 4);
    widget.active_drag_kind = Some(WaveformActiveDragKind::SelectionMove(
        WaveformSelectionKind::Play,
    ));
    widget.live_selection_preview = Some(LiveSelectionPreview {
        kind: WaveformSelectionKind::Play,
        selection: wavecrate::selection::SelectionRange::new(0.35, 0.75),
    });

    let plan = runtime_overlay_plan(&widget, Rect::from_size(200.0, 80.0));
    let fills = fill_rects(&plan);

    assert_beat_guides_at(&fills, &[90.0, 110.0, 130.0]);
    assert!(
        fills.iter().any(|fill| {
            (fill.rect.min.x - 70.0).abs() < 0.001
                && (fill.rect.max.x - 150.0).abs() < 0.001
                && (fill.color.r, fill.color.g, fill.color.b, fill.color.a) == (241, 121, 98, 48)
        }),
        "the live slid playmark selection should still paint"
    );
}

#[test]
fn beat_guides_paint_from_live_editmark_slide_preview() {
    let mut state = WaveformState::synthetic_for_tests();
    state.edit_selection = Some(wavecrate::selection::SelectionRange::new(0.2, 0.6));
    state.edit_mark_ratio = Some(0.2);
    let mut widget = waveform_widget_for_state_with_beat_guides(&state, true, 4);
    widget.active_drag_kind = Some(WaveformActiveDragKind::SelectionMove(
        WaveformSelectionKind::Edit,
    ));
    widget.live_selection_preview = Some(LiveSelectionPreview {
        kind: WaveformSelectionKind::Edit,
        selection: wavecrate::selection::SelectionRange::new(0.35, 0.75),
    });

    let plan = runtime_overlay_plan(&widget, Rect::from_size(200.0, 80.0));
    let fills = fill_rects(&plan);

    assert_beat_guides_at(&fills, &[90.0, 110.0, 130.0]);
    assert!(
        fills.iter().any(|fill| {
            (fill.rect.min.x - 70.0).abs() < 0.001
                && (fill.rect.max.x - 150.0).abs() < 0.001
                && (fill.color.r, fill.color.g, fill.color.b, fill.color.a) == (174, 176, 173, 46)
        }),
        "the live slid editmark selection should still paint"
    );
}

#[test]
fn beat_guides_remain_hidden_for_live_slide_preview_when_disabled() {
    let mut state = WaveformState::synthetic_for_tests();
    state.play_selection = Some(wavecrate::selection::SelectionRange::new(0.2, 0.6));
    state.play_mark_ratio = Some(0.2);
    let mut widget = waveform_widget_for_state_with_beat_guides(&state, false, 4);
    widget.active_drag_kind = Some(WaveformActiveDragKind::SelectionMove(
        WaveformSelectionKind::Play,
    ));
    widget.live_selection_preview = Some(LiveSelectionPreview {
        kind: WaveformSelectionKind::Play,
        selection: wavecrate::selection::SelectionRange::new(0.35, 0.75),
    });

    let plan = runtime_overlay_plan(&widget, Rect::from_size(200.0, 80.0));
    let fills = fill_rects(&plan);

    assert!(
        beat_guide_fills(&fills).is_empty(),
        "disabled beat guides should not paint during live slide previews"
    );
}

#[test]
fn beat_guides_paint_from_base_layer_during_live_playmark_resize() {
    let mut state = WaveformState::synthetic_for_tests();
    state.play_selection = Some(wavecrate::selection::SelectionRange::new(0.2, 0.8));
    state.play_mark_ratio = Some(0.2);
    let mut widget = waveform_widget_for_state_with_beat_guides(&state, true, 4);
    widget.active_drag_kind = Some(WaveformActiveDragKind::SelectionResize(
        WaveformSelectionKind::Play,
        WaveformSelectionEdge::End,
    ));
    widget.live_selection_preview = Some(LiveSelectionPreview {
        kind: WaveformSelectionKind::Play,
        selection: wavecrate::selection::SelectionRange::new(0.2, 0.8),
    });

    let plan = widget.paint_plan_with_defaults(Rect::from_size(200.0, 80.0));
    let fills = fill_rects(&plan);
    let guides = fills
        .iter()
        .filter(|fill| {
            (fill.color.r, fill.color.g, fill.color.b, fill.color.a) == (231, 229, 223, 150)
        })
        .collect::<Vec<_>>();

    assert_eq!(guides.len(), 3);
    for expected_x in [70.0, 100.0, 130.0] {
        assert!(
            guides.iter().any(|fill| {
                (fill.rect.center().x - expected_x).abs() < 0.01
                    && (fill.rect.min.y - 11.0).abs() < 0.01
                    && (fill.rect.max.y - 69.0).abs() < 0.01
            }),
            "expected live resize beat guide at x={expected_x}, got {guides:?}"
        );
    }
    assert!(
        fills.iter().any(|fill| {
            (fill.rect.min.x - 40.0).abs() < 0.001
                && (fill.rect.max.x - 160.0).abs() < 0.001
                && (fill.color.r, fill.color.g, fill.color.b, fill.color.a) == (241, 121, 98, 48)
        }),
        "the live resized playmark selection should still paint"
    );

    let runtime_plan = runtime_overlay_plan(&widget, Rect::from_size(200.0, 80.0));
    let runtime_fills = fill_rects(&runtime_plan);
    assert!(
        runtime_fills.iter().all(
            |fill| (fill.color.r, fill.color.g, fill.color.b, fill.color.a) != (231, 229, 223, 150)
        ),
        "runtime resize preview should not double-paint beat guides over the base layer"
    );
}

#[test]
fn beat_guides_paint_as_resize_fallback_until_preview_is_live() {
    let mut state = WaveformState::synthetic_for_tests();
    state.play_selection = Some(wavecrate::selection::SelectionRange::new(0.2, 0.6));
    state.play_mark_ratio = Some(0.2);
    let mut widget = waveform_widget_for_state_with_beat_guides(&state, true, 4);
    widget.active_drag_kind = Some(WaveformActiveDragKind::SelectionResize(
        WaveformSelectionKind::Play,
        WaveformSelectionEdge::End,
    ));

    let plan = widget.paint_plan_with_defaults(Rect::from_size(200.0, 80.0));
    let fills = fill_rects(&plan);
    let guides = fills
        .iter()
        .filter(|fill| {
            (fill.color.r, fill.color.g, fill.color.b, fill.color.a) == (231, 229, 223, 150)
        })
        .collect::<Vec<_>>();

    assert_eq!(guides.len(), 3);
    for expected_x in [60.0, 80.0, 100.0] {
        assert!(
            guides.iter().any(|fill| {
                (fill.rect.center().x - expected_x).abs() < 0.01
                    && (fill.rect.min.y - 11.0).abs() < 0.01
                    && (fill.rect.max.y - 69.0).abs() < 0.01
            }),
            "expected fallback beat guide at x={expected_x}, got {guides:?}"
        );
    }
}

#[test]
fn active_selection_drag_widget_props_keep_extracted_range_overlays_only() {
    let mut state = WaveformState::synthetic_for_tests();
    state
        .extracted_ranges
        .push(wavecrate::selection::SelectionRange::new(0.2, 0.6));
    state.start_similar_sections(wavecrate::selection::SelectionRange::new(0.1, 0.2));
    state.finish_similar_sections_scan(vec![wavecrate::selection::SelectionRange::new(0.3, 0.7)]);
    state.play_selection = Some(wavecrate::selection::SelectionRange::new(0.4, 0.8));
    state.apply_interaction(WaveformInteraction::BeginSelection {
        kind: WaveformSelectionKind::Play,
        visible_ratio: 0.4,
    });

    let props = WaveformWidgetProps::from_state(&state, false, false, 4);

    assert_eq!(props.static_range_overlay_counts(), (1, 0));
    assert_eq!(
        props.active_drag_kind,
        Some(WaveformActiveDragKind::Selection(
            WaveformSelectionKind::Play
        ))
    );
}

#[test]
fn live_selection_preview_paints_in_runtime_overlay() {
    let state = WaveformState::synthetic_for_tests();
    let mut widget = waveform_widget_for_state(&state);
    let bounds = Rect::from_size(200.0, 80.0);
    widget.active_drag_kind = Some(WaveformActiveDragKind::Selection(
        WaveformSelectionKind::Play,
    ));
    widget.live_selection_preview = Some(LiveSelectionPreview {
        kind: WaveformSelectionKind::Play,
        selection: wavecrate::selection::SelectionRange::new(0.2, 0.6),
    });

    let plan = runtime_overlay_plan(&widget, bounds);
    let fills = fill_rects(&plan);

    assert!(fills.iter().any(|fill| {
        (fill.rect.min.x - 40.0).abs() < 0.001
            && (fill.rect.max.x - 120.0).abs() < 0.001
            && (fill.color.r, fill.color.g, fill.color.b, fill.color.a) == (241, 121, 98, 48)
    }));
}

#[test]
fn sample_slide_preview_paints_thin_bottom_strip() {
    let mut state = WaveformState::synthetic_for_tests();
    state.apply_interaction(WaveformInteraction::BeginSampleSlide { visible_ratio: 0.0 });
    state.apply_interaction(WaveformInteraction::UpdateSampleSlide {
        visible_ratio: 0.25,
    });
    let widget = waveform_widget_for_state(&state);
    let bounds = Rect::from_size(200.0, 80.0);

    let plan = runtime_overlay_plan(&widget, bounds);
    let fills = fill_rects(&plan);

    assert!(fills.iter().any(|fill| {
        (fill.rect.min.y - 76.0).abs() < 0.001
            && (fill.rect.max.y - 80.0).abs() < 0.001
            && (fill.rect.min.x - 0.0).abs() < 0.001
            && (fill.rect.max.x - 200.0).abs() < 0.001
            && (fill.color.r, fill.color.g, fill.color.b, fill.color.a) == (241, 121, 98, 120)
    }));
    assert!(
        fills.iter().all(|fill| {
            (fill.color.r, fill.color.g, fill.color.b) != (241, 121, 98)
                || fill.rect.height() <= 4.0
        }),
        "sample slide preview should stay a bottom strip"
    );
}

#[test]
fn live_playmark_preview_paints_full_interactive_chrome() {
    let state = WaveformState::synthetic_for_tests();
    let mut widget = waveform_widget_for_state(&state);
    let bounds = Rect::from_size(200.0, 80.0);
    widget.active_drag_kind = Some(WaveformActiveDragKind::Selection(
        WaveformSelectionKind::Play,
    ));
    widget.live_selection_preview = Some(LiveSelectionPreview {
        kind: WaveformSelectionKind::Play,
        selection: wavecrate::selection::SelectionRange::new(0.2, 0.6),
    });

    let plan = runtime_overlay_plan(&widget, bounds);
    let fills = fill_rects(&plan);

    assert!(fills.iter().any(|fill| {
        (fill.rect.min.x - 40.0).abs() < 0.001
            && (fill.rect.max.x - 120.0).abs() < 0.001
            && (fill.color.r, fill.color.g, fill.color.b, fill.color.a) == (241, 121, 98, 48)
    }));
    assert!(
        fills.iter().any(|fill| {
            (fill.rect.min.x - 49.0).abs() < 0.001
                && (fill.rect.max.x - 111.0).abs() < 0.001
                && (fill.rect.min.y - 0.0).abs() < 0.001
                && (fill.rect.max.y - 7.0).abs() < 0.001
                && (fill.color.r, fill.color.g, fill.color.b, fill.color.a) == (241, 121, 98, 185)
        }),
        "live playmark drag should keep the move handle visible"
    );
    assert!(
        fills.iter().any(|fill| {
            (fill.rect.min.x - 104.0).abs() < 0.001
                && (fill.rect.max.x - 120.0).abs() < 0.001
                && (fill.rect.min.y - 64.0).abs() < 0.001
                && (fill.rect.max.y - 80.0).abs() < 0.001
                && (fill.color.r, fill.color.g, fill.color.b, fill.color.a) == (241, 121, 98, 235)
        }),
        "live playmark drag should keep the export handle visible"
    );
}

#[test]
fn live_editmark_preview_paints_full_interactive_chrome() {
    let state = WaveformState::synthetic_for_tests();
    let mut widget = waveform_widget_for_state(&state);
    let bounds = Rect::from_size(200.0, 80.0);
    widget.active_drag_kind = Some(WaveformActiveDragKind::Selection(
        WaveformSelectionKind::Edit,
    ));
    widget.live_selection_preview = Some(LiveSelectionPreview {
        kind: WaveformSelectionKind::Edit,
        selection: wavecrate::selection::SelectionRange::new(0.2, 0.6),
    });

    let plan = runtime_overlay_plan(&widget, bounds);
    let fills = fill_rects(&plan);

    assert!(fills.iter().any(|fill| {
        (fill.rect.min.x - 40.0).abs() < 0.001
            && (fill.rect.max.x - 120.0).abs() < 0.001
            && (fill.color.r, fill.color.g, fill.color.b, fill.color.a) == (174, 176, 173, 46)
    }));
    assert!(
        fills.iter().any(|fill| {
            (fill.rect.min.x - 49.0).abs() < 0.001
                && (fill.rect.max.x - 111.0).abs() < 0.001
                && (fill.rect.min.y - 0.0).abs() < 0.001
                && (fill.rect.max.y - 7.0).abs() < 0.001
                && (fill.color.r, fill.color.g, fill.color.b, fill.color.a) == (174, 176, 173, 180)
        }),
        "live editmark drag should keep the move handle visible"
    );
    assert!(
        fills.iter().any(|fill| {
            (fill.rect.min.x - 36.5).abs() < 0.001
                && (fill.rect.max.x - 43.5).abs() < 0.001
                && (fill.rect.min.y - 58.0).abs() < 0.001
                && (fill.rect.max.y - 80.0).abs() < 0.001
                && (fill.color.r, fill.color.g, fill.color.b, fill.color.a) == (174, 176, 173, 190)
        }),
        "live editmark drag should keep resize handles visible"
    );
    assert!(
        fills.iter().any(|fill| {
            (fill.rect.min.x - 74.0).abs() < 0.001
                && (fill.rect.max.x - 86.0).abs() < 0.001
                && (fill.rect.min.y - 0.0).abs() < 0.001
                && (fill.rect.max.y - 10.0).abs() < 0.001
                && (fill.color.r, fill.color.g, fill.color.b, fill.color.a) == (174, 176, 173, 225)
        }),
        "live editmark drag should keep the gain handle visible"
    );
}

#[test]
fn similar_sections_paint_as_green_waveform_overlays() {
    let mut state = WaveformState::synthetic_for_tests();
    state.start_similar_sections(wavecrate::selection::SelectionRange::new(0.1, 0.2));
    state.finish_similar_sections_scan(vec![wavecrate::selection::SelectionRange::new(0.2, 0.6)]);
    let widget = waveform_widget_for_state(&state);
    let plan = widget.paint_plan_with_defaults(Rect::from_size(200.0, 80.0));

    let fills = fill_rects(&plan);
    assert!(fills.iter().any(|fill| {
        (fill.rect.min.x - 40.0).abs() < 0.001
            && (fill.rect.max.x - 120.0).abs() < 0.001
            && (fill.color.r, fill.color.g, fill.color.b, fill.color.a) == (114, 235, 184, 54)
    }));
    assert!(fills.iter().any(|fill| {
        (fill.rect.min.x - 40.0).abs() < 0.001
            && (fill.rect.max.x - 120.0).abs() < 0.001
            && (fill.rect.min.y - 0.0).abs() < 0.001
            && (fill.rect.max.y - 2.0).abs() < 0.001
            && (fill.color.r, fill.color.g, fill.color.b, fill.color.a) == (155, 255, 218, 210)
    }));
    assert!(fills.iter().any(|fill| {
        (fill.rect.min.x - 40.0).abs() < 0.001
            && (fill.rect.max.x - 120.0).abs() < 0.001
            && (fill.rect.min.y - 78.0).abs() < 0.001
            && (fill.rect.max.y - 80.0).abs() < 0.001
            && (fill.color.r, fill.color.g, fill.color.b, fill.color.a) == (155, 255, 218, 210)
    }));
}

#[test]
fn similar_section_hover_paints_brighter_runtime_overlay() {
    let mut state = WaveformState::synthetic_for_tests();
    state.start_similar_sections(wavecrate::selection::SelectionRange::new(0.1, 0.2));
    state.finish_similar_sections_scan(vec![wavecrate::selection::SelectionRange::new(0.2, 0.6)]);
    let mut widget = waveform_widget_for_state(&state);
    let bounds = Rect::from_size(200.0, 80.0);

    let output = widget.handle_input(bounds, WidgetInput::pointer_move(Point::new(80.0, 40.0)));
    assert!(output.is_none());
    assert_eq!(widget.hover_cursor_ratio, None);

    let plan = runtime_overlay_plan(&widget, bounds);
    let fills = fill_rects(&plan);
    assert!(fills.iter().any(|fill| {
        (fill.rect.min.x - 40.0).abs() < 0.001
            && (fill.rect.max.x - 120.0).abs() < 0.001
            && (fill.color.r, fill.color.g, fill.color.b, fill.color.a) == (156, 255, 218, 92)
    }));
    assert!(fills.iter().any(|fill| {
        (fill.rect.min.x - 40.0).abs() < 0.001
            && (fill.rect.max.x - 120.0).abs() < 0.001
            && (fill.rect.min.y - 0.0).abs() < 0.001
            && (fill.rect.max.y - 2.0).abs() < 0.001
            && (fill.color.r, fill.color.g, fill.color.b, fill.color.a) == (219, 255, 240, 255)
    }));
    assert!(fills.iter().any(|fill| {
        (fill.rect.min.x - 40.0).abs() < 0.001
            && (fill.rect.max.x - 120.0).abs() < 0.001
            && (fill.rect.min.y - 78.0).abs() < 0.001
            && (fill.rect.max.y - 80.0).abs() < 0.001
            && (fill.color.r, fill.color.g, fill.color.b, fill.color.a) == (219, 255, 240, 255)
    }));
}

#[test]
fn edit_selection_paints_start_and_end_boundary_lines() {
    let mut state = WaveformState::synthetic_for_tests();
    state.edit_selection = Some(wavecrate::selection::SelectionRange::new(0.2, 0.6));
    let widget = waveform_widget_for_state(&state);
    let plan = widget.paint_plan_with_defaults(Rect::from_size(200.0, 80.0));

    let fills = fill_rects(&plan);
    assert!(fills.iter().any(|fill| {
        (fill.rect.center().x - 40.0).abs() < 1.0
            && (fill.color.r, fill.color.g, fill.color.b, fill.color.a) == (174, 176, 173, 230)
    }));
    assert!(fills.iter().any(|fill| {
        (fill.rect.center().x - 120.0).abs() < 1.0
            && (fill.color.r, fill.color.g, fill.color.b, fill.color.a) == (174, 176, 173, 230)
    }));
}

#[test]
fn edit_selection_flash_paints_bright_overlay() {
    let mut state = WaveformState::synthetic_for_tests();
    state.edit_selection = Some(wavecrate::selection::SelectionRange::new(0.2, 0.6));
    state.flash_edit_selection();
    let widget = waveform_widget_for_state(&state);
    let plan = widget.paint_plan_with_defaults(Rect::from_size(200.0, 80.0));

    let fills = fill_rects(&plan);
    assert!(fills.iter().any(|fill| {
        (fill.rect.min.x - 40.0).abs() < 0.001
            && (fill.rect.max.x - 120.0).abs() < 0.001
            && (fill.color.r, fill.color.g, fill.color.b, fill.color.a) == (174, 176, 173, 118)
    }));
    assert!(fills.iter().any(|fill| {
        (fill.rect.center().x - 40.0).abs() < 1.0
            && (fill.color.r, fill.color.g, fill.color.b, fill.color.a) == (174, 176, 173, 255)
    }));
    assert!(fills.iter().any(|fill| {
        (fill.rect.center().x - 120.0).abs() < 1.0
            && (fill.color.r, fill.color.g, fill.color.b, fill.color.a) == (174, 176, 173, 255)
    }));
}

#[test]
fn copied_file_flash_paints_waveform_confirmation_overlay() {
    let mut state = WaveformState::synthetic_for_tests();
    state.flash_copied_file();
    let widget = waveform_widget_for_state(&state);
    let plan = widget.paint_plan_with_defaults(Rect::from_size(200.0, 80.0));

    assert!(fill_rects(&plan).iter().any(|fill| {
        fill.rect == Rect::from_size(200.0, 80.0)
            && fill.color == radiant::gui::types::Rgba8::new(255, 174, 89, 46)
    }));

    state.apply_interaction(WaveformInteraction::Frame);
    assert!(state.copy_flash_frames() > 0);
}

#[test]
fn protected_source_error_flash_paints_waveform_error_overlay() {
    let mut state = WaveformState::synthetic_for_tests();
    state.flash_protected_source_error();
    let widget = waveform_widget_for_state(&state);
    let plan = widget.paint_plan_with_defaults(Rect::from_size(200.0, 80.0));

    assert!(fill_rects(&plan).iter().any(|fill| {
        fill.rect == Rect::from_size(200.0, 80.0)
            && fill.color == radiant::gui::types::Rgba8::new(255, 69, 54, 62)
    }));

    state.apply_interaction(WaveformInteraction::Frame);
    assert!(state.protected_source_error_flash_frames() > 0);
}

#[test]
fn edit_selection_denied_flash_paints_red_twice() {
    let mut state = WaveformState::synthetic_for_tests();
    state.edit_selection = Some(wavecrate::selection::SelectionRange::new(0.2, 0.6));
    state.flash_denied_selection(WaveformSelectionKind::Edit);

    let initial_plan =
        waveform_widget_for_state(&state).paint_plan_with_defaults(Rect::from_size(200.0, 80.0));
    assert!(contains_denied_selection_fill(&initial_plan));

    for _ in 0..6 {
        state.apply_interaction(WaveformInteraction::Frame);
    }
    let between_plan =
        waveform_widget_for_state(&state).paint_plan_with_defaults(Rect::from_size(200.0, 80.0));
    assert!(!contains_denied_selection_fill(&between_plan));

    for _ in 0..6 {
        state.apply_interaction(WaveformInteraction::Frame);
    }
    let second_plan =
        waveform_widget_for_state(&state).paint_plan_with_defaults(Rect::from_size(200.0, 80.0));
    assert!(contains_denied_selection_fill(&second_plan));
}

#[test]
fn edit_fade_curve_paints_s_curve_shape_as_polyline() {
    let mut state = WaveformState::synthetic_for_tests();
    let selection = wavecrate::selection::SelectionRange::new(0.2, 0.6)
        .with_gain(0.5)
        .with_fade_in(0.5, 0.8)
        .with_fade_out(0.25, 0.0);
    state.edit_selection = Some(selection);
    let widget = waveform_widget_for_state(&state);
    let plan = widget.paint_plan_with_defaults(Rect::from_size(200.0, 80.0));

    let curves = stroke_polylines(&plan)
        .into_iter()
        .filter(|stroke| {
            (
                stroke.color.r,
                stroke.color.g,
                stroke.color.b,
                stroke.color.a,
            ) == (174, 176, 173, 225)
                && stroke.width == 2.0
                && stroke.points.len() >= 10
        })
        .collect::<Vec<_>>();
    assert_eq!(
        curves.len(),
        2,
        "expected leading and trailing fade curve strokes"
    );

    let leading = curves
        .iter()
        .find(|stroke| {
            let points = stroke.points.as_ref();
            (points[0].x - 40.0).abs() < 0.001
                && (points.last().expect("last leading fade point").x - 80.0).abs() < 0.001
        })
        .expect("leading fade curve stroke");
    let leading_points = leading.points.as_ref();
    let leading_bend = leading_points[2];
    let leading_expected_y = 80.0 - 80.0 * wavecrate::selection::fade_curve_value(0.2, 0.8);
    assert!((leading_points[0].y - 80.0).abs() < 0.001);
    assert!((leading_points.last().expect("last leading fade point").y - 0.0).abs() < 0.001);
    assert!((leading_bend.x - 48.0).abs() < 0.001);
    assert!((leading_bend.y - leading_expected_y).abs() < 0.001);
    assert!(
        leading_bend.y > 70.0,
        "high-curve fade-in should paint the eased bend, not a straight ramp"
    );

    let trailing = curves
        .iter()
        .find(|stroke| {
            let points = stroke.points.as_ref();
            (points[0].x - 100.0).abs() < 0.001
                && (points.last().expect("last trailing fade point").x - 120.0).abs() < 0.001
        })
        .expect("trailing fade curve stroke");
    let trailing_points = trailing.points.as_ref();
    let trailing_mid = trailing_points[5];
    let trailing_expected_y =
        80.0 - 80.0 * (1.0 - wavecrate::selection::fade_curve_value(0.5, 0.0));
    assert!((trailing_points[0].y - 0.0).abs() < 0.001);
    assert!((trailing_points.last().expect("last trailing fade point").y - 80.0).abs() < 0.001);
    assert!((trailing_mid.x - 110.0).abs() < 0.001);
    assert!((trailing_mid.y - trailing_expected_y).abs() < 0.001);
}

fn contains_denied_selection_fill(plan: &SurfacePaintPlan) -> bool {
    fill_rects(plan).iter().any(|fill| {
        (fill.rect.min.x - 40.0).abs() < 0.001
            && (fill.rect.max.x - 120.0).abs() < 0.001
            && (fill.color.r, fill.color.g, fill.color.b, fill.color.a) == (239, 76, 61, 130)
    })
}

#[test]
fn edit_fade_curve_stays_inside_selection_when_outer_mute_extends_handles() {
    let mut state = WaveformState::synthetic_for_tests();
    let selection = wavecrate::selection::SelectionRange::new(0.2, 0.6)
        .with_fade_in(0.25, 0.2)
        .with_fade_in_mute(0.25)
        .with_fade_out(0.25, 0.7)
        .with_fade_out_mute(0.25);
    state.edit_selection = Some(selection);
    let widget = waveform_widget_for_state(&state);
    let plan = widget.paint_plan_with_defaults(Rect::from_size(200.0, 80.0));

    let curves = stroke_polylines(&plan)
        .into_iter()
        .filter(|stroke| {
            (
                stroke.color.r,
                stroke.color.g,
                stroke.color.b,
                stroke.color.a,
            ) == (174, 176, 173, 225)
                && stroke.width == 2.0
                && stroke.points.len() >= 10
        })
        .collect::<Vec<_>>();
    assert_eq!(curves.len(), 2);

    assert!(curves.iter().any(|stroke| {
        let points = stroke.points.as_ref();
        (points[0].x - 40.0).abs() < 0.001
            && (points.last().expect("last leading fade point").x - 60.0).abs() < 0.001
    }));
    assert!(curves.iter().any(|stroke| {
        let points = stroke.points.as_ref();
        (points[0].x - 100.0).abs() < 0.001
            && (points.last().expect("last trailing fade point").x - 120.0).abs() < 0.001
    }));
}
