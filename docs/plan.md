## Goal
- Move collection data out of the JSON config into SQLite, switch the config to a lean TOML settings file, migrate existing config.json content, and ensure all app files live in a stable user directory instead of temporary locations.

## Proposed solutions
- Introduce a dedicated SQLite-backed storage for collections (and other persisted library data like sources if needed), keeping the config file for app settings only.
- Replace JSON config with TOML, keeping schema focused on app flags/preferences and storing it under the user-specific app directory.
- Add migration that detects the current config.json path, imports sources/collections into SQLite, rewrites settings to TOML, and cleans up legacy paths safely.
- Audit path resolution so databases, config, logs, and other key files are rooted under the user folder (no temp dirs), with fallbacks/diagnostics when paths are unavailable.

## Step-by-step plan
1. [x] Audit current persistence flow (config usage, app_dirs paths, collection handling, per-source SQLite) to map which data lives in config.json and which files might land in temp locations.
2. [x] Design the new storage layout: lean TOML config schema for app settings; SQLite schema/location for collections (and persisted sources if we move them) under the app’s user directory; decide file names and migration targets.
3. [x] Implement storage changes and migration logic: add/extend SQLite layer for collections, switch config read/write to TOML, and create a migration that reads the legacy config.json, seeds the database, writes the new config, and preserves/backups legacy data when needed.
4. [x] Update application logic to use the new storage: adjust controllers/UI to load/save collections and sources from SQLite, keep config interactions limited to app settings, and refresh any docs/help text that reference config behavior.
5. [x] Add and run tests for the new persistence path: migration coverage, config TOML round-trips, SQLite collection ops, and checks that key files resolve to user directories; execute relevant cargo tests to guard regressions.

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
