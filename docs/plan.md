## Goal
- Add fuzzy search to the sample browser so users can quickly locate samples within the current filtering view.

## Proposed solutions
- Introduce a search query field in the sample browser header and filter visible rows with a fuzzy matcher scored against existing sample labels.
- Leverage a lightweight Rust fuzzy-matching crate (or a small in-house matcher) to rank and filter without disrupting current tag-based filters.
- Preserve existing autoscroll, selection, and tagging flows by applying search filtering on top of the current triage filter results.

## Step-by-step plan
1. [-] Review current sample browser data flow (state, controller rebuild logic, UI filter header) to identify hook points for search query + results.
2. [-] Choose or implement a fuzzy matching strategy that fits existing dependencies and label caching; define how scores map to inclusion/ordering.
3. [-] Add UI + state for the search field, wire controller filtering to combine triage filters with fuzzy results, and keep selection/autoscroll stable.
4. [-] Add targeted tests for filtered visibility/order and update user-facing docs or hotkey hints to explain fuzzy search behaviour.

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
