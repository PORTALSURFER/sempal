<!-- Goal: Add GPU-friendly embedding-based auto-tagging, similarity search, and retraining loop. -->

<!-- Phase 1 - Plumbing -->
<!-- - Add SQLite migrations for samples, embeddings, predictions, labels, ann_index_meta, and jobs. -->
<!-- - Implement job types: ANALYZE_SAMPLE, REBUILD_INDEX, RETRAIN_CLASSIFIER (optional). -->
<!-- - Ensure scan/import enqueues ANALYZE_SAMPLE and respects analysis_version invalidation. -->
<!-- - Track job status, retries, and last_error; persist across restarts. -->

<!-- Phase 2 - Audio Preprocessing -->
<!-- - Decode supported formats (wav/aiff/flac/mp3/ogg) with existing pipeline. -->
<!-- - Downmix to mono and resample to 16 kHz float32 [-1, 1]. -->
<!-- - Add windowing: full analysis <= 6s; for longer, pick energy windows (start/mid/end or RMS top-K). -->
<!-- - Add silence trim and min-length padding for very short samples. -->

<!-- Phase 3 - Embeddings (YAMNet) -->
<!-- - Choose runtime: TFLite (preferred) or ONNX Runtime; no Python in app. -->
<!-- - Run YAMNet inference to get frame embeddings; pool to 1024-D vector. -->
<!-- - Store embedding blob with model_id and dtype; normalize vectors for cosine similarity. -->
<!-- - Record analysis_version (hash of model + preprocessing params). -->

<!-- Phase 4 - Classifier Head -->
<!-- - Implement logistic regression head (softmax) with W [C x 1024] and b [C]. -->
<!-- - Load classifier artifact from bundled model; version in settings. -->
<!-- - Store auto_category, confidence, and optional top-K in predictions. -->
<!-- - Apply UNKNOWN thresholding and expose confidence bands in UI. -->

Phase 5 - Similar Sounds
- Integrate HNSW (hnsw_rs) with cosine/dot metric.
- Persist index to app data and track meta in ann_index_meta.
- Update index as embeddings arrive; rebuild if incompatible version.
- Add "Find Similar" UI to query top-N neighbors.
- Improve similarity quality (better windowing/labeling before rebuild).

Phase 6 - Labels and Correction Loop
- Store user overrides in labels; allow opt-in or always-on.
- Add UI for manual tagging and review workflow.
- Add retrain trigger that exports embeddings + labels and runs trainer.

Phase 7 - Training Tooling (Dev)
- Build export tool: join embeddings + labels, stratified train/val/test split.
- Train multinomial logistic regression with class balancing.
- Export artifact: model_id, embedding_model_id, classes, W, b, temperature.
- Emit evaluation: confusion matrix, per-class PR/F1, top-K accuracy.

Phase 8 - Performance and UX
- Run analysis/inference on background workers; bound CPU usage.
- Keep UI responsive with worker limits and progress reporting.
- Expose model stats and data coverage in training UI.

Phase 9 - Installer & Packaging (Windows)
- Choose installer tech (e.g., NSIS/Inno Setup/Wix) and lock down requirements.
- Add bundling pipeline for release artifacts (exe, models, ONNX runtime, icons).
- Build an egui-based installer app with SemPal styling (install path picker, license, progress).
- Wire installer to copy binaries/resources and create app data folders if missing.
- Register uninstall entry in Windows (Add/Remove Programs).
- Add code-signing hooks (optional, but prepare for it).
- Verify installed app finds bundled models + runtimes and can retrain.
- Document build steps and required assets in release checklist.

Open Questions and Recommendations
- Embedding runtime: prefer TFLite for a single YAMNet-like embedder; use ONNX Runtime only if we expect frequent model swaps or already ship ORT.
- Output type: multi-class softmax as the primary category; add multi-label tags later as a secondary system.
- UNKNOWN behavior: if max probability < threshold, show "Uncertain" and avoid auto-committing a category.
- Index deletion: lazy-delete vectors, filter by DB at query time, rebuild when tombstones exceed ~8-10% or on analysis_version change.
