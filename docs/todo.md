Training-free discovery roadmap, audited against the current codebase. Each item is a standalone issue with status and app-specific details.

Issue 1: Document and align the CLAP embedding contract (Status: Partial)
Scope:
- Formalize the embedding contract: input windowing, mono mixdown, normalization target, sample rate, and repeat/pad behavior.
- Align app embedding preprocessing with curated dataset preprocessing.
Current state:
- CLAP inference exists with fixed 48kHz, 10s repeat/pad, and L2 normalization.
- Analysis pipeline decodes to 16kHz and feeds embeddings through resample + sanitize only.
Acceptance criteria:
- One canonical contract documented in `docs/feature_vector.md` or a new `docs/embedding_contract.md`.
- App embedding path uses `preprocess_mono_for_embedding` or explicitly justifies divergence.
- Golden embedding tests updated if preprocessing changes.
Primary files:
- `src/analysis/embedding.rs`
- `src/analysis/audio.rs`
- `src/egui_app/controller/analysis_jobs/pool.rs`
- `docs/feature_vector.md`

Issue 2: Add anchor-label schema and UMAP layout tables (Status: Not started)
Scope:
- Add tables for training-free labels and anchors.
- Add `layout_umap` table for 2D coordinates.
Current state:
- `embeddings`, `labels_user`, `labels_weak`, and `ann_index_meta` exist.
- `labels` table exists but is unused and does not match anchor-based design.
Acceptance criteria:
- New tables (example names): `tf_labels`, `tf_anchors`, `layout_umap`.
- Migrations include backfill strategy and are compatible with existing databases.
- Any legacy `labels` table usage documented or migrated.
Primary files:
- `src/sample_sources/library/schema.rs`
- `src/sample_sources/library.rs`

Issue 3: Embed ingestion: model_version tracking and lifecycle (Status: Partial)
Scope:
- Store `model_version` or `model_id` consistently with timestamps.
- Support re-embedding when model_version changes.
Current state:
- Embeddings table includes `model_id`, `dim`, `dtype`, `l2_normed`.
- Backfill jobs and analysis pipeline insert embeddings and update ANN index.
Acceptance criteria:
- Embedding writes include `created_at` and `model_version` (or `model_id` already used, but tracked in metadata).
- Re-embedding job path that can invalidate and recompute embeddings by model change.
- Clear UX messaging for re-embedding progress.
Primary files:
- `src/egui_app/controller/analysis_jobs/db.rs`
- `src/egui_app/controller/analysis_jobs/enqueue.rs`
- `src/egui_app/controller/analysis_jobs/pool.rs`
- `src/sample_sources/library/schema.rs`

Issue 4: ANN index load-from-disk and parameter validation (Status: Partial)
Scope:
- Load HNSW index from disk when available and valid.
- Validate index params vs current `model_id` and embedding dim.
Current state:
- Index builds from DB and persists, but always rebuilds on startup.
Acceptance criteria:
- Index load fast-path using `hnsw_rs` file load.
- On param mismatch, auto-rebuild and update metadata.
- Manual rebuild command or UI action.
Primary files:
- `src/analysis/ann_index.rs`
- `src/sample_sources/library/schema.rs`

Issue 5: Similarity search by audio clip (Status: Not started)
Scope:
- Support "query by audio blob" or temp file in app.
- Use embedding inference and ANN search against library.
Current state:
- Similarity search by sample_id exists via UI "Find similar".
Acceptance criteria:
- Controller function for ad-hoc audio input (selection, drag-drop, or file).
- UI entry point (context menu, button, or drag target).
- Results list uses existing browser filter flow.
Primary files:
- `src/egui_app/controller/wavs/similar.rs`
- `src/egui_app/ui/sample_browser_filter.rs`
- `src/egui_app/controller/analysis_jobs/pool.rs`

Issue 6: Anchor label CRUD and thresholds (Status: Not started)
Scope:
- CRUD for anchor labels, anchors, thresholds, gap, and topK.
Current state:
- Only `labels_user` and `labels_weak` exist; no anchor labels.
Acceptance criteria:
- DB CRUD for labels and anchors (create, update, delete).
- Weighting and threshold fields stored and editable.
- Clean integration with existing label concepts (avoid conflicts with `labels_user`).
Primary files:
- `src/sample_sources/library/schema.rs`
- `src/egui_app/controller`

