use super::*;

#[test]
fn toolbar_icon_assets_parse_and_paint_through_radiant_icon_button() {
    for icon in [
        crate::native_app::test_support::toolbar::ToolbarIcon::FocusLoaded,
        crate::native_app::test_support::toolbar::ToolbarIcon::Loop,
        crate::native_app::test_support::toolbar::ToolbarIcon::Random,
        crate::native_app::test_support::toolbar::ToolbarIcon::SimilarSections,
        crate::native_app::test_support::toolbar::ToolbarIcon::ZeroCrossingSnap,
        crate::native_app::test_support::toolbar::ToolbarIcon::BpmSnap,
        crate::native_app::test_support::toolbar::ToolbarIcon::BeatGuides,
        crate::native_app::test_support::toolbar::ToolbarIcon::Metronome,
        crate::native_app::test_support::toolbar::ToolbarIcon::Play,
        crate::native_app::test_support::toolbar::ToolbarIcon::Stop,
    ] {
        assert_eq!(
            crate::native_app::test_support::toolbar::toolbar_icon_color(true, false),
            radiant::prelude::Rgba8::new(216, 215, 211, 255)
        );
        assert_eq!(
            crate::native_app::test_support::toolbar::toolbar_icon_color(true, true),
            radiant::prelude::Rgba8::new(233, 88, 67, 255)
        );
        assert_eq!(
            crate::native_app::test_support::toolbar::toolbar_icon_color(false, false),
            radiant::prelude::Rgba8::new(153, 155, 154, 255)
        );
        let mut primitives = Vec::new();
        crate::native_app::test_support::toolbar::toolbar_icon_glyph(icon, true, false)
            .append_paint(&mut primitives, 101, Rect::from_size(28.0, 24.0));
        assert!(
            primitives.iter().any(|primitive| primitive.svg().is_some()),
            "toolbar icon cache should produce a retained Radiant SVG"
        );
        let frame =
            crate::native_app::test_support::toolbar::toolbar_icon_button(101, icon, true, false)
                .view_frame_at_size_with_default_theme(Vector2::new(28.0, 24.0));
        assert!(
            frame.paint_plan.svgs().next().is_some(),
            "toolbar icon should paint as a retained Radiant SVG"
        );
    }
}

#[test]
fn toolbar_icon_button_routes_messages_through_radiant_builder() {
    for (icon, message) in [
        (
            crate::native_app::test_support::toolbar::ToolbarIcon::FocusLoaded,
            crate::native_app::test_support::state::GuiMessage::FocusLoadedFile,
        ),
        (
            crate::native_app::test_support::toolbar::ToolbarIcon::Loop,
            crate::native_app::test_support::state::GuiMessage::ToggleLoopPlayback,
        ),
        (
            crate::native_app::test_support::toolbar::ToolbarIcon::Random,
            crate::native_app::test_support::state::GuiMessage::PlayRandomSampleRange,
        ),
        (
            crate::native_app::test_support::toolbar::ToolbarIcon::SimilarSections,
            crate::native_app::test_support::state::GuiMessage::ToggleSimilarSections,
        ),
        (
            crate::native_app::test_support::toolbar::ToolbarIcon::ZeroCrossingSnap,
            crate::native_app::test_support::state::GuiMessage::ToggleZeroCrossingSnap,
        ),
        (
            crate::native_app::test_support::toolbar::ToolbarIcon::BpmSnap,
            crate::native_app::test_support::state::GuiMessage::ToggleBpmSnap,
        ),
        (
            crate::native_app::test_support::toolbar::ToolbarIcon::BeatGuides,
            crate::native_app::test_support::state::GuiMessage::ToggleBeatGuides,
        ),
        (
            crate::native_app::test_support::toolbar::ToolbarIcon::Metronome,
            crate::native_app::test_support::state::GuiMessage::ToggleMetronome,
        ),
    ] {
        assert_eq!(
            crate::native_app::test_support::toolbar::toolbar_icon_button(101, icon, true, false)
                .view_dispatch_widget_output(
                    101,
                    radiant::widgets::WidgetOutput::typed(
                        radiant::widgets::ButtonMessage::Activate {
                            provenance: radiant::widgets::InteractionProvenance::Programmatic,
                        }
                    ),
                ),
            Some(message)
        );
    }

    assert_eq!(
        crate::native_app::test_support::toolbar::toolbar_icon_button(
            101,
            crate::native_app::test_support::toolbar::ToolbarIcon::Random,
            true,
            false,
        )
        .view_dispatch_widget_output(
            101,
            radiant::widgets::WidgetOutput::typed(
                radiant::widgets::ButtonMessage::ActivateWithModifiers {
                    provenance: radiant::widgets::InteractionProvenance::Pointer {
                        modifiers: PointerModifiers {
                            command: true,
                            ..Default::default()
                        },
                        timestamp: None,
                        sequence_range: None,
                    },
                },
            ),
        ),
        Some(
            crate::native_app::test_support::state::GuiMessage::ToggleStickyRandomSampleRangePlayback
        )
    );

    assert_eq!(
        crate::native_app::test_support::toolbar::toolbar_icon_button(
            101,
            crate::native_app::test_support::toolbar::ToolbarIcon::Random,
            true,
            false,
        )
        .view_dispatch_widget_output(
            101,
            radiant::widgets::WidgetOutput::typed(
                radiant::widgets::ButtonMessage::ActivateWithModifiers {
                    provenance: radiant::widgets::InteractionProvenance::Pointer {
                        modifiers: PointerModifiers {
                            shift: true,
                            ..Default::default()
                        },
                        timestamp: None,
                        sequence_range: None,
                    },
                },
            ),
        ),
        Some(crate::native_app::test_support::state::GuiMessage::PlayRandomListedSampleRange)
    );
}

