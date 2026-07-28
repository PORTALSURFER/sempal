use super::{
    HarvestState, HarvestStateTarget, NativeAppState, context_menu, emit_gui_action,
    sample_path_label,
};

impl NativeAppState {
    pub(in crate::native_app) fn mark_context_sample_harvest_done(&mut self) {
        self.set_context_sample_harvest_state(HarvestState::Done, "Marked harvest done", "done");
    }

    pub(in crate::native_app) fn mark_context_sample_harvest_ignored(&mut self) {
        self.set_context_sample_harvest_state(
            HarvestState::Ignored,
            "Ignored in harvest",
            "ignored",
        );
    }

    pub(in crate::native_app) fn reset_context_sample_harvest(&mut self) {
        self.set_context_sample_harvest_state(HarvestState::New, "Reset harvest state", "reset");
    }

    pub(in crate::native_app) fn toggle_selected_harvest_done(&mut self) {
        let started_at = std::time::Instant::now();
        let Some(targets) =
            self.selected_harvest_state_targets("browser.harvest.toggle_done", started_at)
        else {
            return;
        };
        let previous_visible_ids = self
            .library
            .folder_browser
            .selected_audio_file_ids_matching_tags(&self.metadata.tags_by_file);
        let target_state = if targets
            .iter()
            .all(|target| target.state == HarvestState::Done)
        {
            HarvestState::New
        } else {
            HarvestState::Done
        };
        let mut applied_any = false;
        for target in &targets {
            self.invalidate_harvest_touched_for_path(&target.path);
            if let Err(error) =
                wavecrate::sample_sources::library::upsert_harvest_file(&target.identity)
            {
                self.finish_toggle_selected_harvest_done_error(
                    &targets,
                    applied_any,
                    &previous_visible_ids,
                    started_at,
                    error.to_string(),
                );
                return;
            }
            if let Err(error) = wavecrate::sample_sources::library::set_harvest_state(
                &target.identity.key,
                target_state,
            ) {
                self.finish_toggle_selected_harvest_done_error(
                    &targets,
                    applied_any,
                    &previous_visible_ids,
                    started_at,
                    error.to_string(),
                );
                return;
            }
            applied_any = true;
        }

        self.library
            .folder_browser
            .refresh_after_harvest_state_change_matching_tags(
                previous_visible_ids,
                &self.metadata.tags_by_file,
            );
        let target_label = harvest_state_targets_label(&targets);
        self.ui.status.sample = harvest_state_toggle_status(target_state, &targets);
        emit_gui_action(
            "browser.harvest.toggle_done",
            Some("browser"),
            Some(target_label.as_str()),
            harvest_state_toggle_outcome(target_state),
            started_at,
            None,
        );
    }

    fn set_context_sample_harvest_state(
        &mut self,
        state: HarvestState,
        status_prefix: &'static str,
        outcome: &'static str,
    ) {
        let started_at = std::time::Instant::now();
        let Some(targets) = self.take_context_sample_harvest_state_targets(
            "browser.context_menu.harvest.state",
            started_at,
        ) else {
            return;
        };
        let previous_visible_ids = self
            .library
            .folder_browser
            .selected_audio_file_ids_matching_tags(&self.metadata.tags_by_file);

        let mut changed = false;
        for target in &targets {
            self.invalidate_harvest_touched_for_path(&target.path);
            if let Err(error) =
                wavecrate::sample_sources::library::upsert_harvest_file(&target.identity)
            {
                if changed {
                    self.library
                        .folder_browser
                        .refresh_after_harvest_state_change_matching_tags(
                            previous_visible_ids.clone(),
                            &self.metadata.tags_by_file,
                        );
                }
                self.ui.status.sample = format!("Update harvest state failed: {error}");
                emit_gui_action(
                    "browser.context_menu.harvest.state",
                    Some("browser"),
                    Some(context_menu::target_label(&target.path).as_str()),
                    "error",
                    started_at,
                    Some(&error.to_string()),
                );
                return;
            }
            match wavecrate::sample_sources::library::set_harvest_state(&target.identity.key, state)
            {
                Ok(_) => {
                    changed = true;
                }
                Err(error) => {
                    if changed {
                        self.library
                            .folder_browser
                            .refresh_after_harvest_state_change_matching_tags(
                                previous_visible_ids.clone(),
                                &self.metadata.tags_by_file,
                            );
                    }
                    self.ui.status.sample = format!("Update harvest state failed: {error}");
                    emit_gui_action(
                        "browser.context_menu.harvest.state",
                        Some("browser"),
                        Some(context_menu::target_label(&target.path).as_str()),
                        "error",
                        started_at,
                        Some(&error.to_string()),
                    );
                    return;
                }
            }
        }

        self.library
            .folder_browser
            .refresh_after_harvest_state_change_matching_tags(
                previous_visible_ids,
                &self.metadata.tags_by_file,
            );
        let target_label = harvest_state_targets_label(&targets);
        self.ui.status.sample = format!("{status_prefix} {target_label}");
        emit_gui_action(
            "browser.context_menu.harvest.state",
            Some("browser"),
            Some(&target_label),
            outcome,
            started_at,
            None,
        );
    }

    fn finish_toggle_selected_harvest_done_error(
        &mut self,
        targets: &[HarvestStateTarget],
        applied_any: bool,
        previous_visible_ids: &[String],
        started_at: std::time::Instant,
        error: String,
    ) {
        if applied_any {
            self.library
                .folder_browser
                .refresh_after_harvest_state_change_matching_tags(
                    previous_visible_ids.to_vec(),
                    &self.metadata.tags_by_file,
                );
        }
        let target_label = harvest_state_targets_label(targets);
        self.ui.status.sample = format!("Update harvest state failed: {error}");
        emit_gui_action(
            "browser.harvest.toggle_done",
            Some("browser"),
            Some(target_label.as_str()),
            "error",
            started_at,
            Some(error.as_str()),
        );
    }
}

fn harvest_state_toggle_status(state: HarvestState, targets: &[HarvestStateTarget]) -> String {
    let prefix = match state {
        HarvestState::Done => "Marked harvest done",
        HarvestState::New => "Reset harvest state",
        _ => "Updated harvest state",
    };
    if targets.len() == 1 {
        format!("{prefix} {}", sample_path_label(&targets[0].path))
    } else {
        format!("{prefix} {} samples", targets.len())
    }
}

fn harvest_state_toggle_outcome(state: HarvestState) -> &'static str {
    match state {
        HarvestState::Done => "done",
        HarvestState::New => "reset",
        _ => "updated",
    }
}

fn harvest_state_targets_label(targets: &[HarvestStateTarget]) -> String {
    match targets {
        [] => String::from("0 samples"),
        [target] => sample_path_label(&target.path),
        targets => format!("{} samples", targets.len()),
    }
}
