use super::*;

pub(super) fn tag_selected(controller: &mut EguiController, target: SampleTag) {
    let Some(selected_index) = controller.selected_row_index() else {
        return;
    };
    let refocus_path = controller
        .wav_entry(selected_index)
        .map(|entry| entry.relative_path.clone());
    let primary_row = match refocus_path
        .as_deref()
        .and_then(|path| controller.visible_row_for_path(path))
    {
        Some(row) => row,
        None => return,
    };
    let rows = controller.action_rows_from_primary(primary_row);
    controller.ui.collections.selected_sample = None;
    controller.focus_browser_context();
    controller.ui.browser.autoscroll = true;
    let mut last_error = None;
    let mut applied: Vec<(SourceId, PathBuf, SampleTag)> = Vec::new();
    let mut contexts = Vec::with_capacity(rows.len());
    let mut seen = std::collections::HashSet::new();
    for row in rows {
        match controller.resolve_browser_sample(row) {
            Ok(ctx) => {
                if seen.insert(ctx.entry.relative_path.clone()) {
                    contexts.push(ctx);
                }
            }
            Err(err) => last_error = Some(err),
        }
    }
    for ctx in contexts {
        let before = (
            ctx.source.id.clone(),
            ctx.entry.relative_path.clone(),
            ctx.entry.tag,
        );
        match controller.set_sample_tag_for_source(
            &ctx.source,
            &ctx.entry.relative_path,
            target,
            true,
        ) {
            Ok(()) => applied.push(before),
            Err(err) => last_error = Some(err),
        }
    }
    if !applied.is_empty() {
        let label = match target {
            SampleTag::Keep => "Tag keep",
            SampleTag::Trash => "Tag trash",
            SampleTag::Neutral => "Tag neutral",
        };
        let redo_updates: Vec<(SourceId, PathBuf, SampleTag)> = applied
            .iter()
            .map(|(source_id, path, _)| (source_id.clone(), path.clone(), target))
            .collect();
        let refocus_path_undo = refocus_path.clone();
        controller.push_undo_entry(super::undo::UndoEntry::<EguiController>::new(
            label,
            move |controller: &mut EguiController| {
                for (source_id, path, tag) in applied.iter() {
                    let source = controller
                        .library
                        .sources
                        .iter()
                        .find(|s| &s.id == source_id)
                        .cloned()
                        .ok_or_else(|| "Source not available".to_string())?;
                    controller.set_sample_tag_for_source(&source, path, *tag, false)?;
                }
                if let Some(path) = refocus_path_undo.as_deref() {
                    controller.selection_state.suppress_autoplay_once = true;
                    if let Some(row) = controller.visible_row_for_path(path) {
                        controller.focus_browser_row_only(row);
                    }
                }
                Ok(())
            },
            move |controller: &mut EguiController| {
                for (source_id, path, tag) in redo_updates.iter() {
                    let source = controller
                        .library
                        .sources
                        .iter()
                        .find(|s| &s.id == source_id)
                        .cloned()
                        .ok_or_else(|| "Source not available".to_string())?;
                    controller.set_sample_tag_for_source(&source, path, *tag, false)?;
                }
                Ok(())
            },
        ));
    }
    controller.refocus_after_filtered_removal(primary_row);
    if let Some(err) = last_error {
        controller.set_status(err, StatusTone::Error);
    }
}

pub(super) fn move_selection_column(controller: &mut EguiController, delta: isize) {
    use crate::egui_app::state::TriageFlagFilter::*;
    let filters = [All, Keep, Trash, Untagged];
    let current = controller.ui.browser.filter;
    let current_idx = filters.iter().position(|f| f == &current).unwrap_or(0) as isize;
    let target_idx = (current_idx + delta).clamp(0, (filters.len() as isize) - 1) as usize;
    let target = filters[target_idx];
    controller.set_browser_filter(target);
}

pub(super) fn tag_selected_left(controller: &mut EguiController) {
    let target = match controller.selected_tag() {
        Some(SampleTag::Keep) => SampleTag::Neutral,
        _ => SampleTag::Trash,
    };
    controller.tag_selected(target);
}
