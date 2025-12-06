use super::*;

impl EguiController {
    /// Manually trigger a scan of the selected source.
    pub fn request_scan(&mut self) {
        if self.scan_in_progress {
            self.set_status("Scan already in progress", StatusTone::Info);
            return;
        }
        let Some(source) = self.current_source() else {
            self.set_status("Select a source to scan", StatusTone::Warning);
            return;
        };
        let (tx, rx) = std::sync::mpsc::channel();
        self.scan_rx = Some(rx);
        self.scan_in_progress = true;
        self.set_status(
            format!("Scanning {}", source.root.display()),
            StatusTone::Busy,
        );
        let source_id = source.id.clone();
        std::thread::spawn(move || {
            let result = (|| -> Result<crate::sample_sources::scanner::ScanStats, crate::sample_sources::scanner::ScanError> {
                let db = SourceDatabase::open(&source.root)?;
                crate::sample_sources::scanner::scan_once(&db)
            })();
            let _ = tx.send(ScanResult { source_id, result });
        });
    }

    pub(super) fn poll_scan(&mut self) {
        if let Some(rx) = &self.scan_rx {
            if let Ok(result) = rx.try_recv() {
                self.scan_in_progress = false;
                self.scan_rx = None;
                if Some(&result.source_id) != self.selected_source.as_ref() {
                    return;
                }
                match result.result {
                    Ok(stats) => {
                        self.set_status(
                            format!(
                                "Scan complete: {} added, {} updated, {} removed",
                                stats.added, stats.updated, stats.removed
                            ),
                            StatusTone::Info,
                        );
                        if let Some(source) = self.current_source() {
                            self.wav_cache.remove(&source.id);
                            self.label_cache.remove(&source.id);
                        }
                        self.queue_wav_load();
                    }
                    Err(err) => {
                        self.set_status(format!("Scan failed: {err}"), StatusTone::Error);
                    }
                }
            }
        }
    }
}
