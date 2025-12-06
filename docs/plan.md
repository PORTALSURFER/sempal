**Goal**
Identify why selecting samples in the sample source lists feels slow, pinpoint the real bottlenecks in the current Rust/Slint pipeline, and outline changes that deliver a much faster, more responsive selection experience without breaking existing features.

**Proposed solutions**
- Profile the selection path end-to-end (UI click → `handle_wav_clicked` → `update_wav_view` → waveform enqueue) using flamegraphs/tracing to quantify delays; use the existing `flamegraph.svg` as a baseline.
- Eliminate linear scans and string allocations on selection by introducing fast path lookups (e.g., path/tag/index maps or emitting indices from the Slint lists) instead of iterating all `wav_entries` in `src/app/sources.rs`.
- Keep list updates incremental: rely on `WavModels`’ cached lookups to flip flags, avoid full model rebuilds when only selection/tag state changes, and reduce redundant `entry_index` passes.
- Move heavy IO off the UI thread: load `db.list_files()` and file existence checks asynchronously, stream wav rows in chunks, and consider pagination/virtualization to avoid instantiating thousands of ListView items at once.
- Strengthen caching/prefetching in the waveform/audio path (grow `WaveformCache`, prefetch neighbours) to cut repeated decodes when hopping between nearby samples.
- Simplify the Slint item tree and bindings: reduce nesting and item count in the wav/source lists, replace per-frame bindings with event-driven updates, and compute heavy derived values in Rust before pushing into simple properties.
- Reduce per-frame text/layout and allocations: keep dynamic text minimal, reuse buffers/models instead of rebuilding, and avoid per-frame `collect()`/vector churn in hot paths.
- Tame wgpu churn: minimize per-frame buffer/bind-group creation, reuse pipelines where possible, and batch updates to lower descriptor allocator pressure (femtovg/Slint-driven).
- Improve profiling fidelity: capture flamegraphs with full debug info (`-C debuginfo=2`, unstripped) to break down the current ~44% “Unknown” block and attribute hotspots.

**Step-by-step plan**
1. [~] Capture a baseline trace of sample selection (large source, rapid clicks) and review `flamegraph.svg`/new perf data to measure time from click to visual update.
2. [~] Document the concrete hot paths (e.g., string-based scans in `handle_wav_clicked`/`update_wav_view`, synchronous `db.list_files`, model rebuild costs, Slint item tree/layout churn) with numbers.
3. [x] Implement selection-path micro-optimizations: add indexed lookups or index-based UI events, reuse cached lookups, and remove redundant `entry_index` walks.
4. [x] Reduce list/model churn: keep `WavModels` updates in-place, trim Slint wav/source list item tree depth/width, and eliminate per-frame bindings in those panels.
5. [~] Offload and streamline IO/rendering: async wav list loads, background file existence checks, larger waveform cache/prefetch, and evaluate paging/virtualization for very large lists.
6. [-] Cut per-frame text/layout and wgpu churn: minimize dynamic text, reuse buffers/bind groups/pipelines where possible, and batch small GPU updates.
7. [~] Validate gains with benches (selection/tagging hot paths), UI smoke tests on big datasets, and gather feedback on any feature-level redesign options before shipping; rerun flamegraph with full debug symbols to break down the “Unknown” block.
