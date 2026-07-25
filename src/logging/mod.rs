//! Logging setup for the application.
//!
//! Initializes a global tracing subscriber that writes to both stdout and a
//! per-launch log file. Files are timestamped and kept to a bounded count to
//! avoid unbounded growth.

mod contract;
mod files;
mod policy;

pub use contract::{
    ACTION_EVENT_TARGET, ActionDebugEvent, DB_EVENT_TARGET, DbDebugEvent, emit_action_debug_event,
    emit_db_debug_event,
};
pub use policy::{
    DEBUG_LOGGING_ARG, DEBUG_LOGGING_ENV_VAR, DEBUG_LOGGING_SHORT_ARG, DebugLoggingMode,
    DebugLoggingSettings,
};

use std::{
    backtrace::Backtrace,
    panic,
    path::{Path, PathBuf},
    sync::{
        OnceLock,
        atomic::{AtomicBool, Ordering},
    },
};

use time::{UtcOffset, format_description::FormatItem, macros::format_description};
use tracing_appender::{non_blocking::WorkerGuard, rolling};
use tracing_subscriber::{Registry, fmt, prelude::*};

static LOG_GUARD: OnceLock<WorkerGuard> = OnceLock::new();
static DEBUG_LOGGING_ENABLED: AtomicBool = AtomicBool::new(false);

#[cfg(test)]
thread_local! {
    static TEST_DEBUG_LOGGING_OVERRIDE: std::cell::Cell<Option<bool>> =
        const { std::cell::Cell::new(None) };
}

/// Errors that may occur while initializing logging.
#[derive(Debug, thiserror::Error)]
pub enum LoggingError {
    /// No platform-specific data directory could be resolved.
    #[error("No suitable data directory available for logs")]
    NoDataDir,
    /// Failed to create or access the log directory.
    #[error("Failed to prepare log directory {path}: {source}")]
    CreateDir {
        /// Log directory path.
        path: PathBuf,
        /// Underlying IO error.
        source: std::io::Error,
    },
    /// Failed to resolve the configured persistence profile for logs.
    #[error("Invalid log persistence profile '{profile}'")]
    InvalidProfile {
        /// Rejected profile name.
        profile: String,
    },
    /// Failed to enumerate existing log files for pruning.
    #[error("Failed to read log directory {path}: {source}")]
    ReadDir {
        /// Log directory path.
        path: PathBuf,
        /// Underlying IO error.
        source: std::io::Error,
    },
    /// Failed to remove an obsolete log file.
    #[error("Failed to remove old log file {path}: {source}")]
    RemoveFile {
        /// Log file path that failed to delete.
        path: PathBuf,
        /// Underlying IO error.
        source: std::io::Error,
    },
    /// Failed to format a timestamp for the log filename.
    #[error("Failed to format log filename time: {0}")]
    FormatTime(time::error::Format),
    /// Failed to set the global tracing subscriber.
    #[error("Failed to install global tracing subscriber: {0}")]
    SetGlobal(tracing::subscriber::SetGlobalDefaultError),
    /// Failed to create the initial log file for this launch.
    #[error("Failed to create log file at {path}: {source}")]
    CreateLogFile {
        /// Log file path.
        path: PathBuf,
        /// Underlying IO error.
        source: std::io::Error,
    },
}

/// Initialize tracing to write to stdout and a per-launch log file.
///
/// Subsequent calls are no-ops. Failures are returned so callers can degrade
/// gracefully without aborting startup.
pub fn init<I>(args: I) -> Result<(), LoggingError>
where
    I: IntoIterator<Item = std::ffi::OsString>,
{
    if LOG_GUARD.get().is_some() {
        return Ok(());
    }

    let settings = DebugLoggingSettings::from_process(args);
    let launch_log = files::prepare_launch_log_file()?;
    let file_appender = rolling::never(&launch_log.dir, launch_log.file_name.clone());
    let (file_writer, guard) = tracing_appender::non_blocking(file_appender);

    let timer = build_timer();
    let env_filter = settings.env_filter();
    let stdout_layer = fmt::layer()
        .with_timer(timer.clone())
        .with_writer(std::io::stdout);
    let file_layer = fmt::layer()
        .with_ansi(false)
        .with_timer(timer)
        .with_writer(file_writer);

    let subscriber = Registry::default()
        .with(env_filter)
        .with(stdout_layer)
        .with(file_layer);
    tracing::subscriber::set_global_default(subscriber).map_err(LoggingError::SetGlobal)?;
    let _ = LOG_GUARD.set(guard);
    DEBUG_LOGGING_ENABLED.store(settings.mode().enabled(), Ordering::Relaxed);
    wavecrate_library::diagnostics::set_debug_logging_enabled(settings.mode().enabled());

    tracing::info!(
        log_path = %launch_log.path.display(),
        debug_logging_mode = settings.mode().as_str(),
        debug_logging_launch_arg = settings.enabled_by_launch_arg(),
        debug_logging_filter_source = settings.filter_source(),
        debug_logging_filter = settings.filter_description(),
        "Logging initialized"
    );
    if let Some(invalid_value) = settings.invalid_debug_value() {
        tracing::warn!(
            env_var = DEBUG_LOGGING_ENV_VAR,
            invalid_value,
            "Ignoring invalid debug logging flag; expected one of 1/0/true/false/on/off/yes/no"
        );
    }
    Ok(())
}

