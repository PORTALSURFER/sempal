//! Optional SQLite extension loader for accelerated vector operations.
//!
//! By default, Sempal runs entirely on built-in SQLite capabilities.
//! If `SEMPAL_SQLITE_EXT` points at a loadable extension, Sempal will attempt
//! to load it and continue with a safe fallback if loading fails.

use rusqlite::Connection;

/// Environment variable pointing at a loadable SQLite extension (.so/.dll/.dylib).
pub const SQLITE_EXT_ENV: &str = "SEMPAL_SQLITE_EXT";

/// Attempt to load the optional SQLite extension specified by `SEMPAL_SQLITE_EXT`.
///
/// This is a best-effort operation:
/// - If the env var is unset, this is a no-op.
/// - If loading fails, the error is returned to the caller so it can be logged/ignored.
pub fn try_load_optional_extension(conn: &Connection) -> Result<(), rusqlite::Error> {
    let Ok(path) = std::env::var(SQLITE_EXT_ENV) else {
        return Ok(());
    };
    if path.trim().is_empty() {
        return Ok(());
    }
    unsafe {
        conn.load_extension_enable()?;
    }
    let load_result = unsafe { conn.load_extension(path, Option::<&str>::None) };
    let _ = conn.load_extension_disable();
    load_result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_env_var_is_noop() {
        unsafe {
            std::env::remove_var(SQLITE_EXT_ENV);
        }
        let conn = Connection::open_in_memory().unwrap();
        try_load_optional_extension(&conn).unwrap();
    }
}
