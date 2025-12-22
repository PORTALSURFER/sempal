## Goal
- Replace the trained classification pipeline with a training-free system using CLAP embeddings, ANN similarity search, user anchors/labels, and a 2D UMAP map that fits the current Rust + SQLite architecture.

## Proposed solutions
- **Incremental MVP first:** Deliver CLAP embeddings + ANN similarity search, then layer anchors/labels, then the UMAP map; reduces risk and validates each stage.
- **Backend-first integration:** Implement data model, embedding ingestion, and ANN index in Rust, then expose APIs for UI to adopt; keeps UI changes clean.
- **Hybrid offline tooling:** Use a small offline tool for UMAP (and optional clustering), while keeping runtime UI and scoring in-app; avoids heavy runtime dependencies.

## Step-by-step plan
1. [-] Review existing embedding/feature vector work (e.g., `docs/todov2.md` and related Rust modules) to align with current ingestion and DB patterns.
2. [-] Define the CLAP embedding contract: model artifact format, input windowing, mono mixdown, normalization target, output dim, and `model_version` tracking.
3. [-] Add or update SQLite schema/migrations for `embeddings`, `labels`, `anchors`, `layout_umap`, and `index_meta`, including backfill strategy.
4. [-] Implement embedding ingestion: audio decode, preprocessing, CLAP inference, L2-normalization, and persistence keyed by `model_version`.
5. [-] Integrate ANN (HNSW) index lifecycle: build from embeddings, persist/load, and incremental updates on new embeddings.
6. [-] Implement similarity APIs (by `sample_id` and by audio blob) and wire ANN queries with cosine similarity scoring.
7. [-] Implement label/anchor CRUD plus scoring (max or mean-of-topK), gap logic, and confidence buckets.
8. [-] Implement efficient label match retrieval using per-anchor ANN candidate sets, union/dedupe, then scoring and ranking.
9. [-] Add UI workflows for anchors and label suggestions (add anchor, review matches, optional auto-tag), staying consistent with current UI patterns.
10. [-] Build the offline UMAP pipeline and persist `(x, y)` to `layout_umap` for the current `model_version`.
11. [-] Implement the 2D map UI with pan/zoom, hover audition, selection, and anchor actions using canvas/WebGL with LOD rendering.
12. [-] Add optional clustering (HDBSCAN) and overlays/filters for cluster and label views.
13. [-] Add calibration tooling for thresholds and gap tuning from user feedback.
14. [-] Add tests and metrics: embedding norm checks, ANN recall sanity on subsets, anchor scoring unit tests, and latency/frame-time stats.

## Code Style & Architecture Rules Reminder
- Keep files under 400 lines; split when necessary.
- When functions require more than 5 arguments, group related values into a struct.
- Each module must have one clear responsibility; split when responsibilities mix.
- Do not use generic buckets like `misc.rs` or `util.rs`. Name modules by domain or purpose.
- Name folders by feature first, not layer first.
- Keep functions under 30 lines; extract helpers as needed.
- Each function must have a single clear responsibility.
- Prefer many small structs over large ones.
- All public objects, functions, structs, traits, and modules must be documented.
- All code should be well tested whenever feasible.
- “Feasible” should be interpreted broadly: tests are expected in almost all cases.
- Prefer small, focused unit tests that validate behaviour clearly.
- Do not allow untested logic unless explicitly approved by the user.
