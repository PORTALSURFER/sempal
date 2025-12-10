## Goal
- Update `docs/styleguide.md` colors to match the palette currently used in the app.

## Proposed solutions
- Extract the canonical color palette from the live app theme variables to use as the source of truth.
- Compare existing color tokens and swatches in `docs/styleguide.md` against the app palette to spot mismatches.
- Refresh the style guide color tables/swatches and usage notes to reflect the live palette and clarify primary/secondary/supporting roles.
- Add quick guidance on keeping the style guide synced with future palette changes.

## Step-by-step plan
1. [x] Inventory current app color sources (theme variables/constants) and record the canonical palette and token names.
2. [x] Review `docs/styleguide.md` to identify mismatches or outdated color entries versus the live palette.
3. [x] Update `docs/styleguide.md` color tables/swatches and usage notes to align with the current app palette without altering other sections.
4. [x] Quick pass to ensure naming, hex values, and guidance stay consistent and concise.

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
