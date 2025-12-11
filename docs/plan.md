## Goal
- Redesign drag-and-drop handling so one authoritative target definition drives all drop actions (collections, triage, folders, future panels) without conflicting hover updates.

## Proposed solutions
- Introduce a `DragTarget` enum held in `DragState` so panels report entering/exiting targets with structured data instead of toggling booleans.
- Create a priority-aware resolver that records the latest target per panel (collections panel, triage area, folder panel, waveform) and exposes a deterministic active target.
- Maintain per-panel hover ownership tokens so when a panel loses the pointer it explicitly clears its target contribution, preventing stale state.
- Add integration-style tests that simulate drag paths bouncing between panels to ensure the resolver consistently selects the expected target and warning flows still work.

## Step-by-step plan
1. [x] Audit current drag state mutations across all panels (`ui/collections_panel.rs`, `ui/sample_browser_panel.rs`, `ui/sources_panel.rs`, `ui/waveform_view.rs`) and document every `update_active_drag` invocation plus its intent (see `docs/drag_audit.md` for details).
2. [-] Design the `DragTarget` enum (variants for triage columns, collections rows/drop zone, folder panel, external drag, none) and extend `DragState` to store the current target plus optional debug history.
3. [-] Define how external drag-outs (DAW/OS drops) interact with the new system—either as a dedicated enum variant or by pausing internal targets—and ensure `maybe_launch_external_drag`/`start_external_drag` cooperate with the unified drag state.
4. [-] Refactor `update_active_drag` to accept the new target enum (and optional priority) along with pointer metadata; update each panel to call it on hover enter/exit and remove direct mutations of `hovering_*` fields.
5. [-] Update `finish_active_drag`, `handle_sample_drop`, and selection drop paths to match on the new target enum so folder moves, collection adds, triage tagging, and warnings work off the unified state.
6. [-] Add or adjust controller tests (and consider UI simulation tests) that reproduce key drag paths, including transitions between panels, external drag-out handoffs, and the “no active collection” case, ensuring the resolver always picks the intended target.
7. [-] Provide optional debug instrumentation (e.g., feature-flagged logging or an inspector view) that surfaces target transitions for future troubleshooting without overwhelming normal logs.

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
