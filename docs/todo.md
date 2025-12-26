- lets design a smarter context changing ux system which understands the layout, so that alt+arrow key movement will correctly move around the 2d plane based on direction, without hardcoding all this.
the idea to to have contexts chromes, like the sample browser or waveform etc, to navigate these, the user can use alt+arrows.
navigation inside these contexts, like for example, navigating the browser list, the user can use plain arrow keys.

..
  - TODO 4: Extract the core analysis job runner pipeline (src/egui_app/controller/
    analysis_jobs/pool/job_runner.rs) into a “job execution” module and isolate error
    aggregation/reporting. Add tests for backfill retry behavior and job status
    updates to ensure correctness under failures.
  - TODO 5: Tighten up and document the embedding inference pipeline (src/analysis/
    embedding/infer.rs at 456 lines). Split into preprocessing, batch scheduling, and
    backend IO; add doc comments for the public inference entry points and tests for
    batch sizing + fallback behavior.
