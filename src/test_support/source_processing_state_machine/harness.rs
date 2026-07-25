use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver},
    },
    time::Duration,
};

use rusqlite::Connection;
use wavecrate_scan::sample_sources::SourceDbError;
use wavecrate_scan::{
    CommittedSourceDelta, ScanError, ScanMode, complete_deferred_hashes, scan_with_progress,
    sync_paths,
};

use crate::{
    native_source_fixture::{FixtureName, FixtureProfile, FixtureProvisionRequest, provision},
    sample_sources::{SampleSource, SourceId},
};

use super::super::{
    DatabasePhase, SourceAuditLifecycleCause, SourceProcessingEvent, SourceProcessingSupervisor,
};
use super::{
    Event, FailureBoundary, FailureSnapshot, ReferenceModel, ScanCause,
    invariants::filesystem_inventory,
};

pub(super) struct StateMachineHarness {
    pub(super) _config_base: tempfile::TempDir,
    pub(super) source: SampleSource,
    pub(super) escape_target: PathBuf,
    pub(super) model: ReferenceModel,
    pub(super) supervisor: Option<SourceProcessingSupervisor>,
    pub(super) supervisor_events: Option<Receiver<SourceProcessingEvent>>,
    pub(super) template: Vec<u8>,
    pub(super) last_revision: u64,
    pub(super) accepted_publications: BTreeSet<(u64, u64, ScanCause)>,
    pub(super) accepted_revisions: Vec<String>,
    pub(super) next_failure: Option<FailureBoundary>,
    pub(super) retired_roots: Vec<PathBuf>,
    pub(super) observable_commits: u64,
    pub(super) pending_publication_retries: Vec<(CommittedSourceDelta, ScanCause)>,
    pub(super) expected_supervisor_publications: BTreeSet<(u64, u64, ScanCause)>,
    pub(super) observed_supervisor_publications: BTreeSet<(u64, u64, ScanCause)>,
    pub(super) stale_supervisor_publications: BTreeSet<(u64, u64, ScanCause)>,
    pub(super) expected_rejected_supervisor_publications: BTreeSet<(u64, u64, ScanCause)>,
    pub(super) rejected_supervisor_publications: BTreeSet<(u64, u64, ScanCause)>,
    pub(super) last_actual_output_revisions: BTreeMap<u64, (i64, i64)>,
    pub(super) actual_queue_admissions: u64,
    pub(super) max_actual_pending_scopes: usize,
}

impl StateMachineHarness {
    pub(super) fn new(with_supervisor: bool) -> Result<Self, String> {
        let config_base = tempfile::tempdir().map_err(|error| error.to_string())?;
        let manifest = provision(&FixtureProvisionRequest {
            config_base: config_base.path().to_path_buf(),
            fixture: FixtureName::SmallMultiSource,
            profile: FixtureProfile::AutomatedTests,
            reset: true,
        })?;
        let source_manifest = manifest
            .sources
            .iter()
            .find(|source| source.directory_name == "source-beta")
            .ok_or_else(|| String::from("small fixture is missing source-beta"))?;
        let escape_target = manifest
            .sources
            .iter()
            .find(|source| source.directory_name == "source-alpha")
            .map(|source| source.root.clone())
            .ok_or_else(|| String::from("small fixture is missing source-alpha"))?;
        let source = SampleSource::new_with_id(
            SourceId::from_string(source_manifest.source_id.clone()),
            source_manifest.root.clone(),
        );
        let template = fs::read(source.root.join("mutable/change-me.wav"))
            .map_err(|error| format!("read deterministic mutation template: {error}"))?;
        let files = filesystem_inventory(&source.root)?;
        let database = source.open_db().map_err(|error| error.to_string())?;
        let last_revision = database
            .get_wav_paths_revision()
            .map_err(|error| error.to_string())?;
        let (supervisor, supervisor_events) = if with_supervisor {
            let (supervisor, events) = start_observed_supervisor(&source);
            (Some(supervisor), Some(events))
        } else {
            (None, None)
        };
        Ok(Self {
            _config_base: config_base,
            source,
            escape_target,
            model: ReferenceModel::new(files),
            supervisor,
            supervisor_events,
            template,
            last_revision,
            accepted_publications: BTreeSet::new(),
            accepted_revisions: vec![format!("1:{last_revision}")],
            next_failure: None,
            retired_roots: Vec::new(),
            observable_commits: 0,
            pending_publication_retries: Vec::new(),
            expected_supervisor_publications: BTreeSet::new(),
            observed_supervisor_publications: BTreeSet::new(),
            stale_supervisor_publications: BTreeSet::new(),
            expected_rejected_supervisor_publications: BTreeSet::new(),
            rejected_supervisor_publications: BTreeSet::new(),
            last_actual_output_revisions: BTreeMap::new(),
            actual_queue_admissions: 0,
            max_actual_pending_scopes: 0,
        })
    }

