use super::*;

impl DropHandler {
    /// Handle a dropped file and load the waveform if supported.
    pub fn handle_drop(&self, path: &std::path::Path) -> bool {
        let Some(app) = self.app() else {
            return false;
        };
        if !Self::is_wav(path) {
            self.set_status(
                &app,
                "Unsupported file type (please drop a .wav)",
                StatusState::Warning,
            );
            return false;
        }
        match self.renderer.load_waveform(path) {
            Ok(loaded) => {
                app.set_waveform(loaded.image);
                let mut player = self.player.borrow_mut();
                player.stop();
                player.set_audio(loaded.audio_bytes, loaded.duration_seconds);
                self.playhead_timer.stop();
                app.set_playhead_position(0.0);
                app.set_playhead_visible(false);
                self.clear_selection(&app);
                self.set_status(
                    &app,
                    format!("Loaded {}", path.display()),
                    StatusState::Info,
                );
                true
            }
            Err(error) => {
                self.set_status(&app, error, StatusState::Error);
                false
            }
        }
    }

    /// Begin playback, optionally looping the active selection.
    pub fn play_audio(&self, looped: bool) -> EventResult {
        let Some(app) = self.app() else {
            return EventResult::Propagate;
        };
        match self.start_playback(looped, self.usable_selection(), None) {
            Ok(_) => {
                self.set_status(
                    &app,
                    if looped { "Looping selection" } else { "Playing audio" },
                    StatusState::Info,
                );
                self.start_playhead_updates();
                EventResult::PreventDefault
            }
            Err(error) => {
                self.set_status(&app, error, StatusState::Error);
                EventResult::PreventDefault
            }
        }
    }

    /// Seek to an absolute position in the current waveform.
    pub fn seek_to(&self, position: f32) {
        let Some(app) = self.app() else {
            return;
        };
        let progress = position.clamp(0.0, 1.0);
        self.playhead_timer.stop();
        match self.player.borrow_mut().play_from_fraction(progress) {
            Ok(_) => {
                app.set_playhead_position(progress);
                app.set_playhead_visible(true);
                self.set_status(&app, "Playing audio", StatusState::Info);
                self.start_playhead_updates();
            }
            Err(error) => {
                self.set_status(&app, error, StatusState::Error);
                app.set_playhead_visible(false);
            }
        }
    }

    /// Start a new selection drag gesture.
    pub fn start_selection_drag(&self, position: f32) {
        let Some(app) = self.app() else { return };
        let was_looping = *self.loop_enabled.borrow() && self.player.borrow().is_playing();
        self.selection_drag_looping.replace(was_looping);
        self.player.borrow_mut().stop();
        self.playhead_timer.stop();
        app.set_playhead_visible(false);
        let range = {
            let mut selection = self.selection.borrow_mut();
            selection.begin_new(position)
        };
        self.apply_selection(&app, Some(range));
    }

    /// Update the selection drag to a new position.
    pub fn update_selection_drag(&self, position: f32) {
        let Some(app) = self.app() else { return };
        let next_range = {
            let mut selection = self.selection.borrow_mut();
            selection.update_drag(position)
        };
        if let Some(range) = next_range {
            self.apply_selection(&app, Some(range));
        }
    }

    /// Finish a selection drag and restart looping playback if needed.
    pub fn finish_selection_drag(&self) {
        self.selection.borrow_mut().finish_drag();
        let should_restart = *self.selection_drag_looping.borrow();
        self.selection_drag_looping.replace(false);
        self.restart_loop_if_active(should_restart);
    }

    /// Clear the current selection if present.
    pub fn clear_selection_request(&self) {
        let Some(app) = self.app() else { return };
        let cleared = self.selection.borrow_mut().clear();
        if cleared {
            self.apply_selection(&app, None);
        }
    }

    /// Begin dragging a specific selection edge if a selection exists.
    pub fn begin_edge_drag(&self, edge: SelectionEdge) {
        let Some(app) = self.app() else { return };
        let range = {
            let mut selection = self.selection.borrow_mut();
            if selection.begin_edge_drag(edge) {
                selection.range()
            } else {
                None
            }
        };
        if let Some(range) = range {
            self.apply_selection(&app, Some(range));
        }
    }

    fn clear_selection(&self, app: &HelloWorld) {
        let cleared = self.selection.borrow_mut().clear();
        if cleared {
            self.apply_selection(app, None);
        }
    }

    /// Handle the loop toggle UI change or keyboard shortcut.
    pub fn handle_loop_toggle(&self, enabled: bool) {
        self.set_loop_enabled(enabled);
        let is_playing = self.player.borrow().is_playing();
        let progress = if is_playing {
            self.player.borrow().progress()
        } else {
            None
        };
        if enabled && is_playing {
            self.restart_playback(true, self.usable_selection(), progress);
        } else if !enabled && is_playing {
            self.restart_playback(false, self.usable_selection(), progress);
        } else if let Some(app) = self.app() {
            self.set_status(
                &app,
                if enabled { "Loop enabled" } else { "Loop disabled" },
                StatusState::Info,
            );
        }
    }

