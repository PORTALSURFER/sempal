use super::*;

fn paint_transient_overlay_at<Bridge>(
    runtime: &mut SurfaceRuntime<Bridge, crate::native_app::test_support::state::GuiMessage>,
    animation_time: std::time::Duration,
) -> Vec<radiant::runtime::PaintPrimitive>
where
    Bridge: RuntimeBridge<crate::native_app::test_support::state::GuiMessage>,
{
    let frame = runtime.frame_with_default_theme();
    let mut primitives = Vec::new();
    runtime.host_paint_transient_overlay(
        TransientOverlayContext::new(
            &frame.paint_plan,
            Vector2::new(900.0, 900.0),
            animation_time,
        ),
        &mut primitives,
    );
    primitives
}

#[test]
fn app_bridge_idle_vertical_scroll_starts_zero_entry_fade_on_paint_only_cadence() {
    let root = temp_gui_root("wavecrate-app-bridge-overflow-fade-root");
    fs::create_dir_all(&root).expect("create source root");
    for index in 0..100 {
        let folder = root.join(format!("folder_{index:02}"));
        fs::create_dir_all(&folder).expect("create folder");
        fs::write(folder.join("sample.wav"), []).expect("write sample");
    }
    let mut state = crate::native_app::test_support::state::NativeAppStateFixture::default()
        .with_synthetic_waveform()
        .with_sample_status("")
        .with_folder_browser(
            crate::native_app::test_support::state::FolderBrowserState::from_root(root.clone()),
        )
        .build();
    crate::native_app::test_support::sample_browser::prepare_sample_browser_view(&mut state);
    let bridge = radiant::app(state)
        .view(crate::native_app::test_support::state::view)
        .handle_message(|state, message, context| state.handle_message(message, context))
        .into_bridge();
    let mut runtime = SurfaceRuntime::new(bridge, Vector2::new(900.0, 900.0));
    apply_strict_update_diagnostics(&mut runtime);
    let baseline_activity = runtime.host_animation_activity();
    let tree_rect = runtime
        .layout()
        .rects
        .get(&crate::native_app::ui::ids::FOLDER_TREE_LIST_ID)
        .copied()
        .expect("folder tree list should be laid out");

    assert!(runtime.wheel_or_scroll_at(tree_rect.center(), Vector2::new(0.0, 110.0)));
    let zero_entry = paint_transient_overlay_at(&mut runtime, std::time::Duration::ZERO);
    assert!(
        !zero_entry.iter().any(|primitive| {
            matches!(primitive, radiant::runtime::PaintPrimitive::FillPath(_))
        })
    );
    assert_eq!(
        runtime.host_animation_activity().needs_frame_message(),
        baseline_activity.needs_frame_message(),
        "overflow fades must not add a frame-message cadence"
    );

    let advancing = paint_transient_overlay_at(&mut runtime, std::time::Duration::from_millis(130));
    assert!(
        advancing.iter().any(|primitive| {
            matches!(primitive, radiant::runtime::PaintPrimitive::FillPath(_))
        })
    );
    assert_eq!(
        runtime.host_animation_activity().needs_frame_message(),
        baseline_activity.needs_frame_message(),
        "advancing overflow fades must remain paint-only"
    );

    let _settled = paint_transient_overlay_at(&mut runtime, std::time::Duration::from_millis(260));
    assert_eq!(
        runtime.host_animation_activity().needs_frame_message(),
        baseline_activity.needs_frame_message(),
        "settled overflow fades must not leave a frame-message cadence"
    );
    let _ = fs::remove_dir_all(root);
}

#[test]
fn app_bridge_idle_waveform_zoom_starts_zero_entry_fade_on_paint_only_cadence() {
    let mut state = gui_state_for_span_tests();
    crate::native_app::test_support::sample_browser::prepare_sample_browser_view(&mut state);
    let bridge = radiant::app(state)
        .view(crate::native_app::test_support::state::view)
        .handle_message(|state, message, context| state.handle_message(message, context))
        .into_bridge();
    let mut runtime = SurfaceRuntime::new(bridge, Vector2::new(900.0, 900.0));
    apply_strict_update_diagnostics(&mut runtime);
    let baseline_activity = runtime.host_animation_activity();
    let waveform_rect = *runtime
        .layout()
        .rects
        .get(&crate::native_app::test_support::waveform::WAVEFORM_WIDGET_ID)
        .expect("waveform widget should be laid out");

    assert!(runtime.wheel_or_scroll_at(waveform_rect.center(), Vector2::new(0.0, -40.0)));
    let zero_entry = paint_transient_overlay_at(&mut runtime, std::time::Duration::ZERO);
    assert!(
        !zero_entry.iter().any(|primitive| {
            matches!(primitive, radiant::runtime::PaintPrimitive::FillPath(_))
        })
    );
    assert_eq!(
        runtime.host_animation_activity().needs_frame_message(),
        baseline_activity.needs_frame_message(),
        "waveform fades must not add a frame-message cadence"
    );
    let advancing = paint_transient_overlay_at(&mut runtime, std::time::Duration::from_millis(130));
    assert!(
        advancing.iter().any(|primitive| {
            matches!(primitive, radiant::runtime::PaintPrimitive::FillPath(_))
        })
    );
    assert_eq!(
        runtime.host_animation_activity().needs_frame_message(),
        baseline_activity.needs_frame_message(),
        "advancing waveform fades must remain paint-only"
    );

    let _settled = paint_transient_overlay_at(&mut runtime, std::time::Duration::from_millis(260));
    assert_eq!(
        runtime.host_animation_activity().needs_frame_message(),
        baseline_activity.needs_frame_message(),
        "settled waveform fades must not leave a frame-message cadence"
    );
}

