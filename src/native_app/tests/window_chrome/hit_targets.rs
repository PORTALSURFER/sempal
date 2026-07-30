use super::*;

#[test]
fn full_app_scene_routes_waveform_hit_target() {
    let state = gui_state_for_span_tests();
    let runtime = native_runtime_for_tests(state, Vector2::new(900.0, 620.0));
    let point = waveform_rect(&runtime).center();

    assert_eq!(
        runtime.widget_at(point),
        Some(crate::native_app::test_support::waveform::WAVEFORM_WIDGET_ID)
    );
}

#[test]
fn absent_playmark_layers_leave_all_waveform_pointer_gestures_available() {
    let state = gui_state_for_span_tests();
    let mut runtime = native_runtime_for_tests(state, Vector2::new(900.0, 620.0));
    let rect = waveform_rect(&runtime);
    let press = Point::new(rect.min.x + rect.width() * 0.2, rect.center().y);
    let drag = Point::new(rect.min.x + rect.width() * 0.4, rect.center().y);

    assert_eq!(
        runtime.dispatch_event(Event::pointer_move(press)),
        Some(crate::native_app::test_support::waveform::WAVEFORM_WIDGET_ID)
    );
    assert_eq!(
        runtime.hovered_widget(),
        Some(crate::native_app::test_support::waveform::WAVEFORM_WIDGET_ID)
    );
    assert_eq!(
        runtime.dispatch_event(Event::primary_press(press)),
        Some(crate::native_app::test_support::waveform::WAVEFORM_WIDGET_ID)
    );
    assert_eq!(
        runtime.dispatch_event(Event::pointer_move(drag)),
        Some(crate::native_app::test_support::waveform::WAVEFORM_WIDGET_ID)
    );
    assert_eq!(
        runtime.dispatch_event(Event::primary_release(drag)),
        Some(crate::native_app::test_support::waveform::WAVEFORM_WIDGET_ID)
    );
    assert_eq!(
        runtime.bridge().state().waveform.current.play_selection(),
        Some(wavecrate::selection::SelectionRange::new(0.2, 0.4))
    );

    let state = gui_state_for_span_tests();
    let mut runtime = native_runtime_for_tests(state, Vector2::new(900.0, 620.0));
    assert_eq!(
        runtime.dispatch_event(Event::secondary_press(press)),
        Some(crate::native_app::test_support::waveform::WAVEFORM_WIDGET_ID)
    );
    assert_eq!(
        runtime.dispatch_event(Event::pointer_move(drag)),
        Some(crate::native_app::test_support::waveform::WAVEFORM_WIDGET_ID)
    );
    assert_eq!(
        runtime.dispatch_event(Event::secondary_release(drag)),
        Some(crate::native_app::test_support::waveform::WAVEFORM_WIDGET_ID)
    );
    assert_eq!(
        runtime.bridge().state().waveform.current.edit_selection(),
        Some(wavecrate::selection::SelectionRange::new(0.2, 0.4))
    );
}

#[test]
fn playmark_local_controls_leave_waveform_input_outside_their_painted_bounds() {
    let mut state = gui_state_for_span_tests();
    state.waveform.current.set_play_selection_range(0.25, 0.75);
    let mut runtime = native_runtime_for_tests(state, Vector2::new(900.0, 620.0));
    let theme = radiant::theme::ThemeTokens::default();
    let _ = runtime.frame(&theme);
    let rect = waveform_rect(&runtime);
    let point = Point::new(rect.min.x + rect.width() * 0.1, rect.center().y);

    assert_eq!(
        runtime.dispatch_event(Event::pointer_move(point)),
        Some(crate::native_app::test_support::waveform::WAVEFORM_WIDGET_ID)
    );
    assert_eq!(
        runtime.hovered_widget(),
        Some(crate::native_app::test_support::waveform::WAVEFORM_WIDGET_ID)
    );
    assert_eq!(
        runtime.dispatch_event(Event::primary_press(point)),
        Some(crate::native_app::test_support::waveform::WAVEFORM_WIDGET_ID)
    );
    assert_eq!(
        runtime.dispatch_event(Event::primary_release(point)),
        Some(crate::native_app::test_support::waveform::WAVEFORM_WIDGET_ID)
    );
    assert!(
        runtime
            .bridge()
            .state()
            .waveform
            .current
            .play_mark_ratio()
            .is_some_and(|ratio| (ratio - 0.1).abs() <= 0.000_001)
    );
}

