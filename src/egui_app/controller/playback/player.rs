use super::*;
use std::time::{Duration, Instant};

pub(super) fn play_audio(
    controller: &mut EguiController,
    looped: bool,
    start_override: Option<f32>,
) -> Result<(), String> {
    if controller.is_recording() {
        return Err("Stop recording before playback".into());
    }
    controller.audio.pending_loop_disable_at = None;
    if controller.sample_view.wav.loaded_audio.is_none() {
        if let Some(pending) = controller.runtime.jobs.pending_audio() {
            controller
                .runtime
                .jobs
                .set_pending_playback(Some(PendingPlayback {
                    source_id: pending.source_id,
                    relative_path: pending.relative_path,
                    looped,
                    start_override,
                }));
            controller.set_status("Loading audio…", StatusTone::Busy);
            return Ok(());
        }
        let Some(selected) = controller.sample_view.wav.selected_wav.clone() else {
            return Err("Load a .wav file first".into());
        };
        let Some(source) = controller.current_source() else {
            return Err("Load a .wav file first".into());
        };
        let pending_playback = PendingPlayback {
            source_id: source.id.clone(),
            relative_path: selected.clone(),
            looped,
            start_override,
        };
        controller
            .runtime
            .jobs
            .set_pending_playback(Some(pending_playback.clone()));
        controller.queue_audio_load_for(
            &source,
            &selected,
            AudioLoadIntent::Selection,
            Some(pending_playback),
        )?;
        controller.set_status(format!("Loading {}", selected.display()), StatusTone::Busy);
        return Ok(());
    }
    let player = controller.ensure_player()?;
    let Some(player) = player else {
        return Err("Audio unavailable".into());
    };
    player
        .borrow_mut()
        .set_min_span_seconds(super::bpm_min_selection_seconds(controller));
    let selection = controller
        .selection_state
        .range
        .range()
        .or(controller.ui.waveform.selection)
        .filter(|range| range.width() >= super::selection_min_width(controller));
    let span_end = selection.as_ref().map(|r| r.end()).unwrap_or(1.0);
    let (audition_start, audition_end) = if looped {
        selection
            .as_ref()
            .map(|range| (range.start(), range.end()))
            .unwrap_or((0.0, 1.0))
    } else {
        let span_start = start_override
            .or_else(|| selection.as_ref().map(|range| range.start()))
            .unwrap_or(0.0);
        (span_start, span_end)
    };
    let audition_gain = normalized_audition_gain(controller, audition_start, audition_end);
    player.borrow_mut().set_playback_gain(audition_gain);
    let mut start = 0.0;
    if looped {
        if let Some(range) = selection {
            if let Some(start_pos) = start_override {
                if start_pos >= range.start() && start_pos <= range.end() {
                    start = start_pos;
                    player
                        .borrow_mut()
                        .play_looped_range_from(range.start(), range.end(), start_pos)?;
                } else {
                    start = range.start();
                    player
                        .borrow_mut()
                        .play_range(range.start(), range.end(), true)?;
                }
            } else {
                start = range.start();
                player
                    .borrow_mut()
                    .play_range(range.start(), range.end(), true)?;
            }
        } else if let Some(start_pos) = start_override {
            start = start_pos;
            player.borrow_mut().play_full_wrapped_from(start_pos)?;
        } else {
            player.borrow_mut().play_range(0.0, 1.0, true)?;
        }
    } else {
        start = start_override
            .or_else(|| selection.as_ref().map(|range| range.start()))
            .unwrap_or(0.0);
        player.borrow_mut().play_range(start, span_end, false)?;
    }
    controller.ui.waveform.playhead.active_span_end = Some(span_end.clamp(0.0, 1.0));
    controller.ui.waveform.playhead.visible = true;
    controller.ui.waveform.playhead.position = start;
    super::playhead_trail::start_or_seek_trail(
        &mut controller.ui.waveform.playhead,
        start,
        start_override.is_some(),
    );
    if start_override.is_some() {
        controller.ui.waveform.playhead.recent_seek = Some(crate::egui_app::state::PlayheadSeek {
            position: start,
            started_at: Instant::now(),
        });
    }
    Ok(())
}