Issue 7: Anchor scoring logic and confidence buckets (Status: Not started)
Scope:
- Implement max or mean-of-topK scoring.
- Apply gap logic and bucket classification.
Current state:
- No anchor scoring logic implemented.
Acceptance criteria:
- Pure function for scoring with unit tests.
- Gap logic returns high/medium/low confidence buckets.
- Pluggable aggregation strategy (max vs topK mean).
Primary files:
- New module under `src/analysis` or `src/labeling`
- Tests under `src/analysis` or `tests/`

Issue 8: Efficient label matching via ANN candidates (Status: Not started)
Scope:
- Candidate generation using per-anchor ANN queries.
- Union/dedupe + scoring pass.
Current state:
- ANN index exists, not used for label scoring.
Acceptance criteria:
- For each label, fetch candidate set via ANN per anchor.
- Compute scores and return ranked list.
- Performance validated on large libraries (use existing bench harness if possible).
Primary files:
- `src/analysis/ann_index.rs`
- New label matching module

Issue 9: Anchor workflows in UI (Status: Not started)
Scope:
- Add "Add as anchor" flows from browser and future map.
- Add "Review matches" with confidence buckets.
Current state:
- No anchor UI or label review UI.
Acceptance criteria:
- Context action to add selected sample as anchor to a label.
- View to list matches with accept/reject.
- Optional auto-tag for high confidence (with confirmation).
Primary files:
- `src/egui_app/ui/*`
- `src/egui_app/controller/*`

Issue 10: Offline UMAP pipeline + storage (Status: Not started)
Scope:
- Generate 2D layout from embeddings.
- Store results in `layout_umap` keyed by model_version.
Current state:
- No UMAP pipeline or storage.
Acceptance criteria:
- CLI or offline tool to build UMAP from DB embeddings.
- Writes to `layout_umap` with `umap_version`.
- Rebuild path when model changes.
Primary files:
- New tool under `src/bin/`
- `src/sample_sources/library/schema.rs`

Issue 11: 2D map UI for exploration (Status: Not started)
Scope:
- Pan/zoom, hover audition, selection, and anchor actions.
- Level-of-detail rendering for large datasets.
Current state:
- No map UI.
Acceptance criteria:
- Canvas/WebGL-based map view with LOD.
- Viewport query to DB to avoid full-load.
- Interaction hooks for audition and anchor assignment.
Primary files:
- `src/egui_app/ui/*`
- `src/egui_app/controller/*`

Issue 12: Optional clustering overlay (Status: Not started)
Scope:
- Run clustering (HDBSCAN) offline and display clusters.
Current state:
- No clustering pipeline or UI.
Acceptance criteria:
- Cluster IDs stored in DB (table or column).
- UI overlay to filter or highlight clusters.
Primary files:
- New tool under `src/bin/`
- `src/egui_app/ui/*`

Issue 13: Label calibration workflow (Status: Not started)
Scope:
- UI flow to thumbs up/down and derive thresholds.
Current state:
- No calibration tooling.
Acceptance criteria:
- Calibration view per label with sample set.
- Suggested `threshold` and `gap` saved to label config.
Primary files:
- `src/egui_app/ui/*`
- `src/egui_app/controller/*`

Issue 14: Correctness tests for similarity + labels (Status: Partial)
Scope:
- Tests for embedding normalization and ANN recall.
- Tests for anchor scoring logic.
Current state:
- Embedding golden test exists.
- No ANN recall tests or anchor scoring tests.
Acceptance criteria:
- ANN recall sanity test on small fixture dataset.
- Unit tests for max/topK and gap logic.
Primary files:
- `src/analysis/embedding.rs`
- New test module(s)

Issue 15: Performance and quality metrics (Status: Partial)
Scope:
- Track similarity latency, map render frame time, label coverage.
Current state:
- Bench harness exists for SQL latency; no similarity/map metrics.
Acceptance criteria:
- Bench or metrics endpoint for similarity query latency.
- Frame-time or draw-call counters for map UI (if added).
- Label coverage stats per label.
Primary files:
- `src/bin/bench/*`
- `src/egui_app/controller/*`
