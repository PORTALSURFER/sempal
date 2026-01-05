## Goal
- Plan a shift from the current PANNs-heavy similarity pipeline toward a faster, descriptor-first system inspired by Sononym/Ableton, while preserving existing functionality and compatibility.

## Proposed solutions
- Replace or supplement PANNs embeddings with a lightweight descriptor vector (spectral, temporal, pitch, timbre) and a weighted similarity metric.
- Add a small learned projection (e.g., PCA) over descriptors to reduce dimensionality without introducing large neural nets.
- Shorten analysis windows and optimize preprocessing (batching, caching) to reduce per-sample compute time.
- Keep PANNs as an optional backend during transition for A/B comparisons and fallback.

## Step-by-step plan
1. [-] Audit current embedding pipeline (PANNs preprocessing, model ID contract, storage, ANN index) to identify integration points and constraints.
2. [-] Define a descriptor feature set aligned with Sononym/Ableton-style aspects (timbre/MFCC stats, spectral shape, envelope/attack-decay, pitch stats, duration), including normalization strategy and target dimensionality.
3. [-] Design storage/schema updates for descriptor vectors (tables, model IDs, versioning) without breaking existing embeddings.
4. [-] Implement descriptor extraction modules and unit tests, reusing existing FFT/mel utilities where possible.
5. [-] Add similarity scoring for descriptor vectors (weighted cosine/L2) and expose tunable weights in config/UI if appropriate.
6. [-] Implement optional dimensionality reduction (PCA/offline projection) and integrate into indexing/search.
7. [-] Update indexing/search pipeline to handle multiple embedding backends (PANNs vs descriptors) and ensure smooth migration.
8. [-] Benchmark extraction and query performance on representative sample sets; tune weights/window lengths for quality vs speed.
9. [-] Add regression tests/fixtures for descriptor extraction, similarity ranking, and DB migrations.
10. [-] Document the new similarity pipeline, configuration knobs, and migration guidance.

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
