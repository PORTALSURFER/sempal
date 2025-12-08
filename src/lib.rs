//! Library exports for reuse in benchmarks and tests.
/// Audio playback utilities.
pub mod audio;
/// Shared egui UI modules.
pub mod egui_app;
/// Platform helpers for copying files to the clipboard.
pub mod external_clipboard;
/// Platform helpers for external drag-and-drop.
pub mod external_drag;
/// Sample source management.
pub mod sample_sources;
/// Selection math utilities.
pub mod selection;
/// Waveform decoding and rendering helpers.
pub mod waveform;