    pub(super) fn run(mut self, events: &[Event]) -> Result<(), FailureSnapshot> {
        if let Err(message) = self.initialize() {
            return Err(self.failure(0, &Event::Quiesce, message));
        }
        for (event_index, event) in events.iter().enumerate() {
            if let Err(message) = self.apply(event) {
                return Err(self.failure(event_index, event, message));
            }
        }
        if let Err(message) = self.quiesce() {
            return Err(self.failure(events.len(), &Event::Quiesce, message));
        }
        if let Some(mut supervisor) = self.supervisor.take() {
            let report = supervisor.shutdown();
            if report["joined"] != true {
                return Err(self.failure(
                    events.len(),
                    &Event::Quiesce,
                    String::from("source-processing supervisor did not join"),
                ));
            }
            if let Err(message) = self.collect_publications_from(&supervisor) {
                return Err(self.failure(events.len(), &Event::Quiesce, message));
            }
            if let Err(message) = self.assert_actual_publications() {
                return Err(self.failure(events.len(), &Event::Quiesce, message));
            }
        }
        Ok(())
    }

    fn initialize(&mut self) -> Result<(), String> {
        if self.supervisor.is_none() {
            return self.quiesce();
        }
        self.next_failure = None;
        self.model.queue(ScanCause::Retry);
        self.flush(ScanCause::Retry)?;
        self.wait_for_supervisor_convergence()?;
        self.create(6, false)?;
        self.flush(ScanCause::Foreground)?;
        self.wait_for_supervisor_convergence()?;
        self.assert_actual_publications()
    }

    fn apply(&mut self, event: &Event) -> Result<(), String> {
        match *event {
            Event::Create { slot } => self.create(slot, false),
            Event::SameSizeModify { slot } => self.modify(slot),
            Event::Move { slot, nested } => self.move_file(slot, nested),
            Event::Delete { slot } => self.delete(slot),
            Event::NestedDirectoryChange { slot } => self.create(slot, true),
            Event::WatcherBatch => self.flush(ScanCause::Watcher),
            Event::WatcherOverflow => {
                self.model.queue(ScanCause::WatcherOverflow);
                self.flush(ScanCause::WatcherOverflow)
            }
            Event::FocusChanged { active } => {
                self.model.focused = active;
                if let Some(supervisor) = &self.supervisor {
                    supervisor.set_foreground_activity(!active);
                    if active {
                        supervisor.request_lifecycle_audit_probe(
                            SourceAuditLifecycleCause::FocusRegained,
                            &[],
                        );
                    }
                }
                if active {
                    self.model.queue(ScanCause::Focus);
                }
                self.assert_queue_bound()
            }
            Event::ExplicitRefresh => {
                self.model.queue(ScanCause::Foreground);
                self.flush(ScanCause::Foreground)
            }
            Event::Cancel => self.cancel_scan(),
            Event::ShutdownRestart => self.shutdown_restart(),
            Event::SourceRemoveReadd => self.remove_readd(),
            Event::RootOfflineOnline => self.root_offline_online(),
            Event::RootReplacement => self.root_replacement(),
            Event::PartialEnumeration => self.partial_enumeration(),
            Event::SymlinkEscape => self.symlink_escape(),
            Event::DatabaseBusy => {
                self.next_failure = Some(FailureBoundary::Transaction);
                self.flush(ScanCause::Watcher)
            }
            Event::InjectFailure { boundary } => {
                self.next_failure = Some(boundary);
                Ok(())
            }
            Event::Quiesce => self.quiesce(),
        }
    }

