## Goal
- Refactor the oversized UI/controller modules (e.g., `src/egui_app/ui.rs`, `src/egui_app/state.rs`, `src/waveform.rs`) into focused submodules that meet the project structure rules (files under 400 lines, clear responsibilities) without changing existing behaviour.

## Proposed solutions
- Partition `src/egui_app/ui.rs` into a lean orchestrator plus feature-focused submodules (input/hotkeys, overlays, waveform viewport, panels) while preserving current UI flow.
- Split `src/egui_app/state.rs` into feature-aligned state modules (status/sources/browser/waveform/hotkeys/audio/controls) with documented public types and minimal cross-coupling.
- Decompose `src/waveform.rs` into rendering, decoding, and view-model submodules with clear APIs and dedicated tests.
- Keep public interfaces stable via re-exports or adapter functions to avoid breaking consumers.

## Step-by-step plan
1. [x] Inventory responsibilities in `src/egui_app/ui.rs`, identify natural module boundaries (e.g., hotkey handling, overlays, waveform viewport, panels), and decide target submodule layout.
2. [x] Extract `ui.rs` responsibilities into new submodules, wire them through `EguiApp` as the orchestrator, and trim the root file below 400 lines while preserving behaviour.
3. [x] Split `src/egui_app/state.rs` into feature-specific state modules with documented types; update imports/re-exports to keep the public surface unchanged and keep each file under 400 lines.
4. [x] Restructure `src/waveform.rs` into separate decoding, rendering, and view-model modules; ensure APIs remain stable and add/adjust tests for relocated logic.
5. [x] Run formatting/lint/tests (e.g., `cargo fmt`, `cargo clippy`, `cargo test`) and fix regressions to confirm the refactor is non-breaking.

## Code Style & Architecture Rules Reminder
- Keep files under 400 lines; split when necessary.
- When functions require more than 5 arguments, group related values into a struct.
- Each module must have one clear responsibility; split when responsibilities mix.
- Do not use generic buckets like misc.rs or util.rs. Name modules by domain or purpose.
- Name folders by feature first, not layer first.
- Keep functions under 30 lines; extract helpers as needed.
- Each function must have a single clear responsibility.
- Prefer many small structs over large ones.
- All public objects, functions, structs, traits, and modules must be documented.
- All code should be well tested whenever feasible.
- “Feasible” should be interpreted broadly: tests are expected in almost all cases.
- Prefer small, focused unit tests that validate behaviour clearly.
- Do not allow untested logic unless explicitly approved by the user.
