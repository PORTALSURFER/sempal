## Goal
- Add an egui context menu to collection sample list rows that can delete items, rename an item, mark items as trashed/neutral/keep, and normalize a sample by overwriting its original file.

## Proposed solutions
- Add a context menu to collection sample rows that targets the clicked row (and any current selection) with actions for delete, rename, tag updates, and normalization, including sensible confirmation and disabled states.
- Extend controller/state wiring to support these actions by reusing existing sample source metadata (paths, tags, caches) so collection, triage, and waveform views stay in sync without breaking current behaviours.
- Implement normalization and file mutations through shared decoding/writing utilities to update the on-disk wav, refresh database metadata, and keep UI/cache state consistent, while avoiding breaking changes.

## Step-by-step plan
1. [x] Review collection sample state/view models and selection handling to determine how to target clicked vs. selected samples and resolve full paths from source + relative paths.
2. [x] Add an egui context menu on collection sample rows exposing delete, rename, tag (trash/neutral/keep), and normalize actions with appropriate enable/disable and confirmation behaviour.
3. [x] Implement controller/domain handlers: delete (remove from collection, source DB, filesystem), rename (filesystem + DB + collection metadata), tag updates via existing tagging flows, and normalization that decodes, scales, and overwrites the wav while refreshing metadata.
4. [x] Refresh UI/state paths so collection lists, triage lists, waveform/selection caches, and export folders reflect changes; ensure selection/focus is preserved or cleared appropriately after actions.
5. [x] Add focused tests around the new controller helpers (delete/rename/tag/normalize) and run targeted checks to guard regressions.

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
