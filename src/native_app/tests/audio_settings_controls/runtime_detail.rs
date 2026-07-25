use super::*;

#[test]
fn audio_settings_snapshot_uses_cached_device_options() {
    let mut state = gui_state_for_span_tests();
    state.audio.hosts = vec![crate::native_app::test_support::audio::AudioHostSummary {
        id: String::from("cached-host"),
        label: String::from("Cached Host"),
        is_default: true,
    }];

    let host_ids = crate::native_app::test_support::settings::audio_settings_host_ids(&state);

    assert_eq!(host_ids, vec![String::from("cached-host")]);
}

#[test]
fn opening_settings_queues_discovery_without_replacing_cached_options() {
    let mut state = gui_state_for_span_tests();
    state.audio.hosts = vec![crate::native_app::test_support::audio::AudioHostSummary {
        id: String::from("cached-host"),
        label: String::from("Cached Host"),
        is_default: true,
    }];
    let mut context = radiant::prelude::UiUpdateContext::default();

    state.apply_message(
        crate::native_app::test_support::state::GuiMessage::Settings(
            crate::native_app::app::SettingsMessage::OpenGeneralSettings,
        ),
        &mut context,
    );

    assert!(
        state
            .background
            .audio_options_refresh_task
            .active()
            .is_some()
    );
    assert_eq!(state.audio.hosts[0].id, "cached-host");
}

#[test]
fn stale_audio_discovery_completion_cannot_replace_newer_options() {
    let mut state = gui_state_for_span_tests();
    state.audio.hosts = vec![crate::native_app::test_support::audio::AudioHostSummary {
        id: String::from("initial-host"),
        label: String::from("Initial Host"),
        is_default: true,
    }];
    let mut context = radiant::prelude::UiUpdateContext::default();
    state.queue_audio_options_refresh(&mut context);
    let stale_ticket = state
        .background
        .audio_options_refresh_task
        .active()
        .expect("first discovery should be active");
    state.queue_audio_options_refresh(&mut context);
    let current_ticket = state
        .background
        .audio_options_refresh_task
        .active()
        .expect("new discovery should replace the first");

    state.finish_audio_options_refresh(radiant::prelude::TaskCompletion {
        ticket: stale_ticket,
        output: crate::native_app::app::AudioOptionsRefreshResult {
            hosts: vec![crate::native_app::test_support::audio::AudioHostSummary {
                id: String::from("stale-host"),
                label: String::from("Stale Host"),
                is_default: true,
            }],
            ..Default::default()
        },
    });
    assert_eq!(state.audio.hosts[0].id, "initial-host");
    assert_eq!(
        state.background.audio_options_refresh_task.active(),
        Some(current_ticket)
    );

    state.finish_audio_options_refresh(radiant::prelude::TaskCompletion {
        ticket: current_ticket,
        output: crate::native_app::app::AudioOptionsRefreshResult {
            hosts: vec![crate::native_app::test_support::audio::AudioHostSummary {
                id: String::from("current-host"),
                label: String::from("Current Host"),
                is_default: true,
            }],
            ..Default::default()
        },
    });
    assert_eq!(state.audio.hosts[0].id, "current-host");
}

#[test]
fn audio_output_picker_records_intent_and_defers_open_and_persistence() {
    let mut state = gui_state_for_span_tests();
    let persisted = state.ui.settings.persisted.audio_output.clone();
    let mut context = radiant::prelude::UiUpdateContext::default();

    state.apply_message(
        crate::native_app::test_support::state::GuiMessage::Settings(
            crate::native_app::app::SettingsMessage::SetAudioOutputHost(Some(String::from(
                "requested-host",
            ))),
        ),
        &mut context,
    );

    assert_eq!(
        state.audio.output_config.host.as_deref(),
        Some("requested-host")
    );
    assert_eq!(state.ui.settings.persisted.audio_output, persisted);
    assert_eq!(state.current_settings_core().audio_output, persisted);
    assert!(state.background.audio_open.active().is_some());
    assert!(
        state
            .background
            .audio_options_refresh_task
            .active()
            .is_some()
    );
    assert!(state.audio.output_resolved.is_none());
    assert_eq!(state.ui.status.sample, "Audio output change pending");
    assert_eq!(state.audio_engine_pill_label(), "starting");
    assert_eq!(state.audio_engine_detail_label(), "Audio output starting");
}

#[test]
fn failed_audio_open_keeps_unconfirmed_output_out_of_persisted_settings() {
    let mut state = gui_state_for_span_tests();
    let persisted = state.ui.settings.persisted.audio_output.clone();
    let mut context = radiant::prelude::UiUpdateContext::default();

    state.apply_message(
        crate::native_app::test_support::state::GuiMessage::Settings(
            crate::native_app::app::SettingsMessage::SetAudioOutputHost(Some(String::from(
                "disconnected-host",
            ))),
        ),
        &mut context,
    );
    state.background.audio_open.cancel();
    let completion = state
        .background
        .audio_open
        .begin()
        .complete(Err(String::from("output device disconnected")));
    state.finish_audio_player_open(completion, &mut context);

    assert_eq!(
        state.audio.settings_error.as_deref(),
        Some("output device disconnected")
    );
    assert_eq!(state.ui.settings.persisted.audio_output, persisted);
    assert_eq!(state.current_settings_core().audio_output, persisted);
    assert!(state.audio.output_config_persist_pending);
    assert!(state.audio.output_resolved.is_none());
}

