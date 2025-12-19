use super::*;

pub(super) fn select_wav_by_path(controller: &mut EguiController, path: &Path) {
    select_wav_by_path_with_rebuild(controller, path, true);
}

pub(super) fn select_wav_by_path_with_rebuild(
    controller: &mut EguiController,
    path: &Path,
    rebuild: bool,
) {
    if !controller.wav_entries.lookup.contains_key(path) {
        return;
    }
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
    if path_changed {
        controller.queue_prediction_load_for_selection();
    }
    let missing = controller
        .wav_entries
        .lookup
        .get(path)
        .and_then(|index| controller.wav_entries.entries.get(*index))
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
    let path = match controller.wav_entries.entries.get(index) {
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

pub(super) fn selected_tag(controller: &EguiController) -> Option<SampleTag> {
    controller
        .selected_row_index()
        .and_then(|idx| controller.wav_entries.entries.get(idx))
        .map(|entry| entry.tag)
}

pub(super) fn rebuild_wav_lookup(controller: &mut EguiController) {
    controller.wav_entries.lookup.clear();
    for (index, entry) in controller.wav_entries.entries.iter().enumerate() {
        let path = entry.relative_path.clone();
        controller.wav_entries.lookup.insert(path.clone(), index);

        let path_str = path.to_string_lossy();
        if path_str.contains('\\') {
            let normalized = path_str.replace('\\', "/");
            controller
                .wav_entries
                .lookup
                .entry(PathBuf::from(normalized))
                .or_insert(index);
        }
        if path_str.contains('/') {
            let normalized = path_str.replace('/', "\\");
            controller
                .wav_entries
                .lookup
                .entry(PathBuf::from(normalized))
                .or_insert(index);
        }
    }
}

pub(super) fn sync_browser_after_wav_entries_mutation(
    controller: &mut EguiController,
    source_id: &SourceId,
) {
    rebuild_wav_lookup(controller);
    controller.ui.browser.similar_query = None;
    controller.ui_cache.browser.search.invalidate();
    controller.rebuild_browser_lists();
    controller.ui_cache.browser.labels.insert(
        source_id.clone(),
        controller.build_label_cache(&controller.wav_entries.entries),
    );
}

pub(super) fn sync_browser_after_wav_entries_mutation_keep_search_cache(
    controller: &mut EguiController,
    source_id: &SourceId,
) {
    rebuild_wav_lookup(controller);
    controller.ui.browser.similar_query = None;
    controller.rebuild_browser_lists();
    controller.ui_cache.browser.labels.insert(
        source_id.clone(),
        controller.build_label_cache(&controller.wav_entries.entries),
    );
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

pub(super) fn ensure_wav_cache_lookup(controller: &mut EguiController, source_id: &SourceId) {
    controller.cache.wav.ensure_lookup(source_id);
}

pub(super) fn rebuild_wav_cache_lookup(controller: &mut EguiController, source_id: &SourceId) {
    controller.cache.wav.rebuild_lookup(source_id);
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
        .map_err(|err| err.to_string())?;
    let mut tagging = tagging_service::TaggingService::new(
        controller.selection_state.ctx.selected_source.as_ref(),
        &mut controller.wav_entries.entries,
        &controller.wav_entries.lookup,
        &mut controller.cache.wav,
    );
    tagging.apply_sample_tag(source, path, target_tag, require_present)?;
    let _ = db.set_tag(path, target_tag);
    if controller.selection_state.ctx.selected_source.as_ref() == Some(&source.id) {
        controller.rebuild_browser_lists();
    }
    Ok(())
}
