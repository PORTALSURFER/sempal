use super::{
    Arc, AtomicBool, ContentAuditActivity, ExecutionOutcome, InFlightWorkGuard, Instant,
    RuntimeCandidate, Shared, execute_candidate,
};
use crate::native_app::source_processing::scheduler::BudgetPermit;
use std::{
    collections::BTreeMap,
    sync::mpsc::{Receiver, SyncSender, TryRecvError, TrySendError, sync_channel},
    thread::{self, JoinHandle},
};

pub(super) struct ExecutionRequest {
    pub(super) candidate: RuntimeCandidate,
    pub(super) permit: BudgetPermit,
    pub(super) cancel: Arc<AtomicBool>,
    pub(super) in_flight: InFlightWorkGuard,
}

pub(super) struct ExecutionResult {
    pub(super) candidate: RuntimeCandidate,
    pub(super) permit: BudgetPermit,
    pub(super) lifecycle_generation: u64,
    pub(super) result: Result<ExecutionOutcome, String>,
    pub(super) elapsed_ms: f64,
    pub(super) in_flight: InFlightWorkGuard,
}

/// Fixed-size, bounded execution pool owned by the one source-processing coordinator.
pub(super) struct ExecutionPool {
    request_tx: Option<SyncSender<ExecutionRequest>>,
    result_rx: Receiver<ExecutionResult>,
    workers: Vec<JoinHandle<()>>,
    capacity: usize,
    in_flight_by_source: BTreeMap<String, usize>,
}

impl ExecutionPool {
    pub(super) fn new(shared: &Arc<Shared>, worker_count: usize) -> Self {
        let worker_count = worker_count.max(1);
        let (request_tx, request_rx) = sync_channel::<ExecutionRequest>(worker_count);
        let request_rx = Arc::new(std::sync::Mutex::new(request_rx));
        let (result_tx, result_rx) = sync_channel::<ExecutionResult>(worker_count);
        let mut workers = Vec::with_capacity(worker_count);
        for index in 0..worker_count {
            let shared = Arc::clone(shared);
            let request_rx = Arc::clone(&request_rx);
            let result_tx = result_tx.clone();
            workers.push(
                thread::Builder::new()
                    .name(format!("wavecrate-source-execution-{index}"))
                    .spawn(move || run_worker(shared, request_rx, result_tx))
                    .expect("spawn bounded source execution worker"),
            );
        }
        Self {
            request_tx: Some(request_tx),
            result_rx,
            workers,
            capacity: worker_count,
            in_flight_by_source: BTreeMap::new(),
        }
    }

    pub(super) fn try_dispatch(
        &mut self,
        request: ExecutionRequest,
    ) -> Result<(), ExecutionRequest> {
        let source_id = request.candidate.source.id.as_str().to_string();
        match self
            .request_tx
            .as_ref()
            .expect("execution pool admission is open")
            .try_send(request)
        {
            Ok(()) => {
                *self.in_flight_by_source.entry(source_id).or_default() += 1;
                Ok(())
            }
            Err(TrySendError::Full(request) | TrySendError::Disconnected(request)) => Err(request),
        }
    }

    pub(super) fn try_result(&mut self) -> Option<ExecutionResult> {
        match self.result_rx.try_recv() {
            Ok(result) => {
                let source_id = result.candidate.source.id.as_str();
                if let Some(count) = self.in_flight_by_source.get_mut(source_id) {
                    *count = count.saturating_sub(1);
                    if *count == 0 {
                        self.in_flight_by_source.remove(source_id);
                    }
                }
                Some(result)
            }
            Err(TryRecvError::Empty | TryRecvError::Disconnected) => None,
        }
    }

    pub(super) fn capacity(&self) -> usize {
        self.capacity
    }

    pub(super) fn in_flight_count(&self) -> usize {
        self.in_flight_by_source.values().sum()
    }

    pub(super) fn source_is_in_flight(&self, source_id: &str) -> bool {
        self.in_flight_by_source.contains_key(source_id)
    }

    pub(super) fn shutdown(&mut self) -> bool {
        self.request_tx.take();
        join_all_workers(self.workers.drain(..))
    }
}

fn join_all_workers(workers: impl IntoIterator<Item = JoinHandle<()>>) -> bool {
    let mut joined = true;
    for worker in workers {
        joined = worker.join().is_ok() && joined;
    }
    joined
}