fn normalized_audition_gain(controller: &EguiController, start: f32, end: f32) -> f32 {
    if !controller.ui.waveform.normalized_audition_enabled {
        return 1.0;
    }
    let Some(decoded) = controller.sample_view.waveform.decoded.as_ref() else {
        return 1.0;
    };
    let Some(peak) = decoded.max_abs_in_span(start, end) else {
        return 1.0;
    };
    if peak <= f32::EPSILON {
        return 1.0;
    }
    1.0 / peak
}

pub(super) fn is_playing(controller: &EguiController) -> bool {
    controller
        .audio
        .player
        .as_ref()
        .map(|p| p.borrow().is_playing())
        .unwrap_or(false)
}

pub(super) fn tick_playhead(controller: &mut EguiController) {
    controller.poll_background_jobs();
    let Some(player) = controller.audio.player.as_ref().cloned() else {
        if controller.sample_view.waveform.decoded.is_none() {
            hide_waveform_playhead(controller);
        }
        return;
    };
    let should_resume = {
        let player_ref = player.borrow();
        match controller.audio.pending_loop_disable_at {
            Some(_) if !player_ref.is_playing() || !player_ref.is_looping() => {
                controller.audio.pending_loop_disable_at = None;
                false
            }
            Some(deadline) => Instant::now() >= deadline,
            None => false,
        }
    };
    if should_resume {
        controller.audio.pending_loop_disable_at = None;
        player.borrow_mut().stop();
    }
    let player_ref = player.borrow();
    let is_playing = player_ref.is_playing();
    let progress = player_ref.progress();
    let is_looping = player_ref.is_looping();
    drop(player_ref);
    update_playhead_from_progress(controller, progress, is_looping, is_playing);
    if !is_playing && controller.sample_view.waveform.decoded.is_none() {
        hide_waveform_playhead(controller);
    }
}

pub(super) fn update_playhead_from_progress(
    controller: &mut EguiController,
    progress: Option<f32>,
    is_looping: bool,
    is_playing: bool,
) {
    if let Some(progress) = progress {
        let playhead = &mut controller.ui.waveform.playhead;
        let progress = smooth_progress_after_seek(&mut playhead.recent_seek, progress);
        controller.ui.waveform.playhead.position = progress;
        super::playhead_trail::tick_playhead_trail(
            &mut controller.ui.waveform.playhead,
            progress,
            is_looping,
            is_playing,
        );
        if playhead_completed_span(controller, progress, is_looping) {
            hide_waveform_playhead(controller);
        } else {
            controller.ui.waveform.playhead.visible = true;
        }
    } else {
        hide_waveform_playhead(controller);
    }
}

fn playhead_completed_span(controller: &EguiController, progress: f32, is_looping: bool) -> bool {
    if is_looping {
        return false;
    }
    let target = controller
        .ui
        .waveform
        .playhead
        .active_span_end
        .unwrap_or(1.0)
        .clamp(0.0, 1.0);
    progress + super::PLAYHEAD_COMPLETION_EPSILON >= target
}

fn smooth_progress_after_seek(
    recent_seek: &mut Option<crate::egui_app::state::PlayheadSeek>,
    progress: f32,
) -> f32 {
    const SEEK_SMOOTH_SECS: f32 = 0.08;
    const SEEK_CLEAR_SECS: f32 = 0.20;
    const EPS: f32 = 1e-6;

    let progress = progress.clamp(0.0, 1.0);
    let Some(seek) = *recent_seek else {
        return progress;
    };

    let elapsed = seek.started_at.elapsed();
    if elapsed.as_secs_f32() >= SEEK_CLEAR_SECS || progress + EPS < seek.position {
        *recent_seek = None;
        return progress;
    }

    if elapsed.as_secs_f32() >= SEEK_SMOOTH_SECS || progress <= seek.position + EPS {
        return progress;
    }

    let t = (elapsed.as_secs_f32() / SEEK_SMOOTH_SECS).clamp(0.0, 1.0);
    let eased = t * t * (3.0 - 2.0 * t);
    seek.position + (progress - seek.position) * eased
}

pub(super) fn hide_waveform_playhead(controller: &mut EguiController) {
    super::playhead_trail::stash_active_trail(&mut controller.ui.waveform.playhead);
    controller.ui.waveform.playhead.visible = false;
    controller.ui.waveform.playhead.active_span_end = None;
    controller.ui.waveform.playhead.recent_seek = None;
}

