# Goal
- Prevent drag-and-drop from duplicating selection exports across destinations: dropping a selection onto a collection should not add it to the sample browser, and dropping into the sample browser should not also add it to the active collection.

# Proposed solutions
- Tighten drag target resolution so drops only apply to the explicitly hovered area, removing the implicit fallback to the currently selected collection.
- Add a collection-targeted selection export path that registers the file for collection use without pushing it into the sample browser lists or changing browser focus.
- Align UI/status updates and tests with the clarified drop behaviour to ensure regressions are caught.

# Step-by-step plan
1. [-] Trace the current drag/drop flow for samples and selections (controller drag handling, selection export, collection add helpers) to confirm where implicit dual-addition occurs.
2. [-] Update drop target resolution to avoid defaulting to the active collection when hovering the sample browser or no collection target is indicated.
3. [-] Implement collection-only selection exports: skip adding exported clips to browser state while still ensuring the collection can reference and tag the file.
4. [-] Refresh UI/status cues and extend/adjust tests to lock in the new single-destination drop behaviour.

# Code Style & Architecture Rules Reminder
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
