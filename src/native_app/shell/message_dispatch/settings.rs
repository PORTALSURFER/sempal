use radiant::prelude as ui;
use std::time::Instant;
use wavecrate::sample_sources::config::clamp_rating_decay_weeks;

use crate::native_app::app::{
    AudioSettingsDropdown, GuiMessage, NativeAppState, SettingsMessage, emit_gui_action,
};

impl NativeAppState {
    pub(super) fn apply_settings_message(
        &mut self,
        message: SettingsMessage,
        context: &mut ui::UiUpdateContext<GuiMessage>,
    ) {
        match message {
            SettingsMessage::SetVolume(volume) => self.set_volume(volume),
            SettingsMessage::SetNormalizedAuditionEnabled(enabled) => {
                self.set_normalized_audition_enabled(enabled, context);
            }
            SettingsMessage::ToggleHelpTooltips => self.toggle_help_tooltips(),
            SettingsMessage::ToggleAudioSettings => self.toggle_audio_settings(context),
            SettingsMessage::OpenGeneralSettings => self.open_general_settings(context),
            SettingsMessage::SelectSettingsTab(tab) => self.select_settings_tab(tab),
            SettingsMessage::CloseAudioSettings => self.close_audio_settings_window(),
            SettingsMessage::ToggleAudioBackendDropdown => self.toggle_audio_backend_dropdown(),
            SettingsMessage::ToggleAudioOutputDropdown => self.toggle_audio_output_dropdown(),
            SettingsMessage::ToggleAudioSampleRateDropdown => {
                self.toggle_audio_sample_rate_dropdown();
            }
            SettingsMessage::CloseAudioSettingsDropdowns => self.close_audio_settings_dropdowns(),
            SettingsMessage::SetAudioOutputHost(host) => self.set_audio_output_host(host, context),
            SettingsMessage::SetAudioOutputDevice(device) => {
                self.set_audio_output_device(device, context)
            }
            SettingsMessage::SetAudioOutputSampleRate(sample_rate) => {
                self.set_audio_output_sample_rate(sample_rate, context);
            }
            SettingsMessage::SetRatingDecayWeeks(weeks) => self.set_rating_decay_weeks(weeks),
            SettingsMessage::PickTrashFolder => self.pick_trash_folder(context),
            SettingsMessage::ClearTrashFolder => self.clear_trash_folder(),
            SettingsMessage::ClearRebuildableCaches => self.clear_rebuildable_caches(context),
            SettingsMessage::GlobalStorageUsageFinished(completion) => {
                self.finish_global_storage_usage_refresh(completion);
            }
        }
    }

    fn toggle_audio_backend_dropdown(&mut self) {
        self.toggle_audio_settings_dropdown(AudioSettingsDropdown::Backend);
    }

    fn toggle_help_tooltips(&mut self) {
        self.ui.chrome.help_tooltips_enabled = !self.ui.chrome.help_tooltips_enabled;
    }

    fn toggle_audio_output_dropdown(&mut self) {
        self.toggle_audio_settings_dropdown(AudioSettingsDropdown::Output);
    }

    fn toggle_audio_sample_rate_dropdown(&mut self) {
        self.toggle_audio_settings_dropdown(AudioSettingsDropdown::SampleRate);
    }

    fn set_rating_decay_weeks(&mut self, weeks: u16) {
        let started_at = Instant::now();
        let weeks = clamp_rating_decay_weeks(weeks);
        if self.ui.settings.persisted.controls.rating_decay_weeks == weeks {
            return;
        }
        self.ui.settings.persisted.controls.rating_decay_weeks = weeks;
        self.persist_user_configuration("settings.rating_decay_weeks.persist", started_at);
        emit_gui_action(
            "settings.rating_decay_weeks",
            Some("settings"),
            Some(&weeks.to_string()),
            "changed",
            started_at,
            None,
        );
    }
}