    pub(super) fn flush(&mut self, cause: ScanCause) -> Result<(), String> {
        if !self.model.root_online || !self.model.source_configured {
            self.model.queue(ScanCause::Retry);
            return Ok(());
        }
        let effective_cause = self.authoritative_cause(cause);
        let reject_transaction = self.take_failure(FailureBoundary::Transaction);
        let reject_watcher_delivery =
            cause == ScanCause::Watcher && self.take_failure(FailureBoundary::WatcherDelivery);
        let scan_permit = self.supervisor.as_ref().and_then(|supervisor| {
            supervisor
                .budget_handle()
                .acquire_scan(self.source.id.as_str())
        });
        let scan_writer = scan_permit.as_ref().map(|permit| permit.scan_writer());
        let _scan_writer_guard = scan_writer
            .as_ref()
            .map(|writer| writer.lock(DatabasePhase::SerialCompatibility));
        let database = self.database()?;
        let transaction_lock = if reject_transaction {
            database
                .set_busy_timeout_for_tests(Duration::ZERO)
                .map_err(|error| format!("configure transaction contention: {error}"))?;
            let path = self
                .source
                .db_path()
                .map_err(|error| format!("resolve transaction database: {error}"))?;
            let lock = Connection::open(path)
                .map_err(|error| format!("open transaction lock: {error}"))?;
            lock.busy_timeout(Duration::ZERO)
                .map_err(|error| format!("configure transaction lock: {error}"))?;
            lock.execute_batch("BEGIN IMMEDIATE")
                .map_err(|error| format!("acquire transaction lock: {error}"))?;
            Some(lock)
        } else {
            None
        };
        let pending_paths = self
            .model
            .watcher_paths
            .iter()
            .map(PathBuf::from)
            .collect::<Vec<_>>();
        let reject_hashing = self.take_failure(FailureBoundary::Hashing);
        let scan_result = if reject_hashing {
            let cancel = AtomicBool::new(false);
            let mut progressed = false;
            let result =
                scan_with_progress(&database, ScanMode::Quick, Some(&cancel), &mut |_, _| {
                    if progressed {
                        cancel.store(true, Ordering::Release);
                    }
                    progressed = true;
                });
            self.model.queue(ScanCause::Retry);
            self.model.retry_count = self.model.retry_count.saturating_add(1);
            result
        } else if effective_cause == ScanCause::Watcher && !pending_paths.is_empty() {
            sync_paths(&database, &pending_paths)
        } else {
            scan_with_progress(&database, ScanMode::Quick, None, &mut |_, _| {})
        };
        if reject_transaction {
            transaction_lock
                .as_ref()
                .expect("transaction injection must own its lock")
                .execute_batch("ROLLBACK")
                .map_err(|error| format!("release transaction lock: {error}"))?;
            return match scan_result {
                Err(ScanError::Db(SourceDbError::Busy)) => {
                    self.model.retry_count = self.model.retry_count.saturating_add(1);
                    self.model.queue(ScanCause::Retry);
                    Ok(())
                }
                Err(error) => Err(format!(
                    "transaction boundary returned unexpected failure: {error}"
                )),
                Ok(_) => Err(String::from(
                    "transaction boundary accepted an injected busy failure",
                )),
            };
        }
        let stats = match scan_result {
            Ok(stats) => stats,
            Err(ScanError::Incomplete { committed, .. }) if reject_hashing => *committed,
            Err(ScanError::Canceled) if reject_hashing => return Ok(()),
            Err(error) => return Err(format!("source reconciliation failed: {error}")),
        };
        let stats = complete_deferred_hashes(&database, stats)
            .map_err(|error| format!("complete deferred hashes: {error}"))?;
        let publication_lost = self.take_failure(FailureBoundary::Publication);
        self.accept_commit(effective_cause, &stats.committed_delta, publication_lost)?;
        self.admit_pending_publication_retries()?;
        if publication_lost && self.supervisor.is_none() {
            if !stats.committed_delta.is_empty() {
                self.pending_publication_retries
                    .push((stats.committed_delta.clone(), effective_cause));
            }
        } else {
            self.admit_supervisor_delta(
                &stats.committed_delta,
                effective_cause,
                reject_watcher_delivery,
                publication_lost,
            )?;
            if reject_watcher_delivery {
                self.model.retry_count = self.model.retry_count.saturating_add(1);
                self.model.queue(ScanCause::Retry);
            }
        }
        self.model.watcher_paths.clear();
        if effective_cause == ScanCause::Watcher {
            self.model.queued_causes.remove(&ScanCause::Watcher);
        } else {
            self.model.queued_causes.clear();
        }
        if publication_lost {
            self.model.queue(ScanCause::Retry);
            self.model.retry_count = self.model.retry_count.saturating_add(1);
        }
        if let Some(supervisor) = &self.supervisor {
            if stats.committed_delta.is_empty() {
                supervisor
                    .request_source_processing(self.source.id.as_str(), "state_machine_scan_noop");
            }
        }
        self.assert_committed_manifest(&database)
    }

    fn authoritative_cause(&self, requested: ScanCause) -> ScanCause {
        [
            ScanCause::Lifecycle,
            ScanCause::Restart,
            ScanCause::WatcherOverflow,
            ScanCause::Foreground,
            ScanCause::Focus,
            ScanCause::Retry,
        ]
        .into_iter()
        .find(|cause| self.model.queued_causes.contains(cause))
        .unwrap_or(requested)
    }

    fn quiesce(&mut self) -> Result<(), String> {
        self.require_online()?;
        self.next_failure = None;
        self.model.queue(ScanCause::Retry);
        self.flush(ScanCause::Retry)?;
        self.model.queued_causes.clear();
        self.model.watcher_paths.clear();
        let database = self.database()?;
        self.assert_committed_manifest(&database)?;
        if let Some(supervisor) = &self.supervisor {
            supervisor
                .request_source_processing(self.source.id.as_str(), "state_machine_quiescence");
        }
        if self.supervisor.is_some() {
            self.wait_for_supervisor_convergence()?;
        }
        self.assert_actual_publications()?;
        Ok(())
    }
}

pub(super) fn start_observed_supervisor(
    source: &SampleSource,
) -> (SourceProcessingSupervisor, Receiver<SourceProcessingEvent>) {
    let (event_sender, event_receiver) = mpsc::channel();
    (
        SourceProcessingSupervisor::start_with_event_sink(vec![source.clone()], event_sender),
        event_receiver,
    )
}
