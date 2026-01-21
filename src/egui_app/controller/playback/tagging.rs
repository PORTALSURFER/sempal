use super::*;

pub(crate) fn tag_selected(controller: &mut EguiController, target: crate::sample_sources::Rating) {
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
    controller.focus_browser_context();
    controller.ui.browser.autoscroll = true;
    let mut last_error = None;
    let mut applied: Vec<(SourceId, PathBuf, crate::sample_sources::Rating)> = Vec::new();
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
        let label = if target == crate::sample_sources::Rating::KEEP_1 {
            "Tag keep"
        } else if target == crate::sample_sources::Rating::TRASH_3 {
            "Tag trash"
        } else if target == crate::sample_sources::Rating::NEUTRAL {
            "Tag neutral"
        } else {
            "Tag sample"
        };
        let redo_updates: Vec<(SourceId, PathBuf, crate::sample_sources::Rating)> = applied
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
                Ok(super::undo::UndoExecution::Applied)
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
                Ok(super::undo::UndoExecution::Applied)
            },
        ));
    }
    controller.refocus_after_filtered_removal(primary_row);
    if let Some(err) = last_error {
        controller.set_status(err, StatusTone::Error);
    }

    if controller.settings.controls.advance_after_rating
        && controller.ui.browser.selected_visible == Some(primary_row)
    {
        if controller.random_navigation_mode_enabled() {
            controller.focus_random_visible_sample();
        } else {
            let next_row = primary_row + 1;
            if next_row < controller.ui.browser.visible.len() {
                controller.focus_browser_row(next_row);
            }
        }
    }
}

pub(crate) fn move_selection_column(controller: &mut EguiController, delta: isize) {
    use crate::egui_app::state::TriageFlagFilter::*;
    let filters = [All, Keep, Trash, Untagged];
    let current = controller.ui.browser.filter;
    let current_idx = filters.iter().position(|f| f == &current).unwrap_or(0) as isize;
    let target_idx = (current_idx + delta).clamp(0, (filters.len() as isize) - 1) as usize;
    let target = filters[target_idx];
    controller.set_browser_filter(target);
}

pub(crate) fn tag_selected_left(controller: &mut EguiController) {
    let target = match controller.selected_tag() {
        Some(tag) if tag.is_keep() => crate::sample_sources::Rating::NEUTRAL,
        _ => crate::sample_sources::Rating::TRASH_3,
    };
    controller.tag_selected(target);
}

pub(crate) fn adjust_selected_rating(controller: &mut EguiController, delta: i8) {
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
    controller.focus_browser_context();
    controller.ui.browser.autoscroll = true;
    let mut last_error = None;

    // Use a HashMap to store previous values to allow per-item untagging if needed
    // However, the standard pattern in tagging.rs is to store (SourceId, Path, Rating).
    let mut previous_values: Vec<(SourceId, PathBuf, crate::sample_sources::Rating)> = Vec::new();
    let mut applied_updates: Vec<(SourceId, PathBuf, crate::sample_sources::Rating)> = Vec::new();
    
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
        let current_rating = ctx.entry.tag;
        let mut new_val = current_rating.val() + delta;

        // If we were rated and hit 0, skip it
        if current_rating.val() != 0 && new_val == 0 {
            new_val += delta;
        }

        let new_val = new_val.clamp(-3, 3);
        let target = crate::sample_sources::Rating::new(new_val);

        if target != current_rating {
             let before = (
                ctx.source.id.clone(),
                ctx.entry.relative_path.clone(),
                current_rating,
            );
            
            match controller.set_sample_tag_for_source(
                &ctx.source,
                &ctx.entry.relative_path,
                target,
                true,
            ) {
                Ok(()) => {
                    previous_values.push(before);
                    applied_updates.push((ctx.source.id.clone(), ctx.entry.relative_path.clone(), target));
                }
                Err(err) => last_error = Some(err),
            }
        }
    }

    if !applied_updates.is_empty() {
        let label = if delta > 0 { "Increase rating" } else { "Decrease rating" };
        
        // Capture for closures
        let redo_updates = applied_updates.clone();
        let undo_values = previous_values;
        let refocus_path_undo = refocus_path.clone();

        controller.push_undo_entry(super::undo::UndoEntry::<EguiController>::new(
            label,
            move |controller: &mut EguiController| {
                for (source_id, path, tag) in undo_values.iter() {
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
                Ok(super::undo::UndoExecution::Applied)
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
                Ok(super::undo::UndoExecution::Applied)
            },
        ));
    }
    controller.refocus_after_filtered_removal(primary_row);
    if let Some(err) = last_error {
        controller.set_status(err, StatusTone::Error);
    }



    if controller.settings.controls.advance_after_rating
        && controller.ui.browser.selected_visible == Some(primary_row)
    {
        if controller.random_navigation_mode_enabled() {
            controller.focus_random_visible_sample();
        } else {
            let next_row = primary_row + 1;
            if next_row < controller.ui.browser.visible.len() {
                controller.focus_browser_row(next_row);
            }
        }
    }
}
