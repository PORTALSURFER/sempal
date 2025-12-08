## Goal
Ensure the Windows build of Sempal always displays the custom app icon in the taskbar/pinned apps instead of intermittently falling back to the default Windows placeholder.

## Proposed Solutions
- Investigate how the current runtime PNG-based icon setup interacts with Windows shell resources to identify why the icon sometimes disappears.
- Provide a proper multi-resolution `.ico` asset derived from `assets/logo3.png` and bundle it via the Windows resource pipeline so the OS always has a native icon to show.
- Keep the existing runtime `ViewportBuilder` icon initialization but load it from the `.ico` bytes (or a resilient PNG fallback) to ensure the window icon and the executable resource stay in sync.
- Add validation steps (build-time checks or manual QA notes) so regressions around icon bundling are caught early.

## Step-by-Step Plan
1. [x] Review `src/main.rs`, `build.rs`, and the assets directory to document how icons are currently loaded at runtime and during packaging, and pinpoint where Windows might miss a compiled icon resource.
2. [x] Generate or import a multi-size `.ico` derived from `assets/logo3.png`, add it under `assets/`, and document the conversion approach so it can be regenerated.
3. [x] Update `Cargo.toml`/`build.rs` to include the `.ico` through a Windows resource script (e.g., `winresource` or `embed-resource`), ensuring the executable always exposes the custom icon to the shell.
4. [x] Adjust `load_app_icon()` (or a helper) to prefer the `.ico` data, fall back to the PNG when necessary, and add logging/tests to prevent silent icon failures.
5. [~] Build the Windows target and perform manual verification (and/or automated assertions if feasible) confirming both the taskbar icon and runtime window icon show the custom artwork reliably. (Release/test builds complete; visual taskbar confirmation requires manual QA on Windows shell.)

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
