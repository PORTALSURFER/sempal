
  - TODO 1: Split src/audio/player.rs (556 lines) into focused submodules (e.g., playback setup, state/progress,
    fade/loop helpers). Extract helpers like reset_playback_state() and build_sink_with_fade() to remove duplicated
    field resets and sink setup; add unit tests for looping/progress using the existing elapsed_override test hook.
  - TODO 2: Reduce constructor bulk in src/egui_app/controller.rs by moving the large state initialization into src/
    egui_app/controller/controller_state.rs (e.g., ControllerState::new(renderer, player) or Default impls for nested
    state structs). This keeps EguiController::new short and makes state init easier to test.
  - TODO 3: Fix the missing test registration in src/egui_app/controller/tests/waveform.rs —
    cropping_selection_overwrites_file is missing #[test], so it never runs. Add the attribute or convert it into a
    helper called by an actual #[test].
  - TODO 4: Add doc comments for public types in src/egui_app/controller/controller_state.rs that currently lack them
    (e.g., AnalysisJobStatus, FeatureStatus, FeatureCache, SimilarityPrepStage). This aligns with the project rule
    that all public items must be documented.
  - TODO 5: Avoid repeated cloning of stats.changed_samples in src/egui_app/controller/background_jobs/scan.rs.
    Consider passing ownership once into the spawned job (or using Arc<Vec<ChangedSample>>) to reduce memory churn on
    large scans.
