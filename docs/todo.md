Training-free discovery roadmap, audited against the current codebase. Each item is a standalone issue with status and app-specific details.

<!-- Issue 1: Document and align the CLAP embedding contract (Status: Partial) -->
<!-- Scope: -->
<!-- - Formalize the embedding contract: input windowing, mono mixdown, normalization target, sample rate, and repeat/pad behavior. -->
<!-- - Align app embedding preprocessing with curated dataset preprocessing. -->
<!-- Current state: -->
<!-- - CLAP inference exists with fixed 48kHz, 10s repeat/pad, and L2 normalization. -->
<!-- - Analysis pipeline decodes to 16kHz and feeds embeddings through resample + sanitize only. -->
<!-- Acceptance criteria: -->
<!-- - One canonical contract documented in `docs/feature_vector.md` or a new `docs/embedding_contract.md`. -->
<!-- - App embedding path uses `preprocess_mono_for_embedding` or explicitly justifies divergence. -->
<!-- - Golden embedding tests updated if preprocessing changes. -->
<!-- Primary files: -->
<!-- - `src/analysis/embedding.rs` -->
<!-- - `src/analysis/audio.rs` -->
<!-- - `src/egui_app/controller/analysis_jobs/pool.rs` -->
<!-- - `docs/feature_vector.md` -->

<!-- Issue 2: DB schema migration for `tf_labels` + `tf_anchors` (Status: Not started) -->
<!-- Scope: -->
<!-- - Add anchor-label tables with explicit constraints and indexes. -->
<!-- - Define fields for thresholds, gap, topk, weight, and timestamps. -->
<!-- Current state: -->
<!-- - `embeddings`, `labels_user`, `labels_weak`, and `ann_index_meta` exist. -->
<!-- - `labels` table exists but is unused and does not match anchor-based design. -->
<!-- Acceptance criteria: -->
<!-- - Migration adds `tf_labels` and `tf_anchors` with foreign keys to `samples`. -->
<!-- - Indexes exist for label lookup and anchor lookup (`label_id`, `sample_id`). -->
<!-- - Schema tests updated to assert new tables exist. -->
<!-- Primary files: -->
<!-- - `src/sample_sources/library/schema.rs` -->
<!-- - `src/sample_sources/library/tests.rs` -->

<!-- Issue 2b: DB backfill + legacy `labels` compatibility decision (Status: Not started) -->
<!-- Scope: -->
<!-- - Audit any usage of legacy `labels` table and decide on retention or migration. -->
<!-- - Provide a safe migration path without data loss. -->
<!-- Current state: -->
<!-- - `labels` table appears unused by runtime UI but exists in schema. -->
<!-- Acceptance criteria: -->
<!-- - Explicit decision documented (keep unused, migrate, or deprecate). -->
<!-- - If migration: data copied into `tf_labels` with a defined mapping. -->
<!-- - If deprecate: documented warning and no runtime dependency. -->
<!-- Primary files: -->
<!-- - `src/sample_sources/library/schema.rs` -->
<!-- - `docs/todo.md` -->

<!-- Issue 2c: DB schema migration for `layout_umap` (Status: Not started) -->
<!-- Scope: -->
<!-- - Add table for 2D coordinates with `umap_version` and `model_id`. -->
<!-- - Add indexes to support viewport queries. -->
<!-- Current state: -->
<!-- - No `layout_umap` storage in DB. -->
<!-- Acceptance criteria: -->
<!-- - Migration adds `layout_umap(sample_id, model_id, umap_version, x, y, created_at)`. -->
<!-- - Index on `(model_id, umap_version)` and optional spatial bucket index. -->
<!-- - Schema tests updated to assert table and indexes exist. -->
<!-- Primary files: -->
<!-- - `src/sample_sources/library/schema.rs` -->
<!-- - `src/sample_sources/library/tests.rs` -->

<!-- Issue 3: Embed ingestion: model_version tracking and lifecycle (Status: Partial) -->
<!-- Scope: -->
<!-- - Store `model_version` or `model_id` consistently with timestamps. -->
<!-- - Support re-embedding when model_version changes. -->
<!-- Current state: -->
<!-- - Embeddings table includes `model_id`, `dim`, `dtype`, `l2_normed`. -->
<!-- - Backfill jobs and analysis pipeline insert embeddings and update ANN index. -->
<!-- Acceptance criteria: -->
<!-- - Embedding writes include `created_at` and `model_version` (or `model_id` already used, but tracked in metadata). -->
<!-- - Re-embedding job path that can invalidate and recompute embeddings by model change. -->
<!-- - Clear UX messaging for re-embedding progress. -->
<!-- Primary files: -->
<!-- - `src/egui_app/controller/analysis_jobs/db.rs` -->
<!-- - `src/egui_app/controller/analysis_jobs/enqueue.rs` -->
<!-- - `src/egui_app/controller/analysis_jobs/pool.rs` -->
<!-- - `src/sample_sources/library/schema.rs` -->