#[test]
fn main_toolbar_does_not_paint_empty_spacer_border() {
    let state = NativeAppState::load_default().expect("default state loads");
    let frame = crate::native_app::test_support::toolbar::main_toolbar(&state)
        .view_frame_at_size_with_default_theme(Vector2::new(664.0, 34.0));

    assert!(
        !frame
            .paint_plan
            .contains_paint_rect_matching(|rect| rect.width() > 100.0 && rect.height() >= 20.0),
        "empty toolbar spacer should not paint or reserve a large visible rectangle"
    );
}

#[test]
fn transport_controls_form_a_tight_cluster_separated_from_utilities() {
    let state = NativeAppState::load_default().expect("default state loads");
    let frame = crate::native_app::test_support::toolbar::main_toolbar(&state)
        .view_frame_at_size_with_default_theme(Vector2::new(664.0, 34.0));
    let metronome = frame
        .paint_plan
        .first_widget_rect(crate::native_app::test_support::toolbar::TOOLBAR_METRONOME_ID)
        .expect("metronome utility button");
    let play = frame
        .paint_plan
        .first_widget_rect(crate::native_app::ui::ids::TOOLBAR_PLAY_ID)
        .expect("play button");
    let stop = frame
        .paint_plan
        .first_widget_rect(crate::native_app::test_support::toolbar::TOOLBAR_STOP_ID)
        .expect("stop button");
    let random = frame
        .paint_plan
        .first_widget_rect(crate::native_app::test_support::toolbar::TOOLBAR_RANDOM_ID)
        .expect("random button");
    let loop_button = frame
        .paint_plan
        .first_widget_rect(crate::native_app::ui::ids::TOOLBAR_LOOP_ID)
        .expect("loop button");

    assert_eq!(play.center().x - stop.center().x, 30.0);
    assert_eq!(random.center().x - play.center().x, 30.0);
    assert_eq!(loop_button.center().x - random.center().x, 30.0);
    assert!(stop.center().x - metronome.center().x > 30.0);
}

