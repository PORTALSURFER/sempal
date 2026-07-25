use std::time::Instant;

use radiant::prelude as ui;

use crate::native_app::app::{
    AppSettingsTab, AudioSettingsDropdown, GuiMessage, NativeAppState, emit_gui_action,
};

impl NativeAppState {
    pub(in crate::native_app) fn toggle_audio_settings(
        &mut self,
        context: &mut ui::UiUpdateContext<GuiMessage>,
    ) {
        let started_at = Instant::now();
        if self.ui.settings.ui.audio_settings_open
            && self.ui.settings.ui.app_settings_tab == AppSettingsTab::AudioEngine
        {
            self.close_audio_settings_window();
        } else {
            self.open_settings_window(AppSettingsTab::AudioEngine, context);
        }
        emit_gui_action(
            "audio.settings.toggle",
            Some("top_bar"),
            None,
            if self.ui.settings.ui.audio_settings_open {
                "opened"
            } else {
                "closed"
            },
            started_at,
            None,
        );
    }

    pub(in crate::native_app) fn open_general_settings(
        &mut self,
        context: &mut ui::UiUpdateContext<GuiMessage>,
    ) {
        let started_at = Instant::now();
        self.open_settings_window(AppSettingsTab::General, context);
        emit_gui_action(
            "settings.general.open",
            Some("top_bar"),
            None,
            "opened",
            started_at,
            None,
        );
    }

    pub(in crate::native_app) fn select_settings_tab(&mut self, tab: AppSettingsTab) {
        let started_at = Instant::now();
        self.ui.settings.ui.app_settings_tab = tab;
        self.close_audio_settings_dropdowns();
        emit_gui_action(
            "settings.tab.select",
            Some("settings"),
            Some(tab.analytics_label()),
            "selected",
            started_at,
            None,
        );
    }

    fn open_settings_window(
        &mut self,
        tab: AppSettingsTab,
        context: &mut ui::UiUpdateContext<GuiMessage>,
    ) {
        self.queue_audio_options_refresh(context);
        self.queue_global_storage_usage_refresh(context);
        self.ui.settings.ui.audio_settings_open = true;
        self.ui.settings.ui.app_settings_tab = tab;
        self.close_audio_settings_dropdowns();
        self.audio.settings_error = None;
    }

    pub(in crate::native_app) fn close_audio_settings_window(&mut self) {
        self.ui.settings.ui.audio_settings_open = false;
        self.close_audio_settings_dropdowns();
    }

    pub(in crate::native_app) fn audio_settings_dropdown_open(&self) -> bool {
        self.ui.settings.ui.audio_settings_dropdown.any_open()
    }

    pub(in crate::native_app) fn close_audio_settings_dropdowns(&mut self) {
        self.ui.settings.ui.audio_settings_dropdown.close();
    }

    pub(in crate::native_app) fn toggle_audio_settings_dropdown(
        &mut self,
        dropdown: AudioSettingsDropdown,
    ) {
        self.ui.settings.ui.audio_settings_dropdown.toggle(dropdown);
    }
}

impl AppSettingsTab {
    fn analytics_label(self) -> &'static str {
        match self {
            Self::General => "general",
            Self::AudioEngine => "audio_engine",
        }
    }
}
