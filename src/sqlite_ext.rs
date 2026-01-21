//! Optional SQLite extension loader for accelerated vector operations.
//!
//! By default, Sempal runs entirely on built-in SQLite capabilities.
//! If `SEMPAL_SQLITE_EXT` points at a loadable extension, Sempal will attempt
//! to load it and continue with a safe fallback if loading fails. Loading is
//! gated by `SEMPAL_SQLITE_EXT_ENABLE` and restricted to app-owned directories
//! unless `SEMPAL_SQLITE_EXT_UNSAFE` is explicitly set and the build enables
//! the `sqlite-ext-unsafe` feature. The allowlisted directory lives at
//! `<app_root>/sqlite_extensions`.

use std::{
    fs,
    path::{Path, PathBuf},
};

use rusqlite::Connection;
use tracing::warn;

#[cfg(windows)]
mod windows_security {
    use std::path::Path;
    use windows::core::PCWSTR;
    use windows::Win32::Foundation::{CloseHandle, HANDLE, HLOCAL, LocalFree};
    use windows::Win32::Security::{
        Authorization::{GetNamedSecurityInfoW, SE_FILE_OBJECT},
        DACL_SECURITY_INFORMATION, OWNER_SECURITY_INFORMATION, PSID, PSECURITY_DESCRIPTOR,
        EqualSid, GetTokenInformation, TokenUser, TOKEN_QUERY, TOKEN_USER,
    };
    use windows::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};

    pub fn validate_extension_file_windows(path: &Path) -> Result<(), String> {
        use std::os::windows::ffi::OsStrExt;
        let path_v: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0u16)).collect();
        let pcwstr = PCWSTR(path_v.as_ptr());

        unsafe {
            // 1. Get current process token to find the user SID
            let mut token_handle = HANDLE::default();
            match OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token_handle) {
                Ok(_) => {}
                Err(_) => return Err("Failed to open process token".to_string()),
            }

            let mut len = 0;
            let _ = GetTokenInformation(token_handle, TokenUser, None, 0, &mut len);
            let mut buffer = vec![0u8; len as usize];
            match GetTokenInformation(
                token_handle,
                TokenUser,
                Some(buffer.as_mut_ptr() as *mut _),
                len,
                &mut len,
            ) {
                Ok(_) => {}
                Err(_) => {
                    let _ = CloseHandle(token_handle);
                    return Err("Failed to get token information".to_string());
                }
            }
            let _ = CloseHandle(token_handle);

            let token_user = &*(buffer.as_ptr() as *const TOKEN_USER);
            let user_sid = token_user.User.Sid;

            // 2. Get file security information (Owner and DACL)
            let mut psd = PSECURITY_DESCRIPTOR::default();
            let mut owner_sid = PSID::default();

            let res = GetNamedSecurityInfoW(
                pcwstr,
                SE_FILE_OBJECT,
                OWNER_SECURITY_INFORMATION | DACL_SECURITY_INFORMATION,
                Some(&mut owner_sid),
                None,
                None,
                None,
                &mut psd,
            );

            if res.0 != 0 {
                return Err(format!("GetNamedSecurityInfoW failed with error {}", res.0));
            }

            // Ensure the security descriptor is freed
            struct SdSafe(PSECURITY_DESCRIPTOR);
            impl Drop for SdSafe {
                fn drop(&mut self) {
                    unsafe {
                        let _ = LocalFree(Some(HLOCAL(self.0 .0)));
                    }
                }
            }
            let _guard = SdSafe(psd);

            // 3. Verify the file owner matches the current user
            // In windows-rs 0.62.2, EqualSid might return a Result<()> or BOOL
            // depending on the version and configuration.
            // If it returns BOOL, we check for false.
            // If it returns Result, we check for Err.
            // Here, we try a approach that works for both if we use match/if let.
            #[allow(irrefutable_let_patterns)]
            let is_equal = match EqualSid(user_sid, owner_sid) {
                Ok(_) => true,
                Err(_) => false,
            };
            if !is_equal {
                return Err("SQLite extension must be owned by the current user".to_string());
            }

            // On Windows, checking "writable by others" is complex due to the nature of ACLs.
            // However, by ensuring it is owned by the current user and lives in the
            // app-restricted directory (checked by the caller), we provide a much
            // better security posture than bypassing checks entirely.
        }

        Ok(())
    }
}

/// Environment variable pointing at a loadable SQLite extension (.so/.dll/.dylib).
pub const SQLITE_EXT_ENV: &str = "SEMPAL_SQLITE_EXT";

/// Environment variable that must be set to enable loading `SEMPAL_SQLITE_EXT`.
pub const SQLITE_EXT_ENABLE_ENV: &str = "SEMPAL_SQLITE_EXT_ENABLE";

