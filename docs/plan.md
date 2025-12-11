## Goal
- Add a small dice logo button to the sample browser toolbar (next to the search bar) that plays a random visible sample on click, and toggles sticky random navigation mode on Shift+click.

## Proposed solutions
- **Text/Unicode icon button:** Add an `egui::Button` with a compact label like `dice` or a non-emoji die glyph, styled to match existing toolbar controls; simplest, no new assets.
- **Image icon button:** Add a small PNG/SVG dice icon under `assets/`, load it as a texture (similar to waveform textures), and render it via an `ImageButton`; gives a true “logo” feel but requires asset plumbing.
- **Right-aligned control cluster:** Place the dice button in the right-to-left toolbar area near the item count, optionally indicating sticky mode via highlight; keeps layout tidy regardless of filter label width.

## Step-by-step plan
1. [x] Locate the sample browser toolbar code (`render_sample_browser_filter` in `src/egui_app/ui/sample_browser_panel.rs`) and review existing layout/styling helpers.
2. [x] Decide on icon approach (text vs. image) based on consistency with other UI affordances and effort; if image, pick/create a small dice asset.
3. [x] Add a dice button to the toolbar horizontal row, positioned near the search field or in the right-aligned cluster, with hover text explaining click vs. Shift+click.
4. [x] Wire click handling: normal click calls `controller.play_random_visible_sample()`, Shift+click calls `controller.toggle_random_navigation_mode()`.
5. [x] Add a visual sticky-mode indicator on the button when `controller.random_navigation_mode_enabled()` is true (e.g., selected state or subtle color change) to reflect toggle state.
6. [x] Add/adjust tests if feasible (prefer controller-level unit tests for random mode and random play invariants) and manually verify UI behavior and layout.
7. [x] Update usage/help text if needed (e.g., `docs/usage.md` or hotkey overlay) to mention the dice button as an alternative to `Shift+R` / `Alt+R`.
8. [x] Fix egui API mismatch for selected-state button styling on older egui versions.

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
