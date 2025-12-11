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

- turn the left and right sidebars into resizable panels.

- when the user dragdrops an audio selection into a collection, a copy of the selection is also added to the currently selected sample source target. please remove this side effect. 
this action should only add a new audio file to the collection

- moving trashed items to the trash folder will freeze the ui, lets turn this into an async background task instead. lets also add a progress bar in the statusbar to indicate the progression of this action.

- in the collection items list, the selected items will block scrolling, lets fix that. the user should be able to freely scroll

- lets add the same select/focus system en styling as in the sample browser list also to the collection browser list.
lets unify these into a flat items list component, using the sample browser as leading in terms of style and core list/navigation functionality.

- in wavefrom context, add 't' to trim audio selection

- in wavefrom context, add hotkeys '/' and '\' to fade audio selection, lets also adjust the fade algo so its a soft S curve, not a hard diagonal. 

- in wavefrom context, add hotkey 'n' to normalize audio selection when a selection is available, otherwise just normalize the whole thing like the sample browser normalize does.

- in wavefrom context, add hotkey 'c' to crop selection, and 'C/shift+c' for crop as non-destructive crop as new sample option, adding a new sample in the same location as the original with _crop001 added, etc.