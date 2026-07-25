use std::{
    sync::mpsc::{Receiver, RecvTimeoutError},
    time::{Duration, Instant},
};

use super::super::{
    SourceProcessingEvent, SourceProcessingHealthEvent, SourceProcessingHealthState,
};
use super::StateMachineHarness;

const MAX_TRANSITION_EVENTS: usize = 4_096;
const TRANSITION_DIAGNOSTIC_TIMEOUT: Duration = Duration::from_secs(30);

impl StateMachineHarness {
    pub(super) fn wait_for_supervisor_offline(&self) -> Result<(), String> {
        let health = wait_for_health(
            self.supervisor_events.as_ref(),
            self.source.id.as_str(),
            |health| health.state == SourceProcessingHealthState::Offline,
            "offline",
        )?;
        if health.retry_at.is_some() {
            return Err(String::from(
                "offline supervisor observation unexpectedly scheduled a retry",
            ));
        }
        Ok(())
    }

    pub(super) fn wait_for_supervisor_online_terminal(&self) -> Result<(), String> {
        wait_for_health(
            self.supervisor_events.as_ref(),
            self.source.id.as_str(),
            |health| {
                matches!(
                    health.state,
                    SourceProcessingHealthState::Ready
                        | SourceProcessingHealthState::DegradedTerminal
                )
            },
            "terminal online",
        )
        .map(|_| ())
    }

    pub(super) fn wait_for_supervisor_convergence(&self) -> Result<(), String> {
        let supervisor = self
            .supervisor
            .as_ref()
            .ok_or_else(|| String::from("integrated state-machine supervisor is missing"))?;
        let events = self
            .supervisor_events
            .as_ref()
            .ok_or_else(|| String::from("integrated state-machine event receiver is missing"))?;
        let deadline = Instant::now() + TRANSITION_DIAGNOSTIC_TIMEOUT;
        for observed_events in 0..=MAX_TRANSITION_EVENTS {
            let runtime = super::super::liveness_tests::runtime_observation(
                supervisor,
                self.source.id.as_str(),
            );
            if let Some(snapshot) = super::super::liveness_tests::readiness_snapshot(&self.source) {
                if runtime.source_active
                    && super::super::liveness_tests::silently_idle(&snapshot, &runtime)
                {
                    return Err(format!(
                        "runtime became silently idle with actionable work after {observed_events} state transitions: {runtime:?}"
                    ));
                }
                if snapshot.is_converged()
                    && runtime.queue_depth == 0
                    && runtime.readiness_queue_depth == 0
                    && runtime.in_flight == 0
                    && !runtime.source_dirty
                {
                    return Ok(());
                }
            }
            if observed_events == MAX_TRANSITION_EVENTS {
                return Err(format!(
                    "runtime exceeded {MAX_TRANSITION_EVENTS} state transitions without convergence: {runtime:?}"
                ));
            }
            let remaining = deadline.saturating_duration_since(Instant::now());
            match events.recv_timeout(remaining) {
                Ok(_) => {}
                Err(RecvTimeoutError::Timeout) => {
                    return Err(format!(
                        "runtime produced no convergence transition before the diagnostic timeout: {runtime:?}"
                    ));
                }
                Err(RecvTimeoutError::Disconnected) => {
                    return Err(String::from(
                        "integrated state-machine event receiver disconnected",
                    ));
                }
            }
        }
        unreachable!("bounded transition loop returns on its final iteration")
    }
}

fn wait_for_health(
    events: Option<&Receiver<SourceProcessingEvent>>,
    source_id: &str,
    predicate: impl Fn(&SourceProcessingHealthEvent) -> bool,
    expected: &str,
) -> Result<SourceProcessingHealthEvent, String> {
    let events =
        events.ok_or_else(|| String::from("integrated state-machine event receiver is missing"))?;
    let deadline = Instant::now() + TRANSITION_DIAGNOSTIC_TIMEOUT;
    for observed_events in 0..MAX_TRANSITION_EVENTS {
        let remaining = deadline.saturating_duration_since(Instant::now());
        match events.recv_timeout(remaining) {
            Ok(SourceProcessingEvent::Health(health))
                if health.lifecycle.source_id == source_id && predicate(&health) =>
            {
                return Ok(health);
            }
            Ok(_) => {}
            Err(RecvTimeoutError::Timeout) => {
                return Err(format!(
                    "supervisor did not publish {expected} health for {source_id} after {observed_events} transitions"
                ));
            }
            Err(RecvTimeoutError::Disconnected) => {
                return Err(String::from(
                    "integrated state-machine event receiver disconnected",
                ));
            }
        }
    }
    Err(format!(
        "supervisor exceeded {MAX_TRANSITION_EVENTS} transitions before publishing {expected} health for {source_id}"
    ))
}
