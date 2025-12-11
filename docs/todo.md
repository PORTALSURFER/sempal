- lets do a housekeeping pass, clean up the codebase, cleanup warnings, reduce file lengths, improve DRYness, improve maintainability, collapse large structs/objects into clearly named smaller objects, add missing docs, improve symbol naming, find and resolve bugs, improve performance, etc.
lets then write every task you find into @todo.md as a new todo item

--

- [x] Collapse nested build script windows resource guard to satisfy clippy.
- [x] Normalize selection module docs and tighten waveform/audio utilities to cut clippy noise (mutability, defaults, map_or usage).
- [x] Derive defaults for identifiers and waveform view variants to simplify config handling.

- [ ] Refactor large UI/controller modules that still exceed 400 lines (e.g., `src/egui_app/ui.rs`, `src/egui_app/state.rs`, `src/waveform.rs`) into focused submodules to align with project structure rules.
- [ ] Replace deprecated uses of `criterion::black_box` in `benches/tagging.rs` with `std::hint::black_box` to future-proof benchmarks.