fn run_worker(
    shared: Arc<Shared>,
    request_rx: Arc<std::sync::Mutex<Receiver<ExecutionRequest>>>,
    result_tx: SyncSender<ExecutionResult>,
) {
    loop {
        let request = {
            let receiver = request_rx
                .lock()
                .unwrap_or_else(|poison| poison.into_inner());
            receiver.recv()
        };
        let Ok(request) = request else {
            return;
        };
        {
            let mut telemetry = shared.telemetry();
            telemetry.active_execution_workers =
                telemetry.active_execution_workers.saturating_add(1);
            telemetry.peak_execution_workers = telemetry
                .peak_execution_workers
                .max(telemetry.active_execution_workers);
        }
        #[cfg(test)]
        if shared
            .execution_workers_paused
            .load(std::sync::atomic::Ordering::Acquire)
        {
            shared
                .execution_workers_started
                .fetch_add(1, std::sync::atomic::Ordering::AcqRel);
            while shared
                .execution_workers_paused
                .load(std::sync::atomic::Ordering::Acquire)
                && !shared.cancel.load(std::sync::atomic::Ordering::Acquire)
            {
                thread::sleep(std::time::Duration::from_millis(1));
            }
        }
        let lifecycle_generation = request.in_flight.lifecycle_generation;
        let content_audit_activity = {
            let control = shared.control();
            ContentAuditActivity {
                playback_active: control.playback_active,
                foreground_active: control.foreground_active,
            }
        };
        let started = Instant::now();
        let result = execute_candidate(
            &request.candidate,
            lifecycle_generation,
            request.cancel.as_ref(),
            &shared.database_writer,
            content_audit_activity,
            &mut |event| shared.publish_event(event),
        );
        let elapsed_ms = started.elapsed().as_secs_f64() * 1_000.0;
        {
            let mut telemetry = shared.telemetry();
            telemetry.active_execution_workers =
                telemetry.active_execution_workers.saturating_sub(1);
            telemetry.execution_elapsed_ms += elapsed_ms;
            telemetry.execution_count = telemetry.execution_count.saturating_add(1);
        }
        if result_tx
            .send(ExecutionResult {
                candidate: request.candidate,
                permit: request.permit,
                lifecycle_generation,
                result,
                elapsed_ms,
                in_flight: request.in_flight,
            })
            .is_err()
        {
            return;
        }
        shared.control().notify("execution_result_ready");
        shared.wake.notify_one();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::native_app::source_processing::scheduler::{
        BudgetTracker, ProcessingBudgets, ProcessingLane, ResourceUse, WorkCandidate,
    };
    use std::sync::atomic::Ordering;
    use wavecrate::sample_sources::{
        SampleSource, SourceId,
        readiness::{ReadinessStage, ReadinessTarget},
    };

    #[test]
    fn fixed_pool_overlaps_embedding_preparation_across_sources() {
        let first_dir = tempfile::tempdir().expect("first source directory");
        let second_dir = tempfile::tempdir().expect("second source directory");
        let first = SampleSource::new_with_id(
            SourceId::from_string("pool-first"),
            first_dir.path().to_path_buf(),
        );
        let second = SampleSource::new_with_id(
            SourceId::from_string("pool-second"),
            second_dir.path().to_path_buf(),
        );
        first.open_db().expect("initialize first source database");
        second.open_db().expect("initialize second source database");
        let shared = Arc::new(Shared::new(vec![first.clone(), second.clone()], None));
        *shared.budgets() = BudgetTracker::new(ProcessingBudgets::for_tests(
            ResourceUse {
                cpu: 2,
                io: 2,
                database: 1,
            },
            ResourceUse {
                cpu: 1,
                io: 1,
                database: 1,
            },
            2,
        ));
        shared
            .execution_workers_paused
            .store(true, Ordering::Release);
        let mut pool = ExecutionPool::new(&shared, 2);

        for source in [first, second] {
            let target = ReadinessTarget::file(
                source.id.as_str(),
                format!("{}-scope", source.id.as_str()),
                "missing.wav",
                ReadinessStage::EmbeddingAspects,
                "test-embedding-version",
                1,
                "content",
            );
            let candidate = RuntimeCandidate {
                schedule: WorkCandidate::readiness(&target, 0),
                source: source.clone(),
                task: super::super::RuntimeTask::Readiness(target),
            };
            let permit = shared
                .budgets()
                .try_acquire(source.id.as_str(), ProcessingLane::Embedding)
                .expect("independent execution permit");
            let cancel = shared
                .control()
                .source_work_cancels
                .get(source.id.as_str())
                .cloned()
                .expect("source cancel token");
            let in_flight = shared
                .begin_in_flight_work(source.id.as_str(), &cancel)
                .expect("source execution admission");
            pool.try_dispatch(ExecutionRequest {
                candidate,
                permit,
                cancel,
                in_flight,
            })
            .map_err(|_| ())
            .expect("bounded execution dispatch");
        }

        let deadline = Instant::now() + std::time::Duration::from_secs(2);
        while shared.execution_workers_started.load(Ordering::Acquire) < 2
            && Instant::now() < deadline
        {
            thread::sleep(std::time::Duration::from_millis(1));
        }
        assert_eq!(shared.execution_workers_started.load(Ordering::Acquire), 2);
        assert_eq!(pool.in_flight_count(), 2);
        assert_eq!(shared.telemetry().peak_execution_workers, 2);

        shared.cancel.store(true, Ordering::Release);
        shared
            .execution_workers_paused
            .store(false, Ordering::Release);
        let deadline = Instant::now() + std::time::Duration::from_secs(5);
        let mut completed = 0;
        while completed < 2 && Instant::now() < deadline {
            if let Some(result) = pool.try_result() {
                shared.budgets().release(result.permit);
                drop(result.in_flight);
                completed += 1;
            } else {
                thread::sleep(std::time::Duration::from_millis(1));
            }
        }
        assert_eq!(completed, 2);
        assert_eq!(pool.in_flight_count(), 0);
        assert!(pool.shutdown());
    }

    #[test]
    fn worker_panic_does_not_detach_remaining_workers_during_shutdown() {
        let panicked = thread::spawn(|| panic!("intentional worker panic"));
        let (release_tx, release_rx) = std::sync::mpsc::channel();
        let waiting = thread::spawn(move || {
            release_rx.recv().expect("release remaining worker");
        });
        let (joined_tx, joined_rx) = std::sync::mpsc::channel();
        let joiner = thread::spawn(move || {
            joined_tx
                .send(join_all_workers([panicked, waiting]))
                .expect("report join result");
        });

        thread::sleep(std::time::Duration::from_millis(20));
        assert!(
            joined_rx.try_recv().is_err(),
            "shutdown returned before the remaining worker exited"
        );
        release_tx.send(()).expect("release remaining worker");
        assert!(!joined_rx.recv().expect("receive join result"));
        joiner.join().expect("join shutdown observer");
    }
}
