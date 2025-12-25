- lets design a smarter context changing ux system which understands the layout, so that alt+arrow key movement will correctly move around the 2d plane based on direction, without hardcoding all this.
the idea to to have contexts chromes, like the sample browser or waveform etc, to navigate these, the user can use alt+arrows.
navigation inside these contexts, like for example, navigating the browser list, the user can use plain arrow keys.

..
- TODO 2: Refactor src/egui_app/controller/analysis_jobs/pool/job_runner.rs (827 lines) by extracting
  embedding backfill worker/queue logic and DB writeback into submodules; add unit tests for backfill
  batching and error aggregation paths to reduce regression risk.
- TODO 3: Refactor src/egui_app/controller/analysis_jobs/pool/job_claim.rs (747 lines) to isolate
  claim/lease renewal, dedup/claim logic, and logging; add tests for claim limits and reclaim behavior
  to make duplicate prevention verifiable.
- TODO 4: Split src/egui_app/ui/chrome/tool_panels.rs (437 lines) into smaller UI sections (audio
  settings, GPU embeddings, analysis options, etc.), and convert repeated panel layout patterns into
  helpers to improve readability and reduce UI drift.
- TODO 5: De‑duplicate staging/invalidation flows in src/egui_app/controller/analysis_jobs/enqueue/
  enqueue_samples.rs (419 lines) by extracting shared “scan → stage → invalidate → enqueue” helpers;
  add tests covering force_full and missing‑feature paths to ensure consistent enqueue behavior.
