use std::time::Instant;

use radiant::prelude as ui;

use crate::native_app::app::{GuiMessage, NativeAppState, SourceSelectionRequest, emit_gui_action};
use crate::native_app::sample_library::source_prep::{
    CacheWarmIntent, MetadataRefreshIntent, ReadinessIntent, SourceFeedbackIntent,
    SourcePrepIntents, SourcePriorityIntent,
};

pub(in crate::native_app) const PROCESS_SOURCE_PREP_INTENTS: SourcePrepIntents =
    SourcePrepIntents {
        readiness: ReadinessIntent::Reanalyze,
        priority: SourcePriorityIntent::PromoteSource,
        metadata_refresh: MetadataRefreshIntent::Force,
        refresh_waveform_cache_projection_if_selected: true,
        cache_warm: CacheWarmIntent::SelectedFolderOrSource,
        feedback: SourceFeedbackIntent::QueuedIfCacheWarmNotScheduled,
    };
pub(in crate::native_app) const PROCESS_SOURCE_PREP_REASON: &str = "user_requested";

impl NativeAppState {
    pub(in crate::native_app) fn select_source(
        &mut self,
        id: String,
        context: &mut ui::UiUpdateContext<GuiMessage>,
    ) {
        let started_at = Instant::now();
        let requested_id = id.clone();
        let task_id = self.next_folder_task_id();
        match self.library.begin_select_source(id, task_id) {
            SourceSelectionRequest::Queued(request) => {
                let label = request.label.clone();
                emit_gui_action(
                    "folder_browser.select_source",
                    Some("folder_browser"),
                    Some(&label),
                    "scan_queued",
                    started_at,
                    None,
                );
                self.launch_folder_scan_with_cause(request, "source_selection", context);
                return;
            }
            SourceSelectionRequest::Deferred => {
                emit_gui_action(
                    "folder_browser.select_source",
                    Some("folder_browser"),
                    Some(&requested_id),
                    "deferred",
                    started_at,
                    Some("scan_already_running"),
                );
                return;
            }
            SourceSelectionRequest::Settled => {
                if self.library.folder_browser.selected_source_id() == requested_id
                    && self.library.folder_browser.selected_source_loaded()
                    && !self.library.folder_browser.source_is_missing(&requested_id)
                {
                    self.queue_selected_source_folder_tree_refresh(
                        context,
                        "folder_browser.source_selection_folder_tree_refresh",
                        "gui-source-selection-folder-tree-refresh",
                    );
                }
            }
        }
        if self.library.folder_browser.selected_source_id() == requested_id
            && self.library.folder_browser.selected_source_loaded()
        {
            emit_gui_action(
                "folder_browser.select_source",
                Some("folder_browser"),
                Some(&requested_id),
                "loaded_cached",
                started_at,
                None,
            );
        } else if self.library.folder_browser.source_is_missing(&requested_id) {
            self.ui.status.sample =
                missing_source_status(self.library.folder_browser.source_root_path(&requested_id));
            emit_gui_action(
                "folder_browser.select_source",
                Some("folder_browser"),
                Some(&requested_id),
                "missing",
                started_at,
                Some("source_root_missing"),
            );
        } else {
            emit_gui_action(
                "folder_browser.select_source",
                Some("folder_browser"),
                None,
                "short_circuit",
                started_at,
                Some("source_not_found"),
            );
        }
    }

    pub(in crate::native_app) fn cancel_active_source_scan(&mut self, retry: bool) -> bool {
        let Some((source_id, label)) = self.library.cancel_active_folder_scan_by_user() else {
            return false;
        };
        self.background
            .source_processing
            .cancel_foreground_source_scan(&source_id, "user_cancelled_source_scan");
        self.ui.chrome.job_details_open = false;
        self.background.progress_tick = 0.0;
        self.ui.status.sample = if retry {
            format!("Retrying source scan for {label}")
        } else {
            format!("Canceled source scan for {label}")
        };
        tracing::info!(
            target: "wavecrate::source_processing",
            source_id,
            terminal_outcome = if retry { "retry_requested" } else { "user_cancelled" },
            "Source scan recovery action applied"
        );
        true
    }

    pub(in crate::native_app) fn retry_active_source_scan(
        &mut self,
        context: &mut ui::UiUpdateContext<GuiMessage>,
    ) {
        let source_id = self
            .library
            .folder_progress()
            .map(|progress| progress.source_id.clone());
        let Some(source_id) = source_id else {
            return;
        };
        if !self.cancel_active_source_scan(true) {
            return;
        }
        self.background
            .source_processing
            .wake_source_for_full_reconciliation(&source_id, "user_retried_source_scan");
        self.refresh_source(source_id, context);
    }