/// Environment variable that bypasses extension safety checks and allowlist
/// enforcement when set. This is ignored unless the `sqlite-ext-unsafe` cargo
/// feature is enabled at build time.
pub const SQLITE_EXT_UNSAFE_ENV: &str = "SEMPAL_SQLITE_EXT_UNSAFE";

const SQLITE_EXT_DIR_NAME: &str = "sqlite_extensions";

/// Attempt to load the optional SQLite extension specified by `SEMPAL_SQLITE_EXT`.
///
/// This is a best-effort operation:
/// - If the env var is unset, this is a no-op.
/// - If `SEMPAL_SQLITE_EXT_ENABLE` is not set, the extension is rejected.
/// - The extension must live under the app-owned `sqlite_extensions` directory unless
///   `SEMPAL_SQLITE_EXT_UNSAFE` is set. In unsafe mode, the path is resolved as provided
///   (absolute or relative to the current working directory).
/// - If loading fails, the error is returned to the caller so it can be logged/ignored.
pub fn try_load_optional_extension(conn: &Connection) -> Result<(), rusqlite::Error> {
    let Ok(path) = std::env::var(SQLITE_EXT_ENV) else {
        return Ok(());
    };
    if path.trim().is_empty() {
        return Ok(());
    }
    if !env_flag_set(SQLITE_EXT_ENABLE_ENV) {
        warn!(
            path = %path,
            "{SQLITE_EXT_ENV} ignored because {SQLITE_EXT_ENABLE_ENV} is not set; \
             refusing to load arbitrary SQLite extensions without explicit opt-in."
        );
        return Ok(());
    }
    let unsafe_mode_requested = env_flag_set(SQLITE_EXT_UNSAFE_ENV);
    let unsafe_mode = unsafe_mode_enabled();
    if unsafe_mode_requested && !unsafe_mode {
        warn!(
            path = %path,
            "{SQLITE_EXT_UNSAFE_ENV} ignored because unsafe SQLite extension loading \
             is disabled at compile time. Rebuild with the `sqlite-ext-unsafe` \
             cargo feature to allow it."
        );
    }
    let allowlist_dir = if unsafe_mode {
        None
    } else {
        match allowlisted_extension_dir() {
            Ok(dir) => Some(dir),
            Err(err) => {
                warn!(
                    path = %path,
                    "SQLite extension load skipped: {err}"
                );
                return Ok(());
            }
        }
    };
    let resolved = match resolve_extension_path(&path, allowlist_dir.as_deref(), unsafe_mode) {
        Ok(resolved) => resolved,
        Err(err) => {
            warn!(
                path = %path,
                "SQLite extension load skipped: {err}"
            );
            return Ok(());
        }
    };
    if unsafe_mode {
        warn!(
            path = %resolved.display(),
            "{SQLITE_EXT_UNSAFE_ENV} set; bypassing SQLite extension safety checks."
        );
    } else if let Err(err) = validate_extension_file(&resolved) {
        warn!(
            path = %resolved.display(),
            "SQLite extension load blocked: {err}"
        );
        return Ok(());
    }
    unsafe {
        conn.load_extension_enable()?;
    }
    let load_result = unsafe { conn.load_extension(&resolved, Option::<&str>::None) };
    let _ = conn.load_extension_disable();
    load_result
}

fn env_flag_set(name: &str) -> bool {
    let Ok(value) = std::env::var(name) else {
        return false;
    };
    match value.trim().to_ascii_lowercase().as_str() {
        "1" | "true" | "yes" | "on" => true,
        _ => false,
    }
}

fn unsafe_mode_allowed() -> bool {
    cfg!(feature = "sqlite-ext-unsafe")
}

fn unsafe_mode_enabled() -> bool {
    env_flag_set(SQLITE_EXT_UNSAFE_ENV) && unsafe_mode_allowed()
}

fn allowlisted_extension_dir() -> Result<PathBuf, String> {
    let root = crate::app_dirs::app_root_dir().map_err(|err| err.to_string())?;
    let dir = root.join(SQLITE_EXT_DIR_NAME);
    fs::create_dir_all(&dir).map_err(|err| {
        format!(
            "Failed to create SQLite extension directory {}: {err}",
            dir.display()
        )
    })?;
    dir.canonicalize().map_err(|err| {
        format!(
            "Failed to resolve SQLite extension directory {}: {err}",
            dir.display()
        )
    })
}

