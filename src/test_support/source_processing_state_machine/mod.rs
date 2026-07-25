use serde::{Deserialize, Serialize};

mod artifacts;
mod harness;
mod invariants;
mod model;
mod mutations;
mod supervisor_observer;
mod supervisor_transitions;

use artifacts::{read_replay, write_failure_artifact};
use harness::StateMachineHarness;
use model::{Event, FailureBoundary, ReferenceModel, ScanCause, generate, generated_path};

pub(super) const NORMAL_SEQUENCE_LENGTH: usize = 24;
const STRESS_SEQUENCE_LENGTH: usize = 32;
const STRESS_SEQUENCE_COUNT: u64 = 1_000;
const REGRESSION_SEEDS: [u64; 6] = [
    0x1184_0000_0000_0000,
    0x1179_0000_0000_0001,
    0x1240_0000_0000_0002,
    0x1242_0000_0000_0003,
    0x1245_0000_0000_0004,
    0x1248_0000_0000_0005,
];

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum ExecutionMode {
    ScannerOnly,
    IntegratedSupervisor,
}

impl ExecutionMode {
    fn uses_supervisor(self) -> bool {
        self == Self::IntegratedSupervisor
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(in crate::native_app::source_processing::supervisor::state_machine_tests) struct FailureSnapshot
{
    pub(in crate::native_app::source_processing::supervisor::state_machine_tests) message: String,
    pub(in crate::native_app::source_processing::supervisor::state_machine_tests) event_index:
        usize,
    pub(in crate::native_app::source_processing::supervisor::state_machine_tests) event: Event,
    pub(in crate::native_app::source_processing::supervisor::state_machine_tests) model:
        serde_json::Value,
    pub(in crate::native_app::source_processing::supervisor::state_machine_tests) accepted_revisions:
        Vec<String>,
    pub(in crate::native_app::source_processing::supervisor::state_machine_tests) accepted_publications:
        Vec<String>,
    pub(in crate::native_app::source_processing::supervisor::state_machine_tests) observable_commits:
        u64,
    pub(in crate::native_app::source_processing::supervisor::state_machine_tests) runtime:
        Option<serde_json::Value>,
}

#[test]
fn source_processing_seeded_state_machine_normal_ci() {
    for seed in REGRESSION_SEEDS {
        run_or_archive(
            seed,
            generate(seed, NORMAL_SEQUENCE_LENGTH),
            ExecutionMode::ScannerOnly,
        );
    }
}

#[test]
#[ignore = "explicit integrated supervisor state-machine lane"]
fn source_processing_seeded_state_machine_integrated_supervisor() {
    let seed = REGRESSION_SEEDS[0];
    run_or_archive(
        seed,
        generate(seed, NORMAL_SEQUENCE_LENGTH),
        ExecutionMode::IntegratedSupervisor,
    );
    run_or_archive(
        0x1248_5055_0000_0001,
        vec![
            Event::InjectFailure {
                boundary: FailureBoundary::Publication,
            },
            Event::Create { slot: 7 },
            Event::WatcherBatch,
            Event::Quiesce,
        ],
        ExecutionMode::IntegratedSupervisor,
    );
    run_or_archive(
        0x1248_5055_0000_0002,
        vec![
            Event::InjectFailure {
                boundary: FailureBoundary::Transaction,
            },
            Event::Create { slot: 8 },
            Event::WatcherBatch,
            Event::Quiesce,
        ],
        ExecutionMode::IntegratedSupervisor,
    );
    run_or_archive(
        0x1248_5055_0000_0003,
        vec![
            Event::InjectFailure {
                boundary: FailureBoundary::WatcherDelivery,
            },
            Event::Create { slot: 9 },
            Event::WatcherBatch,
            Event::Quiesce,
        ],
        ExecutionMode::IntegratedSupervisor,
    );
    run_or_archive(
        0x1248_5055_0000_0004,
        vec![
            Event::InjectFailure {
                boundary: FailureBoundary::Lifecycle,
            },
            Event::SourceRemoveReadd,
            Event::Quiesce,
        ],
        ExecutionMode::IntegratedSupervisor,
    );
}

#[test]
#[ignore = "explicit 1,000-sequence source-processing state-machine stress lane"]
fn source_processing_seeded_state_machine_stress_1000() {
    for seed in 0..STRESS_SEQUENCE_COUNT {
        run_or_archive(
            seed,
            generate(seed, STRESS_SEQUENCE_LENGTH),
            ExecutionMode::ScannerOnly,
        );
    }
}

#[test]
#[ignore = "explicit replay via WAVECRATE_SOURCE_STATE_MACHINE_REPLAY=<seed-or-artifact>"]
fn source_processing_seeded_state_machine_replay() {
    let value = std::env::var("WAVECRATE_SOURCE_STATE_MACHINE_REPLAY").expect(
        "set WAVECRATE_SOURCE_STATE_MACHINE_REPLAY to a decimal seed or replay artifact path",
    );
    let (seed, events, mode) = read_replay(&value).expect("load source state-machine replay");
    run_or_archive(seed, events, mode);
}

fn run_or_archive(seed: u64, events: Vec<Event>, mode: ExecutionMode) {
    let initial_failure = match StateMachineHarness::new(mode.uses_supervisor()) {
        Ok(harness) => match harness.run(&events) {
            Ok(()) => return,
            Err(failure) => failure,
        },
        Err(message) => FailureSnapshot {
            message,
            event_index: 0,
            event: events.first().cloned().unwrap_or(Event::Quiesce),
            model: serde_json::Value::Null,
            accepted_revisions: Vec::new(),
            accepted_publications: Vec::new(),
            observable_commits: 0,
            runtime: None,
        },
    };
    eprintln!(
        "state-machine seed {seed} ({mode:?}) failed before shrinking at event {}: {}",
        initial_failure.event_index, initial_failure.message,
    );

    let (minimized, failure) = minimize_failure(seed, &events, mode);
    let path = write_failure_artifact(seed, minimized, mode, failure.clone())
        .unwrap_or_else(|error| panic!("state-machine failure and artifact write failed: {error}"));
    panic!(
        "seeded source-processing state machine failed in {mode:?}: {}\nreplay artifact: {}\nreplay: WAVECRATE_SOURCE_STATE_MACHINE_REPLAY={} cargo test -p wavecrate --lib source_processing_seeded_state_machine_replay -- --ignored --nocapture",
        failure.message,
        path.display(),
        path.display()
    );
}

fn minimize_failure(
    seed: u64,
    events: &[Event],
    mode: ExecutionMode,
) -> (Vec<Event>, FailureSnapshot) {
    let mut minimized = events.to_vec();
    let mut index = 0;
    while index < minimized.len() {
        if minimized[index].preserves_lifecycle_semantics()
            || matches!(minimized[index], Event::Quiesce)
        {
            index += 1;
            continue;
        }
        let mut candidate = minimized.clone();
        candidate.remove(index);
        if run_failure(&candidate, mode).is_some() {
            minimized = candidate;
        } else {
            index += 1;
        }
    }
    let failure = run_failure(&minimized, mode).unwrap_or_else(|| FailureSnapshot {
        message: format!("seed {seed} failed before shrinking but not after deterministic replay"),
        event_index: 0,
        event: Event::Quiesce,
        model: serde_json::Value::Null,
        accepted_revisions: Vec::new(),
        accepted_publications: Vec::new(),
        observable_commits: 0,
        runtime: None,
    });
    (minimized, failure)
}

fn run_failure(events: &[Event], mode: ExecutionMode) -> Option<FailureSnapshot> {
    match StateMachineHarness::new(mode.uses_supervisor()) {
        Ok(harness) => harness.run(events).err(),
        Err(message) => Some(FailureSnapshot {
            message,
            event_index: 0,
            event: events.first().cloned().unwrap_or(Event::Quiesce),
            model: serde_json::Value::Null,
            accepted_revisions: Vec::new(),
            accepted_publications: Vec::new(),
            observable_commits: 0,
            runtime: None,
        }),
    }
}
