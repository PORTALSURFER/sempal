//! Facade for the single source-processing coordinator and its owned services.

#![cfg_attr(test, allow(dead_code))]

use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::PathBuf,
    sync::{
        Arc, Condvar, Mutex, MutexGuard,
        atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering},
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use rusqlite::params;
use serde_json::Value;
use wavecrate::sample_sources::{
    SampleSource, SourceDatabase, SourceDatabaseConnectionRole, SourceMetadataStorage,
    db::{
        META_LAST_MANIFEST_AUDIT_AT, META_READINESS_DUPLICATE_IDENTITY,
        META_WAV_IDENTITIES_REVISION, META_WAV_PATHS_REVISION,
    },
    readiness::{
        ArtifactPublishOutcome, ClaimedReadinessWork, ReadinessActivity, ReadinessClassification,
        ReadinessDeltaPublicationOutcome, ReadinessEligibility, ReadinessFailureClassification,
        ReadinessFailureOutcome, ReadinessLeaseRenewalOutcome, ReadinessMembership,
        ReadinessProgress, ReadinessRetryPolicy, ReadinessScopeKind, ReadinessSnapshot,
        ReadinessStage, ReadinessStageCounts, ReadinessStore, ReadinessTarget,
        ReadinessTargetDeltaPublication, ReadinessTargetPublication, ReadinessView,
        ReadinessWorkMutationOutcome, SourceAvailability,
    },
    scanner::{
        CommittedSourceDelta, ContentAuditActivity, ContentAuditBudget, ContentAuditStorage,
        ScanError, audit_source_and_record_with_budget_and_progress_and_writer,
        complete_pending_deep_hash_for_path, sync_paths_with_progress,
    },
};

use super::worker::{SourceProcessingFailure, source_database_failure};
use super::{
    SourceDiscoveryPhase, SourceProcessingActivity, SourceProcessingEvent,
    SourceProcessingEventSink, SourceProcessingHealthEvent, SourceProcessingHealthState,
    SourceProcessingLifecycle, SourceProcessingProgressEvent,
    scheduler::{
        BudgetTracker, FairScheduler, PriorityContext, ProcessingBudgets, ProcessingLane,
        WorkCandidate,
    },
};
use crate::native_app::sample_library::similarity_artifacts::{
    SimilarityPublicationFence, finalize_similarity_artifacts_if_ready,
    native_similarity_artifact_version,
};
use crate::native_app::waveform::invalidate_persisted_waveform_cache_ref;

mod admission;
mod cache_ownership;
mod commands;
mod control;
mod coordination;
mod coordinator;
mod coordinator_completion;
mod coordinator_execution;
mod coordinator_policy;
mod discovery;
mod discovery_publication;
mod discovery_reconcile;
mod discovery_schema;
mod discovery_targets;
mod execution;
mod execution_database;
mod execution_lease;
mod execution_pool;
mod execution_readiness;
mod execution_stages;
mod execution_validation;
mod health;
mod lifecycle;
mod model;
mod progress;
mod registry;
mod retirement;
mod retirement_cache;
mod shutdown;
mod source_registry_model;
mod startup;
mod state;
#[cfg(test)]
mod state_machine_observation;
mod telemetry;

pub(in crate::native_app) use admission::SourceScanAdmissionState;
use admission::{SourceProcessingBudgetHandle, install_worker_app_root};
use cache_ownership::*;
pub(in crate::native_app) use commands::SourceAuditLifecycleCause;
use control::*;
use coordination::*;
use coordinator::*;
use coordinator_execution::*;
use coordinator_policy::*;
use discovery::*;
use discovery_publication::*;
use discovery_reconcile::*;
use discovery_schema::*;
use discovery_targets::*;
use execution::*;
pub(in crate::native_app::source_processing) use execution_database::{
    DatabasePhase, DatabaseWriterGate, DatabaseWriterGuard,
};
use execution_lease::*;
use execution_pool::*;
use execution_readiness::*;
use execution_stages::*;
use execution_validation::*;
use health::*;
use model::*;
use progress::*;
use registry::*;
use retirement::*;
use source_registry_model::*;
use state::*;
use telemetry::*;

pub(in crate::native_app::source_processing) use retirement_cache::{
    SourceRetirementOutcome, retire_source_derived_state,
};

const SAFETY_SWEEP_INTERVAL: Duration = Duration::from_secs(30);
const PROGRESS_REFRESH_INTERVAL: Duration = Duration::from_secs(1);
const DISCOVERY_PROGRESS_EVENT_GRACE_INTERVAL: Duration = Duration::from_millis(250);
const DISCOVERY_PROGRESS_REFRESH_INTERVAL: Duration = Duration::from_millis(250);
const DISCOVERY_PROGRESS_LOG_INTERVAL: Duration = Duration::from_secs(2);
const SIMILARITY_SCORE_REFRESH_INTERVAL: Duration = Duration::from_secs(1);
const MANIFEST_AUDIT_INTERVAL_SECONDS: i64 = 24 * 60 * 60;
const MAX_VISIBLE_PRIORITY_PATHS: usize = 128;
const READINESS_LEASE_SECONDS: i64 = 5 * 60;
const READINESS_MAX_ATTEMPTS: u32 = 8;
const READINESS_MANIFEST_VERSION: &str = "source_manifest_v1";
const READINESS_MEMBERSHIP_VERSION: &str = "membership-xor-v1";
const SOURCE_RETIREMENT_RETRY_SECONDS: i64 = 5;
const SOURCE_DISCOVERY_RETRY_SECONDS: i64 = 5;
const PREREQUISITE_INVALIDATION_RETRY_SECONDS: i64 = 5;
const ACTIVE_RECORDING_QUIET_SECONDS: i64 = 5;
const ORPHAN_CACHE_MIN_AGE: Duration = Duration::from_secs(7 * 24 * 60 * 60);
const ORPHAN_CACHE_MAX_SCANNED: usize = 4_096;
const ORPHAN_CACHE_MAX_REMOVED: usize = 32;
const RETAINED_SOURCE_MAX_SCANNED: usize = 1_024;
static ORPHAN_CACHE_SCAN_CURSOR: AtomicUsize = AtomicUsize::new(0);

/// Owned runtime coordinator. All work is joined during shutdown and observes one cancel token.
pub(in crate::native_app) struct SourceProcessingSupervisor {
    shared: Arc<Shared>,
    coordinator: Option<JoinHandle<()>>,
    pub(super) retirement_worker: Option<JoinHandle<()>>,
}

#[cfg(test)]
#[path = "../../test_support/source_processing_liveness/mod.rs"]
mod liveness_tests;

#[cfg(test)]
#[path = "../../test_support/source_processing_state_machine/mod.rs"]
mod state_machine_tests;

#[cfg(test)]
mod tests;
