use super::*;

#[test]
fn audio_engine_pill_activates_settings_toggle() {
    let mut state = NativeAppState::load_default().expect("default state loads");
    state.ui.settings.ui.audio_settings_open = true;
    let surface = crate::native_app::test_support::settings::top_control_bar(&state).into_surface();
    let pill = surface
        .find_widget(crate::native_app::test_support::settings::AUDIO_ENGINE_PILL_ID)
        .and_then(|widget| {
            widget
                .widget_object()
                .as_any()
                .downcast_ref::<BadgeWidget>()
        })
        .expect("audio pill should use a Radiant badge");

    assert!(pill.common.is_active());
    assert_eq!(
        surface.dispatch_widget_output(
            crate::native_app::test_support::settings::AUDIO_ENGINE_PILL_ID,
            radiant::widgets::WidgetOutput::typed(BadgeMessage::Activate),
        ),
        Some(
            crate::native_app::test_support::state::GuiMessage::Settings(
                crate::native_app::app::SettingsMessage::ToggleAudioSettings
            )
        )
    );
}

#[test]
fn general_settings_button_opens_general_tab() {
    let mut state = NativeAppState::load_default().expect("default state loads");
    state.ui.settings.ui.audio_settings_open = true;
    state.ui.settings.ui.app_settings_tab =
        crate::native_app::test_support::state::AppSettingsTab::General;
    let surface = crate::native_app::test_support::settings::top_control_bar(&state).into_surface();
    let button = surface
        .find_widget(crate::native_app::test_support::settings::GENERAL_SETTINGS_BUTTON_ID)
        .and_then(|widget| {
            widget
                .widget_object()
                .as_any()
                .downcast_ref::<IconButtonWidget>()
        })
        .expect("general settings button should use a Radiant icon button");

    assert!(button.common.is_active());
    assert_eq!(
        surface.dispatch_widget_output(
            crate::native_app::test_support::settings::GENERAL_SETTINGS_BUTTON_ID,
            radiant::widgets::WidgetOutput::typed(ButtonMessage::Activate {
                provenance: radiant::widgets::InteractionProvenance::Programmatic,
            }),
        ),
        Some(
            crate::native_app::test_support::state::GuiMessage::Settings(
                crate::native_app::app::SettingsMessage::OpenGeneralSettings
            )
        )
    );
}

#[test]
fn help_tooltips_button_toggles_help_mode() {
    let state = NativeAppState::load_default().expect("default state loads");
    let surface = crate::native_app::test_support::settings::top_control_bar(&state).into_surface();
    let button = surface
        .find_widget(crate::native_app::test_support::settings::HELP_TOOLTIPS_BUTTON_ID)
        .and_then(|widget| {
            widget
                .widget_object()
                .as_any()
                .downcast_ref::<IconButtonWidget>()
        })
        .expect("help tooltips button should use a Radiant icon button");

    assert!(!button.common.is_active());
    assert_eq!(
        surface.dispatch_widget_output(
            crate::native_app::test_support::settings::HELP_TOOLTIPS_BUTTON_ID,
            radiant::widgets::WidgetOutput::typed(ButtonMessage::Activate {
                provenance: radiant::widgets::InteractionProvenance::Programmatic,
            }),
        ),
        Some(
            crate::native_app::test_support::state::GuiMessage::Settings(
                crate::native_app::app::SettingsMessage::ToggleHelpTooltips
            )
        )
    );
}

#[test]
fn normalized_audition_button_toggles_forced_normalized_mode() {
    let mut state = NativeAppState::load_default().expect("default state loads");
    state.audio.normalized_audition_enabled = true;
    let surface = crate::native_app::test_support::settings::top_control_bar(&state).into_surface();
    let button = surface
        .find_widget(crate::native_app::test_support::settings::NORMALIZED_AUDITION_BUTTON_ID)
        .and_then(|widget| {
            widget
                .widget_object()
                .as_any()
                .downcast_ref::<IconButtonWidget>()
        })
        .expect("normalized audition should use a Radiant icon button");

    assert!(button.common.is_active());
    assert_eq!(
        surface.dispatch_widget_output(
            crate::native_app::test_support::settings::NORMALIZED_AUDITION_BUTTON_ID,
            radiant::widgets::WidgetOutput::typed(ButtonMessage::Activate {
                provenance: radiant::widgets::InteractionProvenance::Programmatic,
            }),
        ),
        Some(
            crate::native_app::test_support::state::GuiMessage::Settings(
                crate::native_app::app::SettingsMessage::SetNormalizedAuditionEnabled(false)
            )
        )
    );
}

