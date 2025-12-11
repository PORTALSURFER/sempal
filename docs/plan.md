## Goal
- Make the space bar start playback from the waveform cursor when present, and have Escape clear the active waveform cursor so playback defaults to the start of the sample again.

## Proposed solutions
- Adjust the space bar handler to prefer the persistent waveform cursor as the playback anchor while preserving existing modifier behaviour (shift/ctrl/command).
- Update escape handling to clear the waveform cursor and reset the default start marker, keeping other Escape effects intact.
- Add focused tests around playback start and cursor clearing to lock in the new behaviour without regressing selection and playback flows.

## Step-by-step plan
1. [-] Review current waveform cursor usage for playback start and Escape handling in `src/egui_app/ui.rs`, `controller/playback.rs`, and `controller/waveform_navigation.rs` to map existing fallbacks.
2. [-] Implement space bar behaviour to start from the active waveform cursor (with existing modifier paths unchanged) and ensure cursor-based starts update the last start marker appropriately.
3. [-] Extend Escape handling to clear the waveform cursor and reset the default start to the beginning when a cursor was active, while preserving other Escape side effects.
4. [-] Add or update tests covering space/Escape interactions with the waveform cursor and playback start fallbacks; run the relevant test suite.

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