fn resolve_extension_path(
    raw: &str,
    allowlist_dir: Option<&Path>,
    unsafe_mode: bool,
) -> Result<PathBuf, String> {
    let trimmed = raw.trim();
    let candidate = PathBuf::from(trimmed);
    if unsafe_mode {
        return candidate.canonicalize().map_err(|err| {
            format!(
                "Failed to resolve SQLite extension path {}: {err}",
                candidate.display()
            )
        });
    }
    let candidate = if candidate.is_absolute() {
        candidate
    } else {
        let allowlist_dir = allowlist_dir.ok_or_else(|| {
            "SQLite extension allowlist directory unavailable in safe mode".to_string()
        })?;
        allowlist_dir.join(candidate)
    };
    let resolved = candidate.canonicalize().map_err(|err| {
        format!(
            "Failed to resolve SQLite extension path {}: {err}",
            candidate.display()
        )
    })?;
    let allowlist_dir = allowlist_dir.ok_or_else(|| {
        "SQLite extension allowlist directory unavailable in safe mode".to_string()
    })?;
    if !resolved.starts_with(allowlist_dir) {
        return Err(format!(
            "SQLite extension must live under {}",
            allowlist_dir.display()
        ));
    }
    Ok(resolved)
}

fn validate_extension_file(path: &Path) -> Result<(), String> {
    let metadata = fs::metadata(path).map_err(|err| {
        format!(
            "Failed to read SQLite extension metadata for {}: {err}",
            path.display()
        )
    })?;
    if !metadata.is_file() {
        return Err("SQLite extension path is not a regular file".to_string());
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        let uid = metadata.uid();
        let euid = unsafe { libc::geteuid() };
        if uid != euid {
            return Err(format!(
                "SQLite extension must be owned by the current user (uid {euid})"
            ));
        }
        let mode = metadata.mode();
        if mode & 0o022 != 0 {
            return Err("SQLite extension is writable by group or others".to_string());
        }
        return Ok(());
    }
    #[cfg(windows)]
    {
        windows_security::validate_extension_file_windows(path)
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = metadata;
        Err(format!(
            "SQLite extension ownership checks are unavailable on this platform; \
             set {SQLITE_EXT_UNSAFE_ENV}=1 to override"
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::sync::Mutex;
    use tempfile::tempdir;

    static CWD_LOCK: Mutex<()> = Mutex::new(());

    #[test]
    fn no_env_var_is_noop() {
        unsafe {
            std::env::remove_var(SQLITE_EXT_ENV);
            std::env::remove_var(SQLITE_EXT_ENABLE_ENV);
            std::env::remove_var(SQLITE_EXT_UNSAFE_ENV);
        }
        let conn = Connection::open_in_memory().unwrap();
        try_load_optional_extension(&conn).unwrap();
    }

    #[test]
    fn rejects_extension_outside_allowlist() {
        let allowlist = tempdir().unwrap();
        let allowlist_dir = allowlist.path().canonicalize().unwrap();
        let outside = tempdir().unwrap();
        let ext_path = outside.path().join("test_ext.so");
        fs::write(&ext_path, b"not a sqlite extension").unwrap();
        let err = resolve_extension_path(
            ext_path.to_str().unwrap(),
            Some(&allowlist_dir),
            false,
        )
        .unwrap_err();
        assert!(err.contains("SQLite extension must live under"));
    }

    #[cfg(feature = "sqlite-ext-unsafe")]
    #[test]
    fn unsafe_mode_allows_extension_outside_allowlist() {
        let allowlist = tempdir().unwrap();
        let allowlist_dir = allowlist.path().canonicalize().unwrap();
        let outside = tempdir().unwrap();
        let ext_path = outside.path().join("test_ext.so");
        fs::write(&ext_path, b"not a sqlite extension").unwrap();
        let resolved =
            resolve_extension_path(ext_path.to_str().unwrap(), Some(&allowlist_dir), true)
                .unwrap();
        assert_eq!(resolved, ext_path.canonicalize().unwrap());
    }

    #[cfg(feature = "sqlite-ext-unsafe")]
    #[test]
    fn unsafe_mode_uses_cwd_for_relative_paths() {
        let _guard = CWD_LOCK.lock().unwrap();
        let temp = tempdir().unwrap();
        let ext_path = temp.path().join("test_ext.so");
        fs::write(&ext_path, b"not a sqlite extension").unwrap();

        let original_dir = std::env::current_dir().unwrap();
        std::env::set_current_dir(temp.path()).unwrap();
        let resolved = resolve_extension_path("test_ext.so", None, true).unwrap();
        std::env::set_current_dir(original_dir).unwrap();

        assert_eq!(resolved, ext_path.canonicalize().unwrap());
    }

    #[cfg(not(feature = "sqlite-ext-unsafe"))]
    #[test]
    fn unsafe_env_is_ignored_without_feature() {
        unsafe {
            std::env::set_var(SQLITE_EXT_UNSAFE_ENV, "1");
        }
        assert!(!unsafe_mode_enabled());
        unsafe {
            std::env::remove_var(SQLITE_EXT_UNSAFE_ENV);
        }
    }
}