#[test]
fn playmark_time_label_claims_edit_click_but_leaves_hover_and_secondary_drag_to_waveform() {
    let mut state = gui_state_for_span_tests();
    state.waveform.current.set_play_selection_range(0.25, 0.75);
    let mut runtime = native_runtime_for_tests(state, Vector2::new(900.0, 620.0));
    let theme = radiant::theme::ThemeTokens::default();
    let initial_frame = runtime.frame(&theme);
    let label = initial_frame
        .paint_plan
        .first_text_run("500 ms")
        .expect("playmark time label paint");
    assert_eq!(
        label.widget_id,
        crate::native_app::ui::ids::WAVEFORM_PLAYMARK_LABEL_ID,
        "the interactive label layer must own steady label paint"
    );
    let label_point = label.rect.center();

    assert_eq!(
        runtime.dispatch_event(Event::pointer_move(label_point)),
        Some(crate::native_app::test_support::waveform::WAVEFORM_WIDGET_ID),
        "the time label must leave waveform hover and transient overlays active"
    );
    assert_eq!(
        runtime.dispatch_event(Event::primary_press(label_point)),
        Some(crate::native_app::ui::ids::WAVEFORM_PLAYMARK_LABEL_ID),
        "a normal click must start editing the time label"
    );
    assert!(
        runtime
            .bridge()
            .state()
            .waveform
            .current
            .playmark_label_editor_active()
    );
    assert_eq!(
        runtime.dispatch_event(Event::primary_release(label_point)),
        Some(crate::native_app::ui::ids::WAVEFORM_PLAYMARK_LABEL_ID)
    );
    assert_eq!(
        runtime.bridge().state().waveform.current.play_selection(),
        Some(wavecrate::selection::SelectionRange::new(0.25, 0.75))
    );
    let editing_frame = runtime.frame(&theme);
    assert_eq!(
        editing_frame
            .paint_plan
            .text_runs()
            .filter(|text| text.text.as_str() == "500 ms")
            .count(),
        0,
        "the base playmark label must stop painting while its editor is active"
    );
    assert_eq!(
        editing_frame
            .paint_plan
            .text_inputs()
            .filter(|input| {
                input.widget_id == crate::native_app::ui::ids::WAVEFORM_PLAYMARK_LABEL_ID
            })
            .count(),
        1,
        "edit mode must paint exactly one playmark text input"
    );

    let mut state = gui_state_for_span_tests();
    state.waveform.current.set_play_selection_range(0.25, 0.75);
    let mut runtime = native_runtime_for_tests(state, Vector2::new(900.0, 620.0));
    let frame = runtime.frame(&theme);
    let label_point = frame
        .paint_plan
        .first_text_run("500 ms")
        .map(|text| text.rect.center())
        .expect("playmark time label paint");
    let drag_point = Point::new(waveform_rect(&runtime).max.x - 20.0, label_point.y);
    assert_eq!(
        runtime.dispatch_event(Event::secondary_press(label_point)),
        Some(crate::native_app::test_support::waveform::WAVEFORM_WIDGET_ID),
        "an edit-selection drag may start on the time label"
    );
    assert_eq!(
        runtime.dispatch_event(Event::pointer_move(drag_point)),
        Some(crate::native_app::test_support::waveform::WAVEFORM_WIDGET_ID)
    );
    assert_eq!(
        runtime.dispatch_event(Event::secondary_release(drag_point)),
        Some(crate::native_app::test_support::waveform::WAVEFORM_WIDGET_ID)
    );
    assert!(
        runtime
            .bridge()
            .state()
            .waveform
            .current
            .edit_selection()
            .is_some()
    );
}

