## Goal
- Add a distinct lower-third selection drag handle on the waveform so users can drag a cropped range into the sample list or a collection, creating a saved file in the current source and listing it in the appropriate view (and collection) automatically.

## Proposed solutions
- Render a dedicated drag-handle overlay inside the existing waveform selection (lower third, contrasting color/border) using the current egui painter flow in `waveform_view.rs`.
- Extend controller drag state to differentiate selection-based drags from existing sample drags, reusing the drag overlay visuals while tagging the payload with selection bounds and source info.
- Introduce a cropping/writing helper that uses the already-decoded waveform bytes/duration to export the selected span to a new wav in the active source folder with unique naming, updating the source DB/cache without a full rescan.
- Handle drops onto the triage “Samples” list and collection drop zones to materialize the cropped file, attach it to the current source (and collection when applicable), refresh UI lists, and surface errors/status.
- Cover the new crop/export path with focused tests around range-to-bytes conversion and database/list updates, plus manual validation notes for the drag/drop UX.

## Step-by-step plan
1. [x] Review waveform selection rendering and drag handling (`waveform_view.rs`, controller `playback.rs`, `drag.rs`) to place the lower-third handle and identify state needed for a selection drag payload.
2. [x] Capture selection audio context: retain decoded audio bytes/duration and current source/path in controller state so a selection drag can be turned into an export without reloading.
3. [x] Implement the selection drag handle UI and gestures: paint the handle, start a selection-drag payload with bounds metadata, and integrate with the existing drag overlay for visual feedback.
4. [x] Add controller drop handling for selection drags: on drop over the triage sample list, write the cropped wav into the active source (unique filename), upsert the source DB/cache, and refresh triage lists.
5. [x] Extend drop handling for collections: when a selection drag lands on a collection/drop zone, create the cropped file in the source root, register it, add it to the target collection, and respect export folder syncing/status.
6. [x] Add tests for the cropping/writing helper and list-refresh behaviour, and outline manual checks for the new drag handle UX and collection integration (manual: drag handle shows grab/grabbing cursor; dropping on Samples saves and selects the clip; dropping on a collection saves and attaches the clip/export).

## Code Style & Architecture Rules Reminder
- File and module structure
  - Keep files under 400 lines; split when necessary.
  - When functions require more than 5 arguments, group related values into a struct.
  - Each module must have one clear responsibility; split when responsibilities mix.
  - Do not use generic buckets like `misc.rs` or `util.rs`. Name modules by domain or purpose.
  - Name folders by feature first, not layer first.
- Functions
  - Keep functions under 30 lines; extract helpers as needed.
  - Each function must have a single clear responsibility.
  - Prefer many small structs over large ones.
- Documentation
  - All public objects, functions, structs, traits, and modules must be documented.
- Testing
  - All code should be well tested whenever feasible.
  - "Feasible" should be interpreted broadly: tests are expected in almost all cases.
  - Prefer small, focused unit tests that validate behaviour clearly.
  - Do not allow untested logic unless explicitly approved by the user.
