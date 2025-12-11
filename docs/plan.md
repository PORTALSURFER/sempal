## Goal
- Ensure that when random navigation mode (Alt+R) is active, flagging a filtered sample (e.g., untagged) advances focus to the next random item rather than the next row in list order, while preserving existing behaviour when random mode is off.

## Proposed solutions
- Inspect the current triage tagging flow and `refocus_after_filtered_removal` logic to identify where focus is advanced after a flag removes the item from the filtered list.
- Route focus through the random navigation pathway when random mode is enabled (reusing random history/visibility constraints), with a safe fallback to sequential focus when random navigation is off or no random target exists.
- Expand or adjust tests around random navigation + filtered tagging to lock in the new focus behaviour.

## Step-by-step plan
1. [-] Trace tagging/flagging and focus-advance logic (including filters and random navigation state) to pinpoint the right hook for changing post-flag focus.
2. [-] Implement focus advancement that selects the next random visible sample when random navigation mode is enabled, falling back to list-ordered focus otherwise.
3. [-] Update or add tests covering flagging under filtered views in random mode to verify the new navigation behaviour.

## Code Style & Architecture Rules Reminder
- File and module structure
  - Keep files under 400 lines; split when necessary.
  - When functions require more than 5 arguments, group related values into a struct.
  - Each module must have one clear responsibility; split when responsibilities mix.
  - Do not use generic buckets like `misc.rs` or `util.rs`. Name modules by domain or purpose.
  - Name folders by feature first, not layer first.
- Functions
  - Keep functions under 30 lines; extract helpers as needed.
  - Each function must have a single clear responsibility.
  - Prefer many small structs over large ones.
- Documentation
  - All public objects, functions, structs, traits, and modules must be documented.
- Testing
  - All code should be well tested whenever feasible.
  - “Feasible” should be interpreted broadly: tests are expected in almost all cases.
  - Prefer small, focused unit tests that validate behaviour clearly.
  - Do not allow untested logic unless explicitly approved by the user.
