## Goal
- Add a contextual hotkey system that reacts to whichever UI element (sample browser rows, collection samples, global controls) currently holds focus, and surface a Ctrl+/ popup that lists the active shortcuts.

## Proposed solutions
- Audit existing focus heuristics in `src/egui_app/ui.rs`, `ui::sample_browser_panel.rs`, and `controller::wavs` to determine whether we can reuse `SampleBrowserState.selected` / `CollectionsState.selected_sample` or if we need an explicit `FocusContext` enum shared across the app.
- Introduce a dedicated focus manager inside `UiState` (e.g., `UiFocusState { context: FocusContext, last_interaction: Instant }`) plus controller helpers that update it whenever the user clicks, hovers, or navigates via keyboard so we have a single source of truth for “current focus”.
- Implement a hotkey registry (module in `src/egui_app/controller/hotkeys.rs`) that maps focus contexts to structured `HotkeyAction` definitions (id, label, modifiers, handler fn) so that UI code can query the active actions and dispatch them uniformly.
- Wire `egui_app::ui` to read the registry each frame: when `ctx.input(|i| ...)` sees a matching gesture it should call the associated controller method (`toggle_focused_selection`, `normalize_browser_sample`, `trash_selected_wavs`, `add_sample_to_collection`, etc.) using whichever row is focused.
- Add a lightweight overlay widget (e.g., `ui::hotkey_overlay.rs`) that appears when Ctrl+/ is pressed, rendering the subset of `HotkeyAction`s returned for the active focus plus any always-on global shortcuts. Update `docs/usage.md` (and tooltips) to describe the new UI affordance.

## Step-by-step plan
1. [x] Document the current focus and keyboard handling paths across `ui.rs`, `ui::sample_browser_panel.rs`, and controller selection helpers to confirm what parts can be reused versus replaced.
2. [-] Add a unified `FocusContext` + `UiFocusState` in `state.rs` and extend controller methods so clicks, keyboard navigation, and collection interactions keep the context in sync without breaking existing selection/autoscroll behaviour.
3. [-] Implement a contextual hotkey registry module plus controller-facing dispatch helpers, then update `ui.rs` keyboard handling to rely on it for the new `x/n/d/c` shortcuts (keeping existing behaviour feature-parity where applicable).
4. [-] Build the Ctrl+/ overlay widget that queries the registry for the current focus/global actions and displays them with labels/modifiers, ensuring it coexists with other panels and respects theming.
5. [-] Extend automated tests under `src/egui_app/controller/tests.rs` (and add new ones if needed) to cover focus updates and hotkey dispatch, then refresh `docs/usage.md` (or other user docs) to describe the contextual shortcuts and overlay.

## Current focus + keyboard findings
- `EguiApp::update` (src/egui_app/ui.rs) infers a boolean `collection_focus` from `UiState.collections.selected_sample`, and otherwise assumes browser focus when `browser.selected` is `Some`, so no explicit focus enum exists yet.
- Sample browser focus comes from `SampleBrowserState.selected_visible`/`selected` maintained by controller methods such as `focus_browser_row`, `grow_selection`, etc. (see src/egui_app/controller/wavs.rs) and UI clicks (`ui::sample_browser_panel.rs`).
- Collection selection relies on `CollectionsState.selected_sample`, toggled exclusively through UI clicks and navigation helpers like `nudge_collection_sample`; hotkeys currently skip this path except for arrow keys.
- Keyboard shortcuts today are hardcoded near the top of `EguiApp::update` (space, escape, arrows, ctrl+arrows, `X`), so adding new gestures means duplicating branching logic in that function.

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
- “Feasible” should be interpreted broadly: tests are expected in almost all cases.
- Prefer small, focused unit tests that validate behaviour clearly.
- Do not allow untested logic unless explicitly approved by the user.
