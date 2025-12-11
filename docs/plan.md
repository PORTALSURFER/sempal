## Goal
- Update the folder creation workflow so users pick a parent location first, see a temporary folder row inserted inline, and rename it in-place with Enter to confirm or Escape/click-away to cancel without touching existing features.

## Proposed solutions
- Track inline creation state (target path, pending name, placeholder row) inside the existing folder browser view-model so the UI can render the dummy entry exactly where it will be created.
- Reuse the current filesystem creation logic by triggering it only after the inline edit confirms, ensuring controller + model stay authoritative over folders.
- Extend the UI interactions (toolbar button, context menu, keyboard shortcut) to focus a target row, spawn the inline editor, and listen for Enter/Escape/blur events to either create or discard the pending folder cleanly.

## Step-by-step plan
1. [-] Review the `FolderActionPrompt::Create` flow across controller/state/UI to map which parts must be replaced or extended for inline editing without breaking rename handling.
2. [-] Extend `FolderBrowserUiState` and controller helpers to store inline-creation metadata (target path, placeholder row position, focus flags) and expose helpers for inserting/removing the dummy row while keeping selection/focus intact.
3. [-] Update the UI (`sources_panel.rs`) to render the placeholder folder row in situ, capture keyboard/mouse interactions (Enter/Escape/click outside), and trigger controller callbacks that either call `create_folder` or abort while cleaning up state.
4. [-] Refresh automated coverage (controller tests and any UI-state-focused tests) to reflect the new workflow, ensuring we cover choosing the target, committing the name, cancelling, and focus/selection updates.

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