#[test]
fn app_bridge_scene_routes_primary_waveform_selection_drag() {
    let state = gui_state_for_span_tests();
    let messages = Rc::new(RefCell::new(Vec::new()));
    let captured_messages = Rc::clone(&messages);
    let bridge = radiant::app(state)
        .view(crate::native_app::test_support::state::view)
        .handle_message(move |state, message, context| {
            captured_messages.borrow_mut().push(message.clone());
            state.handle_message(message, context);
        })
        .into_bridge();
    let mut runtime = SurfaceRuntime::new(bridge, Vector2::new(900.0, 620.0));
    apply_strict_update_diagnostics(&mut runtime);
    let rect = *runtime
        .layout()
        .rects
        .get(&crate::native_app::test_support::waveform::WAVEFORM_WIDGET_ID)
        .expect("app bridge should lay out waveform widget");
    let press = Point::new(rect.min.x + rect.width() * 0.25, rect.center().y);
    let drag = Point::new(rect.min.x + rect.width() * 0.75, rect.center().y);

    assert_eq!(
        runtime.widget_at(press),
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

    let messages = messages.borrow();
    assert!(
        messages.iter().any(|message| matches!(
            message,
            crate::native_app::test_support::state::GuiMessage::Waveform(
                WaveformInteraction::BeginSelection {
                    kind: WaveformSelectionKind::Play,
                    ..
                }
            )
        )),
        "{messages:?}"
    );
    assert!(
        messages.iter().any(|message| matches!(
            message,
            crate::native_app::test_support::state::GuiMessage::Waveform(
                WaveformInteraction::FinishSelection { .. }
            )
        )),
        "{messages:?}"
    );
}

#[test]
fn app_bridge_scene_routes_native_file_drop_to_waveform_view() {
    let root = temp_gui_root("wavecrate-app-bridge-native-drop-root");
    let external_root = temp_gui_root("wavecrate-app-bridge-native-drop-external");
    let loops = root.join("loops");
    fs::create_dir_all(&loops).expect("create loops");
    let source = external_root.join("kick.wav");
    write_test_wav_i16(&source, &[0, 100, -100]);
    let mut state = gui_state_for_span_tests();
    state.library.folder_browser =
        crate::native_app::test_support::state::FolderBrowserState::from_sample_sources(&[
            wavecrate::sample_sources::SampleSource::new(root.clone()),
        ]);
    state.library.folder_browser.apply_message(
        crate::native_app::test_support::state::FolderBrowserMessage::ActivateFolder(
            loops.display().to_string(),
            Default::default(),
        ),
    );
    let messages = Rc::new(RefCell::new(Vec::new()));
    let captured_messages = Rc::clone(&messages);
    let waveform_loading_label = Rc::new(RefCell::new(None));
    let captured_waveform_loading_label = Rc::clone(&waveform_loading_label);
    let bridge = radiant::app(state)
        .view(crate::native_app::test_support::state::view)
        .handle_message(move |state, message, context| {
            captured_messages.borrow_mut().push(message.clone());
            state.handle_message(message, context);
            *captured_waveform_loading_label.borrow_mut() = state.waveform.load.label.clone();
        })
        .into_bridge();
    let mut runtime = SurfaceRuntime::new(bridge, Vector2::new(900.0, 620.0));
    apply_strict_update_diagnostics(&mut runtime);
    let rect = *runtime
        .layout()
        .rects
        .get(&crate::native_app::test_support::waveform::WAVEFORM_WIDGET_ID)
        .expect("app bridge should lay out waveform widget");

    runtime.dispatch_native_file_drop(NativeFileDrop::dropped(source, Some(rect.center()), None));

    let copied = loops.join("kick.wav");
    for _ in 0..200 {
        runtime.drain_runtime_messages();
        if copied.is_file() && waveform_loading_label.borrow().as_deref() == Some("kick.wav") {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    assert!(copied.is_file());
    assert_eq!(waveform_loading_label.borrow().as_deref(), Some("kick.wav"));
    let messages = messages.borrow();
    assert!(
        messages.iter().any(|message| matches!(
            message,
            crate::native_app::test_support::state::GuiMessage::WaveformFileDrop(_)
        )),
        "{messages:?}"
    );
    let _ = fs::remove_dir_all(root);
    let _ = fs::remove_dir_all(external_root);
}

#[test]
fn app_bridge_scene_routes_targetless_native_file_drop_to_single_waveform_target() {
    let root = temp_gui_root("wavecrate-app-bridge-targetless-native-drop-root");
    let external_root = temp_gui_root("wavecrate-app-bridge-targetless-native-drop-external");
    let loops = root.join("loops");
    fs::create_dir_all(&loops).expect("create loops");
    let source = external_root.join("kick.wav");
    write_test_wav_i16(&source, &[0, 100, -100]);
    let mut state = gui_state_for_span_tests();
    state.library.folder_browser =
        crate::native_app::test_support::state::FolderBrowserState::from_sample_sources(&[
            wavecrate::sample_sources::SampleSource::new(root.clone()),
        ]);
    state.library.folder_browser.apply_message(
        crate::native_app::test_support::state::FolderBrowserMessage::ActivateFolder(
            loops.display().to_string(),
            Default::default(),
        ),
    );
    let waveform_loading_label = Rc::new(RefCell::new(None));
    let captured_waveform_loading_label = Rc::clone(&waveform_loading_label);
    let bridge = radiant::app(state)
        .view(crate::native_app::test_support::state::view)
        .handle_message(move |state, message, context| {
            state.handle_message(message, context);
            *captured_waveform_loading_label.borrow_mut() = state.waveform.load.label.clone();
        })
        .into_bridge();
    let mut runtime = SurfaceRuntime::new(bridge, Vector2::new(900.0, 620.0));
    apply_strict_update_diagnostics(&mut runtime);

    runtime.dispatch_native_file_drop(NativeFileDrop::dropped(source, None, None));

    let copied = loops.join("kick.wav");
    for _ in 0..200 {
        runtime.drain_runtime_messages();
        if copied.is_file() && waveform_loading_label.borrow().as_deref() == Some("kick.wav") {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    assert!(copied.is_file());
    assert_eq!(waveform_loading_label.borrow().as_deref(), Some("kick.wav"));
    let _ = fs::remove_dir_all(root);
    let _ = fs::remove_dir_all(external_root);
}

#[test]
fn app_bridge_scene_preserves_waveform_drag_during_playback_frame_refresh() {
    let mut state = gui_state_for_span_tests();
    state.waveform.current.start_playback(0.25);
    let messages = Rc::new(RefCell::new(Vec::new()));
    let captured_messages = Rc::clone(&messages);
    let bridge = radiant::app(state)
        .view(crate::native_app::test_support::state::view)
        .handle_message(move |state, message, context| {
            captured_messages.borrow_mut().push(message.clone());
            state.handle_message(message, context);
        })
        .into_bridge();
    let mut runtime = SurfaceRuntime::new(bridge, Vector2::new(900.0, 620.0));
    apply_strict_update_diagnostics(&mut runtime);
    let rect = *runtime
        .layout()
        .rects
        .get(&crate::native_app::test_support::waveform::WAVEFORM_WIDGET_ID)
        .expect("app bridge should lay out waveform widget");
    let press = Point::new(rect.min.x + rect.width() * 0.25, rect.center().y);
    let drag = Point::new(rect.min.x + rect.width() * 0.75, rect.center().y);

    assert_eq!(
        runtime.dispatch_event(Event::primary_press(press)),
        Some(crate::native_app::test_support::waveform::WAVEFORM_WIDGET_ID)
    );
    assert!(runtime.host_animation_activity().needs_frame_message());
    assert!(runtime.host_queue_animation_frame());
    runtime.drain_runtime_messages();
    assert_eq!(
        runtime.dispatch_event(Event::pointer_move(drag)),
        Some(crate::native_app::test_support::waveform::WAVEFORM_WIDGET_ID)
    );
    assert_eq!(
        runtime.dispatch_event(Event::primary_release(drag)),
        Some(crate::native_app::test_support::waveform::WAVEFORM_WIDGET_ID)
    );

    let messages = messages.borrow();
    assert!(
        messages.iter().any(|message| matches!(
            message,
            crate::native_app::test_support::state::GuiMessage::Waveform(
                WaveformInteraction::FinishSelection { .. }
            )
        )),
        "{messages:?}"
    );
}
