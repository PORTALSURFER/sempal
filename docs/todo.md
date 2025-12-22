- lets remove the weak_label system and ui
- lets remove the features column in the ui
- lets remove the categories from the ui

Training-free discovery roadmap, audited against the current codebase. Each item is a standalone issue with status and app-specific details.

<!-- Issue 11: High-precision similarity search + automatic clustering (Status: Not started) -->
<!-- Scope: -->
<!-- - Two-stage similarity search (ANN recall + re-rank for precision). -->
<!-- - Automatic clustering to seed "sounds-like" anchor bases. -->
<!-- - UI workflow to name clusters and promote to labels. -->
<!-- Current state: -->
<!-- - ANN index exists and supports sample-to-sample similarity. -->
<!-- - No re-rank stage or clustering pipeline. -->
<!-- Acceptance criteria: -->
<!-- - Embedding refresh job can rebuild embeddings + ANN index end-to-end. -->
<!-- - Similarity search returns fast candidates + re-ranked results with tunable precision/recall. -->
<!-- - Clustering job produces clusters with centroid + representative samples. -->
<!-- - UI shows clusters, supports naming, and creates labels/anchors from cluster reps. -->
<!-- Step-by-step plan: -->
<!-- 1. Add a similarity search pipeline that returns ANN candidates, then re-ranks by a -->
<!--    higher-precision scorer (e.g., average similarity to a small anchor set). -->
<!-- 2. Add config knobs for ANN topK, re-rank topK, and precision/recall slider mapping. -->
<!-- 3. Implement a clustering job (HDBSCAN or agglomerative) over normalized embeddings. -->
<!-- 4. Persist clusters with centroid + medoid IDs in the library DB. -->
<!-- 5. Add a cluster browser UI with representative samples and naming workflow. -->
<!-- 6. Add "Create label from cluster" action that seeds anchors and thresholds. -->
<!-- 7. Add tests for clustering determinism (fixed seed) and re-rank scoring. -->
<!-- Primary files: -->
<!-- - `src/analysis/ann_index.rs` -->
<!-- - `src/analysis/*` -->
<!-- - `src/egui_app/controller/wavs/similar.rs` -->
<!-- - `src/egui_app/controller/analysis_jobs/*` -->
<!-- - `src/sample_sources/library/schema.rs` -->
<!-- - `src/egui_app/ui/*` -->
<!-- - `docs/feature_vector.md` -->