#[cfg(test)]
pub(super) fn playhead_completed_span_for_tests(
    controller: &EguiController,
    progress: f32,
    is_looping: bool,
) -> bool {
    playhead_completed_span(controller, progress, is_looping)
}

#[cfg(test)]
pub(super) fn hide_waveform_playhead_for_tests(controller: &mut EguiController) {
    hide_waveform_playhead(controller);
}

#[cfg(test)]
mod tests {
    use super::smooth_progress_after_seek;
    use crate::egui_app::state::PlayheadSeek;
    use std::time::{Duration, Instant};

    #[test]
    fn smooth_playhead_progress_after_seek_starts_at_seek_position() {
        let mut seek = Some(PlayheadSeek {
            position: 0.25,
            started_at: Instant::now(),
        });
        let progress = smooth_progress_after_seek(&mut seek, 0.40);
        assert!(progress >= 0.25);
        assert!(progress <= 0.40);
    }

    #[test]
    fn smooth_playhead_progress_after_seek_clears_after_timeout() {
        let mut seek = Some(PlayheadSeek {
            position: 0.25,
            started_at: Instant::now() - Duration::from_millis(500),
        });
        let progress = smooth_progress_after_seek(&mut seek, 0.40);
        assert_eq!(progress, 0.40);
        assert!(seek.is_none());
    }
}

pub(in crate::egui_app::controller) fn apply_selection(
    controller: &mut EguiController,
    range: Option<SelectionRange>,
) {
    let label = range.and_then(|selection| selection_duration_label(controller, selection));
    controller.ui.waveform.selection = range;
    controller.ui.waveform.selection_duration = label;
}

pub(super) fn update_waveform_hover_time(controller: &mut EguiController, position: Option<f32>) {
    if let (Some(position), Some(audio)) =
        (position, controller.sample_view.wav.loaded_audio.as_ref())
    {
        let clamped = position.clamp(0.0, 1.0);
        let seconds = audio.duration_seconds * clamped;
        controller.ui.waveform.hover_time_label = Some(format_timestamp_hms_ms(seconds));
    } else {
        controller.ui.waveform.hover_time_label = None;
    }
}

pub(super) fn selection_duration_label(
    controller: &EguiController,
    range: SelectionRange,
) -> Option<String> {
    let audio = controller.sample_view.wav.loaded_audio.as_ref()?;
    let seconds = (audio.duration_seconds * range.width()).max(0.0);
    Some(format_selection_duration(seconds))
}

pub(in crate::egui_app::controller) fn apply_volume(controller: &mut EguiController, volume: f32) {
    let clamped = volume.clamp(0.0, 1.0);
    controller.ui.volume = clamped;
    if let Some(player) = controller.audio.player.as_ref() {
        player.borrow_mut().set_volume(clamped);
    }
}

pub(in crate::egui_app::controller) fn ensure_player(
    controller: &mut EguiController,
) -> Result<Option<Rc<RefCell<AudioPlayer>>>, String> {
    if controller.audio.player.is_none() {
        let mut created = AudioPlayer::from_config(&controller.settings.audio_output)
            .map_err(|err| format!("Audio init failed: {err}"))?;
        created.set_volume(controller.ui.volume);
        created.set_anti_clip_settings(
            controller.settings.controls.anti_clip_fade_enabled,
            controller.settings.controls.anti_clip_fade_ms,
        );
        controller.audio.player = Some(Rc::new(RefCell::new(created)));
        controller.update_audio_output_status();
    }
    Ok(controller.audio.player.clone())
}

pub(super) fn defer_loop_disable_after_cycle(
    controller: &mut EguiController,
) -> Result<(), String> {
    controller.audio.pending_loop_disable_at = None;
    let Some(player_rc) = controller.ensure_player()? else {
        return Ok(());
    };
    let player_ref = player_rc.borrow();
    let remaining = player_ref.remaining_loop_duration();
    let is_playing = player_ref.is_playing();
    let is_looping = player_ref.is_looping();
    drop(player_ref);

    if !is_playing || !is_looping {
        return Ok(());
    }

    let Some(remaining) = remaining else {
        player_rc.borrow_mut().stop();
        return Ok(());
    };
    if remaining <= Duration::from_millis(5) {
        player_rc.borrow_mut().stop();
        return Ok(());
    }

    controller.audio.pending_loop_disable_at = Some(Instant::now() + remaining);
    Ok(())
}
