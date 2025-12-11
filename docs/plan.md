## Goal
- Replace deprecated use of `criterion::black_box` in `benches/tagging.rs` with `std::hint::black_box` to future-proof benchmarks while keeping behaviour unchanged.

## Proposed solutions
- Use `std::hint::black_box` and adjust imports/usages to remove reliance on Criterion's deprecated helper.
- Ensure benchmark structure stays the same so existing measurements remain valid.
- Validate the benchmark still builds/runs (e.g., `cargo bench --bench tagging`) after the change.

## Step-by-step plan
1. [x] Inspect `benches/tagging.rs` to confirm any remaining `criterion::black_box` usages and existing imports.
2. [x] Update the benchmark to call `std::hint::black_box`, cleaning up imports without altering behaviour (already present; no code changes needed).
3. [x] Run relevant checks (e.g., `cargo bench --bench tagging` or `cargo check`) to confirm the benchmark builds with the new black box.

## Code Style & Architecture Rules Reminder
### File and module structure
- Keep files under 400 lines; split when necessary.
- When functions require more than 5 arguments, group related values into a struct.
- Each module must have one clear responsibility; split when responsibilities mix.
- Do not use generic buckets like `misc.rs` or `util.rs`. Name modules by domain or purpose.
- Name folders by feature first, not layer first.

### Functions
- Keep functions under 30 lines; extract helpers as needed.
- Each function must have a single clear responsibility.
- Prefer many small structs over large ones.

### Documentation
- All public objects, functions, structs, traits, and modules must be documented.

### Testing
- All code should be well tested whenever feasible.
- "Feasible" should be interpreted broadly: tests are expected in almost all cases.
- Prefer small, focused unit tests that validate behaviour clearly.
- Do not allow untested logic unless explicitly approved by the user.
