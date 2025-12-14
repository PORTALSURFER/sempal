use super::*;
use rand::Rng;
use rand::seq::IteratorRandom;
#[cfg(test)]
use rand::{SeedableRng, rngs::StdRng};

pub(super) fn play_random_visible_sample(controller: &mut EguiController) {
    let mut rng = rand::rng();
    play_random_visible_sample_internal(controller, &mut rng, super::SHOULD_PLAY_RANDOM_SAMPLE);
}

#[cfg(test)]
pub(super) fn play_random_visible_sample_with_seed(controller: &mut EguiController, seed: u64) {
    let mut rng = StdRng::seed_from_u64(seed);
    play_random_visible_sample_internal(controller, &mut rng, false);
}

pub(super) fn focus_random_visible_sample(controller: &mut EguiController) {
    let mut rng = rand::rng();
    play_random_visible_sample_internal(controller, &mut rng, false);
}

pub(super) fn play_previous_random_sample(controller: &mut EguiController) {
    if controller.random_history.entries.is_empty() {
        controller.set_status("No random history yet", StatusTone::Info);
        return;
    }
    let current = controller
        .random_history
        .cursor
        .unwrap_or_else(|| controller.random_history.entries.len().saturating_sub(1));
    if current == 0 {
        controller.random_history.cursor = Some(0);
        controller.set_status("Reached start of random history", StatusTone::Info);
        return;
    }
    let target = current - 1;
    controller.random_history.cursor = Some(target);
    if let Some(entry) = controller.random_history.entries.get(target).cloned() {
        play_random_history_entry(controller, entry);
    }
}

pub(super) fn toggle_random_navigation_mode(controller: &mut EguiController) {
    controller.ui.browser.random_navigation_mode = !controller.ui.browser.random_navigation_mode;
    if controller.ui.browser.random_navigation_mode {
        controller.set_status(
            "Random navigation on: Up/Down jump to random samples",
            StatusTone::Info,
        );
    } else {
        controller.set_status("Random navigation off", StatusTone::Info);
    }
}

pub(super) fn random_navigation_mode_enabled(controller: &EguiController) -> bool {
    controller.ui.browser.random_navigation_mode
}

fn play_random_visible_sample_internal<R: Rng + ?Sized>(
    controller: &mut EguiController,
    rng: &mut R,
    start_playback: bool,
) {
    let Some(source_id) = controller.selection_ctx.selected_source.clone() else {
        controller.set_status("Select a source first", StatusTone::Info);
        return;
    };
    let Some((visible_row, entry_index)) = controller
        .visible_browser_indices()
        .iter()
        .copied()
        .enumerate()
        .choose(rng)
    else {
        controller.set_status("No samples available to randomize", StatusTone::Info);
        return;
    };
    let Some(path) = controller
        .wav_entries
        .get(entry_index)
        .map(|entry| entry.relative_path.clone())
    else {
        return;
    };
    push_random_history(controller, source_id, path.clone());
    controller.focus_browser_row_only(visible_row);
    if start_playback && let Err(err) = controller.play_audio(controller.ui.waveform.loop_enabled, None) {
        controller.set_status(err, StatusTone::Error);
    }
}

fn push_random_history(controller: &mut EguiController, source_id: SourceId, relative_path: PathBuf) {
    if let Some(cursor) = controller.random_history.cursor
        && cursor + 1 < controller.random_history.entries.len()
    {
        controller.random_history.entries.truncate(cursor + 1);
    }
    controller.random_history.entries.push_back(RandomHistoryEntry {
        source_id,
        relative_path,
    });
    if controller.random_history.entries.len() > RANDOM_HISTORY_LIMIT {
        controller.random_history.entries.pop_front();
        if let Some(cursor) = controller.random_history.cursor {
            controller.random_history.cursor = Some(cursor.saturating_sub(1));
        }
    }
    controller.random_history.cursor = Some(controller.random_history.entries.len().saturating_sub(1));
}

fn play_random_history_entry(controller: &mut EguiController, entry: RandomHistoryEntry) {
    if controller.selection_ctx.selected_source.as_ref() != Some(&entry.source_id) {
        controller.jobs.pending_playback = Some(PendingPlayback {
            source_id: entry.source_id.clone(),
            relative_path: entry.relative_path.clone(),
            looped: controller.ui.waveform.loop_enabled,
            start_override: None,
        });
        controller.jobs.pending_select_path = Some(entry.relative_path.clone());
        controller.select_source_internal(Some(entry.source_id), Some(entry.relative_path));
        return;
    }
    if let Some(row) = controller.visible_row_for_path(&entry.relative_path) {
        controller.focus_browser_row_only(row);
    } else {
        controller.select_wav_by_path(&entry.relative_path);
    }
    if let Err(err) = controller.play_audio(controller.ui.waveform.loop_enabled, None) {
        controller.set_status(err, StatusTone::Error);
    }
}
