use radiant::prelude as ui;
use std::time::Instant;

use crate::native_app::{
    app::{GuiMessage, NativeAppState, emit_gui_action},
    sample_library::{
        folder_browser::scan,
        source_prep::{
            CacheWarmIntent, MetadataRefreshIntent, ReadinessIntent, SourceFeedbackIntent,
            SourcePrepIntents, SourcePriorityIntent,
        },
    },
};

pub(in crate::native_app) const VERIFIED_SOURCE_PREP_INTENTS: SourcePrepIntents =
    SourcePrepIntents {
        readiness: ReadinessIntent::RequestConvergence,
        priority: SourcePriorityIntent::PromoteIfSelected,
        metadata_refresh: MetadataRefreshIntent::IfNotLoaded,
        refresh_waveform_cache_projection_if_selected: true,
        cache_warm: CacheWarmIntent::Preserve,
        feedback: SourceFeedbackIntent::Preserve,
    };
pub(in crate::native_app) const VERIFIED_SOURCE_PREP_REASON: &str = "source_verified";

impl NativeAppState {
    pub(in crate::native_app) fn queue_selected_source_folder_tree_refresh(
        &mut self,
        context: &mut ui::UiUpdateContext<GuiMessage>,
        action: &'static str,
        task_name: &'static str,
    ) -> bool {
        let Some(request) = self
            .library
            .folder_browser
            .selected_source_folder_tree_refresh_request()
        else {
            return false;
        };
        let source_id = request.source_id.clone();
        let started_at = Instant::now();
        context
            .business()
            .background(task_name)
            .latest(&mut self.background.folder_tree_refresh_task)
            .run(
                move |_| scan::refresh_folder_tree_only(request),
                GuiMessage::FolderTreeRefreshFinished,
            );
        emit_gui_action(
            action,
            Some("folder_browser"),
            Some(&source_id),
            "queued",
            started_at,
            None,
        );
        true
    }

    pub(in crate::native_app) fn finish_folder_tree_refresh(
        &mut self,
        completion: ui::TaskCompletion<scan::FolderTreeRefreshResult>,
        context: &mut ui::UiUpdateContext<GuiMessage>,
    ) {
        let started_at = Instant::now();
        let Some(result) = self
            .background
            .folder_tree_refresh_task
            .finish_completion(completion)
        else {
            return;
        };
        let source_id = result.source_id.clone();
        let current = self
            .library
            .folder_browser
            .folder_tree_refresh_result_is_current(&result);
        let changed = self
            .library
            .folder_browser
            .apply_folder_tree_refresh_result(result);
        self.reapply_desired_rating_overlay();
        if changed {
            self.persist_user_configuration("folder_browser.folder_tree_refresh", started_at);
        }
        if current {
            self.queue_source_prep(
                source_id.clone(),
                VERIFIED_SOURCE_PREP_INTENTS,
                VERIFIED_SOURCE_PREP_REASON,
                context,
            );
        }
        emit_gui_action(
            "folder_browser.folder_tree_refresh",
            Some("folder_browser"),
            Some(&source_id),
            if changed { "patched" } else { "unchanged" },
            started_at,
            None,
        );
    }
}
