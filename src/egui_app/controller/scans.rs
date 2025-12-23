use super::*;
use crate::egui_app::state::ProgressTaskKind;
use crate::sample_sources::scanner::ScanMode;
use std::sync::{Arc, atomic::AtomicBool};

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
        if self.runtime.jobs.scan_in_progress() {
            self.set_status_message(StatusMessage::ScanAlreadyRunning);
            return;
        }
        let Some(source) = self.current_source() else {
            self.set_status_message(StatusMessage::SelectSourceToScan);
            return;
        };
        self.prepare_for_scan(&source, mode);
        let status_label = match mode {
            ScanMode::Quick => "Quick sync",
            ScanMode::Hard => "Hard sync",
        };
        self.set_status_message(StatusMessage::custom(
            format!("{status_label} on {}", source.root.display()),
            StatusTone::Busy,
        ));
        self.show_status_progress(ProgressTaskKind::Scan, status_label, 0, true);
        self.update_progress_detail("Scanning audio files…");

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