#[test]
fn successful_audio_persist_completion_confirms_the_selected_output() {
    let mut state = gui_state_for_span_tests();
    state.audio.output_config.host = Some(String::from("confirmed-host"));
    state.audio.output_config_persist_pending = true;
    let mut context = radiant::prelude::UiUpdateContext::default();
    state.queue_audio_output_persist(&mut context);
    let ticket = state
        .background
        .audio_output_persist_task
        .active()
        .expect("audio persistence should be active");
    let mut persisted = state.current_settings_core();
    persisted.audio_output = state.audio.output_config.clone();

    state.finish_audio_output_persist(radiant::prelude::TaskCompletion {
        ticket,
        output: crate::native_app::app::AudioOutputPersistResult {
            persisted,
            result: Ok(()),
        },
    });

    assert_eq!(
        state.ui.settings.persisted.audio_output.host.as_deref(),
        Some("confirmed-host")
    );
    assert!(!state.audio.output_config_persist_pending);
}

#[test]
fn audio_engine_detail_distinguishes_selected_host_from_runtime_fallback() {
    let mut state = gui_state_for_span_tests();
    state.audio.output_config.host = Some(String::from("asio"));
    state.audio.hosts = vec![
        crate::native_app::test_support::audio::AudioHostSummary {
            id: String::from("wasapi"),
            label: String::from("WASAPI"),
            is_default: true,
        },
        crate::native_app::test_support::audio::AudioHostSummary {
            id: String::from("asio"),
            label: String::from("ASIO"),
            is_default: false,
        },
    ];
    state.audio.output_resolved = Some(crate::native_app::test_support::audio::ResolvedOutput {
        host_id: String::from("wasapi"),
        device_name: String::from("Studio"),
        sample_rate: 48_000,
        buffer_size_frames: None,
        channel_count: 2,
        used_fallback: true,
    });

    assert_eq!(
        state.audio_engine_detail_label(),
        "ASIO selected | using WASAPI | Studio | 48 kHz"
    );
}

#[test]
fn audio_engine_pill_prefers_runtime_sample_rate() {
    let mut state = gui_state_for_span_tests();
    state.audio.output_config.sample_rate = Some(44_100);
    state.audio.output_resolved = Some(crate::native_app::test_support::audio::ResolvedOutput {
        host_id: String::from("wasapi"),
        device_name: String::from("Studio"),
        sample_rate: 48_000,
        buffer_size_frames: None,
        channel_count: 2,
        used_fallback: false,
    });

    assert_eq!(state.audio_engine_pill_label(), "48 kHz");
}

#[test]
fn audio_engine_pill_shows_no_audio_before_runtime_resolves() {
    let mut state = gui_state_for_span_tests();
    state.audio.output_config.sample_rate = Some(44_100);

    assert_eq!(state.audio_engine_pill_label(), "no audio");
}

#[test]
fn audio_engine_pill_uses_warning_style_without_runtime_output() {
    let mut state = gui_state_for_span_tests();
    state.audio.output_config.sample_rate = Some(44_100);

    assert_eq!(
        state.audio_engine_pill_style(),
        WidgetStyle::subtle(WidgetTone::Warning)
    );
}

#[test]
fn audio_engine_pill_uses_neutral_style_with_runtime_output() {
    let mut state = gui_state_for_span_tests();
    state.audio.output_resolved = Some(crate::native_app::test_support::audio::ResolvedOutput {
        host_id: String::from("wasapi"),
        device_name: String::from("Studio"),
        sample_rate: 48_000,
        buffer_size_frames: None,
        channel_count: 2,
        used_fallback: false,
    });

    assert_eq!(
        state.audio_engine_pill_style(),
        WidgetStyle::subtle(WidgetTone::Neutral)
    );
}

#[test]
fn audio_engine_pill_shows_off_for_audio_output_error() {
    let mut state = gui_state_for_span_tests();
    state.audio.settings_error = Some(String::from(
        "Audio output stream error: output device disconnected",
    ));

    assert_eq!(state.audio_engine_pill_label(), "OFF");
    assert_eq!(
        state.audio_engine_pill_style(),
        WidgetStyle::strong(WidgetTone::Danger)
    );
}

#[test]
fn runtime_output_error_marks_audio_engine_unavailable() {
    let mut state = gui_state_for_span_tests();
    state.audio.output_resolved = Some(crate::native_app::test_support::audio::ResolvedOutput {
        host_id: String::from("wasapi"),
        device_name: String::from("Studio"),
        sample_rate: 48_000,
        buffer_size_frames: None,
        channel_count: 2,
        used_fallback: false,
    });
    state.audio.current_playback_span = Some((0.0, 1.0));

    state.mark_audio_output_unavailable(String::from(
        "Audio output stream error: output device disconnected",
    ));

    assert!(state.audio.output_resolved.is_none());
    assert!(state.audio.current_playback_span.is_none());
    assert_eq!(
        state.audio.settings_error.as_deref(),
        Some("Audio output stream error: output device disconnected")
    );
    assert_eq!(state.audio_engine_pill_label(), "OFF");
    assert!(state.ui.status.sample.starts_with("Audio output OFF:"));
}

#[test]
fn audio_sample_rate_label_matches_status_chip_format() {
    assert_eq!(
        crate::native_app::test_support::state::format_sample_rate_label(48_000),
        "48 kHz"
    );
    assert_eq!(
        crate::native_app::test_support::state::format_sample_rate_label(44_100),
        "44.1 kHz"
    );
    assert_eq!(
        crate::native_app::test_support::state::format_sample_rate_label(960),
        "960 Hz"
    );
}
