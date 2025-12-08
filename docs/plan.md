## Goal
- Add a context menu to source list items that offers hard sync (full rescan), quick sync (new/removed/modified check), remap source path, and remove source options.

## Proposed solutions
- Introduce explicit quick vs hard sync flows in the controller/scanner: quick sync reuses the incremental diff scan, while hard sync forces a full rescan/reset of cached/missing state to repair drift.
- Add a source row context menu in the sources panel (patterned after existing browser/collection menus) that preserves selection and routes actions through controller methods with clear status messaging.
- Wire controller operations for the new menu items: trigger the appropriate scan mode, reopen/refresh caches, prompt for a replacement folder when remapping, and reuse removal logic while keeping UI state consistent.
- Extend test coverage (scanner/database and controller helpers where feasible) to lock in scan behaviours, remap persistence, and removal side effects; plan manual smoke checks for the new menu actions.

## Step-by-step plan
1. [x] Inspect source panel UI and controller scan/remove flows to identify existing hooks for context menus and scan triggers.
2. [x] Define and implement quick vs hard sync semantics in scanner/controller, including cache invalidation and status messaging without breaking current scan behaviour.
3. [x] Add context menu rendering for source rows with quick sync, hard sync, remap source, and remove source actions, keeping selection/hover behaviour intact.
4. [x] Implement controller logic for the new menu actions (scan requests, remap with config persistence and reload, removal reuse) and ensure UI updates reflect results.
5. [~] Add/extend automated tests for scan modes and source mutation flows, then do targeted manual validation of context menu actions and scan outcomes.

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
