## Goal
- Plan a per-source single-file ANN index format that sits alongside each source `library.db`, matching current HNSW performance and flexibility.

## Proposed solutions
- Implement a custom single-file container that stores HNSW graph/data plus id map with fixed offsets for mmap-friendly access.
- Keep HNSW serialization but add a thin wrapper that packs/unpacks the two HNSW files and id map into one file without changing the search algorithm.
- Add versioned header metadata to support future format changes and multiple embedding models.

## Step-by-step plan
1. [-] Audit the current ANN storage flow (file paths, HNSW dump/load, id map handling) and confirm where the per-source DB path is resolved.
2. [-] Define the single-file container format (header, version, model_id, offsets, lengths, checksum) and decide binary vs JSON for the id map.
3. [-] Add a new storage module to read/write the container file, keeping mmap-friendly layout and minimal copying.
4. [-] Update ANN build/load paths to use the container file next to `library.db`, with fallback to legacy files and automatic migration.
5. [-] Add tests for container round-trip, migration from old files, and consistency with existing ANN search results.
6. [-] Benchmark load/search performance vs the current multi-file approach and confirm parity.
7. [-] Document the new file format, migration behavior, and any cleanup tooling.

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
