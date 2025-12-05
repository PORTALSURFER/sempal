use super::*;

impl DropHandler {
    /// Launch a background scan for the given source when allowed by the tracker.
    pub(super) fn start_scan_for(&self, source: SampleSource, force: bool) {
        if *self.shutting_down.borrow() {
            return;
        }
        {
            let tracker = self.scan_tracker.borrow();
            if !tracker.can_start(&source.id, force) {
                if tracker.is_active(&source.id) {
                    if let Some(app) = self.app() {
                        self.set_status(&app, "Scan already in progress", StatusState::Info);
                    }
                } else if let Some(app) = self.app() {
                    self.set_status(&app, "Using existing scan results", StatusState::Info);
                }
                return;
            }
        }
        self.scan_tracker.borrow_mut().mark_started(&source.id);
        let tx = self.scan_tx.clone();
        if let Some(app) = self.app() {
            self.set_status(
                &app,
                format!("Scanning {}", source.root.display()),
                StatusState::Busy,
            );
        }
        thread::spawn(move || {
            let result = (|| -> Result<ScanStats, ScanError> {
                let db = SourceDatabase::open(&source.root)?;
                scan_once(&db)
            })();
            let _ = tx.send(ScanJobResult {
                source_id: source.id,
                result,
            });
        });
    }

    /// Start polling for completed scan jobs on a timer.
    pub(super) fn start_scan_polling(&self) {
        if *self.shutting_down.borrow() {
            return;
        }
        let poller = self.clone();
        self.scan_poll_timer.start(
            slint::TimerMode::Repeated,
            Duration::from_millis(200),
            move || poller.process_scan_queue(),
        );
    }

    /// Process any queued scan results.
    pub(super) fn process_scan_queue(&self) {
        let Some(app) = self.app() else {
            return;
        };
        while let Ok(message) = self.scan_rx.borrow().try_recv() {
            self.handle_scan_result(&app, message);
        }
    }

    fn handle_scan_result(&self, app: &HelloWorld, message: ScanJobResult) {
        if !self
            .sources
            .borrow()
            .iter()
            .any(|source| source.id == message.source_id)
        {
            self.scan_tracker.borrow_mut().forget(&message.source_id);
            return;
        }
        match message.result {
            Ok(stats) => {
                self.scan_tracker
                    .borrow_mut()
                    .mark_completed(&message.source_id);
                let state = if self.scan_tracker.borrow().has_active() {
                    StatusState::Busy
                } else {
                    StatusState::Info
                };
                self.set_status(
                    app,
                    format!(
                        "Scan complete: {} added, {} updated, {} removed",
                        stats.added, stats.updated, stats.removed
                    ),
                    state,
                );
                if self
                    .selected_source
                    .borrow()
                    .as_ref()
                    .is_some_and(|id| id == &message.source_id)
                {
                    self.refresh_wavs(app);
                }
            }
            Err(error) => {
                self.scan_tracker
                    .borrow_mut()
                    .mark_failed(&message.source_id);
                self.set_status(app, format!("Scan failed: {error}"), StatusState::Error);
            }
        }
    }
}
