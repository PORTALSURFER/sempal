## Goal
- Add full stereo recording (including ASIO support on Windows) and expose record/play/stop controls in the waveform UI for normal playback.

## Proposed solutions
- Implement an audio input/recording pipeline using CPAL input streams (ASIO host on Windows), writing recorded buffers to WAV via `hound`.
- Add input device/host selection alongside existing output options, persist choices, and default to stereo capture when available.
- Reuse the existing waveform load + playback path by saving recordings to a known recordings folder and loading them as a normal sample.
- Add waveform transport controls (Record/Play/Stop) in `src/egui_app/ui/waveform_view/controls.rs`, wiring them to controller actions with clear state handling.

## Step-by-step plan
1. [x] Review current playback, waveform view controls, and audio output config to identify the best insertion points for input/recording state (`src/audio`, `src/egui_app/controller`, `src/egui_app/state`, `src/egui_app/ui/waveform_view`).
2. [x] Add audio input discovery helpers (hosts/devices/sample rates) in `src/audio/input.rs`, mirroring output selection, and ensure ASIO host/device enumeration works on Windows.
3. [x] Implement a `AudioRecorder` (new module under `src/audio`) that opens a CPAL input stream, captures interleaved stereo frames, and writes WAV files with correct sample rate/channel metadata.
4. [x] Extend config/state for audio input selection and recording status in `src/egui_app/state/audio.rs` plus config persistence (likely `src/egui_app/controller/config.rs` and config types) to remember input host/device/sample rate.
5. [x] Add controller APIs for recording lifecycle (start/stop/cancel), update status messaging, and prevent conflicting playback while recording; route finished recordings to a recordings folder under `app_root_dir()`.
6. [x] Integrate recorded files into the existing waveform load path: create or reuse a “Recordings” source folder and call the existing `load_waveform_for_selection` flow to show the new take immediately.
7. [x] Add Record/Play/Stop buttons to the waveform controls UI (`src/egui_app/ui/waveform_view/controls.rs`) and wire to controller actions; disable/guard controls based on playback/recording state.
8. [x] Add focused tests for the recorder (WAV header + sample count), config persistence for input options, and controller behavior when starting/stopping recording.

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