<!-- Issue 4: ANN index load-from-disk and parameter validation (Status: Partial) -->
<!-- Scope: -->
<!-- - Load HNSW index from disk when available and valid. -->
<!-- - Validate index params vs current `model_id` and embedding dim. -->
<!-- Current state: -->
<!-- - Index builds from DB and persists, but always rebuilds on startup. -->
<!-- Acceptance criteria: -->
<!-- - Index load fast-path using `hnsw_rs` file load. -->
<!-- - On param mismatch, auto-rebuild and update metadata. -->
<!-- - Manual rebuild command or UI action. -->
<!-- Primary files: -->
<!-- - `src/analysis/ann_index.rs` -->
<!-- - `src/sample_sources/library/schema.rs` -->

<!-- Issue 5: Similarity search by audio clip (Status: Not started) -->
<!-- Scope: -->
<!-- - Support "query by audio blob" or temp file in app. -->
<!-- - Use embedding inference and ANN search against library. -->
<!-- Current state: -->
<!-- - Similarity search by sample_id exists via UI "Find similar". -->
<!-- Acceptance criteria: -->
<!-- - Controller function for ad-hoc audio input (selection, drag-drop, or file). -->
<!-- - UI entry point (context menu, button, or drag target). -->
<!-- - Results list uses existing browser filter flow. -->
<!-- Primary files: -->
<!-- - `src/egui_app/controller/wavs/similar.rs` -->
<!-- - `src/egui_app/ui/sample_browser_filter.rs` -->
<!-- - `src/egui_app/controller/analysis_jobs/pool.rs` -->

<!-- Issue 6: Anchor label CRUD and thresholds (Status: Not started) -->
<!-- Scope: -->
<!-- - CRUD for anchor labels, anchors, thresholds, gap, and topK. -->
<!-- Current state: -->
<!-- - Only `labels_user` and `labels_weak` exist; no anchor labels. -->
<!-- Acceptance criteria: -->
<!-- - DB CRUD for labels and anchors (create, update, delete). -->
<!-- - Weighting and threshold fields stored and editable. -->
<!-- - Clean integration with existing label concepts (avoid conflicts with `labels_user`). -->
<!-- Primary files: -->
<!-- - `src/sample_sources/library/schema.rs` -->
<!-- - `src/egui_app/controller` -->

<!-- Issue 7: Anchor scoring logic and confidence buckets (Status: Not started) -->
<!-- Scope: -->
<!-- - Implement max or mean-of-topK scoring. -->
<!-- - Apply gap logic and bucket classification. -->
<!-- Current state: -->
<!-- - No anchor scoring logic implemented. -->
<!-- Acceptance criteria: -->
<!-- - Pure function for scoring with unit tests. -->
<!-- - Gap logic returns high/medium/low confidence buckets. -->
<!-- - Pluggable aggregation strategy (max vs topK mean). -->
<!-- Primary files: -->
<!-- - New module under `src/analysis` or `src/labeling` -->
<!-- - Tests under `src/analysis` or `tests/` -->

<!-- Issue 8: Efficient label matching via ANN candidates (Status: Not started) -->
<!-- Scope: -->
<!-- - Candidate generation using per-anchor ANN queries. -->
<!-- - Union/dedupe + scoring pass. -->
<!-- Current state: -->
<!-- - ANN index exists, not used for label scoring. -->
<!-- Acceptance criteria: -->
<!-- - For each label, fetch candidate set via ANN per anchor. -->
<!-- - Compute scores and return ranked list. -->
<!-- - Performance validated on large libraries (use existing bench harness if possible). -->
<!-- Primary files: -->
<!-- - `src/analysis/ann_index.rs` -->
<!-- - New label matching module -->

<!-- Issue 9a: Anchor UX flow spec (Status: Not started) -->
<!-- Scope: -->
<!-- - Define user flows: create label, add anchors, review matches, auto-tag. -->
<!-- - Define entry points (browser row, context menu, map point). -->
<!-- Current state: -->
<!-- - No anchor UI or label review UI. -->
<!-- Acceptance criteria: -->
<!-- - UX flow documented in `docs/egui_layout.md` or new doc. -->
<!-- - States include empty, loading, confidence buckets, and error handling. -->
<!-- Primary files: -->
<!-- - `docs/egui_layout.md` -->

<!-- Issue 9b: Anchor controller + persistence wiring (Status: Not started) -->
<!-- Scope: -->
<!-- - Controller methods to create/update labels and anchors. -->
<!-- - Persistence layer for CRUD operations and reads. -->
<!-- Current state: -->
<!-- - No anchor CRUD or state integration. -->
<!-- Acceptance criteria: -->
<!-- - Controller API for `create_label`, `add_anchor`, `remove_anchor`, `update_thresholds`. -->
<!-- - DB CRUD queries added and unit-tested where feasible. -->
<!-- Primary files: -->
<!-- - `src/egui_app/controller/*` -->
<!-- - `src/sample_sources/library/schema.rs` -->

