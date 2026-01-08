use super::*;

impl EguiController {
    pub fn show_hint_of_day(&mut self) {
        let hint = crate::egui_app::hints::random_hint();
        self.ui.hints.title = hint.title.to_string();
        self.ui.hints.body = hint.body.to_string();
        self.ui.hints.open = true;
    }

    pub fn dismiss_hint_of_day(&mut self) {
        self.ui.hints.open = false;
    }

    pub fn set_hint_on_startup(&mut self, enabled: bool) {
        if self.settings.hints.show_on_startup == enabled {
            return;
        }
        self.settings.hints.show_on_startup = enabled;
        self.ui.hints.show_on_startup = enabled;
        if let Err(err) = self.persist_config("Failed to save hint settings") {
            self.set_status(err, StatusTone::Warning);
        }
    }

    pub(super) fn maybe_open_hint_of_day(&mut self) {
        if !self.settings.hints.show_on_startup {
            return;
        }
        if self.ui.hints.open {
            return;
        }
        self.show_hint_of_day();
    }
}
