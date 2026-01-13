use super::*;
use crate::egui_app::controller::library::analysis_jobs;
use tracing::{debug, warn};
use crate::sample_sources::Rating;

pub(crate) fn select_wav_by_path(controller: &mut EguiController, path: &Path) {
    select_wav_by_path_with_rebuild(controller, path, true);
}

pub(crate) fn select_wav_by_path_with_rebuild(
    controller: &mut EguiController,
    path: &Path,
    rebuild: bool,
) {
    if controller
        .audio
        .pending_age_update
        .as_ref()
        .is_some_and(|u| u.relative_path != path)
    {
        controller.commit_pending_age_update();
    }
    let Some(index) = controller.wav_index_for_path(path) else {
        return;
    };
    controller.ui.collections.selected_sample = None;
    if controller.current_source().is_none() {
        if let Some(source_id) = controller
            .selection_state
            .ctx
            .last_selected_browsable_source
            .clone()
            .filter(|id| controller.library.sources.iter().any(|s| &s.id == id))
        {
            controller.selection_state.ctx.selected_source = Some(source_id);
            controller.refresh_sources_ui();
        } else if let Some(first) = controller.library.sources.first().cloned() {
            controller
                .selection_state
                .ctx
                .last_selected_browsable_source = Some(first.id.clone());
            controller.selection_state.ctx.selected_source = Some(first.id);
            controller.refresh_sources_ui();
        }
    }
    let path_changed = controller.sample_view.wav.selected_wav.as_deref() != Some(path);
    if path_changed {
        controller.ui.waveform.last_start_marker = None;
    }
    controller.sample_view.wav.selected_wav = Some(path.to_path_buf());
    controller.ui.browser.last_focused_path = Some(path.to_path_buf());
    if path_changed {
        controller.record_focus_history(path);
        controller.clear_focused_similarity_highlight();
    }
    let missing = controller
        .wav_entries
        .entry(index)
        .map(|entry| entry.missing)
        .unwrap_or(false);
    if missing {
        controller.show_missing_waveform_notice(path);
        controller.set_status(
            format!("File missing: {}", path.display()),
            StatusTone::Warning,
        );
        controller.selection_state.suppress_autoplay_once = false;
        controller.clear_focused_similarity_highlight();
        if rebuild {
            controller.rebuild_browser_lists();
        }
        return;
    }
    if path_changed {
        if let Some(source) = controller.current_source() {
            let sample_id = analysis_jobs::build_sample_id(source.id.as_str(), path);
            controller.refresh_focused_similarity_highlight(&sample_id, Some(index));
        } else {
            controller.clear_focused_similarity_highlight();
        }
    }
    if let Some(source) = controller.current_source() {
        let autoplay = controller.settings.feature_flags.autoplay_selection
            && !controller.selection_state.suppress_autoplay_once;
        controller.selection_state.suppress_autoplay_once = false;
        let pending_playback = if autoplay {
            Some(PendingPlayback {
                source_id: source.id.clone(),
                relative_path: path.to_path_buf(),
                looped: controller.ui.waveform.loop_enabled,
                start_override: None,
            })
        } else {
            None
        };
        if let Err(err) = controller.queue_audio_load_for(
            &source,
            path,
            AudioLoadIntent::Selection,
            pending_playback,
        ) {
            controller.set_status(err, StatusTone::Error);
        }
    } else {
        controller.selection_state.suppress_autoplay_once = false;
    }
    if rebuild {
        controller.rebuild_browser_lists();
    }
}

pub(crate) fn select_wav_by_index(controller: &mut EguiController, index: usize) {
    let path = match controller.wav_entry(index) {
        Some(entry) => entry.relative_path.clone(),
        None => return,
    };
    select_wav_by_path(controller, &path);
}

pub(crate) fn select_from_browser(controller: &mut EguiController, path: &Path) {
    controller.ui.collections.selected_sample = None;
    controller.focus_browser_context();
    select_wav_by_path(controller, path);
}

pub(crate) fn triage_flag_drop_target(controller: &EguiController) -> TriageFlagColumn {
    match controller.ui.browser.filter {
        TriageFlagFilter::All | TriageFlagFilter::Untagged => TriageFlagColumn::Neutral,
        TriageFlagFilter::Keep => TriageFlagColumn::Keep,
        TriageFlagFilter::Trash => TriageFlagColumn::Trash,
    }
}

pub(crate) fn selected_tag(controller: &mut EguiController) -> Option<Rating> {
    controller
        .selected_row_index()
        .and_then(|idx| controller.wav_entry(idx))
        .map(|entry| entry.tag)
}

pub(crate) fn rebuild_wav_lookup(controller: &mut EguiController) {
    controller.wav_entries.lookup.clear();
    let mut entries = Vec::new();
    for (page_index, page) in controller.wav_entries.pages.iter() {
        let base = page_index * controller.wav_entries.page_size;
        for (idx, entry) in page.iter().enumerate() {
            entries.push((entry.relative_path.clone(), base + idx));
        }
    }
    for (path, index) in entries {
        controller.wav_entries.insert_lookup(path, index);
    }
}

