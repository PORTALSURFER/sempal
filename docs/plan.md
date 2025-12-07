## Goal
- Add a context menu to collection items that lets users pick an export folder, automatically copy new collection files there, remove files on collection removal, and offer a manual “refresh export” to reconcile external changes.

## Proposed solutions
- Extend the collection model and persisted config with an optional export target path plus any metadata needed to reconcile on refresh.
- Hook collection add/remove flows to copy/delete files to the export folder, surfacing status feedback while guarding against missing sources or IO errors.
- Add an egui context menu for collection rows/items to set or clear the export path via a folder picker and trigger a refresh that re-syncs the collection with the export directory contents.

## Step-by-step plan
1. [x] Review collection data flow (model, config persistence, controller/UI wiring) to choose insertion points for export paths and menu actions without breaking existing selection/drag behaviour.
2. [x] Extend the collection domain/state to store an export path (and any per-export metadata if needed) and persist it through config load/save and view model structures.
3. [x] Implement export handling: when adding/removing collection members, copy/delete files in the export folder, handling missing sources/IO errors and updating status messaging and UI counts.
4. [x] Add egui context menu on collection list/items to set/clear export path using the existing `rfd::FileDialog` flow and to trigger a manual “refresh export”.
5. [x] Build refresh-export reconciliation that scans the export folder to add missing items (when source paths resolve) and prune entries whose files are gone, updating config/UI accordingly.
6. [x] Add focused tests for collection export bookkeeping and config round-trips, plus manual validation notes for the new UI flows.
7. [x] Show a warning indicator for collections without an export path in the UI list.
8. [x] Prompt for export path when creating a new collection via the existing add flow.
9. [x] Nest exports under per-collection subfolders inside the chosen base directory.
10. [x] Add an option to open the collection export folder in the OS file explorer.
11. [x] Flatten exports and refresh to ignore subfolders and keep files at the collection export root.
12. [x] Allow renaming collections and their export folder from the UI.

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
