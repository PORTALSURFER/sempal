## Goal
- Add an options menu that lets users select the audio output device, sample rate, and common output settings, including ASIO driver support on Windows, without breaking existing playback.

## Proposed solutions
- Surface audio host/device enumeration (WASAPI/ASIO/CoreAudio/ALSA, etc.) through the controller by wrapping rodio/cpal selection while keeping a safe default fallback.
- Persist audio output preferences (host/backend, device identifier, sample rate, buffer/latency knobs) in the config and reload them safely with migrations.
- Extend `AudioPlayer` to open streams against the chosen device/settings with fallbacks, including ASIO hosts on Windows, and route errors to the status bar.
- Build a dedicated options UI section using the existing Options menu to manage audio output settings with validation and helpful messaging.

## Step-by-step plan
1. [x] Review the current audio pipeline (`src/audio.rs`, controller lifecycle, config load/save) to locate integration points for host/device selection and sample-rate overrides.
2. [x] Extend persisted config/state to store audio output preferences (backend including ASIO on Windows, device identifier, sample rate, buffer/latency), with migration defaults and validation when unavailable.
3. [x] Update the audio backend to instantiate `AudioPlayer` using the selected host/device/settings, add fallbacks to defaults on failure, and surface warnings/errors to the UI without disrupting playback.
4. [x] Implement Options UI to list available hosts/devices/settings, allow switching (including ASIO where available), and persist user choices while matching existing menu styling.
5. [~] Add tests/diagnostics for config migration and device selection logic (platform-gated where needed) and document the new audio settings behavior for manual verification.

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