    fn apply_selection(&self, app: &HelloWorld, range: Option<SelectionRange>) {
        let dragging = self.selection.borrow().is_dragging();
        if let Some(range) = range.or_else(|| self.selection.borrow().range()) {
            app.set_selection_visible(true);
            app.set_selection_start(range.start());
            app.set_selection_end(range.end());
            if !dragging {
                self.restart_loop_if_active(false);
            }
        } else {
            app.set_selection_visible(false);
            app.set_selection_start(0.0);
            app.set_selection_end(0.0);
        }
    }

    fn selection_range(&self) -> Option<SelectionRange> {
        self.selection.borrow().range()
    }

    fn usable_selection(&self) -> Option<SelectionRange> {
        self.selection_range()
            .filter(|range| range.width() >= MIN_SELECTION_WIDTH)
    }

    fn start_playback(
        &self,
        looped: bool,
        selection: Option<SelectionRange>,
        resume_from: Option<f32>,
    ) -> Result<(), String> {
        let mut player = self.player.borrow_mut();
        let span = selection
            .filter(|range| range.width() >= MIN_SELECTION_WIDTH)
            .unwrap_or_else(|| SelectionRange::new(0.0, 1.0));
        let start = Self::resume_point(&span, resume_from);
        player.play_range(start, span.end(), looped)
    }

    fn restart_playback(
        &self,
        looped: bool,
        selection: Option<SelectionRange>,
        resume_from: Option<f32>,
    ) {
        let Some(app) = self.app() else { return };
        match self.start_playback(looped, selection, resume_from) {
            Ok(_) => {
                self.set_status(
                    &app,
                    if looped { "Looping selection" } else { "Playing audio" },
                    StatusState::Info,
                );
                self.start_playhead_updates();
            }
            Err(error) => {
                self.set_status(&app, error, StatusState::Error);
                app.set_playhead_visible(false);
            }
        }
    }

    pub(super) fn resume_point(span: &SelectionRange, resume_from: Option<f32>) -> f32 {
        let start = span.start();
        let end = span.end();
        if let Some(position) = resume_from {
            if position >= start && position < end {
                return position;
            }
        }
        start
    }

    pub(super) fn set_loop_enabled(&self, enabled: bool) {
        self.loop_enabled.replace(enabled);
        if let Some(app) = self.app() {
            app.set_loop_enabled(enabled);
        }
    }

    fn restart_loop_if_active(&self, force: bool) {
        if *self.loop_enabled.borrow() && (force || self.player.borrow().is_playing()) {
            let selection = self.usable_selection();
            let restart_from = selection.as_ref().map(|range| range.start());
            self.restart_playback(true, selection, restart_from);
        }
    }

    /// Begin ticking the UI playhead on a timer.
    pub(super) fn start_playhead_updates(&self) {
        self.playhead_timer.stop();
        let timer = self.playhead_timer.clone();
        let app = self.app.clone();
        let player = self.player.clone();
        let timer_for_tick = timer.clone();
        timer.start(
            slint::TimerMode::Repeated,
            Duration::from_millis(30),
            move || Self::tick_playhead(&app, &player, &timer_for_tick),
        );
    }

    fn tick_playhead(
        app_handle: &Rc<RefCell<Option<slint::Weak<HelloWorld>>>>,
        player: &Rc<RefCell<AudioPlayer>>,
        timer: &slint::Timer,
    ) {
        let Some(app) = app_handle.borrow().as_ref().and_then(|a| a.upgrade()) else {
            timer.stop();
            return;
        };
        let mut player = player.borrow_mut();
        let Some(progress) = player.progress() else {
            app.set_playhead_visible(false);
            timer.stop();
            return;
        };
        app.set_playhead_position(progress);
        if !player.is_playing() {
            app.set_playhead_visible(false);
            timer.stop();
            return;
        }
        app.set_playhead_visible(true);
        if progress >= 1.0 {
            player.stop();
            app.set_playhead_visible(false);
            timer.stop();
        }
    }

    fn is_wav(path: &std::path::Path) -> bool {
        path.extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("wav"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resume_point_returns_within_span() {
        let span = SelectionRange::new(0.2, 0.6);
        let position = DropHandler::resume_point(&span, Some(0.4));
        assert_eq!(position, 0.4);
    }

    #[test]
    fn resume_point_out_of_span_defaults_to_start() {
        let span = SelectionRange::new(0.2, 0.6);
        assert_eq!(DropHandler::resume_point(&span, Some(0.9)), 0.2);
        assert_eq!(DropHandler::resume_point(&span, Some(0.1)), 0.2);
    }

    #[test]
    fn resume_point_at_end_defaults_to_start() {
        let span = SelectionRange::new(0.2, 0.6);
        assert_eq!(DropHandler::resume_point(&span, Some(0.6)), 0.2);
    }
}
