use super::{
    Arc, Ordering, SampleSource, Shared, SourceProcessingEventSink, SourceProcessingSupervisor,
    install_worker_app_root, run_coordinator, run_retirement_worker, thread,
};

impl SourceProcessingSupervisor {
    #[cfg(test)]
    pub(in crate::native_app) fn start(sources: Vec<SampleSource>) -> Self {
        Self::start_with_playback_state(sources, false)
    }

    pub(in crate::native_app) fn start_with_event_sink(
        sources: Vec<SampleSource>,
        event_sink: impl SourceProcessingEventSink + 'static,
    ) -> (Self, Vec<super::SourceProcessingRegistration>) {
        Self::start_with_options(sources, false, Some(Arc::new(event_sink)), false)
    }

    #[cfg(test)]
    pub(super) fn start_with_playback_state(
        sources: Vec<SampleSource>,
        playback_active: bool,
    ) -> Self {
        Self::start_with_playback_state_and_event_sink(sources, playback_active, None)
    }

    #[cfg(test)]
    pub(super) fn start_with_playback_state_and_event_sink(
        sources: Vec<SampleSource>,
        playback_active: bool,
        event_sink: Option<Arc<dyn SourceProcessingEventSink>>,
    ) -> Self {
        let (supervisor, _) = Self::start_with_options(sources, playback_active, event_sink, false);
        supervisor
    }

    fn start_with_options(
        sources: Vec<SampleSource>,
        playback_active: bool,
        event_sink: Option<Arc<dyn SourceProcessingEventSink>>,
        synthetic_test_execution: bool,
    ) -> (Self, Vec<super::SourceProcessingRegistration>) {
        let app_root = wavecrate::app_dirs::app_root_dir()
            .expect("source-processing supervisor should resolve its persistence root");
        let shared = Arc::new(Shared::new(sources, event_sink));
        let registrations = {
            let mut control = shared.control();
            control.playback_active = playback_active;
            control.source_registrations()
        };
        shared
            .synthetic_test_execution
            .store(synthetic_test_execution, Ordering::Release);
        let thread_shared = Arc::clone(&shared);
        let coordinator_app_root = app_root.clone();
        let coordinator = thread::Builder::new()
            .name(String::from("wavecrate-source-supervisor"))
            .spawn(move || {
                let _app_root_guard = install_worker_app_root(coordinator_app_root);
                run_coordinator(thread_shared);
            })
            .expect("spawn source processing supervisor");
        let retirement_shared = Arc::clone(&shared);
        let retirement_app_root = app_root;
        let retirement_worker = thread::Builder::new()
            .name(String::from("wavecrate-source-retirement"))
            .spawn(move || {
                let _app_root_guard = install_worker_app_root(retirement_app_root);
                run_retirement_worker(retirement_shared);
            })
            .expect("spawn source retirement worker");
        (
            Self {
                shared,
                coordinator: Some(coordinator),
                retirement_worker: Some(retirement_worker),
            },
            registrations,
        )
    }

    #[cfg(test)]
    pub(in crate::native_app) fn dormant() -> Self {
        Self {
            shared: Arc::new(Shared::new(Vec::new(), None)),
            coordinator: None,
            retirement_worker: None,
        }
    }

    #[cfg(test)]
    pub(in crate::native_app) fn dormant_with_sources(
        sources: Vec<SampleSource>,
    ) -> (Self, Vec<super::SourceProcessingRegistration>) {
        let shared = Arc::new(Shared::new(sources, None));
        let registrations = shared.control().source_registrations();
        (
            Self {
                shared,
                coordinator: None,
                retirement_worker: None,
            },
            registrations,
        )
    }

    #[cfg(any(test, feature = "legacy-controller"))]
    pub(in crate::native_app) fn is_running(&self) -> bool {
        self.coordinator.is_some() && self.retirement_worker.is_some()
    }

    #[cfg(test)]
    pub(super) fn start_synthetic_profile(
        sources: Vec<SampleSource>,
        playback_active: bool,
    ) -> Self {
        let (supervisor, _) = Self::start_with_options(sources, playback_active, None, true);
        supervisor
    }

    #[cfg(test)]
    pub(super) fn start_with_registrations(
        sources: Vec<SampleSource>,
    ) -> (Self, Vec<super::SourceProcessingRegistration>) {
        Self::start_with_options(sources, false, None, true)
    }

    #[cfg(test)]
    pub(super) fn start_without_forced_manifest_audit(sources: Vec<SampleSource>) -> Self {
        let shared = Arc::new(Shared::new(sources, None));
        shared.control().force_manifest_audit_sources.clear();
        shared.control().force_reanalysis_sources.clear();
        let thread_shared = Arc::clone(&shared);
        let coordinator = thread::Builder::new()
            .name(String::from("wavecrate-source-supervisor"))
            .spawn(move || run_coordinator(thread_shared))
            .expect("spawn source processing supervisor");
        let retirement_shared = Arc::clone(&shared);
        let retirement_worker = thread::Builder::new()
            .name(String::from("wavecrate-source-retirement"))
            .spawn(move || run_retirement_worker(retirement_shared))
            .expect("spawn source retirement worker");
        Self {
            shared,
            coordinator: Some(coordinator),
            retirement_worker: Some(retirement_worker),
        }
    }
}
