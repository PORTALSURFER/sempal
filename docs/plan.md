## Goal
- Migrate the entire UI from Slint to egui, preserving current functionality (sample sources, triage columns, waveform/selection, playback controls, collections sidebar with drag/drop tagging) and maintaining existing state/persistence semantics.

## Proposed solutions
- Recreate the window and layout using egui panels/splitters, mirroring the current structure: sources list, central waveform + triage lists, right-hand collections pane.
- Implement custom drag/drop within egui: draggable sample rows with hover previews and drop targets for collections, keeping tagging semantics (no movement between lists).
- Port Slint data models to pure Rust egui state structs, reusing existing app logic (collections, wav list handling, playback) with minimal rewrites.
- Wrap audio/waveform rendering in egui-friendly components (texture upload for waveform image, playhead/selection overlays, keyboard + mouse interactions).
- Replace Slint callback wiring with egui event handling loop, ensuring hotkeys, selection, scrolling, and status updates remain intact.

## Step-by-step plan
1. [x] Inventory current UI features and interactions (sources, wav triage, waveform selection/loop, collections/drag-drop, status) to map them to egui widgets and events.
2. [x] Design egui layout structure (top bar, left sources panel, central waveform + triage columns, right collections pane) with theming consistent with existing app.
3. [x] Define egui-side state models mirroring current Slint models (rows for sources/wavs/collections, drag state, selection state) and connect to existing app logic/persistence.
4. [x] Implement egui rendering for core panels: sources list with actions, triage lists with selection/highlight, status bar, top bar controls.
5. [x] Implement waveform view in egui (texture rendering + overlays) and wire playback/selection interactions and keyboard shortcuts.
6. [~] Implement collections panel with add/select, member list, and drag/drop tagging from wav rows, including hover/drop feedback and preview rendering.
5. [-] Implement waveform view in egui (texture rendering + overlays) and wire playback/selection interactions and keyboard shortcuts.
6. [-] Implement collections panel with add/select, member list, and drag/drop tagging from wav rows, including hover/drop feedback and preview rendering.
7. [-] Replace application entry/runtime to launch egui (winit integration), remove Slint dependencies, and ensure audio/worker threads communicate with the egui state.
8. [-] Add or update tests for state transitions (collections, selection, tagging) and perform manual QA of the egui UI parity; remove Slint assets/deps once validated.
