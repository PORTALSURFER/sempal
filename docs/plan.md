# Goal
- Color sample item label text using the triage flag color (e.g., red/green) while preserving the regular/default styling when no flag is set.

# Proposed solutions
- Identify where sample item labels render and how triage flags are sourced; expose the flag color through the existing view model or component props.
- Bind the label text color to the triage flag color with a safe default, ensuring accessibility and theme consistency.
- Add targeted tests/visual checks to verify red/green/default rendering without affecting other label states.

# Step-by-step plan
1. [x] Map the sample item rendering pipeline (data model → view model → component) to locate the triage flag value and current label styling entry point.
2. [x] Thread the triage flag color into the label rendering path with a default/fallback when no flag is present, keeping typography and layout unchanged.
3. [x] Validate the behavior with focused tests or snapshots covering flagged red, flagged green, and unflagged labels; adjust any related docs or story fixtures if needed.

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
