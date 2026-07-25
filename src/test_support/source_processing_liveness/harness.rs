use super::*;

pub(super) struct LivenessHarness {
    pub(super) source_parent: tempfile::TempDir,
    pub(super) source: SampleSource,
    pub(super) supervisor: SourceProcessingSupervisor,
    pub(super) watcher: Option<GuiSourceWatcherHandle>,
    watcher_rx: Receiver<GuiMessage>,
    pub(super) watcher_stimulus: WatcherStimulus,
    watcher_events_committed: u64,
    pub(super) expected_source_generation: i64,
}

impl LivenessHarness {
    pub(super) fn new(source_id: &str) -> Self {
        let source_parent = tempfile::tempdir().expect("temporary liveness source parent");
        let source_root = source_parent.path().join("source");
        fs::create_dir(&source_root).expect("create liveness source root");
        write_test_wav(&source_root.join("kick.wav"), 0.0);
        let unique_source_id = format!("{source_id}-{}", uuid::Uuid::new_v4());
        let source =
            SampleSource::new_with_id(SourceId::from_string(unique_source_id), source_root)
                .protected();
        source.open_db().expect("create liveness source database");
        let (watcher_tx, watcher_rx) = std::sync::mpsc::channel();
        let watcher = GuiSourceWatcherHandle::spawn(vec![source.clone()], watcher_tx);
        watcher.wait_until_ready_for_tests();
        let supervisor = SourceProcessingSupervisor::start(vec![source.clone()]);
        Self {
            source_parent,
            source,
            supervisor,
            watcher: Some(watcher),
            watcher_rx,
            watcher_stimulus: WatcherStimulus::Startup,
            watcher_events_committed: 0,
            expected_source_generation: 1,
        }
    }

    pub(super) fn commit_targeted_paths(&mut self, paths: Vec<PathBuf>, stimulus: WatcherStimulus) {
        self.watcher
            .as_ref()
            .expect("active watcher")
            .inject_paths_for_tests(
                paths
                    .iter()
                    .map(|path| self.source.root.join(path))
                    .collect(),
            );
        self.drive_watcher_until(stimulus, |event_paths, overflowed| {
            overflowed
                || paths.iter().any(|expected| {
                    event_paths.iter().any(|observed| {
                        observed == expected
                            || expected.starts_with(observed)
                            || observed.starts_with(expected)
                    })
                })
        });
    }

    pub(super) fn commit_overflow_audit(&mut self, stimulus: WatcherStimulus) {
        self.watcher
            .as_ref()
            .expect("active watcher")
            .force_overflow_for_tests();
        self.drive_watcher_until(stimulus, |_paths, overflowed| overflowed);
    }

    pub(super) fn force_watcher_restart(&mut self) {
        self.watcher
            .as_ref()
            .expect("active watcher")
            .force_restart_for_tests();
        self.drive_watcher_until(WatcherStimulus::WatcherRestart, |_paths, overflowed| {
            overflowed
        });
    }

    pub(super) fn force_root_refresh(&mut self, stimulus: WatcherStimulus) {
        self.watcher
            .as_ref()
            .expect("active watcher")
            .force_root_refresh_for_tests();
        self.drive_watcher_until(stimulus, |_paths, overflowed| overflowed);
    }

    pub(super) fn commit_internal_mutation(
        &mut self,
        operation: FileMutationOperation,
        changes: Vec<FileMutationChange>,
    ) {
        let operation_id = self.watcher_events_committed.saturating_add(1);
        let committed = reconcile_file_mutation_for_liveness_test(
            self.source.clone(),
            operation_id,
            operation,
            changes,
        )
        .expect("commit Wavecrate-owned mutation");
        assert_eq!(committed.source_id, self.source.id.as_str());
        self.watcher_stimulus = WatcherStimulus::InternalMutation;
        self.watcher_events_committed = operation_id;
        self.expected_source_generation = self.current_manifest_generation();
        self.supervisor
            .wake_source(self.source.id.as_str(), "liveness_internal_mutation_commit");
    }

    pub(super) fn await_fully_ready(&self) -> ReadinessSnapshot {
        self.await_state(true, SourceAvailability::Active)
    }

    pub(super) fn await_availability(&self, availability: SourceAvailability) -> ReadinessSnapshot {
        self.await_state(false, availability)
    }

