## Goal
- Add a folder browser below the sources list (keeping the sources list to roughly 1/4 of the sidebar height) that filters the sample browser contents to the selected source folders, with multi-selection and focus/`x` toggle handling; selecting a nested folder should deselect its direct parent.

## Proposed solutions
- Build a folder tree from the current source’s wav entries (relative paths) during browser rebuilds, caching per source and refreshing on source changes or rescans; clear selections when data no longer matches.
- Render a new folder browser section beneath the sources list with a bounded height, sharing list styling/focus affordances, supporting click/keyboard selection, and applying parent/child deselection rules.
- Apply the selected folders as an additional filter in the sample browser rebuild pipeline so no selection shows all samples, and selected folders restrict visible rows to matching prefixes while preserving existing search/tag filters.

## Step-by-step plan
1. [x] Inspect the sources panel layout, sample browser rebuild/filter logic, and focus/hotkey plumbing to identify integration points for a folder tree and folder-scoped filtering.
2. [x] Define folder browser UI state and controller data flow: derive a directory tree from wav entries of the active source, track expanded + selected folders, and clear/repair selections when sources or scans change.
3. [x] Extend sample browser filtering to respect selected folders (empty = all), ensuring selections stay in sync with visible data and that nested selections deselect parents as required.
4. [x] Update the sources sidebar UI: cap the sources list height (~1/4 of the panel) and add a scrollable folder browser below with focus highlighting, mouse/keyboard navigation, multi-select toggling via clicks/`x`, and parent-child deselection behavior.
5. [~] Add or update tests (and manual checks) for folder filtering, selection edge cases, and hotkey/focus overlays, verifying existing browser interactions remain unchanged.

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
  - “Feasible” should be interpreted broadly: tests are expected in almost all cases.
  - Prefer small, focused unit tests that validate behaviour clearly.
  - Do not allow untested logic unless explicitly approved by the user.