<!-- Issue 9c: Anchor UI implementation (Status: Not started) -->
<!-- Scope: -->
<!-- - UI actions for add-as-anchor and manage anchors. -->
<!-- - Match review list with accept/reject and preview. -->
<!-- Current state: -->
<!-- - No anchor UI or label review UI. -->
<!-- Acceptance criteria: -->
<!-- - Context action to add selected sample as anchor to a label. -->
<!-- - Match review view with confidence buckets and actions. -->
<!-- - Optional auto-tag gated behind confirmation. -->
<!-- Primary files: -->
<!-- - `src/egui_app/ui/*` -->
<!-- - `src/egui_app/controller/*` -->

<!-- Issue 10a: CLI tool to build UMAP layout (Status: Not started) -->
<!-- Scope: -->
<!-- - Offline tool reads embeddings from DB for a `model_id`. -->
<!-- - Writes `layout_umap` rows with `umap_version`. -->
<!-- Current state: -->
<!-- - No UMAP pipeline or storage. -->
<!-- Acceptance criteria: -->
<!-- - New CLI binary with args: `--db`, `--model-id`, `--umap-version`, `--seed`. -->
<!-- - Writes rows to `layout_umap` and reports count. -->
<!-- - Fails fast if embeddings missing or dim mismatch. -->
<!-- Primary files: -->
<!-- - `src/bin/*` -->
<!-- - `src/sample_sources/library/schema.rs` -->

<!-- Issue 10b: UMAP validation + export artifacts (Status: Not started) -->
<!-- Scope: -->
<!-- - Validate layout coverage and coordinate ranges. -->
<!-- - Optionally export summary stats for QA. -->
<!-- Current state: -->
<!-- - No UMAP validation tooling. -->
<!-- Acceptance criteria: -->
<!-- - Validation step checks: % covered, NaN/inf, min/max ranges. -->
<!-- - Emits a small JSON report next to the DB or logs. -->
<!-- - Fails if coverage below threshold (configurable). -->
<!-- Primary files: -->
<!-- - `src/bin/*` -->

<!-- Issue 11a: Map UI viewport + rendering (Status: Not started) -->
<!-- Scope: -->
<!-- - Pan/zoom interaction and LOD rendering of points. -->
<!-- Current state: -->
<!-- - No map UI. -->
<!-- Acceptance criteria: -->
<!-- - Canvas/WebGL-based view with zoom and pan. -->
<!-- - LOD rendering (heatmap or decimated points) when zoomed out. -->
<!-- Primary files: -->
<!-- - `src/egui_app/ui/*` -->

<!-- Issue 11b: Map data controller + persistence (Status: Not started) -->
<!-- Scope: -->
<!-- - Query layout points by viewport and zoom level. -->
<!-- - Provide map data to UI without full-load. -->
<!-- Current state: -->
<!-- - No map data controller or queries. -->
<!-- Acceptance criteria: -->
<!-- - Controller method: `map_points_for_viewport(xmin, xmax, ymin, ymax, zoom)`. -->
<!-- - DB query uses `layout_umap` and returns sample ids + coords. -->
<!-- Primary files: -->
<!-- - `src/egui_app/controller/*` -->
<!-- - `src/sample_sources/library/schema.rs` -->

<!-- Issue 11c: Map interactions (Status: Not started) -->
<!-- Scope: -->
<!-- - Hover audition, select, and anchor assignment actions. -->
<!-- Current state: -->
<!-- - No map interactions. -->
<!-- Acceptance criteria: -->
<!-- - Hover shows label + preview; click auditions. -->
<!-- - Right-click or action menu supports "Add as anchor". -->
<!-- Primary files: -->
<!-- - `src/egui_app/ui/*` -->
<!-- - `src/egui_app/controller/*` -->

Issue 12a: CLI tool to build HDBSCAN clusters (Status: Not started)
Scope:
- Offline clustering over embeddings or UMAP coords.
- Writes cluster IDs to DB.
Current state:
- No clustering pipeline or UI.
Acceptance criteria:
- New CLI with args: `--db`, `--model-id`, `--method=embedding|umap`.
- Persists `cluster_id` per sample into a new table or column.
- Logs cluster counts and noise ratio.
Primary files:
- `src/bin/*`
- `src/sample_sources/library/schema.rs`

Issue 12b: Clustering validation report (Status: Not started)
Scope:
- Validate clustering output quality and coverage.
Current state:
- No clustering validation tooling.
Acceptance criteria:
- Emits summary: number of clusters, % noise, min/max cluster sizes.
- Fails or warns on extreme noise ratios (configurable).
Primary files:
- `src/bin/*`

Issue 12c: Cluster overlay UI (Status: Not started)
Scope:
- Display cluster boundaries or color overlays in map.
- Filter by cluster id.
Current state:
- No clustering UI.
Acceptance criteria:
- Map coloring by cluster id with legend or filter.
- Optional toggle to hide noise points.
Primary files:
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