    fn await_state(
        &self,
        require_fully_ready: bool,
        expected_availability: SourceAvailability,
    ) -> ReadinessSnapshot {
        let deadline = Instant::now() + LIVENESS_TIMEOUT;
        let mut silent_idle_confirmations = 0;
        loop {
            if let Some(snapshot) = readiness_snapshot(&self.source) {
                let runtime = runtime_observation(&self.supervisor, self.source.id.as_str());
                let ready = if expected_availability == SourceAvailability::Active {
                    if require_fully_ready {
                        snapshot.is_fully_ready()
                    } else {
                        snapshot.is_converged()
                    }
                } else {
                    snapshot.availability == expected_availability
                };
                if ready && snapshot.source_generation >= self.expected_source_generation {
                    if require_fully_ready {
                        assert_exact_artifact_coverage(&self.source, &snapshot);
                    }
                    return snapshot;
                }

                if silently_idle(&snapshot, &runtime) {
                    silent_idle_confirmations += 1;
                    if silent_idle_confirmations >= SILENT_IDLE_CONFIRMATIONS {
                        panic!(
                            "source processing became silently idle:\n{}",
                            diagnostic_json(
                                &self.source,
                                &self.supervisor,
                                self.watcher_stimulus,
                                self.watcher_events_committed,
                            )
                        );
                    }
                } else {
                    silent_idle_confirmations = 0;
                }
            }
            if Instant::now() >= deadline {
                panic!(
                    "source processing did not reach the expected state:\n{}",
                    diagnostic_json(
                        &self.source,
                        &self.supervisor,
                        self.watcher_stimulus,
                        self.watcher_events_committed,
                    )
                );
            }
            thread::sleep(POLL_INTERVAL);
        }
    }

    pub(super) fn shutdown(&mut self) -> serde_json::Value {
        self.watcher.take();
        self.supervisor.shutdown()
    }

    pub(super) fn restart_runtime(&mut self) {
        let (watcher_tx, watcher_rx) = std::sync::mpsc::channel();
        self.watcher = Some(GuiSourceWatcherHandle::spawn(
            vec![self.source.clone()],
            watcher_tx,
        ));
        self.watcher
            .as_ref()
            .expect("restarted watcher")
            .wait_until_ready_for_tests();
        self.watcher_rx = watcher_rx;
        self.supervisor = SourceProcessingSupervisor::start(vec![self.source.clone()]);
    }

    fn current_manifest_generation(&self) -> i64 {
        open_connection(&self.source)
            .and_then(|connection| {
                connection
                    .query_row(
                        "SELECT CAST(value AS INTEGER)
                         FROM metadata
                         WHERE key = ?1",
                        [META_WAV_PATHS_REVISION],
                        |row| row.get(0),
                    )
                    .map_err(|error| error.to_string())
            })
            .expect("read committed manifest generation")
    }

    fn drive_watcher_until(
        &mut self,
        stimulus: WatcherStimulus,
        accepts: impl Fn(&[PathBuf], bool) -> bool,
    ) {
        let deadline = Instant::now() + LIVENESS_TIMEOUT;
        loop {
            let remaining = deadline.saturating_duration_since(Instant::now());
            let message = self
                .watcher_rx
                .recv_timeout(remaining.min(Duration::from_secs(1)));
            let Ok(GuiMessage::SourceFilesystemChanged {
                source_id,
                paths,
                overflowed,
                source_root_available: reported_source_root_available,
                ..
            }) = message
            else {
                if Instant::now() >= deadline {
                    panic!("real source watcher did not publish the expected {stimulus:?} event");
                }
                continue;
            };
            if source_id != self.source.id.as_str() || !accepts(&paths, overflowed) {
                continue;
            }
            let source_root_available = self.source.root.is_dir();
            if source_root_available != reported_source_root_available {
                tracing::debug!(
                    source_id,
                    reported_source_root_available,
                    source_root_available,
                    "Liveness harness observed root availability change after watcher publication"
                );
            }
            if source_root_available {
                if overflowed {
                    let database = self
                        .source
                        .open_db()
                        .expect("open source for watcher overflow audit");
                    audit_source_and_record(&database, None, usize::MAX, now_epoch_seconds())
                        .expect("watcher overflow audit must commit");
                } else {
                    let result = sync_source_database_paths(
                        source_id.clone(),
                        self.source.root.clone(),
                        self.source.database_root().expect("source database root"),
                        paths.clone(),
                        paths.len(),
                        &AtomicBool::new(false),
                    );
                    let success = result.result.expect("watcher targeted reconciliation");
                    assert!(
                        success.incomplete_error.is_none(),
                        "watcher targeted reconciliation must be complete: {:?}",
                        success.incomplete_error
                    );
                }
            }
            self.watcher_stimulus = stimulus;
            self.watcher_events_committed = self.watcher_events_committed.saturating_add(1);
            if source_root_available {
                self.expected_source_generation = self.current_manifest_generation();
            }
            self.supervisor
                .wake_source(self.source.id.as_str(), "liveness_real_watcher_commit");
            return;
        }
    }
}
