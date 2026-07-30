use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    time::Instant,
};

use radiant::prelude as ui;
use wavecrate::sample_sources::{ExistingFileMetadataUpdate, Rating, SourceDatabase};

use crate::native_app::app::{GuiMessage, NativeAppState, emit_gui_action};
use crate::native_app::sample_library::file_actions::sample_path_label;
use crate::native_app::sample_library::folder_browser_actions::file_navigation_reveal_direction;
use crate::native_app::sample_library::sample_list::{
    SAMPLE_BROWSER_LIST_ID, SAMPLE_BROWSER_ROW_HEIGHT, SAMPLE_BROWSER_SELECTION_CONTEXT_ROWS,
};
use crate::native_app::transaction_history::TransactionContext;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(in crate::native_app) struct RatingPersistRequest {
    pub(in crate::native_app) source_id: String,
    pub(in crate::native_app) lifecycle_generation: Option<u64>,
    pub(in crate::native_app) root: PathBuf,
    pub(in crate::native_app) database_root: PathBuf,
    pub(in crate::native_app) relative_path: PathBuf,
    pub(in crate::native_app) absolute_path: PathBuf,
    pub(in crate::native_app) rating: Rating,
    pub(in crate::native_app) locked: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct RatingUpdate {
    source_id: String,
    lifecycle_generation: Option<u64>,
    root: PathBuf,
    database_root: PathBuf,
    relative_path: PathBuf,
    absolute_path: PathBuf,
    previous_rating: Rating,
    previous_locked: bool,
    rating: Rating,
    locked: bool,
}

#[derive(Debug, Default)]
struct RatingAdjustmentPlan {
    updates: Vec<RatingUpdate>,
    auto_trash_updates: Vec<RatingUpdate>,
}

impl NativeAppState {
    pub(in crate::native_app) fn reapply_desired_rating_overlay(&mut self) {
        let desired = self.background.rating_persist.desired_snapshot();
        for request in desired {
            if request.lifecycle_generation.is_some_and(|generation| {
                self.background
                    .source_lifecycle_generations
                    .get(&request.source_id)
                    != Some(&generation)
            }) || !self
                .library
                .folder_browser
                .source_exists(&request.source_id)
            {
                continue;
            }
            let _ = self.library.folder_browser.set_file_rating_state(
                &request.absolute_path,
                request.rating,
                request.locked,
            );
        }
    }

    pub(in crate::native_app) fn finish_rating_persist(
        &mut self,
        completion: ui::TaskCompletion<crate::native_app::app::RatingPersistBatchResult>,
        context: &mut ui::UiUpdateContext<GuiMessage>,
    ) {
        let Some(result) = self.background.rating_persist.finish(completion) else {
            return;
        };
        for item in result.results {
            if let Some(Err(error)) = item.result {
                self.ui.status.sample = format!(
                    "Rating for {} not saved: {error}",
                    sample_path_label(&item.absolute_path)
                );
            }
        }
        let auto_trash_paths = self.background.rating_persist.take_committed_auto_trash();
        if !auto_trash_paths.is_empty() {
            self.move_negative_threshold_files_to_trash(auto_trash_paths, Instant::now(), context);
        }
        self.background.rating_persist.schedule_if_idle(context);
    }

    #[cfg(test)]
    pub(in crate::native_app) fn adjust_selected_rating(
        &mut self,
        delta: i8,
        context: &mut ui::UiUpdateContext<GuiMessage>,
    ) {
        self.adjust_selected_rating_with_policy(delta, context, true);
    }

    pub(in crate::native_app) fn adjust_selected_rating_without_advance(
        &mut self,
        delta: i8,
        context: &mut ui::UiUpdateContext<GuiMessage>,
    ) {
        self.adjust_selected_rating_with_policy(delta, context, false);
    }

    pub(in crate::native_app) fn add_keep_rating_to_handoff_paths(
        &mut self,
        paths: &[PathBuf],
        context: &mut ui::UiUpdateContext<GuiMessage>,
    ) -> Result<usize, String> {
        let plan = self.rating_adjustment_plan_for_paths(paths, 1);
        if plan.is_empty() {
            return Ok(0);
        }
        let touched_paths = plan
            .updates
            .iter()
            .map(|update| update.absolute_path.clone())
            .collect::<Vec<_>>();
        let applied = self.apply_rating_update_states(&plan.updates, RatingUpdateMode::After)?;
        if applied > 0 {
            self.background.rating_persist.schedule_if_idle(context);
            self.schedule_harvest_touched_for_paths(&touched_paths, context);
        }
        Ok(applied)
    }

    pub(in crate::native_app) fn unlock_context_sample(
        &mut self,
        context: &mut ui::UiUpdateContext<GuiMessage>,
    ) {
        let started_at = Instant::now();
        let Some(menu) = self.ui.browser_interaction.context_menu.take() else {
            return;
        };
        let absolute_path = menu.path.clone();
        let path_key = normalized_rating_path(&absolute_path);
        let Some((loaded_path, previous_rating, previous_locked)) =
            self.rating_row_state_for_path(&path_key)
        else {
            self.ui.status.sample = String::from("Sample is unavailable");
            emit_gui_action(
                "browser.context_menu.sample.unlock",
                Some("browser"),
                Some(sample_path_label(&absolute_path).as_str()),
                "error",
                started_at,
                Some("sample_unavailable"),
            );
            return;
        };
        if previous_rating != Rating::KEEP_3 || !previous_locked {
            self.ui.status.sample = String::from("Sample is not locked");
            emit_gui_action(
                "browser.context_menu.sample.unlock",
                Some("browser"),
                Some(sample_path_label(&loaded_path).as_str()),
                "blocked",
                started_at,
                Some("sample_not_locked"),
            );
            return;
        }
        let Some((root, database_root, relative_path)) = self
            .library
            .folder_browser
            .source_database_relative_file_path(&loaded_path)
        else {
            self.ui.status.sample = String::from("Sample is unavailable");
            emit_gui_action(
                "browser.context_menu.sample.unlock",
                Some("browser"),
                Some(sample_path_label(&loaded_path).as_str()),
                "error",
                started_at,
                Some("source_unavailable"),
            );
            return;
        };
        let Some(source_id) = self
            .library
            .folder_browser
            .source_id_for_file_path(&loaded_path)
        else {
            self.ui.status.sample = String::from("Sample is unavailable");
            return;
        };
        let update = RatingUpdate {
            source_id: source_id.clone(),
            lifecycle_generation: self
                .background
                .source_lifecycle_generations
                .get(&source_id)
                .copied(),
            root,
            database_root,
            relative_path,
            absolute_path: loaded_path.clone(),
            previous_rating,
            previous_locked,
            rating: previous_rating,
            locked: false,
        };
        let previous_visible_ids = self
            .library
            .folder_browser
            .selected_audio_file_ids_matching_tags(&self.metadata.tags_by_file);
        let applied = match self
            .apply_rating_update_states(std::slice::from_ref(&update), RatingUpdateMode::After)
        {
            Ok(applied) => applied,
            Err(error) => {
                self.ui.status.sample = format!("Unlock failed: {error}");
                emit_gui_action(
                    "browser.context_menu.sample.unlock",
                    Some("browser"),
                    Some(sample_path_label(&loaded_path).as_str()),
                    "error",
                    started_at,
                    Some(self.ui.status.sample.as_str()),
                );
                return;
            }
        };
        self.background.rating_persist.schedule_if_idle(context);
        if applied == 0 {
            self.ui.status.sample = String::from("Sample is unavailable");
            emit_gui_action(
                "browser.context_menu.sample.unlock",
                Some("browser"),
                Some(sample_path_label(&loaded_path).as_str()),
                "error",
                started_at,
                Some("sample_unavailable"),
            );
            return;
        }
        self.ui.status.sample = format!("Unlocked {}", sample_path_label(&loaded_path));
        self.schedule_harvest_touched_for_paths(std::slice::from_ref(&loaded_path), context);
        self.register_rating_transaction_with_label("Unlock sample", vec![update]);
        self.library
            .folder_browser
            .reconcile_visible_file_selection_after_tag_filter(
                previous_visible_ids,
                &self.metadata.tags_by_file,
            );
        emit_gui_action(
            "browser.context_menu.sample.unlock",
            Some("browser"),
            Some(sample_path_label(&loaded_path).as_str()),
            "success",
            started_at,
            None,
        );
    }

    fn adjust_selected_rating_with_policy(
        &mut self,
        delta: i8,
        context: &mut ui::UiUpdateContext<GuiMessage>,
        allow_advance: bool,
    ) {
        let started_at = Instant::now();
        let advance_visible_ids = self.rating_advance_visible_ids_before_adjustment(allow_advance);
        let previous_visible_ids = self
            .library
            .folder_browser
            .selected_audio_file_ids_matching_tags(&self.metadata.tags_by_file);
        let advance_previous_index = advance_visible_ids.as_ref().and_then(|_| {
            self.library
                .folder_browser
                .selected_audio_file_index_matching_tags(&self.metadata.tags_by_file)
        });
        let plan = self.rating_adjustment_plan_for_selected_files(delta);
        if plan.is_empty() {
            self.ui.status.sample = String::from("Select a sample to rate");
            emit_gui_action(
                "browser.rating.adjust",
                Some("browser"),
                Some(direction_label(delta)),
                "empty",
                started_at,
                None,
            );
            return;
        }

        let applied = match self.apply_rating_update_states(&plan.updates, RatingUpdateMode::After)
        {
            Ok(applied) => applied,
            Err(error) => {
                self.ui.status.sample = format!("Rating failed: {error}");
                emit_gui_action(
                    "browser.rating.adjust",
                    Some("browser"),
                    Some(direction_label(delta)),
                    "error",
                    started_at,
                    Some(self.ui.status.sample.as_str()),
                );
                return;
            }
        };
        if applied == 0 && plan.auto_trash_updates.is_empty() {
            self.ui.status.sample = String::from("Rating did not change");
            emit_gui_action(
                "browser.rating.adjust",
                Some("browser"),
                Some(direction_label(delta)),
                "blocked",
                started_at,
                Some("no_rows_updated"),
            );
            return;
        }

        if applied > 0 {
            self.ui.status.sample = format!(
                "Rated {applied} sample{}",
                if applied == 1 { "" } else { "s" }
            );
        }
        emit_gui_action(
            "browser.rating.adjust",
            Some("browser"),
            Some(direction_label(delta)),
            "success",
            started_at,
            None,
        );

        if applied > 0 {
            let touched_paths = plan
                .updates
                .iter()
                .map(|update| update.absolute_path.clone())
                .collect::<Vec<_>>();
            self.schedule_harvest_touched_for_paths(&touched_paths, context);
            self.register_rating_transaction(delta, plan.updates);
        }

        for update in &plan.auto_trash_updates {
            if let Some(revision) = self
                .background
                .rating_persist
                .revision_for(&update.source_id, &update.relative_path)
            {
                self.background.rating_persist.defer_auto_trash(
                    &update.source_id,
                    &update.relative_path,
                    revision,
                    update.absolute_path.clone(),
                );
            }
        }
        self.background.rating_persist.schedule_if_idle(context);

        if applied > 0 && allow_advance && self.ui.settings.persisted.controls.advance_after_rating
        {
            if let Some(visible_ids) = advance_visible_ids {
                self.advance_after_rating_in_visible_order(
                    &visible_ids,
                    advance_previous_index,
                    context,
                );
            } else {
                self.navigate_browser(1, false, false, context);
            }
        }
        if applied > 0 {
            self.library
                .folder_browser
                .reconcile_visible_file_selection_after_tag_filter(
                    previous_visible_ids,
                    &self.metadata.tags_by_file,
                );
        }
    }

    fn rating_advance_visible_ids_before_adjustment(
        &self,
        allow_advance: bool,
    ) -> Option<Vec<String>> {
        if !allow_advance
            || !self.ui.settings.persisted.controls.advance_after_rating
            || self.library.folder_browser.random_navigation_enabled()
        {
            return None;
        }
        Some(
            self.library
                .folder_browser
                .selected_audio_files_matching_tags(&self.metadata.tags_by_file)
                .into_iter()
                .map(|file| file.id.clone())
                .collect(),
        )
    }

    fn advance_after_rating_in_visible_order(
        &mut self,
        visible_ids_before_rating: &[String],
        previous_index: Option<usize>,
        context: &mut ui::UiUpdateContext<GuiMessage>,
    ) {
        let previous_selection = self
            .library
            .folder_browser
            .selected_file_id()
            .map(str::to_owned);
        let candidate = previous_index
            .and_then(|index| visible_ids_before_rating.get(index.saturating_add(1)))
            .map(String::as_str);
        let Some(path) = self.rating_advance_visible_target(previous_index, candidate) else {
            return;
        };

        if Some(path.as_str()) != previous_selection.as_deref() {
            self.cancel_metadata_tag_entry();
            self.metadata.selected_tag = None;
        }
        if self.library.folder_browser.selected_file_id() != Some(path.as_str()) {
            self.library
                .folder_browser
                .focus_file_preserving_selection_matching_tags(
                    path.clone(),
                    &self.metadata.tags_by_file,
                );
        }
        if let Some(index) = self
            .library
            .folder_browser
            .selected_audio_file_index_matching_tags(&self.metadata.tags_by_file)
        {
            let reveal_direction = file_navigation_reveal_direction(previous_index, index, 1);
            context.scroll_fixed_row_into_view(
                SAMPLE_BROWSER_LIST_ID,
                index,
                SAMPLE_BROWSER_ROW_HEIGHT,
                SAMPLE_BROWSER_SELECTION_CONTEXT_ROWS,
                SAMPLE_BROWSER_SELECTION_CONTEXT_ROWS,
                reveal_direction,
            );
        }
        self.load_navigation_sample(path, context);
    }

    fn rating_advance_visible_target(
        &self,
        previous_index: Option<usize>,
        candidate: Option<&str>,
    ) -> Option<String> {
        let listing = self
            .library
            .folder_browser
            .browser_listing_snapshot(&self.metadata.tags_by_file);
        if let Some(candidate) = candidate
            && listing.contains(candidate)
        {
            return Some(candidate.to_owned());
        }
        listing
            .target_after_removed_or_hidden(previous_index.unwrap_or(0))
            .map(str::to_owned)
    }

    fn rating_adjustment_plan_for_paths(
        &self,
        paths: &[PathBuf],
        delta: i8,
    ) -> RatingAdjustmentPlan {
        if delta == 0 {
            return RatingAdjustmentPlan::default();
        }
        let mut plan = RatingAdjustmentPlan::default();
        let mut seen = Vec::new();
        for path in paths.iter().map(|path| normalized_rating_path(path)) {
            if seen.iter().any(|existing| existing == &path) {
                continue;
            }
            seen.push(path.clone());
            let Some((absolute_path, previous_rating, previous_locked)) =
                self.rating_row_state_for_path(&path)
            else {
                continue;
            };
            if previous_locked {
                continue;
            }
            let Some((root, database_root, relative_path)) = self
                .library
                .folder_browser
                .source_database_relative_file_path(&absolute_path)
            else {
                continue;
            };
            let Some((rating, locked)) = next_rating_state(previous_rating, delta) else {
                continue;
            };
            let Some(source_id) = self
                .library
                .folder_browser
                .source_id_for_file_path(&absolute_path)
            else {
                continue;
            };
            plan.updates.push(RatingUpdate {
                source_id: source_id.clone(),
                lifecycle_generation: self
                    .background
                    .source_lifecycle_generations
                    .get(&source_id)
                    .copied(),
                root,
                database_root,
                relative_path,
                absolute_path,
                previous_rating,
                previous_locked,
                rating,
                locked,
            });
        }
        plan
    }

    fn rating_row_state_for_path(&self, path: &Path) -> Option<(PathBuf, Rating, bool)> {
        self.library
            .folder_browser
            .loaded_source_audio_files()
            .into_iter()
            .find(|file| normalized_rating_path(Path::new(&file.id)) == path)
            .map(|file| (PathBuf::from(&file.id), file.rating, file.rating_locked))
    }

    fn rating_adjustment_plan_for_selected_files(&self, delta: i8) -> RatingAdjustmentPlan {
        if delta == 0 {
            return RatingAdjustmentPlan::default();
        }
        let mut plan = RatingAdjustmentPlan::default();
        for candidate in self
            .library
            .folder_browser
            .selected_file_rating_candidates_matching_tags(&self.metadata.tags_by_file)
            .into_iter()
            .filter(|candidate| !candidate.locked)
        {
            let Some((root, database_root, relative_path)) = self
                .library
                .folder_browser
                .source_database_relative_file_path(&candidate.path)
            else {
                continue;
            };
            let (rating, locked) = if should_auto_trash_on_rating(candidate.rating, delta) {
                (candidate.rating, candidate.locked)
            } else {
                let Some(next) = next_rating_state(candidate.rating, delta) else {
                    continue;
                };
                next
            };
            let Some(source_id) = self
                .library
                .folder_browser
                .source_id_for_file_path(&candidate.path)
            else {
                continue;
            };
            let update = RatingUpdate {
                source_id: source_id.clone(),
                lifecycle_generation: self
                    .background
                    .source_lifecycle_generations
                    .get(&source_id)
                    .copied(),
                root,
                database_root,
                relative_path,
                absolute_path: candidate.path,
                previous_rating: candidate.rating,
                previous_locked: candidate.locked,
                rating,
                locked,
            };
            if should_auto_trash_on_rating(candidate.rating, delta) {
                plan.auto_trash_updates.push(update.clone());
            }
            plan.updates.push(update);
        }
        plan
    }

    fn register_rating_transaction(&mut self, delta: i8, updates: Vec<RatingUpdate>) {
        let label = format!("Rate {}", if delta < 0 { "down" } else { "up" });
        self.register_rating_transaction_with_label(label, updates);
    }

    fn register_rating_transaction_with_label(
        &mut self,
        label: impl Into<String>,
        updates: Vec<RatingUpdate>,
    ) {
        let undo_updates = updates.clone();
        let redo_updates = updates;
        self.begin_transaction(label.into());
        self.register_transaction_action(
            "Apply rating changes",
            move |transaction| {
                transaction
                    .apply_rating_update_states(&undo_updates, RatingUpdateMode::Before)
                    .map(|_| ())
            },
            move |transaction| {
                transaction
                    .apply_rating_update_states(&redo_updates, RatingUpdateMode::After)
                    .map(|_| ())
            },
        );
        self.commit_transaction();
    }

    fn apply_rating_update_states(
        &mut self,
        updates: &[RatingUpdate],
        mode: RatingUpdateMode,
    ) -> Result<usize, String> {
        let mut applied = 0usize;
        for source_updates in group_updates_by_source(
            updates
                .iter()
                .cloned()
                .map(|update| update.for_mode(mode))
                .collect(),
        )
        .into_values()
        {
            let mut source_updates = source_updates;
            for update in &mut source_updates {
                update.lifecycle_generation = self
                    .background
                    .source_lifecycle_generations
                    .get(&update.source_id)
                    .copied()
                    .or(update.lifecycle_generation);
                self.background
                    .rating_persist
                    .enqueue(update.persist_request());
            }
            for update in source_updates {
                if self.apply_rating_update_to_loaded_browser_row(&update) {
                    applied += 1;
                }
            }
        }
        Ok(applied)
    }

    fn apply_rating_update_to_loaded_browser_row(&mut self, update: &RatingUpdate) -> bool {
        if self.library.folder_browser.set_file_rating_state(
            &update.absolute_path,
            update.rating,
            update.locked,
        ) {
            return true;
        }
        self.library
            .folder_browser
            .refresh_file_path_across_sources(&update.absolute_path)
            && self.library.folder_browser.set_file_rating_state(
                &update.absolute_path,
                update.rating,
                update.locked,
            )
    }
}

impl TransactionContext<'_> {
    fn apply_rating_update_states(
        &mut self,
        updates: &[RatingUpdate],
        mode: RatingUpdateMode,
    ) -> Result<usize, String> {
        self.state.apply_rating_update_states(updates, mode)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum RatingUpdateMode {
    Before,
    After,
}

impl RatingUpdate {
    fn for_mode(mut self, mode: RatingUpdateMode) -> Self {
        if mode == RatingUpdateMode::Before {
            self.rating = self.previous_rating;
            self.locked = self.previous_locked;
        }
        self
    }
}

fn next_rating_state(current: Rating, delta: i8) -> Option<(Rating, bool)> {
    if current == Rating::KEEP_3 && delta > 0 {
        return Some((Rating::KEEP_3, true));
    }
    if current == Rating::TRASH_3 && delta < 0 {
        return None;
    }

    let mut new_value = current.val() + delta.signum();
    if current.val() != 0 && new_value == 0 {
        new_value += delta.signum();
    }
    let rating = Rating::new(new_value.clamp(-3, 3));
    (rating != current).then_some((rating, false))
}

fn should_auto_trash_on_rating(current: Rating, delta: i8) -> bool {
    current == Rating::TRASH_3 && delta < 0
}

fn normalized_rating_path(path: &Path) -> PathBuf {
    path.components().collect()
}

impl RatingAdjustmentPlan {
    fn is_empty(&self) -> bool {
        self.updates.is_empty() && self.auto_trash_updates.is_empty()
    }
}

fn group_updates_by_source(
    updates: Vec<RatingUpdate>,
) -> BTreeMap<(PathBuf, PathBuf), Vec<RatingUpdate>> {
    let mut by_source: BTreeMap<(PathBuf, PathBuf), Vec<RatingUpdate>> = BTreeMap::new();
    for update in updates {
        by_source
            .entry((update.root.clone(), update.database_root.clone()))
            .or_default()
            .push(update);
    }
    by_source
}

pub(in crate::native_app) fn persist_rating_requests(
    requests: &[RatingPersistRequest],
    current: impl Fn(&RatingPersistRequest) -> bool,
) -> Vec<Option<Result<(), String>>> {
    let mut results = vec![None; requests.len()];
    let mut groups: BTreeMap<(PathBuf, PathBuf), Vec<usize>> = BTreeMap::new();
    for (index, request) in requests.iter().enumerate() {
        groups
            .entry((request.root.clone(), request.database_root.clone()))
            .or_default()
            .push(index);
    }
    for ((root, database_root), indexes) in groups {
        let active = indexes
            .iter()
            .copied()
            .filter(|index| current(&requests[*index]))
            .collect::<Vec<_>>();
        if active.is_empty() {
            continue;
        }
        let result = (|| {
            let db = SourceDatabase::open_for_user_metadata_write_with_database_root(
                &root,
                &database_root,
            )
            .map_err(|err| err.to_string())?;
            let mut batch = db.write_batch().map_err(|err| err.to_string())?;
            for index in &active {
                let request = &requests[*index];
                if matches!(
                    batch
                        .ensure_existing_live_file(&request.relative_path)
                        .map_err(|err| err.to_string())?,
                    ExistingFileMetadataUpdate::Missing
                ) {
                    return Err(format!(
                        "rating persistence deferred until source row exists: {}",
                        request.relative_path.display()
                    ));
                }
                batch
                    .set_tag(&request.relative_path, request.rating)
                    .map_err(|err| err.to_string())?;
                batch
                    .set_locked(&request.relative_path, request.locked)
                    .map_err(|err| err.to_string())?;
            }
            batch
                .commit_auxiliary_state()
                .map_err(|err| err.to_string())
        })();
        for index in active {
            results[index] = Some(result.clone());
        }
    }
    results
}

impl RatingUpdate {
    fn persist_request(&self) -> RatingPersistRequest {
        RatingPersistRequest {
            source_id: self.source_id.clone(),
            lifecycle_generation: self.lifecycle_generation,
            root: self.root.clone(),
            database_root: self.database_root.clone(),
            relative_path: self.relative_path.clone(),
            absolute_path: self.absolute_path.clone(),
            rating: self.rating,
            locked: self.locked,
        }
    }
}

fn direction_label(delta: i8) -> &'static str {
    if delta < 0 { "down" } else { "up" }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn next_rating_skips_neutral_when_changing_direction() {
        assert_eq!(
            next_rating_state(Rating::KEEP_1, -1),
            Some((Rating::TRASH_1, false))
        );
        assert_eq!(
            next_rating_state(Rating::TRASH_1, 1),
            Some((Rating::KEEP_1, false))
        );
    }

    #[test]
    fn next_rating_locks_keep_three_on_fourth_keep() {
        assert_eq!(
            next_rating_state(Rating::KEEP_3, 1),
            Some((Rating::KEEP_3, true))
        );
    }

    #[test]
    fn next_rating_stops_at_trash_three_without_trash_move() {
        assert_eq!(next_rating_state(Rating::TRASH_3, -1), None);
    }

    #[test]
    fn fourth_negative_rating_triggers_auto_trash_threshold() {
        assert!(should_auto_trash_on_rating(Rating::TRASH_3, -1));
        assert!(!should_auto_trash_on_rating(Rating::new(-2), -1));
        assert!(!should_auto_trash_on_rating(Rating::TRASH_3, 1));
    }
}
