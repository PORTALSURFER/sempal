
  - TODO 2: Reduce constructor bulk in src/egui_app/controller.rs by moving the large state initialization into src/
    egui_app/controller/controller_state.rs (e.g., ControllerState::new(renderer, player) or Default impls for nested
    state structs). This keeps EguiController::new short and makes state init easier to test.
  - TODO 3: Fix the missing test registration in src/egui_app/controller/tests/waveform.rs —
    cropping_selection_overwrites_file is missing #[test], so it never runs. Add the attribute or convert it into a
    helper called by an actual #[test].
  - TODO 4: Reduce complexity in similarity resolution path.
      - File: src/egui_app/controller/wavs/similar/resolve.rs
      - Why: big logic handling row→sample resolution, filtering, and result
        composition.
      - Suggested: extract a “resolve visible rows” helper and a “sample id lookup”
        helper; make resolve path purely data flow.
      - Tests: add edge-case tests for empty/filtered lists and for missing sample id
        resolution.
  - TODO 5: Add doc comments for public types in src/egui_app/controller/controller_state.rs that currently lack them
    (e.g., AnalysisJobStatus, FeatureStatus, FeatureCache, SimilarityPrepStage). This aligns with the project rule
    that all public items must be documented.
  - TODO 6: Avoid repeated cloning of stats.changed_samples in src/egui_app/controller/background_jobs/scan.rs.
    Consider passing ownership once into the spawned job (or using Arc<Vec<ChangedSample>>) to reduce memory churn on
    large scans.

