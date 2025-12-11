## Goal
- Move the sample browser's item count label so it sits flush against the right sidebar while keeping the existing filter buttons and search box behaviour intact.

## Proposed solutions
- Rework `render_sample_browser_filter` so the existing horizontal row splits into left-aligned controls (filter buttons plus search field) and a right-aligned count label rendered via `ui.with_layout(egui::Layout::right_to_left(...))`, ensuring the count always hugs the sidebar.
- Alternatively, wrap the left controls in a child `ui.horizontal` and then allocate a secondary `ui` with a right-aligned layout for the label, allowing us to keep the rendering logic localized and responsive without introducing spacer hacks.

## Step-by-step plan
1. [x] Review `src/egui_app/ui/sample_browser_panel.rs::render_sample_browser_filter` to confirm current spacing, palette usage, and how `visible_count` is calculated so the refactor preserves behaviour.
2. [x] Update the layout so the count label is rendered inside a right-aligned layout segment (e.g., right-to-left horizontal with fixed width or min size) while the filter buttons and search box remain grouped on the left, verifying it still respects dynamic widths and focus handling.
3. [-] Manually test (or capture UI screenshots) across narrow and wide browser sizes to confirm the label hugs the sidebar, the row wraps gracefully, and no regressions occur in filter/search interactions; add or adjust UI regression tests if available.

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
- "Feasible" should be interpreted broadly: tests are expected in almost all cases.
- Prefer small, focused unit tests that validate behaviour clearly.
- Do not allow untested logic unless explicitly approved by the user.
