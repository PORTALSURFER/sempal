//! Always-on, resource-bounded processing for configured sample sources.

mod events;
mod scheduler;
mod supervisor;
mod worker;

pub(in crate::native_app) use events::{
    SourceDiscoveryPhase, SourceProcessingActivity, SourceProcessingEvent,
    SourceProcessingEventSink, SourceProcessingHealthEvent, SourceProcessingHealthState,
    SourceProcessingLifecycle, SourceProcessingProgressEvent,
};
pub(in crate::native_app) use supervisor::{
    SourceAuditLifecycleCause, SourceProcessingSupervisor, SourceScanAdmissionState,
};
pub(in crate::native_app) use worker::run_internal_source_analysis_from_args;
#[cfg(not(test))]
pub(in crate::native_app) use worker::wait_for_cancellable_child;

pub(in crate::native_app) fn manifest_delta_requires_browser_refresh(
    delta: &wavecrate::sample_sources::scanner::CommittedSourceDelta,
) -> bool {
    !delta.created.is_empty()
        || !delta.moved.is_empty()
        || !delta.deleted.is_empty()
        || delta
            .changed
            .iter()
            .any(|change| change.source_metadata_changed)
}
