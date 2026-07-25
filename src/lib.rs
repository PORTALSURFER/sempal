#![deny(missing_docs)]
#![deny(warnings)]

//! Library exports for reuse in benchmarks and tests.
extern crate alloc;
extern crate self as wavecrate;
/// Retained controller internals used only by compatibility tests and tools.
#[cfg(any(test, feature = "legacy-controller"))]
#[allow(dead_code)]
mod app;
/// Backend-neutral app-core projection and action helpers used by legacy GUI tooling.
#[cfg(any(test, feature = "legacy-controller"))]
pub mod app_core;
/// Application directory helpers.
pub use wavecrate_library::app_dirs;
#[cfg(test)]
mod app_dirs_tests;
/// Audio playback utilities.
pub mod audio;
/// Shared helpers used by companion binaries such as the updater helper.
pub mod companion_apps;
/// Internal helpers for parsing environment-flag booleans.
mod env_flags;
/// Platform helpers for copying files to the clipboard.
pub mod external_clipboard;
/// Platform helpers for external drag-and-drop.
pub mod external_drag;
/// Legacy-controller GUI test contracts, scenario types, and artifact helpers.
#[cfg(any(test, feature = "legacy-controller"))]
pub mod gui_test;
/// Shared helpers for low-overhead hot-path telemetry instrumentation.
mod hotpath_telemetry;
mod http_client;
/// GitHub issue reporting via the Wavecrate gateway.
pub mod issue_gateway;
/// Logging setup helpers.
pub mod logging;
/// Production Radiant application composition and native GUI behavior.
pub mod native_app;
/// Shared runtime host glue that starts native `radiant` hosts.
///
/// The runtime boundary only adapts launch options and forwards lifecycle/error
/// events; it does not define UI widgets, input handling policies, or layout
/// logic.
pub mod native_runtime;
/// Deterministic production-native sample-source fixtures for runtime validation.
pub mod native_source_fixture;
/// Readiness-owned feature and embedding stage execution.
pub mod readiness_execution;
/// Build-time release metadata.
pub mod release_metadata;
/// Sample source management.
pub mod sample_sources;
/// Selection math utilities.
pub mod selection;
#[cfg(test)]
mod test_isolation_sentinels;
/// Shared display formatting used across Wavecrate UI runtimes.
mod ui_formatting;
/// Optional SQLite extension loader.
pub use wavecrate_library::sqlite_ext;
/// Update check and updater helper utilities.
pub mod updater;
/// WAV header sanitization helpers.
pub mod wav_sanitize;
/// Waveform decoding and rendering helpers.
pub mod waveform;