#[test]
fn apply_edit_mark_edits_button_appears_only_for_pending_effects() {
    let mut state = gui_state_for_span_tests();
    state
        .waveform
        .current
        .set_edit_selection_range(wavecrate::selection::SelectionRange::new(0.2, 0.6));

    let plain = crate::native_app::test_support::toolbar::main_toolbar(&state)
        .view_frame_at_size_with_default_theme(Vector2::new(720.0, 34.0));
    assert!(
        plain
            .paint_plan
            .first_widget_rect(
                crate::native_app::test_support::toolbar::TOOLBAR_APPLY_EDIT_MARK_EDITS_ID
            )
            .is_none(),
        "plain edit marks should not show an apply button"
    );

    state.waveform.current.set_edit_selection_range(
        wavecrate::selection::SelectionRange::new(0.2, 0.6).with_gain(0.5),
    );
    let edited = crate::native_app::test_support::toolbar::main_toolbar(&state)
        .view_frame_at_size_with_default_theme(Vector2::new(720.0, 34.0));
    assert!(
        edited
            .paint_plan
            .first_widget_rect(
                crate::native_app::test_support::toolbar::TOOLBAR_APPLY_EDIT_MARK_EDITS_ID
            )
            .is_some(),
        "pending edit mark effects should show an apply button"
    );
    assert_eq!(
        crate::native_app::test_support::toolbar::main_toolbar(&state).view_dispatch_widget_output(
            crate::native_app::test_support::toolbar::TOOLBAR_APPLY_EDIT_MARK_EDITS_ID,
            radiant::widgets::WidgetOutput::typed(radiant::widgets::ButtonMessage::Activate {
                provenance: radiant::widgets::InteractionProvenance::Programmatic,
            },),
        ),
        Some(crate::native_app::test_support::state::GuiMessage::RequestApplyEditSelectionEffects)
    );
}

#[test]
fn main_toolbar_control_projection_makes_order_and_identity_explicit() {
    use crate::native_app::test_support::toolbar::{
        ToolbarControlProjection, ToolbarIcon, ToolbarProjection,
    };

    let projection = ToolbarProjection::from_model(
        crate::native_app::app_chrome::view_models::toolbar::MainToolbarViewModel {
            random_available: true,
            similar_sections_available: false,
            similar_sections_enabled: true,
            sticky_random_sample_range_playback: true,
            loop_playback: true,
            playing: false,
            zero_crossing_snap_enabled: true,
            bpm_snap_enabled: true,
            beat_guides_enabled: true,
            metronome_enabled: true,
            beat_guide_count: 8,
            pending_edit_mark_edits: true,
            help_tooltips_enabled: true,
        },
    );

    assert!(projection.help_tooltips_enabled);
    assert_eq!(projection.controls.len(), 12);

    let icon_control = |index| match projection.controls[index] {
        ToolbarControlProjection::Icon(button) => button,
        control => panic!("expected toolbar icon control at {index}, got {control:?}"),
    };

    assert_eq!(icon_control(0).icon, ToolbarIcon::FocusLoaded);
    assert_eq!(
        icon_control(0).id,
        crate::native_app::test_support::toolbar::TOOLBAR_FOCUS_LOADED_ID
    );
    assert_eq!(icon_control(1).icon, ToolbarIcon::SimilarSections);
    assert!(icon_control(1).icon_enabled);
    assert!(icon_control(1).active);
    assert_eq!(icon_control(2).icon, ToolbarIcon::ZeroCrossingSnap);
    assert!(icon_control(2).active);
    assert_eq!(
        icon_control(2).tooltip,
        "Snap play and edit mark edges to nearby zero crossings."
    );
    assert_eq!(
        icon_control(2).id,
        crate::native_app::test_support::toolbar::TOOLBAR_ZERO_CROSSING_SNAP_ID
    );
    assert_eq!(icon_control(3).icon, ToolbarIcon::BpmSnap);
    assert!(icon_control(3).active);
    assert_eq!(
        icon_control(3).id,
        crate::native_app::test_support::toolbar::TOOLBAR_BPM_SNAP_ID
    );
    assert_eq!(icon_control(4).icon, ToolbarIcon::BeatGuides);
    assert!(icon_control(4).active);
    assert!(matches!(
        projection.controls[5],
        ToolbarControlProjection::BeatGuideCountField {
            count: 8,
            id: crate::native_app::test_support::toolbar::TOOLBAR_BEAT_GUIDE_COUNT_ID,
            key: "toolbar-beat-guide-count",
            tooltip: "Beat guide divisions.",
        }
    ));
    assert_eq!(icon_control(6).icon, ToolbarIcon::Metronome);
    assert_eq!(
        icon_control(6).id,
        crate::native_app::test_support::toolbar::TOOLBAR_METRONOME_ID
    );
    assert!(icon_control(6).active);
    assert_eq!(
        icon_control(6).tooltip,
        "Play a metronome from the beat guide divisions."
    );

    assert!(matches!(
        projection.controls[7],
        ToolbarControlProjection::ApplyEditMarkEdits {
            id: crate::native_app::test_support::toolbar::TOOLBAR_APPLY_EDIT_MARK_EDITS_ID,
            tooltip: "Apply edit mark gain and fade edits.",
        }
    ));

    assert_eq!(icon_control(8).icon, ToolbarIcon::Stop);
    assert_eq!(
        icon_control(8).id,
        crate::native_app::test_support::toolbar::TOOLBAR_STOP_ID
    );
    assert_eq!(icon_control(9).icon, ToolbarIcon::Play);
    assert!(!icon_control(9).active);
    assert_eq!(icon_control(9).tooltip, "Play");
    assert_eq!(icon_control(10).icon, ToolbarIcon::Random);
    assert!(icon_control(10).enabled);
    assert!(icon_control(10).active);
    assert!(icon_control(10).tooltip.starts_with("Play random section"));
    assert_eq!(icon_control(11).icon, ToolbarIcon::Loop);
    assert!(icon_control(11).active);
}

