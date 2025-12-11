## Goal
- Ensure dropping a sample into the collections area while no collection is active produces clear feedback telling the user to create a collection first instead of silently failing.

## Proposed solutions
- Detect drop attempts with `hovering_collection` or the collection drop zone when `current_collection_id` is `None`, and surface a warning through `set_status` describing that a collection must be created/selected first.
- Alternatively, disable/ignore collection drop targets when there is no active collection and display an inline UI hint, but this still requires a controller-side warning to cover drag sources outside the panel.

## Step-by-step plan
1. [x] Trace the drag/drop flow (`egui_app/controller/drag.rs`, `ui/collections_panel.rs`) to document when `collection_target` becomes `None` even though the user hovered the collections area.
2. [x] Implement a guard (likely in `finish_active_drag` or `handle_sample_drop`) that checks for the "drop into collection without active collection" scenario and calls `set_status` with guidance to create/select a collection; ensure hover/drop indicators stay consistent.
3. [x] Add/adjust controller tests covering the new warning path to keep behaviour stable.

## Code Style & Architecture Rules Reminder
### File and module structure
- Keep files under 400 lines; split when necessary.
- When functions require more than 5 arguments, group related values into a struct.
- Each module must have one clear responsibility; split when responsibilities mix.
- Do not use generic buckets like `misc.rs` or `util.rs`. Name modules by domain or purpose.
- Name folders by feature first, not layer first.

### Functions
- Keep functions under 30 lines; extract helpers as needed.
- Each function must have a single clear responsibility.
- Prefer many small structs over large ones.

### Documentation
- All public objects, functions, structs, traits, and modules must be documented.

### Testing
- All code should be well tested whenever feasible.
- “Feasible” should be interpreted broadly: tests are expected in almost all cases.
- Prefer small, focused unit tests that validate behaviour clearly.
- Do not allow untested logic unless explicitly approved by the user.
