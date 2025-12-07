# Goal
- Add usage documentation to `docs/usage.md` covering how to install/run Sempal, load sources, triage samples, manage collections, and use keyboard shortcuts.

# Proposed solutions
- Document setup and launch steps so users can start the app reliably on supported platforms.
- Describe the UI workflow (sources panel, waveform view, triage columns, collections) with clear, sequential instructions.
- Capture input methods (mouse interactions and keyboard shortcuts) and how they affect triage and playback.

# Step-by-step plan
1. [x] Review existing docs and UI code to catalog current workflows (sources, waveform, triage columns, collections, playback/looping, shortcuts) that need coverage.
2. [x] Draft an outline for `docs/usage.md` (setup/launch, adding sources, viewing waveforms, triage flows, collections management, shortcuts/controls).
3. [x] Write the detailed usage guidance in `docs/usage.md`, aligning terminology with current UI labels and behaviours.
4. [x] Proofread and validate the new documentation for accuracy against the current features and controls.

# Code Style & Architecture Rules Reminder
- File and module structure:
  - Keep files under 400 lines; split when necessary.
  - When functions require more than 5 arguments, group related values into a struct.
  - Each module must have one clear responsibility; split when responsibilities mix.
  - Do not use generic buckets like `misc.rs` or `util.rs`. Name modules by domain or purpose.
  - Name folders by feature first, not layer first.
- Functions:
  - Keep functions under 30 lines; extract helpers as needed.
  - Each function must have a single clear responsibility.
  - Prefer many small structs over large ones.
- Documentation:
  - All public objects, functions, structs, traits, and modules must be documented.
- Testing:
  - All code should be well tested whenever feasible.
  - “Feasible” should be interpreted broadly: tests are expected in almost all cases.
  - Prefer small, focused unit tests that validate behaviour clearly.
  - Do not allow untested logic unless explicitly approved by the user.
