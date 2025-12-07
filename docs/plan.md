## Goal
- Add an F11 hotkey that toggles between windowed and fullscreen modes, with fullscreen enabled by default at startup.

## Proposed solutions
- Start the eframe viewport in fullscreen via `ViewportBuilder`/`NativeOptions` while keeping the existing window size constraints available for returning to windowed mode.
- Handle the F11 keypress in `EguiApp::update`, toggling fullscreen through the frame/viewport API and tracking the current mode to avoid desynchronization.
- Optionally remember the last windowed dimensions or mode in app state/config so exiting fullscreen restores a sensible size without impacting other features.

## Step-by-step plan
1. [x] Confirm current viewport setup and keyboard handling in `main.rs` and `EguiApp::update`, and choose the correct fullscreen API for eframe 0.27 without affecting other shortcuts.
2. [x] Make fullscreen the default startup mode while preserving the existing window sizing constraints for windowed mode.
3. [x] Add F11 handling to toggle between fullscreen and windowed modes, keeping the tracked state in sync and ensuring current UI behaviour remains intact.
4. [x] Outline manual QA notes to verify default fullscreen launch and F11 toggling across platforms.

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