#[test]
fn main_toolbar_renders_beat_guide_count_as_number_field_without_stepper_buttons() {
    let mut state = NativeAppState::load_default().expect("default state loads");
    state.ui.chrome.beat_guide_count = 16;

    let frame = crate::native_app::test_support::toolbar::main_toolbar(&state)
        .view_frame_at_size_with_default_theme(Vector2::new(664.0, 34.0));
    let input = frame
        .paint_plan
        .text_inputs()
        .find(|input| {
            input.widget_id == crate::native_app::test_support::toolbar::TOOLBAR_BEAT_GUIDE_COUNT_ID
        })
        .expect("beat guide count should render as a text input");

    assert_eq!(input.state.value, "16");
    assert!(
        frame
            .paint_plan
            .first_widget_rect(crate::native_app::ui::ids::TOOLBAR_BEAT_GUIDE_DECREMENT_ID)
            .is_none(),
        "beat guide decrement button should not be projected"
    );
    assert!(
        frame
            .paint_plan
            .first_widget_rect(crate::native_app::ui::ids::TOOLBAR_BEAT_GUIDE_INCREMENT_ID)
            .is_none(),
        "beat guide increment button should not be projected"
    );
}

#[test]
fn random_toolbar_help_tooltip_paints_multiline_guidance() {
    let mut state = gui_state_for_span_tests();
    state.ui.chrome.help_tooltips_enabled = true;
    let bridge = radiant::runtime::DeclarativeOwnedRuntimeBridge::new(
        state,
        |state| crate::native_app::test_support::toolbar::main_toolbar(state).into_surface(),
        |_, _| {},
    );
    let mut runtime = SurfaceRuntime::new(bridge, Vector2::new(664.0, 80.0));
    let random = *runtime
        .layout()
        .rects
        .get(&crate::native_app::test_support::toolbar::TOOLBAR_RANDOM_ID)
        .expect("random toolbar button should lay out");

    runtime.dispatch_event(Event::pointer_move(random.center()));

    let frame = runtime.frame_with_default_theme();
    assert!(frame.paint_plan.contains_text("Play random section"));
    assert!(
        frame
            .paint_plan
            .contains_text("Click: play a random section now.")
    );
    assert!(
        frame
            .paint_plan
            .text_label_strings()
            .iter()
            .any(|text| text.contains("Shift-click: pick a random"))
    );
    assert!(
        frame
            .paint_plan
            .text_label_strings()
            .iter()
            .any(|text| text.contains("Command-click: make Space use random"))
    );
    assert!(frame.paint_plan.contains_text("sections."));
}

