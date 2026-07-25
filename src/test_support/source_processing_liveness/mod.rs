use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    sync::{atomic::AtomicBool, mpsc::Receiver},
    thread,
    time::{Duration, Instant},
};

use rusqlite::{Connection, params};
use serde::Serialize;
use wavecrate::sample_sources::{
    SampleSource, SourceDatabase, SourceDatabaseConnectionRole, SourceId,
    db::{META_LAST_MANIFEST_AUDIT_AT, META_WAV_PATHS_REVISION},
    readiness::{
        ReadinessActivity, ReadinessClassification, ReadinessSnapshot, ReadinessStage,
        ReadinessStore, SourceAvailability,
    },
    scanner::{CommittedSourceDelta, ManifestIdentityDelta, audit_source_and_record},
};

use super::*;
use crate::native_app::app::GuiMessage;
use crate::native_app::sample_library::committed_file_mutations::{
    FileMutationChange, FileMutationOperation, reconcile_file_mutation_for_liveness_test,
};
use crate::native_app::sample_library::folder_scan_actions::sync_source_database_paths;
use crate::native_app::sample_library::source_watcher::GuiSourceWatcherHandle;

mod artifacts;
mod diagnostics;
mod harness;
mod scenarios;

use artifacts::*;
use diagnostics::*;
pub(super) use diagnostics::{readiness_snapshot, runtime_observation, silently_idle};
use harness::*;

const POLL_INTERVAL: Duration = Duration::from_millis(20);
const LIVENESS_TIMEOUT: Duration = Duration::from_secs(45);
const SILENT_IDLE_CONFIRMATIONS: usize = 4;

#[derive(Clone, Copy, Debug, Serialize)]
#[serde(rename_all = "snake_case")]
enum WatcherStimulus {
    Startup,
    Targeted,
    Overflow,
    WatcherRestart,
    ClosedAppAudit,
    RootUnavailable,
    RootAvailable,
    RootIdentityReplacement,
    InternalMutation,
}