/// Returns `true` when the Wavecrate-owned debug logging mode is enabled.
///
/// Rich action/database diagnostics should use this gate instead of treating a
/// broad `RUST_LOG` override as product intent.
pub fn debug_logging_enabled() -> bool {
    #[cfg(test)]
    if let Some(enabled) = TEST_DEBUG_LOGGING_OVERRIDE.with(std::cell::Cell::get) {
        return enabled;
    }
    DEBUG_LOGGING_ENABLED.load(Ordering::Relaxed)
}

/// Return the newest `.log` file under one resolved log directory.
pub fn newest_log_file(dir: &Path) -> Result<Option<PathBuf>, LoggingError> {
    files::newest_log_file(dir)
}

#[cfg(test)]
pub(crate) fn with_debug_logging_enabled_for_tests<T>(enabled: bool, run: impl FnOnce() -> T) -> T {
    struct Reset<'a> {
        cell: &'a std::cell::Cell<Option<bool>>,
        previous: Option<bool>,
    }

    impl Drop for Reset<'_> {
        fn drop(&mut self) {
            self.cell.set(self.previous);
        }
    }

    TEST_DEBUG_LOGGING_OVERRIDE.with(|cell| {
        let previous = cell.replace(Some(enabled));
        let _reset = Reset { cell, previous };
        run()
    })
}

/// Install a panic hook that writes panic context and backtrace to logs.
///
/// The hook preserves the previous panic handler so default panic rendering is still
/// emitted for diagnostics outside tracing consumers.
pub fn install_panic_hook() {
    let previous_hook = panic::take_hook();
    panic::set_hook(Box::new(move |panic_info| {
        let current_thread = std::thread::current();
        let thread = current_thread.name().unwrap_or("<unnamed>");
        let payload = panic_info
            .payload()
            .downcast_ref::<&str>()
            .copied()
            .or_else(|| {
                panic_info
                    .payload()
                    .downcast_ref::<String>()
                    .map(String::as_str)
            })
            .unwrap_or("<non-string panic payload>");
        match panic_info.location() {
            Some(location) => tracing::error!(
                "panic in thread={thread} at {file}:{line}:{column}: {payload}",
                file = location.file(),
                line = location.line(),
                column = location.column()
            ),
            None => tracing::error!("panic in thread={thread}: {payload}"),
        }
        tracing::error!("panic backtrace:\n{:?}", Backtrace::force_capture());
        previous_hook(panic_info);
    }));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn debug_logging_test_override_restores_nested_and_unwound_scopes() {
        let baseline = debug_logging_enabled();
        with_debug_logging_enabled_for_tests(!baseline, || {
            assert_eq!(debug_logging_enabled(), !baseline);
            with_debug_logging_enabled_for_tests(baseline, || {
                assert_eq!(debug_logging_enabled(), baseline);
            });
            assert_eq!(debug_logging_enabled(), !baseline);

            let unwind = std::panic::catch_unwind(|| {
                with_debug_logging_enabled_for_tests(baseline, || {
                    assert_eq!(debug_logging_enabled(), baseline);
                    panic!("exercise test override restoration");
                });
            });
            assert!(unwind.is_err());
            assert_eq!(debug_logging_enabled(), !baseline);
        });
        assert_eq!(debug_logging_enabled(), baseline);
    }

    #[test]
    fn debug_logging_test_override_is_thread_scoped() {
        let baseline = debug_logging_enabled();
        with_debug_logging_enabled_for_tests(!baseline, || {
            assert_eq!(debug_logging_enabled(), !baseline);
            assert_eq!(
                std::thread::spawn(debug_logging_enabled).join().unwrap(),
                baseline
            );
        });
    }
}

fn build_timer() -> fmt::time::OffsetTime<time::format_description::BorrowedFormatItem<'static>> {
    const DISPLAY_FORMAT: &[FormatItem<'static>] =
        format_description!("[year]-[month]-[day] [hour]:[minute]:[second]");
    let offset = UtcOffset::current_local_offset().unwrap_or(UtcOffset::UTC);
    fmt::time::OffsetTime::new(offset, DISPLAY_FORMAT.into())
}