#[test]
fn stale_waveform_loading_label_does_not_mask_waveform_hit_target() {
    let mut state = gui_state_for_span_tests();
    state.waveform.load.label = Some(String::from("previous.wav"));
    state.waveform.load.progress = 0.5;
    let mut runtime = native_runtime_for_tests(state, Vector2::new(900.0, 620.0));
    let rect = waveform_rect(&runtime);
    let point = Point::new(rect.min.x + rect.width() * 0.42, rect.center().y);

    assert_eq!(
        runtime.widget_at(point),
        Some(crate::native_app::test_support::waveform::WAVEFORM_WIDGET_ID)
    );
    assert_eq!(
        runtime.dispatch_event(Event::primary_press(point)),
        Some(crate::native_app::test_support::waveform::WAVEFORM_WIDGET_ID)
    );
    assert_eq!(
        runtime.dispatch_event(Event::primary_release(point)),
        Some(crate::native_app::test_support::waveform::WAVEFORM_WIDGET_ID)
    );

    assert!(
        runtime
            .bridge()
            .state()
            .waveform
            .current
            .play_mark_ratio()
            .is_some_and(|ratio| (ratio - 0.42).abs() <= 0.000_001)
    );
}

#[test]
fn stale_waveform_drop_hover_does_not_mask_waveform_hit_target() {
    let mut state = gui_state_for_span_tests();
    state.ui.browser_interaction.native_file_drop_hover = Some(
        crate::native_app::test_support::state::NativeFileDropHover {
            path: PathBuf::from("stale.wav"),
            supported: true,
        },
    );
    let mut runtime = native_runtime_for_tests(state, Vector2::new(900.0, 620.0));
    let rect = waveform_rect(&runtime);
    let point = Point::new(rect.min.x + rect.width() * 0.38, rect.center().y);

    assert_eq!(
        runtime.widget_at(point),
        Some(crate::native_app::test_support::waveform::WAVEFORM_WIDGET_ID)
    );
    assert_eq!(
        runtime.dispatch_event(Event::secondary_press(point)),
        Some(crate::native_app::test_support::waveform::WAVEFORM_WIDGET_ID)
    );
    assert_eq!(
        runtime.dispatch_event(Event::secondary_release(point)),
        Some(crate::native_app::test_support::waveform::WAVEFORM_WIDGET_ID)
    );

    assert_eq!(
        runtime.bridge().state().waveform.current.edit_mark_ratio(),
        None
    );
}

#[test]
fn active_waveform_sample_load_masks_waveform_hit_target() {
    let (mut state, _source_root, selected_file) =
        native_app_state_with_temp_sample("blocking-load.wav");
    write_test_wav_i16(std::path::Path::new(&selected_file), &[0, 256, -256, 512]);
    let mut context = ui::UiUpdateContext::default();
    state.apply_message(
        crate::native_app::test_support::state::GuiMessage::SelectSampleWithModifiers {
            path: selected_file,
            modifiers: Default::default(),
        },
        &mut context,
    );
    run_command_for_tests(&mut state, context.into_command());
    assert!(state.waveform_input_blocked_by_sample_load());
    let mut runtime = native_runtime_for_tests(state, Vector2::new(900.0, 620.0));
    let rect = waveform_rect(&runtime);
    let point = Point::new(rect.min.x + rect.width() * 0.42, rect.center().y);

    assert_ne!(
        runtime.dispatch_event(Event::primary_press(point)),
        Some(crate::native_app::test_support::waveform::WAVEFORM_WIDGET_ID)
    );
    assert_eq!(
        runtime.bridge().state().waveform.current.play_mark_ratio(),
        None
    );
}