#[test]
fn main_toolbar_view_model_projects_playback_state() {
    let mut state = NativeAppState::load_default().expect("default state loads");

    let empty = crate::native_app::test_support::toolbar::main_toolbar_projection(&state);
    assert_eq!(empty.random_available, state.random_playback_available());
    assert!(!empty.sticky_random_sample_range_playback);
    assert!(!empty.similar_sections_available);
    assert!(!empty.similar_sections_enabled);
    assert!(!empty.loop_playback);
    assert!(!empty.playing);
    assert!(!empty.zero_crossing_snap_enabled);
    assert!(!empty.bpm_snap_enabled);
    assert!(!empty.beat_guides_enabled);
    assert!(!empty.metronome_enabled);
    assert_eq!(empty.beat_guide_count, 4);
    assert!(!empty.pending_edit_mark_edits);

    state.audio.loop_playback = true;
    state.audio.metronome_enabled = true;
    state.ui.chrome.sticky_random_sample_range_playback = true;
    state.ui.chrome.beat_guides_enabled = true;
    state.ui.chrome.bpm_snap_enabled = true;
    state.ui.chrome.beat_guide_count = 8;
    state.waveform.current =
        crate::native_app::test_support::state::WaveformState::synthetic_for_tests();
    state.waveform.current.start_playback(0.25);
    state.waveform.current.toggle_zero_crossing_snap();
    state.waveform.current.set_edit_selection_range(
        wavecrate::selection::SelectionRange::new(0.2, 0.6).with_gain(0.5),
    );
    state.waveform.current.set_play_selection_range(0.1, 0.2);

    let loaded = crate::native_app::test_support::toolbar::main_toolbar_projection(&state);
    assert_eq!(loaded.random_available, state.random_playback_available());
    assert!(loaded.sticky_random_sample_range_playback);
    assert!(loaded.similar_sections_available);
    assert!(!loaded.similar_sections_enabled);
    assert!(loaded.loop_playback);
    assert!(loaded.playing);
    assert!(loaded.zero_crossing_snap_enabled);
    assert!(loaded.bpm_snap_enabled);
    assert!(loaded.beat_guides_enabled);
    assert!(loaded.metronome_enabled);
    assert_eq!(loaded.beat_guide_count, 8);
    assert!(loaded.pending_edit_mark_edits);
}

#[test]
fn sticky_random_toolbar_message_updates_space_playback_mode() {
    let mut state = NativeAppState::load_default().expect("default state loads");
    let mut context = radiant::prelude::UiUpdateContext::default();

    assert!(!state.ui.chrome.sticky_random_sample_range_playback);

    state.apply_message(
        GuiMessage::ToggleStickyRandomSampleRangePlayback,
        &mut context,
    );

    assert!(state.ui.chrome.sticky_random_sample_range_playback);
    assert_eq!(
        state.ui.status.sample,
        "Sticky random playback on: Space plays random sample sections"
    );

    state.apply_message(
        GuiMessage::ToggleStickyRandomSampleRangePlayback,
        &mut context,
    );

    assert!(!state.ui.chrome.sticky_random_sample_range_playback);
    assert_eq!(
        state.ui.status.sample,
        "Sticky random playback off: Space plays selected samples"
    );
}

#[test]
fn zero_crossing_snap_toolbar_message_updates_waveform_state() {
    let mut state = NativeAppState::load_default().expect("default state loads");
    let mut context = radiant::prelude::UiUpdateContext::default();

    assert!(!state.waveform.current.zero_crossing_snap_enabled());

    state.apply_message(GuiMessage::ToggleZeroCrossingSnap, &mut context);

    assert!(state.waveform.current.zero_crossing_snap_enabled());
    assert_eq!(state.ui.status.sample, "Zero crossing snap enabled");

    state.apply_message(GuiMessage::ToggleZeroCrossingSnap, &mut context);

    assert!(!state.waveform.current.zero_crossing_snap_enabled());
    assert_eq!(state.ui.status.sample, "Zero crossing snap disabled");
}

#[test]
fn bpm_snap_toolbar_message_updates_chrome_state_and_status() {
    let mut state = NativeAppState::load_default().expect("default state loads");
    let mut context = radiant::prelude::UiUpdateContext::default();

    assert!(!state.ui.chrome.bpm_snap_enabled);
    state.apply_message(GuiMessage::ToggleBpmSnap, &mut context);
    assert!(state.ui.chrome.bpm_snap_enabled);
    assert_eq!(state.ui.status.sample, "BPM snap enabled");

    state.apply_message(GuiMessage::ToggleBpmSnap, &mut context);
    assert!(!state.ui.chrome.bpm_snap_enabled);
    assert_eq!(state.ui.status.sample, "BPM snap disabled");
}

