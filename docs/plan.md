## Goal
- Give the regular sample/triage list the same context menu actions as the collection sample list (tagging, normalize overwrite, rename, delete-equivalent) without changing existing behaviours.

## Proposed solutions
- Extract shared menu-building helpers so both collection and triage sample lists share identical context menu layout and reuse existing UI patterns.
- Extend the triage-side controller with equivalents for normalize/rename/delete/tag using existing collection helpers for filesystem + database updates, ensuring waveform/selection refresh.
- Cover the new actions with controller/UI tests and manual checks to keep drag/select/autoscroll behaviour stable.

## Step-by-step plan
1. [-] Audit collection context menu features and define their mapping for triage rows (including desired handling for the “delete” action on regular samples).
2. [-] Implement controller pathways for triage actions (tag, normalize overwrite, rename, delete/remove) reusing shared helpers and updating caches, waveform, and loaded audio state.
3. [-] Refactor or share context menu UI code so `triage_panel.rs` renders the same menu as `collection_sample_menu`, keeping selection and drag behaviour unchanged.
4. [-] Add tests covering triage context menu actions and state updates; validate both triage and collection lists still render/scroll/drag as before.
5. [-] Do a quick manual pass (or targeted integration check) to confirm menus appear and actions work on both lists without regressions.

## Code Style & Architecture Rules Reminder
### File and module structure
- Keep files under 400 lines; split when necessary.
- When functions require more than 5 arguments, group related values into a struct.
- Each module must have one clear responsibility; split when responsibilities mix.
- Do not use generic buckets like misc.rs or util.rs. Name modules by domain or purpose.
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
