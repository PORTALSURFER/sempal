//! Library exports for reuse in benchmarks and tests.
/// Application directory helpers.
pub mod app_dirs;
/// Audio playback utilities.
pub mod audio;
/// Background analysis helpers.
pub mod analysis;
/// Shared egui UI modules.
pub mod egui_app;
/// Platform helpers for copying files to the clipboard.
pub mod external_clipboard;
/// Platform helpers for external drag-and-drop.
pub mod external_drag;
/// GitHub issue reporting via the Sempal gateway.
pub mod issue_gateway;
/// Logging setup helpers.
pub mod logging;
/// Training dataset export helpers.
pub mod dataset;
/// Machine learning helpers.
pub mod ml;
/// Sample source management.
pub mod sample_sources;
/// Optional SQLite extension loader.
pub mod sqlite_ext;
/// Selection math utilities.
pub mod selection;
/// Update check + installer helper utilities.
pub mod updater;
/// WAV header sanitization helpers.
pub mod wav_sanitize;
/// Waveform decoding and rendering helpers.
pub mod waveform;
