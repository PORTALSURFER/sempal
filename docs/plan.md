## Goal
- Decide whether to remove or exercise unused controller methods (`fade_waveform_selection_*`, `nudge_folder_focus`, `zoom_waveform_steps`) to avoid dead code drift.

## Proposed solutions
- Add targeted usage in tests or UI flows where these interactions make sense to preserve behaviour coverage.
- Remove or gate the unused methods behind feature flags if they are truly obsolete.
- Convert them to private helpers or document intentional unused status to satisfy linting while clarifying intent.

## Step-by-step plan
1. [x] Confirm current callers and intended UX for each method to determine if functionality should be kept.  
2. [x] Decide per-method: wire into existing flows/tests or mark for removal based on UX alignment.  
3. [x] Implement chosen action (add usage/tests or remove/gate), keep files under size limits, and run clippy/tests to verify.  

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
