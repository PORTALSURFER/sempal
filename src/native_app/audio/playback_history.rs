use std::{
    path::PathBuf,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use radiant::prelude as ui;

use crate::native_app::app::{GuiMessage, NativeAppState, emit_gui_action};

mod navigation;
mod worker;
use worker::persist_last_played;

const LAST_PLAYED_PERSIST_DEBOUNCE: Duration = Duration::from_millis(350);

#[derive(Clone, Debug, PartialEq, Eq)]
pub(in crate::native_app) struct LastPlayedPersistResult {
    pub(in crate::native_app) file_id: String,
    pub(in crate::native_app) result: Result<(), String>,
}

pub(in crate::native_app) use navigation::PlaybackNavigationHistory;
pub(in crate::native_app) use worker::LastPlayedPersistRequest;

impl NativeAppState {
    pub(in crate::native_app) fn record_selected_sample_last_played(
        &mut self,
        context: &mut ui::UiUpdateContext<GuiMessage>,
    ) {
        let Some(file_id) = self
            .library
            .folder_browser
            .selected_file_id()
            .map(str::to_owned)
        else {
            return;
        };
        self.record_sample_last_played(file_id, context);
    }

    pub(in crate::native_app) fn record_sample_last_played(
        &mut self,
        file_id: String,
        context: &mut ui::UiUpdateContext<GuiMessage>,
    ) {
        let absolute_path = PathBuf::from(&file_id);
        let Some((source_root, source_database_root, relative_path)) = self
            .library
            .folder_browser
            .source_database_relative_file_path(&absolute_path)
        else {
            return;
        };
        let played_at = now_unix_secs();
        self.library
            .folder_browser
            .set_file_last_played_at(&absolute_path, played_at);
        let request = LastPlayedPersistRequest {
            file_id,
            source_root,
            source_database_root,
            relative_path,
            played_at,
        };
        self.schedule_last_played_persist(request, LAST_PLAYED_PERSIST_DEBOUNCE, context);
    }

    fn schedule_last_played_persist(
        &mut self,
        request: LastPlayedPersistRequest,
        delay: Duration,
        context: &mut ui::UiUpdateContext<GuiMessage>,
    ) {
        self.audio.pending_last_played_persist = None;
        #[cfg(test)]
        let scheduled_request = request.clone();
        context.after_latest(
            &mut self.audio.last_played_persist_task,
            delay,
            move |ticket| GuiMessage::LastPlayedPersistReady { ticket, request },
        );
        #[cfg(test)]
        if let Some(ticket) = self.audio.last_played_persist_task.active() {
            self.record_scheduled_timer_message(GuiMessage::LastPlayedPersistReady {
                ticket,
                request: scheduled_request,
            });
        }
    }

    pub(in crate::native_app) fn start_last_played_persist(
        &mut self,
        ticket: ui::TaskTicket,
        request: LastPlayedPersistRequest,
        context: &mut ui::UiUpdateContext<GuiMessage>,
    ) {
        if !self.audio.last_played_persist_task.finish(ticket) {
            return;
        }
        if let Some(reason) = self.last_played_persist_wait_reason() {
            tracing::debug!(
                target: "wavecrate::debug::sample_load",
                event = "playback.last_played.persist_deferred",
                reason,
                source = request.file_id.as_str(),
                "Deferred last-played persistence while playback/navigation is active"
            );
            self.audio.pending_last_played_persist = Some(request);
            return;
        }
        self.queue_last_played_persist(request, context);
    }

    pub(in crate::native_app) fn flush_deferred_last_played_persist_if_idle(
        &mut self,
        context: &mut ui::UiUpdateContext<GuiMessage>,
    ) {
        if self.audio.pending_last_played_persist.is_none()
            || self.last_played_persist_wait_reason().is_some()
        {
            return;
        }
        if let Some(request) = self.audio.pending_last_played_persist.take() {
            self.queue_last_played_persist(request, context);
        }
    }

    fn queue_last_played_persist(
        &mut self,
        request: LastPlayedPersistRequest,
        context: &mut ui::UiUpdateContext<GuiMessage>,
    ) {
        context
            .business()
            .priority("gui-last-played-persist", ui::TaskPriority::Idle)
            .run(
                move |_| persist_last_played(request),
                GuiMessage::LastPlayedPersisted,
            );
    }

    pub(in crate::native_app) fn finish_last_played_persist(
        &mut self,
        result: LastPlayedPersistResult,
    ) {
        if let Err(error) = result.result {
            if last_played_persist_skip_is_expected(error.as_str()) {
                emit_gui_action(
                    "playback.last_played.persist",
                    Some("browser"),
                    Some(result.file_id.as_str()),
                    "skipped",
                    std::time::Instant::now(),
                    Some(&error),
                );
                return;
            }
            self.ui.status.sample = format!("Last played not saved: {error}");
            emit_gui_action(
                "playback.last_played.persist",
                Some("browser"),
                Some(result.file_id.as_str()),
                "error",
                std::time::Instant::now(),
                Some(&error),
            );
        }
    }

    fn last_played_persist_wait_reason(&self) -> Option<&'static str> {
        if self.waveform_sample_load_active() {
            return Some("sample_load");
        }
        if self.audio.pending_playback_start.is_some() {
            return Some("pending_playback");
        }
        if self.audio.sample_playback_session.is_some() {
            return Some("sample_playback_session");
        }
        if self.waveform.current.is_playing() {
            return Some("playback");
        }
        if self.audio.playback_progress.active {
            return Some("playback_progress");
        }
        None
    }
}

fn now_unix_secs() -> i64 {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or(Duration::ZERO)
        .as_secs();
    i64::try_from(secs).unwrap_or(i64::MAX)
}

fn last_played_persist_skip_is_expected(error: &str) -> bool {
    error.contains("Database is busy")
}