#[test]
fn beat_guide_toolbar_messages_update_chrome_state() {
    let mut state = NativeAppState::load_default().expect("default state loads");
    let mut context = radiant::prelude::UiUpdateContext::default();

    state.apply_message(GuiMessage::ToggleBeatGuides, &mut context);
    assert!(state.ui.chrome.beat_guides_enabled);

    state.apply_message(GuiMessage::SetBeatGuideCount(7), &mut context);
    assert_eq!(state.ui.chrome.beat_guide_count, 7);

    state.apply_message(GuiMessage::SetBeatGuideCount(0), &mut context);
    assert_eq!(state.ui.chrome.beat_guide_count, 2);

    state.apply_message(GuiMessage::SetBeatGuideCount(100), &mut context);
    assert_eq!(state.ui.chrome.beat_guide_count, 32);

    state.apply_message(
        GuiMessage::ChangeBeatGuideCountInput(String::from("16")),
        &mut context,
    );
    assert_eq!(state.ui.chrome.beat_guide_count, 16);

    state.apply_message(
        GuiMessage::ChangeBeatGuideCountInput(String::from("999")),
        &mut context,
    );
    assert_eq!(state.ui.chrome.beat_guide_count, 16);

    state.apply_message(
        GuiMessage::CommitBeatGuideCountInput(String::from("0")),
        &mut context,
    );
    assert_eq!(state.ui.chrome.beat_guide_count, 2);

    state.apply_message(
        GuiMessage::CommitBeatGuideCountInput(String::from("999")),
        &mut context,
    );
    assert_eq!(state.ui.chrome.beat_guide_count, 32);
}

#[test]
fn beat_guide_count_field_owns_up_down_only_while_focused() {
    let state = NativeAppState::load_default().expect("default state loads");
    let mut runtime = SurfaceRuntime::new(
        radiant::runtime::DeclarativeOwnedRuntimeBridge::new(
            state,
            |state| crate::native_app::test_support::toolbar::main_toolbar(state).into_surface(),
            |state, message| {
                let mut context = ui::UiUpdateContext::default();
                state.apply_message(message, &mut context);
            },
        ),
        Vector2::new(664.0, 34.0),
    );
    let input_id = crate::native_app::test_support::toolbar::TOOLBAR_BEAT_GUIDE_COUNT_ID;

    assert_eq!(
        runtime.dispatch_event(Event::key_press(WidgetKey::ArrowUp)),
        None,
        "unfocused number field should not receive arrow keys"
    );
    assert_eq!(runtime.bridge().state().ui.chrome.beat_guide_count, 4);

    let input_point = runtime.layout().rects[&input_id].center();
    assert_eq!(
        runtime.dispatch_event(Event::primary_press(input_point)),
        Some(input_id)
    );
    assert_eq!(
        runtime.dispatch_event(Event::primary_release(input_point)),
        Some(input_id)
    );
    assert_eq!(
        runtime.dispatch_event(Event::character('1')),
        Some(input_id)
    );
    assert_eq!(
        runtime.dispatch_event(Event::character('6')),
        Some(input_id)
    );
    assert_eq!(runtime.bridge().state().ui.chrome.beat_guide_count, 16);

    assert_eq!(
        runtime.dispatch_event(Event::key_press(WidgetKey::ArrowUp)),
        Some(input_id)
    );
    assert_eq!(runtime.bridge().state().ui.chrome.beat_guide_count, 17);
    assert_eq!(
        runtime.dispatch_event(Event::key_press(WidgetKey::ArrowDown)),
        Some(input_id)
    );
    assert_eq!(runtime.bridge().state().ui.chrome.beat_guide_count, 16);

    runtime.clear_focus();
    assert_eq!(
        runtime.dispatch_event(Event::key_press(WidgetKey::ArrowDown)),
        None
    );
    assert_eq!(runtime.bridge().state().ui.chrome.beat_guide_count, 16);

    let input_point = runtime.layout().rects[&input_id].center();
    assert_eq!(
        runtime.dispatch_event(Event::primary_press(input_point)),
        Some(input_id)
    );
    assert_eq!(
        runtime.dispatch_event(Event::primary_release(input_point)),
        Some(input_id)
    );
    assert_eq!(
        runtime.dispatch_event(Event::character('0')),
        Some(input_id)
    );
    assert_eq!(runtime.bridge().state().ui.chrome.beat_guide_count, 16);
    runtime.clear_focus();
    assert_eq!(runtime.bridge().state().ui.chrome.beat_guide_count, 2);

    let input_point = runtime.layout().rects[&input_id].center();
    assert_eq!(
        runtime.dispatch_event(Event::primary_press(input_point)),
        Some(input_id)
    );
    assert_eq!(
        runtime.dispatch_event(Event::primary_release(input_point)),
        Some(input_id)
    );
    assert_eq!(
        runtime.dispatch_event(Event::character('9')),
        Some(input_id)
    );
    assert_eq!(runtime.bridge().state().ui.chrome.beat_guide_count, 9);
    assert_eq!(
        runtime.dispatch_event(Event::character('9')),
        Some(input_id)
    );
    assert_eq!(runtime.bridge().state().ui.chrome.beat_guide_count, 9);
    assert_eq!(
        runtime.dispatch_event(Event::character('9')),
        Some(input_id)
    );
    runtime.clear_focus();
    assert_eq!(runtime.bridge().state().ui.chrome.beat_guide_count, 32);

    let input_point = runtime.layout().rects[&input_id].center();
    assert_eq!(
        runtime.dispatch_event(Event::primary_press(input_point)),
        Some(input_id)
    );
    assert_eq!(
        runtime.dispatch_event(Event::primary_release(input_point)),
        Some(input_id)
    );
    assert_eq!(
        runtime.dispatch_event(Event::character('x')),
        Some(input_id)
    );
    assert_eq!(runtime.bridge().state().ui.chrome.beat_guide_count, 32);
}

