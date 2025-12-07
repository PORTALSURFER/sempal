## Goal
- Rework the egui UI to match the Microchip Brutalism style in `docs/styleguide.md`: rectilinear geometry (no rounded corners), beveled diagonals only when softening, dense grid-like layout, metallic dark palette, and hard-edged, mechanical interactions across all panels, controls, and waveform chrome without altering existing behaviours.

## Proposed solutions
- Define a centralized visual system (colors, stroke weights, spacings, bevel/diagonal rules) applied via the existing `apply_visuals` hook and shared render helpers so every widget inherits the rectilinear sci-fi look.
- Refactor shared UI primitives (frames, list rows, scroll containers, buttons, badges, sliders) to enforce sharp rectangles or 45° bevels, textured fills, and the palette from the style guide while keeping layout metrics stable.
- Recompose key surfaces (status bar, waveform frame, browser/collection panels, drag overlays) into layered rectangular compartments with nested borders and micro-line accents, replacing any rounded or circular motifs.
- Validate the refreshed styling with targeted visual passes (e.g., hover/selected states, drag/drop highlights, waveform selection handles) to confirm consistency and avoid interaction regressions.

## Step-by-step plan
1. [x] Audit current theming and primitives (e.g., `apply_visuals`, `helpers::render_list_row`, panel frames, status bar, waveform chrome) against `docs/styleguide.md` to map where rounded/circular elements and soft shading exist.
2. [x] Introduce centralized style tokens (palette, stroke weights, bevel/diagonal rules, spacing) and update shared primitives to draw only rectangles or 45° bevels, adjust hover/selection fills, and align typography to the guide.
3. [x] Restyle panels (sources, collections, sample browser, menus) to use layered rectangular frames, strict dividers, and rectilinear controls/tags while preserving existing data flow and interactions.
4. [x] Redesign the status bar and waveform chrome (frames, selection handles, playhead, loop toggle, drag overlays) to remove rounded corners/circles, add nested borders/grid lines, and apply the metallic palette without changing behaviour.
5. [~] Perform a visual verification pass (hover/active/drag states, autoscroll outlines, waveform selection/drag ergonomics) and adjust any regressions while keeping existing functionality intact.

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
