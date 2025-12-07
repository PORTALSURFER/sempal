## Goal
- Update all sempal crates to their latest compatible versions while keeping existing features stable and fixing any build breakages.

## Proposed solutions
- Use `cargo outdated` and crate release notes to map patch/minor/major upgrade targets across `eframe`/`egui`, `rodio`, `rusqlite`, `rfd`, `directories`, `image`, `sysinfo`, `open`, `serde`, and supporting tooling.
- Upgrade dependencies incrementally (low-risk patch/minor first, then majors) to isolate breaking changes and keep default feature behaviour aligned with the current app.
- Adjust code for API changes in the UI (egui/eframe), audio (rodio/hound), persistence (rusqlite/serde), and build tooling (toml_edit/semver) while preserving existing UX and feature flags.
- Refresh the lockfile and validate builds/tests to ensure the application, build script, and benches remain healthy after upgrades.
- Sanity-check runtime flows (config load/save, source scanning, waveform rendering, audio playback) after updates to catch regressions early.

## Step-by-step plan
1. [x] Inventory current dependencies and decide target versions using `cargo outdated` and changelogs, with special attention to `eframe`/`egui`, `rodio`, `rusqlite`, `rfd`, `sysinfo`, `image`, `directories`, `open`, `serde`, and build/dev tools.
2. [x] Update `Cargo.toml` dependency, dev-dependency, and build-dependency versions in manageable batches, regenerating `Cargo.lock` while keeping existing feature flags consistent.
3. [x] Fix compile-time and API breakages across modules (`audio`, `waveform`, `egui_app`, `sample_sources`, build scripts) to match new crate expectations without altering behaviour.
4. [x] Run `cargo check`/`cargo test` (and clippy/benches if configured) to ensure the workspace builds cleanly; address any failures.
5. [-] Perform a quick manual sanity check of UI launch, config persistence, source scanning, waveform rendering, and audio playback paths to confirm runtime stability.

## Code Style & Architecture Rules Reminder
### File and module structure
- Keep files under 400 lines; split when necessary.
- When functions require more than 5 arguments, group related values into a struct.
- Each module must have one clear responsibility; split when responsibilities mix.
- Do not use generic buckets like misc.rs or util.rs. Name modules by domain or purpose.
- Name folders by feature first, not layer first.

### Functions
- Keep functions under 30 lines; extract helpers as needed.
- Each function must have a single clear responsibility.
- Prefer many small structs over large ones.

### Documentation
- All public objects, functions, structs, traits, and modules must be documented.

### Testing
- All code should be well tested whenever feasible.
- “Feasible” should be interpreted broadly: tests are expected in almost all cases.
- Prefer small, focused unit tests that validate behaviour clearly.
- Do not allow untested logic unless explicitly approved by the user.