#[test]
fn beat_guide_count_field_steps_on_hovered_vertical_wheel_input() {
    let state = NativeAppState::load_default().expect("default state loads");
    let mut runtime = SurfaceRuntime::new(
        radiant::runtime::DeclarativeOwnedRuntimeBridge::new(
            state,
            |state| crate::native_app::test_support::toolbar::main_toolbar(state).into_surface(),
            |state, message| {
                let mut context = ui::UiUpdateContext::default();
                state.apply_message(message, &mut context);
            },
        ),
        Vector2::new(664.0, 34.0),
    );
    let input_id = crate::native_app::test_support::toolbar::TOOLBAR_BEAT_GUIDE_COUNT_ID;
    let input_point = runtime.layout().rects[&input_id].center();

    assert!(runtime.wheel_or_scroll_at(input_point, Vector2::new(0.0, -40.0)));
    assert_eq!(runtime.bridge().state().ui.chrome.beat_guide_count, 5);
    assert!(runtime.wheel_or_scroll_at(input_point, Vector2::new(0.0, 40.0)));
    assert_eq!(runtime.bridge().state().ui.chrome.beat_guide_count, 4);
    assert!(
        !runtime.wheel_or_scroll_at(input_point, Vector2::new(40.0, 0.0)),
        "horizontal-only wheel input should remain available to another target"
    );
    assert_eq!(runtime.bridge().state().ui.chrome.beat_guide_count, 4);

    for _ in 0..40 {
        assert!(runtime.wheel_or_scroll_at(input_point, Vector2::new(0.0, -1.0)));
    }
    assert_eq!(runtime.bridge().state().ui.chrome.beat_guide_count, 32);
    assert!(runtime.wheel_or_scroll_at(input_point, Vector2::new(0.0, 1.0)));
    assert_eq!(runtime.bridge().state().ui.chrome.beat_guide_count, 31);
}

#[test]
fn metronome_toolbar_message_updates_audio_state() {
    let mut state = NativeAppState::load_default().expect("default state loads");
    let mut context = radiant::prelude::UiUpdateContext::default();

    state.apply_message(GuiMessage::ToggleMetronome, &mut context);

    assert!(state.audio.metronome_enabled);
    assert_eq!(state.ui.status.sample, "Metronome enabled");

    state.apply_message(GuiMessage::ToggleMetronome, &mut context);

    assert!(!state.audio.metronome_enabled);
    assert_eq!(state.ui.status.sample, "Metronome disabled");
}

#[test]
fn metronome_config_aligns_to_play_selection_grid_phase() {
    let mut state = gui_state_for_span_tests();
    state.audio.metronome_enabled = true;
    state.ui.chrome.beat_guide_count = 4;
    state.waveform.current.set_play_selection_range(0.25, 0.75);

    let config = state.playback_metronome_config_for_span(0.375, 0.625, 0.5);

    assert_eq!(
        config,
        Some(wavecrate::audio::PlaybackMetronomeConfig::new(4).with_cycle(24_000, 12_000))
    );
}
