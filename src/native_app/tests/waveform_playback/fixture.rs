use super::*;

pub(super) struct WaveformPlaybackScenario {
    pub(super) state: NativeAppState,
    context: ui::UiUpdateContext<crate::native_app::test_support::state::GuiMessage>,
    _source_root: Option<tempfile::TempDir>,
    selected_file: Option<String>,
}

impl WaveformPlaybackScenario {
    pub(super) fn synthetic() -> Self {
        Self {
            state: gui_state_for_span_tests(),
            context: ui::UiUpdateContext::default(),
            _source_root: None,
            selected_file: None,
        }
    }

    pub(super) fn default_loaded_with_player() -> Option<Self> {
        Self::loaded_with_player("loaded-playback.wav", &[0, 1024, -2048, 4096, -1024, 512])
    }

    pub(super) fn retarget_loaded_with_player() -> Option<Self> {
        Self::loaded_with_player("retarget-playback.wav", &[0; 48_000])
    }

    pub(super) fn loaded_with_player(name: &str, samples: &[i16]) -> Option<Self> {
        let (mut state, source_root, selected_file) = native_app_state_with_temp_sample(name);
        write_test_wav_i16(&PathBuf::from(&selected_file), samples);
        if !install_playback_runtime_for_tests(&mut state) {
            return None;
        }
        state.waveform.current = crate::native_app::test_support::state::WaveformState::load_path(
            PathBuf::from(&selected_file),
        )
        .expect("test sample loads");
        Some(Self {
            state,
            context: ui::UiUpdateContext::default(),
            _source_root: Some(source_root),
            selected_file: Some(selected_file),
        })
    }

    pub(super) fn with_temp_wav(name: &str, samples: &[i16]) -> Self {
        let (mut state, source_root, selected_file) = native_app_state_with_temp_sample(name);
        let path = PathBuf::from(&selected_file);
        write_test_wav_i16(&path, samples);
        state
            .library
            .folder_browser
            .select_file(selected_file.clone());
        Self {
            state,
            context: ui::UiUpdateContext::default(),
            _source_root: Some(source_root),
            selected_file: Some(selected_file),
        }
    }

    pub(super) fn with_looping(mut self) -> Self {
        self.state.audio.loop_playback = true;
        self
    }

    pub(super) fn with_unloaded_waveform(mut self) -> Self {
        self.state.waveform.current =
            crate::native_app::test_support::state::WaveformState::empty();
        self
    }

    pub(super) fn select_play_range(&mut self, start: f32, end: f32) {
        self.begin_play_range(start);
        self.update_play_range_drag(end);
        self.finish_play_range_drag(end);
    }

    pub(super) fn flush_background_work(&mut self) {
        let context = std::mem::take(&mut self.context);
        run_command_for_tests(&mut self.state, context.into_command());
    }

    pub(super) fn begin_play_range(&mut self, start: f32) {
        self.apply_waveform(WaveformInteraction::BeginSelection {
            kind: WaveformSelectionKind::Play,
            visible_ratio: start,
        });
    }

    pub(super) fn begin_play_range_start_resize(&mut self, from: f32) {
        self.apply_waveform(WaveformInteraction::BeginSelectionResize {
            kind: WaveformSelectionKind::Play,
            edge: WaveformSelectionEdge::Start,
            visible_ratio: from,
        });
    }

    pub(super) fn begin_play_range_end_resize(&mut self, from: f32) {
        self.apply_waveform(WaveformInteraction::BeginSelectionResize {
            kind: WaveformSelectionKind::Play,
            edge: WaveformSelectionEdge::End,
            visible_ratio: from,
        });
    }

    pub(super) fn update_play_range_drag(&mut self, to: f32) {
        self.apply_waveform(WaveformInteraction::UpdateSelection { visible_ratio: to });
    }

    pub(super) fn finish_play_range_drag(&mut self, to: f32) {
        self.apply_waveform(WaveformInteraction::FinishSelection { visible_ratio: to });
    }

    pub(super) fn flush_playmark_retarget(&mut self) {
        self.state.flush_pending_play_selection_playback_retarget();
    }

    pub(super) fn set_retarget_playhead_ratio(&mut self, ratio: f32) {
        self.state.waveform.current.set_playhead_ratio(ratio);
        self.state.audio.playback_progress = wavecrate::audio::PlaybackRuntimeProgress {
            active: true,
            elapsed: Some(std::time::Duration::ZERO),
            looping: self.state.audio.loop_playback,
            progress: Some(ratio),
            error: None,
        };
        self.state
            .audio
            .reset_playback_visual_progress(ratio, self.state.audio.loop_playback);
    }

    pub(super) fn settle_playmark_retarget(&mut self) {
        std::thread::sleep(std::time::Duration::from_millis(10));
        self.apply_playback_frame();
    }

    pub(super) fn apply_playback_frame(&mut self) {
        self.state.drain_playback_runtime_events();
        self.state.refresh_playback_progress();
        self.state.drain_playback_runtime_events();
    }

    pub(super) fn play_random_range_with_units(&mut self, start_unit: f32, length_unit: f32) {
        self.state.play_random_sample_range_with_units(
            crate::native_app::audio::playback::RandomAuditionUnits::new(start_unit, length_unit),
            &mut self.context,
        );
    }

    pub(super) fn play_selected_sample(&mut self) {
        self.state.play_selected_sample(&mut self.context);
    }

    pub(super) fn start_full_sample_loop(&mut self) {
        self.state.audio.loop_playback = true;
        self.state
            .start_playback_current_span(0.0, 1.0)
            .expect("full sample loop starts");
    }

    pub(super) fn start_deferred_load(&mut self, autoplay: bool) {
        let context = std::mem::take(&mut self.context);
        run_command_for_tests(&mut self.state, context.into_command());
        let selected_file = self
            .selected_file
            .clone()
            .expect("scenario should have a selected temp sample");
        start_deferred_sample_load_for_tests(
            &mut self.state,
            selected_file,
            autoplay,
            &mut self.context,
        );
    }

    pub(super) fn finish_deferred_load(&mut self, autoplay: bool) {
        let selected_file = self
            .selected_file
            .clone()
            .expect("scenario should have a selected temp sample");
        let ticket = active_sample_load_ticket(&self.state).expect("sample load queued");
        self.state.apply_message(
            crate::native_app::test_support::state::GuiMessage::SampleLoadFinished(
                sample_load_completion(
                    ticket,
                    selected_file.clone(),
                    crate::native_app::test_support::state::WaveformState::load_path(
                        PathBuf::from(&selected_file),
                    ),
                    autoplay,
                ),
            ),
            &mut self.context,
        );
    }

    fn apply_waveform(&mut self, interaction: WaveformInteraction) {
        self.state.apply_message(
            crate::native_app::test_support::state::GuiMessage::Waveform(interaction),
            &mut self.context,
        );
    }
}
