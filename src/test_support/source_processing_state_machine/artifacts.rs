use std::{
    fs,
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};

use super::{Event, ExecutionMode, FailureSnapshot};

const SCHEMA_VERSION: u32 = 2;
const FIXTURE_NAME: &str = "small-multi-source";
const FIXTURE_SEED: u64 = 0x5741_5645_4352_4154;

#[derive(Debug, Serialize, Deserialize)]
pub(super) struct ReplayArtifact {
    pub(super) schema_version: u32,
    pub(super) seed: u64,
    pub(super) execution_mode: ExecutionMode,
    pub(super) fixture: String,
    pub(super) fixture_version: u32,
    pub(super) fixture_seed: u64,
    pub(super) events: Vec<Event>,
    pub(super) failure: FailureSnapshot,
    pub(super) replay_command: String,
}

pub(super) fn write_failure_artifact(
    seed: u64,
    events: Vec<Event>,
    execution_mode: ExecutionMode,
    failure: FailureSnapshot,
) -> Result<PathBuf, String> {
    let root = std::env::var_os("CARGO_TARGET_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("target"))
        .join("source-state-machine-failures");
    fs::create_dir_all(&root).map_err(|error| {
        format!(
            "create replay artifact directory {}: {error}",
            root.display()
        )
    })?;
    let path = root.join(format!("seed-{seed:016x}.json"));
    let replay_command = format!(
        "WAVECRATE_SOURCE_STATE_MACHINE_REPLAY={} cargo test -p wavecrate --lib \
         source_processing_seeded_state_machine_replay -- --ignored --nocapture",
        path.display()
    );
    let artifact = ReplayArtifact {
        schema_version: SCHEMA_VERSION,
        seed,
        execution_mode,
        fixture: String::from(FIXTURE_NAME),
        fixture_version: crate::native_source_fixture::FIXTURE_VERSION,
        fixture_seed: FIXTURE_SEED,
        events,
        failure,
        replay_command,
    };
    let bytes = serde_json::to_vec_pretty(&artifact)
        .map_err(|error| format!("serialize replay artifact: {error}"))?;
    fs::write(&path, bytes)
        .map_err(|error| format!("write replay artifact {}: {error}", path.display()))?;
    Ok(path)
}

pub(super) fn read_replay(value: &str) -> Result<(u64, Vec<Event>, ExecutionMode), String> {
    if let Ok(seed) = value.parse::<u64>() {
        return Ok((
            seed,
            super::generate(seed, super::NORMAL_SEQUENCE_LENGTH),
            ExecutionMode::IntegratedSupervisor,
        ));
    }
    let path = Path::new(value);
    let bytes = fs::read(path)
        .map_err(|error| format!("read replay artifact {}: {error}", path.display()))?;
    let artifact: ReplayArtifact = serde_json::from_slice(&bytes)
        .map_err(|error| format!("parse replay artifact {}: {error}", path.display()))?;
    if artifact.schema_version != SCHEMA_VERSION {
        return Err(format!(
            "unsupported source state-machine artifact schema {}",
            artifact.schema_version
        ));
    }
    if artifact.fixture != FIXTURE_NAME
        || artifact.fixture_version != crate::native_source_fixture::FIXTURE_VERSION
        || artifact.fixture_seed != FIXTURE_SEED
    {
        return Err(format!(
            "replay fixture mismatch: artifact={}/v{}/{} current={FIXTURE_NAME}/v{}/{FIXTURE_SEED}",
            artifact.fixture,
            artifact.fixture_version,
            artifact.fixture_seed,
            crate::native_source_fixture::FIXTURE_VERSION,
        ));
    }
    Ok((artifact.seed, artifact.events, artifact.execution_mode))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn replay_artifact_preserves_scanner_only_execution_mode() {
        let directory = tempfile::tempdir().expect("replay artifact directory");
        let path = directory.path().join("replay.json");
        let artifact = artifact(ExecutionMode::ScannerOnly);
        fs::write(
            &path,
            serde_json::to_vec(&artifact).expect("serialize replay artifact"),
        )
        .expect("write replay artifact");

        let (seed, events, mode) =
            read_replay(path.to_str().expect("utf-8 replay path")).expect("read replay");
        assert_eq!(seed, 42);
        assert_eq!(events, [Event::ExplicitRefresh, Event::Quiesce]);
        assert_eq!(mode, ExecutionMode::ScannerOnly);
    }

    #[test]
    fn replay_artifact_rejects_fixture_version_drift() {
        let directory = tempfile::tempdir().expect("replay artifact directory");
        let path = directory.path().join("stale-replay.json");
        let mut artifact = artifact(ExecutionMode::IntegratedSupervisor);
        artifact.fixture_version = artifact.fixture_version.saturating_add(1);
        fs::write(
            &path,
            serde_json::to_vec(&artifact).expect("serialize stale replay artifact"),
        )
        .expect("write stale replay artifact");

        let error = read_replay(path.to_str().expect("utf-8 replay path"))
            .expect_err("fixture drift must reject replay");
        assert!(error.contains("replay fixture mismatch"));
    }

    fn artifact(execution_mode: ExecutionMode) -> ReplayArtifact {
        ReplayArtifact {
            schema_version: SCHEMA_VERSION,
            seed: 42,
            execution_mode,
            fixture: String::from(FIXTURE_NAME),
            fixture_version: crate::native_source_fixture::FIXTURE_VERSION,
            fixture_seed: FIXTURE_SEED,
            events: vec![Event::ExplicitRefresh, Event::Quiesce],
            failure: FailureSnapshot {
                message: String::from("test"),
                event_index: 0,
                event: Event::ExplicitRefresh,
                model: serde_json::Value::Null,
                accepted_revisions: Vec::new(),
                accepted_publications: Vec::new(),
                observable_commits: 0,
                runtime: None,
            },
            replay_command: String::from("test"),
        }
    }
}
