use super::*;
use crate::sample_sources::scanner::ScanMode;

impl EguiController {
    /// Trigger a quick sync (incremental scan) of the selected source.
    pub fn request_quick_sync(&mut self) {
        self.request_scan_with_mode(ScanMode::Quick);
    }

    /// Trigger a hard sync (full rescan that prunes missing rows) of the selected source.
    pub fn request_hard_sync(&mut self) {
        self.request_scan_with_mode(ScanMode::Hard);
    }

    fn request_scan_with_mode(&mut self, mode: ScanMode) {
        if self.runtime.jobs.scan_in_progress {
            self.set_status("Scan already in progress", StatusTone::Info);
            return;
        }
        let Some(source) = self.current_source() else {
            self.set_status("Select a source to scan", StatusTone::Warning);
            return;
        };
        self.prepare_for_scan(&source, mode);
        let (tx, rx) = std::sync::mpsc::channel();
        self.runtime.jobs.scan_rx = Some(rx);
        self.runtime.jobs.scan_in_progress = true;
        let status_label = match mode {
            ScanMode::Quick => "Quick sync",
            ScanMode::Hard => "Hard sync",
        };
        self.set_status(
            format!("{status_label} on {}", source.root.display()),
            StatusTone::Busy,
        );
        let source_id = source.id.clone();
        std::thread::spawn(move || {
            let result = (|| -> Result<
                crate::sample_sources::scanner::ScanStats,
                crate::sample_sources::scanner::ScanError,
            > {
                let db = SourceDatabase::open(&source.root)?;
                match mode {
                    ScanMode::Quick => crate::sample_sources::scanner::scan_once(&db),
                    ScanMode::Hard => crate::sample_sources::scanner::hard_rescan(&db),
                }
            })();
            let _ = tx.send(ScanResult {
                source_id,
                mode,
                result,
            });
        });
    }

    pub(super) fn poll_scan(&mut self) {
        if let Some(rx) = &self.runtime.jobs.scan_rx
            && let Ok(result) = rx.try_recv()
        {
            self.runtime.jobs.scan_in_progress = false;
            self.runtime.jobs.scan_rx = None;
            if Some(&result.source_id) != self.selection_state.ctx.selected_source.as_ref() {
                return;
            }
            let label = match result.mode {
                ScanMode::Quick => "Quick sync",
                ScanMode::Hard => "Hard sync",
            };
            match result.result {
                Ok(stats) => {
                    self.set_status(
                        format!(
                            "{label} complete: {} added, {} updated, {} missing",
                            stats.added, stats.updated, stats.missing
                        ),
                        StatusTone::Info,
                    );
	                    if let Some(source) = self.current_source() {
	                        let mut invalidator = source_cache_invalidator::SourceCacheInvalidator::new(
	                            &mut self.cache.db,
	                            &mut self.cache.wav.entries,
	                            &mut self.cache.wav.lookup,
	                            &mut self.ui_cache.browser.labels,
	                            &mut self.library.missing.wavs,
	                            &mut self.ui_cache.folders.models,
	                        );
	                        invalidator.invalidate_wav_related(&source.id);
	                    }
                    self.queue_wav_load();
                }
                Err(err) => self.set_status(format!("{label} failed: {err}"), StatusTone::Error),
            }
        }
    }

    fn prepare_for_scan(&mut self, source: &SampleSource, mode: ScanMode) {
	        if matches!(mode, ScanMode::Hard) {
	            let mut invalidator = source_cache_invalidator::SourceCacheInvalidator::new(
	                &mut self.cache.db,
	                &mut self.cache.wav.entries,
	                &mut self.cache.wav.lookup,
	                &mut self.ui_cache.browser.labels,
	                &mut self.library.missing.wavs,
	                &mut self.ui_cache.folders.models,
	            );
	            invalidator.invalidate_wav_related(&source.id);
	        }
	    }
}
