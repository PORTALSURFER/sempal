# Codebase review TODOs

## High priority
- Split very large modules (>400 LOC) to improve maintainability and align with project guidelines, especially:
  - `src/egui_app/controller/tests.rs` (3102), consider moving to `tests/` or smaller scoped test modules.
  - `src/egui_app/controller/wavs.rs` (1325), `src/egui_app/controller/source_folders.rs` (1118), `src/egui_app/ui/waveform_view.rs` (744), `src/egui_app/ui/sources_panel.rs` (720), `src/egui_app/controller/playback.rs` (696).
- Reduce monolithic `EguiController` responsibility surface by extracting domain sub-controllers (browser, waveform, drag/drop, hotkeys, collections) behind clear interfaces.
- Add/restore clippy + rustfmt CI checks, and document local workflow in `README.md` or `docs/styleguide.md`.

## Correctness & robustness
- Replace a few production `unwrap`/`expect` sites with explicit error handling or safer patterns:
  - `src/egui_app/controller/wavs.rs:524` uses `unwrap` after a guard; refactor to avoid clippy warning and future footguns.
  - `src/egui_app/controller/sample_browser_actions.rs:160` uses `last().unwrap()`; can use `.last().copied()` with early-return.
- In `src/sample_sources/scanner.rs`, `entries.flatten()` silently drops `read_dir` errors; consider collecting/logging errors so missing permissions or transient IO don’t hide files.
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
