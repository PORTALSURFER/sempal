## Goal
- Cancel inline renaming when the user clicks elsewhere so rename mode exits cleanly without applying unintended changes.

## Proposed solutions
- Detect pointer/focus changes on inline rename inputs (samples and folders) and treat losing focus due to an external click as a cancel path.
- Ensure rename prompts clear their temporary state when cancelled, keeping explicit apply/enter flows intact.
- Add regression tests that simulate clicking away during rename to lock in the cancel behaviour for both sample and folder contexts.

## Step-by-step plan
1. [x] Review existing rename flows (sample browser and folder browser) in controller and UI layers to map how focus requests and temp state are tracked.
2. [x] Implement focus-loss/click-away detection on inline rename widgets to exit rename mode without applying changes, clearing any stored temp text.
3. [x] Wire controller/UI state resets so cancelled renames fully clear prompts and focus flags across both sample and folder rename paths.
4. [x] Add/extend tests covering click-away cancellation for rename flows and run the relevant test suite to confirm behaviour.

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