pub(crate) fn invalidate_cached_audio_for_entry_updates(
    controller: &mut EguiController,
    source_id: &SourceId,
    updates: &[(WavEntry, WavEntry)],
) {
    for (old_entry, new_entry) in updates {
        controller.invalidate_cached_audio(source_id, &old_entry.relative_path);
        controller.invalidate_cached_audio(source_id, &new_entry.relative_path);
    }
}

pub(crate) fn set_sample_tag(
    controller: &mut EguiController,
    path: &Path,
    column: TriageFlagColumn,
) -> Result<(), String> {
    let target_tag = match column {
        TriageFlagColumn::Trash => Rating::TRASH_3,
        TriageFlagColumn::Neutral => Rating::NEUTRAL,
        TriageFlagColumn::Keep => Rating::KEEP_1,
    };
    set_sample_tag_value(controller, path, target_tag)
}

pub(crate) fn set_sample_tag_value(
    controller: &mut EguiController,
    path: &Path,
    target_tag: Rating,
) -> Result<(), String> {
    let Some(source) = controller.current_source() else {
        return Err("Select a source first".into());
    };
    set_sample_tag_for_source(controller, &source, path, target_tag, true)
}

pub(crate) fn set_sample_tag_for_source(
    controller: &mut EguiController,
    source: &SampleSource,
    path: &Path,
    target_tag: Rating,
    require_present: bool,
) -> Result<(), String> {
    let db = controller.database_for(source).map_err(|err| {
        warn!(source_id = %source.id, error = %err, "triage tag: database unavailable");
        err.to_string()
    })?;
    if require_present {
        let exists = db
            .index_for_path(path)
            .map_err(|err| {
                warn!(
                    source_id = %source.id,
                    path = %path.display(),
                    error = %err,
                    "triage tag: index lookup failed"
                );
                err.to_string()
            })?
            .is_some();
        if !exists {
            warn!(
                source_id = %source.id,
                path = %path.display(),
                "triage tag: sample missing in db"
            );
            return Err("Sample not found".into());
        }
    }
    if let Err(err) = db.set_tag(path, target_tag) {
        warn!(
            source_id = %source.id,
            path = %path.display(),
            error = %err,
            "triage tag: db set_tag failed"
        );
    } else {
        debug!(
            source_id = %source.id,
            path = %path.display(),
            ?target_tag,
            "triage tag: db updated"
        );
    }
    let mut updated_active = false;
    if let Some(index) = controller.wav_index_for_path(path) {
        let _ = controller.ensure_wav_page_loaded(index);
        if let Some(entry) = controller.wav_entries.entry_mut(index) {
            entry.tag = target_tag;
            updated_active = true;
        }
    }
    if let Some(cache) = controller.cache.wav.entries.get_mut(&source.id)
        && let Some(index) = cache.lookup.get(path).copied()
        && let Some(entry) = cache.entry_mut(index)
    {
        entry.tag = target_tag;
    }
    if updated_active {
        debug!(
            source_id = %source.id,
            path = %path.display(),
            "triage tag: rebuilding browser list"
        );
        controller.rebuild_browser_lists();
    }
    Ok(())
}

pub(crate) fn set_sample_looped_for_source(
    controller: &mut EguiController,
    source: &SampleSource,
    path: &Path,
    looped: bool,
    require_present: bool,
) -> Result<(), String> {
    let db = controller.database_for(source).map_err(|err| {
        warn!(source_id = %source.id, error = %err, "loop marker: database unavailable");
        err.to_string()
    })?;
    if require_present {
        let exists = db
            .index_for_path(path)
            .map_err(|err| {
                warn!(
                    source_id = %source.id,
                    path = %path.display(),
                    error = %err,
                    "loop marker: index lookup failed"
                );
                err.to_string()
            })?
            .is_some();
        if !exists {
            warn!(
                source_id = %source.id,
                path = %path.display(),
                "loop marker: sample missing in db"
            );
            return Err("Sample not found".into());
        }
    }
    if let Err(err) = db.set_looped(path, looped) {
        warn!(
            source_id = %source.id,
            path = %path.display(),
            error = %err,
            "loop marker: db set_looped failed"
        );
    } else {
        debug!(
            source_id = %source.id,
            path = %path.display(),
            looped,
            "loop marker: db updated"
        );
    }
    if let Some(index) = controller.wav_index_for_path(path) {
        let _ = controller.ensure_wav_page_loaded(index);
        if let Some(entry) = controller.wav_entries.entry_mut(index) {
            entry.looped = looped;
        }
    }
    if let Some(cache) = controller.cache.wav.entries.get_mut(&source.id)
        && let Some(index) = cache.lookup.get(path).copied()
        && let Some(entry) = cache.entry_mut(index)
    {
        entry.looped = looped;
    }
    Ok(())
}
