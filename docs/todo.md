Step-by-step implementation plan for training-free sample discovery, labeling, and map exploration.

1. Define the CLAP embedding pipeline contract: input windowing rules, mono mixdown, gain normalization target, and output vector shape + model_version tagging.
2. Add schema changes: embeddings, labels, anchors, layout_umap, index_meta (including migrations and backfill strategy).
3. Implement embedding ingestion path: load sample metadata, decode audio, compute CLAP embeddings, L2-normalize, persist to DB, and record model_version.
4. Build ANN index integration (HNSW): bulk build from embeddings, persist/load, and incremental updates on new embeddings.
5. Implement similarity search API endpoints (query by sample_id, query by audio blob) and wire to ANN lookup with cosine similarity.
6. Add anchor-based label data model operations: create label, add/remove anchors, update thresholds/topk/gap.
7. Implement label scoring logic (max or mean-of-topK) plus winner/gap rules and confidence buckets.
8. Implement efficient label match retrieval: candidate generation via per-anchor ANN queries, union + dedupe, then scoring and ranking.
9. Add UI/UX for anchors and label workflows: add-as-anchor from sample browser, review matches, and optional auto-tag.
10. Design and run offline UMAP generation pipeline; persist coordinates to layout_umap per model_version.
11. Build 2D map UI: pan/zoom, hover audition, selection, and anchor actions; use canvas/WebGL with LOD rendering.
12. Add optional clustering (HDBSCAN) and overlay/filters for clusters and labels.
13. Add calibration flow for thresholds and gap tuning using user feedback.
14. Add correctness tests: embedding norm check, ANN recall sanity on subset, anchor scoring unit tests.
15. Add performance/quality metrics collection: latency p95, map frame time, and label coverage stats.
