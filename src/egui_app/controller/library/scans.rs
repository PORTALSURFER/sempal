use super::*;
use crate::sample_sources::scanner::ScanMode;
use std::sync::{Arc, atomic::AtomicBool};
use std::time::{Duration, Instant};

const WATCHER_SYNC_INTERVAL: Duration = Duration::from_secs(2);

impl EguiController {
    /// Trigger a quick sync (incremental scan) of the selected source.
    pub fn request_quick_sync(&mut self) {
        self.request_scan_with_mode(ScanMode::Quick, ScanKind::Manual);
    }

    /// Trigger a hard sync (full rescan that prunes missing rows) of the selected source.
    pub fn request_hard_sync(&mut self) {
        self.request_scan_with_mode(ScanMode::Hard, ScanKind::Manual);
    }

    /// Trigger a periodic quick sync to keep the current source in sync with disk.
    pub(crate) fn request_auto_quick_sync_if_due(&mut self, min_interval: Duration) {
        let Some(source) = self.current_source() else {
            return;
        };
        self.request_auto_quick_sync_for_source_if_due(&source.id, min_interval);
    }

    /// Trigger a quick sync for a source based on a file watcher event.
    pub(crate) fn handle_source_watch_event(&mut self, source_id: &SourceId) {
        self.request_auto_quick_sync_for_source_if_due(source_id, WATCHER_SYNC_INTERVAL);
    }

    /// Trigger a quick sync for a specific source when the debounce interval elapses.
    pub(crate) fn request_auto_quick_sync_for_source_if_due(
        &mut self,
        source_id: &SourceId,
        min_interval: Duration,
    ) {
        let Some(source) = self
            .library
            .sources
            .iter()
            .find(|source| &source.id == source_id)
            .cloned()
        else {
            return;
        };
        self.request_auto_quick_sync_for_source(source, min_interval);
    }

    fn request_auto_quick_sync_for_source(
        &mut self,
        source: SampleSource,
        min_interval: Duration,
    ) {
        if self.runtime.jobs.scan_in_progress() {
            return;
        }
        if self.library.missing.sources.contains(&source.id) {
            return;
        }
        let now = Instant::now();
        let last_sync = self.runtime.auto_sync_last_by_source.get(&source.id).copied();
        if !auto_sync_due(last_sync, now, min_interval) {
            return;
        }
        self.runtime
            .auto_sync_last_by_source
            .insert(source.id.clone(), now);
        self.request_scan_for_source(&source, ScanMode::Quick, ScanKind::Auto);
    }

    fn request_scan_with_mode(&mut self, mode: ScanMode, kind: ScanKind) {
        let Some(source) = self.current_source() else {
            self.set_status_message(StatusMessage::SelectSourceToScan);
            return;
        };
        self.request_scan_for_source(&source, mode, kind);
    }

    fn request_scan_for_source(&mut self, source: &SampleSource, mode: ScanMode, kind: ScanKind) {
        if self.runtime.jobs.scan_in_progress() {
            if matches!(kind, ScanKind::Manual) {
                self.set_status_message(StatusMessage::ScanAlreadyRunning);
            }
            return;
        }
        self.prepare_for_scan(source, mode);
        if matches!(kind, ScanKind::Manual) {
            self.begin_scan_progress(mode, source);
        }

        let cancel = Arc::new(AtomicBool::new(false));
        let (tx, rx) = std::sync::mpsc::channel();
        self.runtime.jobs.start_scan(rx, cancel.clone());
        let source_id = source.id.clone();
        let root = source.root.clone();
        std::thread::spawn(move || {
            let result = (|| -> Result<
                crate::sample_sources::scanner::ScanStats,
                crate::sample_sources::scanner::ScanError,
            > {
                let db = SourceDatabase::open(&root)?;
                crate::sample_sources::scanner::scan_with_progress(
                    &db,
                    mode,
                    Some(cancel.as_ref()),
                    &mut |completed, path| {
                        if completed == 1 || completed % 128 == 0 {
                            let _ = tx.send(ScanJobMessage::Progress {
                                completed,
                                detail: Some(path.display().to_string()),
                            });
                        }
                    },
                )
            })();
            let _ = tx.send(ScanJobMessage::Finished(ScanResult {
                source_id,
                mode,
                kind,
                result,
            }));
        });
    }

    fn prepare_for_scan(&mut self, source: &SampleSource, mode: ScanMode) {
        if matches!(mode, ScanMode::Hard) {
            let mut invalidator = source_cache_invalidator::SourceCacheInvalidator::new_from_state(
                &mut self.cache,
                &mut self.ui_cache,
                &mut self.library.missing,
            );
            invalidator.invalidate_wav_related(&source.id);
        }
    }
}

fn auto_sync_due(last_sync: Option<Instant>, now: Instant, min_interval: Duration) -> bool {
    last_sync.map_or(true, |last| now.saturating_duration_since(last) >= min_interval)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn auto_sync_due_respects_interval() {
        let now = Instant::now();
        assert!(auto_sync_due(None, now, Duration::from_secs(5)));
        assert!(!auto_sync_due(
            Some(now),
            now + Duration::from_secs(3),
            Duration::from_secs(5)
        ));
        assert!(auto_sync_due(
            Some(now),
            now + Duration::from_secs(6),
            Duration::from_secs(5)
        ));
    }
}