#[test]
fn release_update_button_opens_download_page() {
    let mut state = NativeAppState::load_default().expect("default state loads");
    state
        .ui
        .release_update
        .finish(Ok(Some(wavecrate::updater::PublicReleaseInfo {
            build_id: String::from("wavecrate-nightly-b999"),
            build_number: 999,
            version: String::from("nightly"),
            released_at: String::from("2026-06-25T20:13:25.000Z"),
            download_page_url: String::from("https://portalsurfer.org/wavecrate/"),
        })));
    let surface = crate::native_app::test_support::settings::top_control_bar(&state).into_surface();
    let button = surface
        .find_widget(crate::native_app::test_support::settings::RELEASE_UPDATE_BUTTON_ID)
        .and_then(|widget| {
            widget
                .widget_object()
                .as_any()
                .downcast_ref::<IconButtonWidget>()
        })
        .expect("release update button should use a Radiant icon button");

    assert!(button.common.is_active());
    assert_eq!(
        surface.dispatch_widget_output(
            crate::native_app::test_support::settings::RELEASE_UPDATE_BUTTON_ID,
            radiant::widgets::WidgetOutput::typed(ButtonMessage::Activate {
                provenance: radiant::widgets::InteractionProvenance::Programmatic,
            }),
        ),
        Some(crate::native_app::test_support::state::GuiMessage::OpenReleaseDownloadPage)
    );
}

#[test]
fn settings_top_bar_actions_open_expected_tabs() {
    let mut state = gui_state_for_span_tests();
    let mut context = radiant::prelude::UiUpdateContext::default();

    state.apply_message(
        crate::native_app::test_support::state::GuiMessage::Settings(
            crate::native_app::app::SettingsMessage::OpenGeneralSettings,
        ),
        &mut context,
    );
    assert!(state.ui.settings.ui.audio_settings_open);
    assert_eq!(
        state.ui.settings.ui.app_settings_tab,
        crate::native_app::test_support::state::AppSettingsTab::General
    );
    assert_eq!(
        state.ui.settings.ui.global_storage_usage,
        crate::native_app::app::GlobalStorageUsageState::Loading
    );

    state.apply_message(
        crate::native_app::test_support::state::GuiMessage::Settings(
            crate::native_app::app::SettingsMessage::ToggleAudioSettings,
        ),
        &mut context,
    );
    assert!(state.ui.settings.ui.audio_settings_open);
    assert_eq!(
        state.ui.settings.ui.app_settings_tab,
        crate::native_app::test_support::state::AppSettingsTab::AudioEngine
    );

    state.apply_message(
        crate::native_app::test_support::state::GuiMessage::Settings(
            crate::native_app::app::SettingsMessage::ToggleAudioSettings,
        ),
        &mut context,
    );
    assert!(!state.ui.settings.ui.audio_settings_open);
}

#[test]
fn settings_help_tooltips_toggle_updates_chrome_state() {
    let mut state = gui_state_for_span_tests();
    let mut context = radiant::prelude::UiUpdateContext::default();

    state.apply_message(
        crate::native_app::test_support::state::GuiMessage::Settings(
            crate::native_app::app::SettingsMessage::ToggleHelpTooltips,
        ),
        &mut context,
    );
    assert!(state.ui.chrome.help_tooltips_enabled);

    state.apply_message(
        crate::native_app::test_support::state::GuiMessage::Settings(
            crate::native_app::app::SettingsMessage::ToggleHelpTooltips,
        ),
        &mut context,
    );
    assert!(!state.ui.chrome.help_tooltips_enabled);
}
