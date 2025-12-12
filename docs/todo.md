# Codebase review TODOs

## Correctness & robustness
- In `src/external_clipboard.rs` (Windows):
  - Remove unnecessary `GlobalUnlock` calls when `GlobalLock` fails.
  - Consider RAII wrappers for `HGLOBAL`/locks to reduce unsafe surface and clarify ownership transfer to the clipboard.
- In `src/waveform/decode.rs`, confirm int scaling is correct for 24-bit WAV (current `samples::<i32>()` path is OK but worth adding fixture coverage).
- In `src/audio.rs`, review span/loop math:
  - `progress` and `remaining_loop_duration` depend on wall-clock; confirm no drift/rounding issues on long loops.

## Performance
- Waveform rendering:
  - Profile `render_color_image_with_size` oversampling/downsampling costs on large files; consider caching sampled columns per zoom level.
  - Avoid per-frame allocations in hot paths (e.g., `Vec` creation in render and browser list rebuilds).
- Audio decoding:
  - `DecodedWaveform.samples` stores full interleaved `f32` for the whole file; consider streaming/decimation for very long samples.
- Browser search:
  - `build_visible_rows` does a full scan and fuzzy match every rebuild; cache labels/scores and invalidate incrementally when possible.

## Testing
- Move heavy integration-like controller tests out of `src/egui_app/controller/tests.rs` into `tests/` with shared fixtures to keep production files smaller.
- Add targeted tests for:
  - Playhead/loop progression edge cases (short samples, selections near end, full wrapped play).
  - WAV decode scaling for different bit depths and channel layouts.
  - Scanner behavior on IO errors and symlink/permission edge cases.

## Documentation & UX
- Add dice toolbar button in sample browser:
  - Click: play random visible sample.
  - Shift+click: toggle “sticky random navigation mode”.
  - Ensure hotkey/overlay docs updated in `docs/usage.md`.
- Update `README.md` “Build from source” with Windows ASIO note and `CPAL_ASIO_DIR` env requirement (also mention for devs).
- Ensure all public structs/enums/fns have `///` docs; many are currently undocumented in `src/egui_app/state/*` and controller modules.

## Cleanup & style
- Remove broad `#![allow(dead_code)]` in `src/egui_app/state/mod.rs` and `src/egui_app/view_model.rs`; prefer per-item allows or delete unused code.
- Replace `#[allow(clippy::too_many_arguments)]` in `src/egui_app/ui/helpers.rs` by grouping params into a struct or builder.
- Standardize error types: a mix of `Result<_, String>` and typed errors exists; consider promoting key subsystems (audio, waveform, sources) to typed errors for consistency.
- Consider feature-gating Windows-only code paths in UI/controller to reduce `cfg_attr(..., allow(dead_code))` clutter.

- moving trashed items to the trash folder will freeze the ui, lets turn this into an async background task instead. lets also add a progress bar in the statusbar to indicate the progression of this action.

- in the collection items list, the selected items will block scrolling, lets fix that. the user should be able to freely scroll

- in wavefrom context, add 't' to trim audio selection

- in wavefrom context, add hotkeys '/' and '\' to fade audio selection, lets also adjust the fade algo so its a soft S curve, not a hard diagonal. 

- in wavefrom context, add hotkey 'n' to normalize audio selection when a selection is available, otherwise just normalize the whole thing like the sample browser normalize does.

- in wavefrom context, add hotkey 'c' to crop selection, and 'C/shift+c' for crop as non-destructive crop as new sample option, adding a new sample in the same location as the original with _crop001 added, etc.

- lets sync up the collection list when a collection export root gets mapped, listing each direct subfolder as a collection entry

- lets design an undo system which tracks every single action we can take with 20 undo steps. map undo to ctrl+z and u, and map redo to U and ctrl+y

- turn the left and right sidebars into resizable panels.

- if we create a new sample in a collection by drag dropping an audio selection into the collection, and then restart the app, the collection item breaks.
I believe we are currently creating the item and place the file in some temp folder, but it should get created at the location of the collection export path and mapped to that


- our CI is tripper over missing dependencies. we will need to skip testing these areas I think as we are in the github actions environment here.

  --- stderr

  thread 'main' (5616) panicked at /home/runner/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/alsa-sys-0.3.1/build.rs:13:18:

  pkg-config exited with status code 1
  > PKG_CONFIG_ALLOW_SYSTEM_LIBS=1 PKG_CONFIG_ALLOW_SYSTEM_CFLAGS=1 pkg-config --libs --cflags alsa

  The system library `alsa` required by crate `alsa-sys` was not found.
  The file `alsa.pc` needs to be installed and the PKG_CONFIG_PATH environment variable must contain its parent directory.
  The PKG_CONFIG_PATH environment variable is not set.

  HINT: if you have installed the library, try setting PKG_CONFIG_PATH to the directory containing `alsa.pc`.

  note: run with `RUST_BACKTRACE=1` environment variable to display a backtrace
warning: build failed, waiting for other jobs to finish...
  --- stderr

  thread 'main' (5616) panicked at /home/runner/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/alsa-sys-0.3.1/build.rs:13:18:

  pkg-config exited with status code 1
  > PKG_CONFIG_ALLOW_SYSTEM_LIBS=1 PKG_CONFIG_ALLOW_SYSTEM_CFLAGS=1 pkg-config --libs --cflags alsa

  The system library `alsa` required by crate `alsa-sys` was not found.
  The file `alsa.pc` needs to be installed and the PKG_CONFIG_PATH environment variable must contain its parent directory.
  The PKG_CONFIG_PATH environment variable is not set.

  HINT: if you have installed the library, try setting PKG_CONFIG_PATH to the directory containing `alsa.pc`.

  note: run with `RUST_BACKTRACE=1` environment variable to display a backtrace
warning: build failed, waiting for other jobs to finish...