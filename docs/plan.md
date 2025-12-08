## Goal
- Enable users to drag a folder from the OS file explorer onto the sample source list to add it as a new Sample Source without using the picker dialog.

## Proposed solutions
- Track the on-screen bounds of the sources panel inside `EguiApp` and watch egui`s `hovered_files`/`dropped_files`; when the cursor is over that rect, highlight it and forward dropped folder paths to `EguiController::add_source_from_path`.
- Introduce a lightweight UI state flag (either in `EguiApp` or `SourcePanelState`) that marks when external drag payloads hover the panel, allowing us to reuse existing render helpers while keeping controller logic untouched except for consuming new drop events.

## Step-by-step plan
1. [x] Inspect the current `EguiApp::update` loop plus `render_sources_panel` to confirm how pointer events and layout rects are managed, and decide where to store the panel bounds for drop-hit testing.
2. [x] Update the sources panel rendering to record its bounding `Rect`, watch egui`s `hovered_files`, and render a visual cue (frame tint or overlay) whenever an external drag hovers the area with a path payload.
3. [x] Extend the app update loop to process `dropped_files`: if the drop position falls inside the stored panel rect, normalize every dropped path, filter to directories, and call `EguiController::add_source_from_path`, bubbling any errors to the status bar.
4. [x] Document manual QA steps to verify the drag/drop flow and watch for regressions in source selection and drag interactions when running the UI locally.

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
