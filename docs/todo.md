- add full stereo recording ability, with ASIO support for windows
in the waveform UX add a record button to start recording, also add a play and stop button, for regular playback

...
- TODO 5: Consolidate sample staging logic in analysis job enqueueing and add
  coverage for invalidation paths. src/egui_app/controller/analysis_jobs/enqueue/
  enqueue_samples.rs repeats “scan entries → check missing → build metadata” across
  backfill/missing-features; extract a shared “stage samples” helper and add tests
  for “analysis version stale” and “content hash change” invalidation so
  regressions are caught early.
