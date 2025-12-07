## Goal
- Move triage from three columns to a single list with hue cues for keep/trash, and add filtering to show all, kept, trashed, or untagged samples.

## Proposed solutions
- Collapse triage state to a single list backed by tag metadata, updating the controller/view model so navigation and selection still work.
- Rebuild the triage UI to render one virtualized list with conditional red/green tinting per row based on tag, keeping the neutral look for untagged items.
- Introduce a filter control (tabs/dropdown) that toggles the visible subset of triage rows without disrupting selection, autoscroll, or drag/drop.
- Adjust drag/drop and playback helpers to respect the filtered view while still operating on the underlying tag-aware list, and cover the new flows with tests.

## Step-by-step plan
1. [-] Audit the current triage data flow (state.rs, controller/wavs.rs, view_model.rs, ui/triage_panel.rs) to map dependencies on the three-column layout and identify required tag metadata for a single list.
2. [-] Refactor triage state and controller helpers to represent one list with tag metadata and filtering options (all/keep/trash/untagged), preserving selection/autoscroll semantics.
3. [-] Update triage rendering to a single virtualized list with per-row hue cues (green for keep, red for trash, neutral otherwise) and confirm drag-hover feedback still works.
4. [-] Add a filter UI control in the triage area (e.g., segmented buttons or dropdown) and wire it to the controller to change the visible subset without losing selection context.
5. [-] Ensure drag/drop targets, playback navigation, and keyboard shortcuts operate correctly against the filtered single list, adjusting hover/selection logic as needed.
6. [-] Add or update tests for triage indexing, filtering, and selection/autoscroll behaviour; outline manual checks for hue rendering and filter toggles.

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
