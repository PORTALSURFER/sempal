## Goal
- Keep missing sources, wav entries, and collection links visible, but clearly marked with a missing icon/colour, and prevent waveform/playback from using stale audio when a missing item is selected.

## Proposed solutions
- Track missing state in persistence: add a boolean flag to wav entries, stop purging rows during scans, and keep missing sources in the config while deriving their status from the filesystem.
- Propagate status flags through controller state and view models so every panel (sources, sample browser, collections, waveform) can react without re-scanning disk paths on every frame.
- Update egui widgets and the waveform panel to render the new indicators, supply a descriptive “missing” message, and suppress playback buffers when unavailable content is selected.
- Back the changes with unit tests for the scanner/database upgrade path, controller selection/playback transitions, and collection rendering to avoid regressions.

## Step-by-step plan
1. [x] Preserve missing items in storage
   - Extend `WavEntry`/SQLite schema with an `is_missing` flag (default false), update scanner `remove_missing` to flip the flag instead of deleting, and ensure all write paths (rename, normalize, exports) reset it when the file reappears.
   - Stop pruning missing sources during config load or wav refresh; replace `drop_missing_source` call sites with a helper that marks the source as missing, raises a status message, but keeps it persisted for future recovery attempts.
2. [x] Surface missing state to the controller/UI
   - Expand controller caches (`wav_entries`, `collections`, drag payloads, etc.) and `view_model` structs (`SourceRowView`, `CollectionRowView`, `CollectionSampleView`) with `missing` flags sourced from the updated data.
   - Track per-source missing markers plus a lookup of missing wav paths so collection samples can check membership without hitting the filesystem repeatedly.
3. [x] Render visual indicators
   - Add a reusable “missing” palette colour/icon helper in `ui::style`/`render_list_row`, then update `sources_panel`, `sample_browser_panel`, and `collections_panel` to show the icon + special colour for any row flagged as missing.
   - Ensure drag/drop affordances and selection markers remain legible against the new colour scheme.
4. [x] Handle waveform + playback for missing files
   - Add state (e.g., `WaveformState::notice`) to show a prominent “file missing” message and blank waveform when a missing entry is focused.
   - When the controller detects a missing selection, clear `decoded_waveform`, `loaded_audio`, stop playback, and skip buffer uploads so previous audio cannot continue playing.
5. [x] Testing and verification
   - Add/extend unit tests covering the DB migration, scanner behaviour, controller selection of missing samples, and collection rendering to confirm the new flags and UI state stay consistent.
   - Manually verify (or document how to verify) that switching between present/missing entries updates indicators and that selecting a missing sample leaves playback silent.

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