    pub(in crate::native_app) fn refresh_source(
        &mut self,
        id: String,
        context: &mut ui::UiUpdateContext<GuiMessage>,
    ) {
        let started_at = Instant::now();
        let task_id = self.next_folder_task_id();
        let source_id = id.clone();
        if let Some(request) = self.library.begin_source_scan(id, task_id) {
            let label = request.label.clone();
            self.ui.browser_interaction.context_menu = None;
            emit_gui_action(
                "folder_browser.source.refresh",
                Some("sources"),
                Some(&label),
                "scan_queued",
                started_at,
                None,
            );
            self.launch_folder_scan_with_cause(request, "user_refresh", context);
        } else if self.library.folder_browser.source_is_missing(&source_id) {
            self.ui.browser_interaction.context_menu = None;
            self.ui.status.sample = String::from("Source missing");
            emit_gui_action(
                "folder_browser.source.refresh",
                Some("sources"),
                Some(&source_id),
                "missing",
                started_at,
                Some("source_root_missing"),
            );
        } else {
            self.ui.browser_interaction.context_menu = None;
            self.ui.status.sample = String::from("Source refresh is already running");
            emit_gui_action(
                "folder_browser.source.refresh",
                Some("sources"),
                None,
                "short_circuit",
                started_at,
                Some("source_not_queued"),
            );
        }
    }

    pub(in crate::native_app) fn refresh_context_source(
        &mut self,
        context: &mut ui::UiUpdateContext<GuiMessage>,
    ) {
        let Some(menu) = self.ui.browser_interaction.context_menu.clone() else {
            return;
        };
        let Some(source_id) = menu.source_id else {
            self.ui.browser_interaction.context_menu = None;
            self.ui.status.sample = String::from("Source is unavailable");
            return;
        };
        self.refresh_source(source_id, context);
    }

    pub(in crate::native_app) fn process_context_source(
        &mut self,
        context: &mut ui::UiUpdateContext<GuiMessage>,
    ) {
        let started_at = Instant::now();
        let Some(menu) = self.ui.browser_interaction.context_menu.clone() else {
            return;
        };
        let Some(source_id) = menu.source_id else {
            self.ui.browser_interaction.context_menu = None;
            self.ui.status.sample = String::from("Source is unavailable");
            return;
        };
        self.ui.browser_interaction.context_menu = None;
        if self
            .library
            .folder_browser
            .source_root_path(&source_id)
            .is_none()
        {
            self.ui.status.sample = String::from("Source is unavailable");
            emit_gui_action(
                "folder_browser.source.process",
                Some("sources"),
                Some(&source_id),
                "error",
                started_at,
                Some("source_unavailable"),
            );
            return;
        }
        if self
            .library
            .folder_browser
            .refresh_source_availability_from_disk(&source_id)
            .unwrap_or(true)
        {
            self.ui.status.sample = String::from("Source missing");
            emit_gui_action(
                "folder_browser.source.process",
                Some("sources"),
                Some(&source_id),
                "missing",
                started_at,
                Some("source_root_missing"),
            );
            return;
        }
        self.queue_source_prep(
            source_id.clone(),
            PROCESS_SOURCE_PREP_INTENTS,
            PROCESS_SOURCE_PREP_REASON,
            context,
        );
        emit_gui_action(
            "folder_browser.source.process",
            Some("sources"),
            Some(&source_id),
            "queued",
            started_at,
            None,
        );
    }

    pub(in crate::native_app) fn maybe_startup_source_scan(
        &mut self,
        context: &mut ui::UiUpdateContext<GuiMessage>,
    ) {
        if !self.ui.startup.source_scan_pending {
            self.maybe_startup_visible_folder_verify(context);
            return;
        }
        self.ui.startup.source_scan_pending = false;
        let started_at = Instant::now();
        let task_id = self.next_folder_task_id();
        if let Some(request) = self.library.begin_selected_source_scan(task_id) {
            let label = request.label.clone();
            emit_gui_action(
                "folder_browser.startup_scan",
                Some("folder_browser"),
                Some(&label),
                "scan_queued",
                started_at,
                None,
            );
            self.launch_folder_scan_with_cause(request, "startup", context);
        } else {
            emit_gui_action(
                "folder_browser.startup_scan",
                Some("folder_browser"),
                None,
                "short_circuit",
                started_at,
                Some("source_not_queued"),
            );
        }
    }
}

fn missing_source_status(root: Option<std::path::PathBuf>) -> String {
    root.map_or_else(
        || String::from("Source missing"),
        |root| format!("Source missing: {}", root.display()),
    )
}
