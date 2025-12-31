use super::*;
use tracing::{debug, warn};

pub(super) fn select_wav_by_path(controller: &mut EguiController, path: &Path) {
    select_wav_by_path_with_rebuild(controller, path, true);
}

pub(super) fn select_wav_by_path_with_rebuild(
    controller: &mut EguiController,
    path: &Path,
    rebuild: bool,
) {
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
        if rebuild {
            controller.rebuild_browser_lists();
        }
        return;
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

pub(super) fn select_wav_by_index(controller: &mut EguiController, index: usize) {
    let path = match controller.wav_entry(index) {
        Some(entry) => entry.relative_path.clone(),
        None => return,
    };
    select_wav_by_path(controller, &path);
}

pub(super) fn select_from_browser(controller: &mut EguiController, path: &Path) {
    controller.ui.collections.selected_sample = None;
    controller.focus_browser_context();
    select_wav_by_path(controller, path);
}

pub(super) fn triage_flag_drop_target(controller: &EguiController) -> TriageFlagColumn {
    match controller.ui.browser.filter {
        TriageFlagFilter::All | TriageFlagFilter::Untagged => TriageFlagColumn::Neutral,
        TriageFlagFilter::Keep => TriageFlagColumn::Keep,
        TriageFlagFilter::Trash => TriageFlagColumn::Trash,
    }
}

pub(super) fn selected_tag(controller: &mut EguiController) -> Option<SampleTag> {
    controller
        .selected_row_index()
        .and_then(|idx| controller.wav_entry(idx))
        .map(|entry| entry.tag)
}

pub(super) fn rebuild_wav_lookup(controller: &mut EguiController) {
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

#[allow(dead_code)]
pub(super) fn sync_browser_after_wav_entries_mutation(
    controller: &mut EguiController,
    source_id: &SourceId,
) {
    rebuild_wav_lookup(controller);
    controller.ui.browser.similar_query = None;
    controller.ui_cache.browser.search.invalidate();
    controller.rebuild_browser_lists();
    controller.ui_cache.browser.labels.remove(source_id);
}

#[allow(dead_code)]
pub(super) fn sync_browser_after_wav_entries_mutation_keep_search_cache(
    controller: &mut EguiController,
    source_id: &SourceId,
) {
    rebuild_wav_lookup(controller);
    controller.ui.browser.similar_query = None;
    controller.rebuild_browser_lists();
    controller.ui_cache.browser.labels.remove(source_id);
}

pub(super) fn invalidate_cached_audio_for_entry_updates(
    controller: &mut EguiController,
    source_id: &SourceId,
    updates: &[(WavEntry, WavEntry)],
) {
    for (old_entry, new_entry) in updates {
        controller.invalidate_cached_audio(source_id, &old_entry.relative_path);
        controller.invalidate_cached_audio(source_id, &new_entry.relative_path);
    }
}

pub(super) fn set_sample_tag(
    controller: &mut EguiController,
    path: &Path,
    column: TriageFlagColumn,
) -> Result<(), String> {
    let target_tag = match column {
        TriageFlagColumn::Trash => SampleTag::Trash,
        TriageFlagColumn::Neutral => SampleTag::Neutral,
        TriageFlagColumn::Keep => SampleTag::Keep,
    };
    set_sample_tag_value(controller, path, target_tag)
}

pub(super) fn set_sample_tag_value(
    controller: &mut EguiController,
    path: &Path,
    target_tag: SampleTag,
) -> Result<(), String> {
    let Some(source) = controller.current_source() else {
        return Err("Select a source first".into());
    };
    set_sample_tag_for_source(controller, &source, path, target_tag, true)
}

pub(super) fn set_sample_tag_for_source(
    controller: &mut EguiController,
    source: &SampleSource,
    path: &Path,
    target_tag: SampleTag,
    require_present: bool,
) -> Result<(), String> {
    let db = controller
        .database_for(source)
        .map_err(|err| {
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
