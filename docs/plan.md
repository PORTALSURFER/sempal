## Goal
- Restyle the waveform selection duration label to be subtle, text-only, top-right aligned, smaller, and colored with a brighter, more opaque version of the selection color.

## Proposed solutions
- Adjust the selection label rendering in `src/egui_app/ui/waveform_view.rs` to remove the background and reposition the text to the selection’s top-right corner.
- Use the existing selection highlight color with higher opacity/brightness for the text and choose a smaller text style for subtlety.
- Keep selection duration computation intact; only tweak presentation and update any affected tests if layout changes are asserted.

## Step-by-step plan
1. [x] Review current selection duration rendering to understand positioning, sizing, and color usage.
2. [x] Update the selection label drawing to remove the background, apply the brighter selection color, shrink the text style, and align it to the selection’s top-right.
3. [x] Run affected tests (or add/update if needed) to ensure selection duration rendering logic remains correct.
4. [x] Iterate on label styling to add a subtle, full-width status bar background while keeping text legible.
5. [x] Adjust selection duration bar to sit at the very top, with swapped colors (dark text on lighter bar) for clearer contrast.
6. [x] Reposition the selection duration bar to the bottom of the selection to avoid clashing with the loop bar.
7. [x] Restyle the selection drag handle to use the selection color with higher opacity/brightness for clearer visibility.

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
