use radiant::prelude as ui;

use crate::native_app::app::{GuiMessage, NativeAppState};

impl NativeAppState {
    pub(super) fn apply_playback_dispatch(
        &mut self,
        message: GuiMessage,
        context: &mut ui::UiUpdateContext<GuiMessage>,
    ) {
        match message {
            GuiMessage::AudioOptionsRefreshFinished(completion) => {
                self.finish_audio_options_refresh(completion)
            }
            GuiMessage::AudioOutputPersisted(completion) => {
                self.finish_audio_output_persist(completion)
            }
            GuiMessage::AudioPlayerOpenFinished(completion) => {
                self.finish_audio_player_open(completion, context)
            }
            GuiMessage::PlaySelectedSample => self.play_selected_sample(context),
            GuiMessage::PlayFromCurrentPlayStart => self.play_from_current_play_start(context),
            GuiMessage::PlayRandomSampleRange => self.play_random_sample_range(context),
            GuiMessage::PlayRandomListedSampleRange => {
                self.play_random_listed_sample_range(context);
            }
            GuiMessage::PlayPreviousPlaybackHistory => {
                self.play_previous_playback_history(context);
            }
            GuiMessage::PlayNextPlaybackHistory => {
                self.play_next_playback_history(context);
            }
            GuiMessage::LastPlayedPersistReady { ticket, request } => {
                self.start_last_played_persist(ticket, request, context);
            }
            GuiMessage::LastPlayedPersisted(result) => self.finish_last_played_persist(result),
            GuiMessage::HarvestSeenPersisted(result) => self.finish_harvest_seen_persist(result),
            GuiMessage::VolumeSettingsPersisted(result) => {
                self.finish_volume_settings_persist(result)
            }
            GuiMessage::StopPlayback => {
                self.stop_playback();
                self.flush_deferred_last_played_persist_if_idle(context);
            }
            GuiMessage::ToggleLoopPlayback => self.toggle_loop_playback(),
            GuiMessage::ToggleMetronome => self.toggle_metronome(),
            _ => unreachable!("playback dispatcher received a non-playback message"),
        }
    }
}
