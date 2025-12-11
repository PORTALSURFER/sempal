//! Logging setup for the application.
//!
//! Initializes a global tracing subscriber that writes to both stdout and a
//! per-launch log file. Files are timestamped and kept to a bounded count to
//! avoid unbounded growth.

use std::{
    fs::{self, OpenOptions},
    path::{Path, PathBuf},
    sync::OnceLock,
    time::SystemTime,
};

use time::{OffsetDateTime, UtcOffset, format_description::FormatItem, macros::format_description};
use tracing_appender::{non_blocking::WorkerGuard, rolling};
use tracing_subscriber::{EnvFilter, Registry, fmt, prelude::*};

use crate::app_dirs;

/// Maximum number of log files to retain.
const MAX_LOG_FILES: usize = 10;
const LOG_FILE_PREFIX: &str = "sempal";

static LOG_GUARD: OnceLock<WorkerGuard> = OnceLock::new();

/// Errors that may occur while initializing logging.
#[derive(Debug, thiserror::Error)]
pub enum LoggingError {
    /// No platform-specific data directory could be resolved.
    #[error("No suitable data directory available for logs")]
    NoDataDir,
    /// Failed to create or access the log directory.
    #[error("Failed to prepare log directory {path}: {source}")]
    CreateDir {
        path: PathBuf,
        source: std::io::Error,
    },
    /// Failed to enumerate existing log files for pruning.
    #[error("Failed to read log directory {path}: {source}")]
    ReadDir {
        path: PathBuf,
        source: std::io::Error,
    },
    /// Failed to remove an obsolete log file.
    #[error("Failed to remove old log file {path}: {source}")]
    RemoveFile {
        path: PathBuf,
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
        path: PathBuf,
        source: std::io::Error,
    },
}

/// Initialize tracing to write to stdout and a rotating log file.
///
/// Subsequent calls are no-ops. Failures are returned so callers can degrade
/// gracefully without aborting startup.
pub fn init() -> Result<(), LoggingError> {
    if LOG_GUARD.get().is_some() {
        return Ok(());
    }

    let log_dir = log_directory()?;
    let log_file_name = format_log_file_name(now_local_or_utc())?;
    let log_path = log_dir.join(&log_file_name);
    ensure_file_exists(&log_path)?;

    let file_appender = rolling::never(&log_dir, log_file_name);
    let (file_writer, guard) = tracing_appender::non_blocking(file_appender);
    prune_old_logs(&log_dir, MAX_LOG_FILES)?;

    let timer = build_timer();
    let env_filter = build_env_filter();
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

    tracing::info!("Logging initialized; log file at {}", log_path.display());
    Ok(())
}

fn log_directory() -> Result<PathBuf, LoggingError> {
    app_dirs::logs_dir().map_err(map_app_dir_error)
}

fn ensure_file_exists(path: &Path) -> Result<(), LoggingError> {
    OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .map(|_| ())
        .map_err(|source| LoggingError::CreateLogFile {
            path: path.to_path_buf(),
            source,
        })
}

fn prune_old_logs(dir: &Path, max_files: usize) -> Result<(), LoggingError> {
    let mut entries = fs::read_dir(dir)
        .map_err(|source| LoggingError::ReadDir {
            path: dir.to_path_buf(),
            source,
        })?
        .filter_map(|entry| entry.ok())
        .filter(|entry| entry.file_type().map(|ft| ft.is_file()).unwrap_or(false))
        .filter(|entry| entry.path().extension().and_then(|ext| ext.to_str()) == Some("log"))
        .map(|entry| {
            let modified = entry
                .metadata()
                .and_then(|meta| meta.modified())
                .unwrap_or(SystemTime::UNIX_EPOCH);
            (modified, entry.path())
        })
        .collect::<Vec<_>>();

    entries.sort_by_key(|(modified, _)| *modified);
    while entries.len() > max_files {
        if let Some((_, path)) = entries.first() {
            fs::remove_file(path).map_err(|source| LoggingError::RemoveFile {
                path: path.to_path_buf(),
                source,
            })?;
        }
        entries.remove(0);
    }
    Ok(())
}

fn format_log_file_name(now: OffsetDateTime) -> Result<String, LoggingError> {
    const NAME_FORMAT: &[FormatItem<'_>] =
        format_description!("[year]-[month]-[day]_[hour]-[minute]-[second]");
    let name = now.format(NAME_FORMAT).map_err(LoggingError::FormatTime)?;
    Ok(format!("{LOG_FILE_PREFIX}_{name}.log"))
}

fn build_timer() -> fmt::time::OffsetTime<time::format_description::BorrowedFormatItem<'static>> {
    const DISPLAY_FORMAT: &[FormatItem<'static>] =
        format_description!("[year]-[month]-[day] [hour]:[minute]:[second]");
    let offset = UtcOffset::current_local_offset().unwrap_or(UtcOffset::UTC);
    fmt::time::OffsetTime::new(offset, DISPLAY_FORMAT.into())
}

fn now_local_or_utc() -> OffsetDateTime {
    OffsetDateTime::now_local().unwrap_or_else(|_| OffsetDateTime::now_utc())
}

fn build_env_filter() -> EnvFilter {
    EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"))
}

fn map_app_dir_error(error: app_dirs::AppDirError) -> LoggingError {
    match error {
        app_dirs::AppDirError::NoBaseDir => LoggingError::NoDataDir,
        app_dirs::AppDirError::CreateDir { path, source } => {
            LoggingError::CreateDir { path, source }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{thread, time::Duration};
    use tempfile::tempdir;

    #[test]
    fn log_filename_has_timestamp_and_prefix() {
        let fixed = OffsetDateTime::from_unix_timestamp(1_700_000_000).unwrap();
        let name = format_log_file_name(fixed).unwrap();
        assert_eq!(name, "sempal_2023-11-14_22-13-20.log");
    }

    #[test]
    fn prune_removes_oldest_files_beyond_limit() {
        let dir = tempdir().unwrap();
        for idx in 0..12 {
            let path = dir.path().join(format!("sempal_{idx}.log"));
            ensure_file_exists(&path).unwrap();
            thread::sleep(Duration::from_millis(10));
        }

        prune_old_logs(dir.path(), 10).unwrap();
        let remaining = fs::read_dir(dir.path())
            .unwrap()
            .filter(|entry| {
                entry.as_ref().ok().map(|e| e.path()).is_some_and(|path| {
                    path.extension()
                        .and_then(|ext| ext.to_str())
                        .map(|ext| ext == "log")
                        .unwrap_or(false)
                })
            })
            .count();
        assert_eq!(remaining, 10);
    }
}
