//! SQLite extension policy decisions.

use std::{
    fs,
    path::{Path, PathBuf},
};

use tracing::warn;

use super::{SQLITE_EXT_DIR_NAME, SQLITE_EXT_ENABLE_ENV, SQLITE_EXT_ENV, SQLITE_EXT_UNSAFE_ENV};

#[cfg(windows)]
mod windows_security {
    use std::path::Path;
    use windows::Win32::Foundation::{CloseHandle, HANDLE, HLOCAL, LocalFree};
    use windows::Win32::Security::{
        Authorization::{GetNamedSecurityInfoW, SE_FILE_OBJECT},
        DACL_SECURITY_INFORMATION, EqualSid, GetTokenInformation, OWNER_SECURITY_INFORMATION,
        PSECURITY_DESCRIPTOR, PSID, TOKEN_QUERY, TOKEN_USER, TokenUser,
    };
    use windows::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};
    use windows::core::PCWSTR;

    pub fn validate_extension_file_windows(path: &Path) -> Result<(), String> {
        use std::os::windows::ffi::OsStrExt;
        let path_v: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0u16)).collect();
        let pcwstr = PCWSTR(path_v.as_ptr());

        unsafe {
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

            struct SdSafe(PSECURITY_DESCRIPTOR);
            impl Drop for SdSafe {
                fn drop(&mut self) {
                    unsafe {
                        let _ = LocalFree(Some(HLOCAL(self.0.0)));
                    }
                }
            }
            let _guard = SdSafe(psd);

            let is_equal = EqualSid(user_sid, owner_sid).is_ok();
            if !is_equal {
                return Err("SQLite extension must be owned by the current user".to_string());
            }
        }

        Ok(())
    }
}

/// Structured non-error outcome for optional SQLite extension handling.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ExtensionLoadOutcome {
    /// No extension path was configured.
    NotConfigured,
    /// An extension path was configured but explicit loading opt-in was absent.
    Disabled {
        /// Configured extension path that was ignored.
        path: String,
    },
    /// Unsafe loading was requested but the binary was not built with that feature.
    UnsafeUnavailable {
        /// Configured extension path that requested unavailable unsafe mode.
        path: String,
    },
    /// The app-owned allowlist directory could not be prepared.
    AllowlistUnavailable {
        /// Configured extension path that could not be evaluated.
        path: String,
        /// Human-readable reason the allowlist directory was unavailable.
        reason: String,
    },
    /// The configured extension path could not be resolved.
    Unavailable {
        /// Configured extension path that could not be resolved.
        path: String,
        /// Human-readable path resolution failure.
        reason: String,
    },
    /// The resolved extension path failed safety validation.
    Blocked {
        /// Resolved extension path that failed policy validation.
        path: PathBuf,
        /// Human-readable safety validation failure.
        reason: String,
    },
    /// The extension was loaded successfully.
    Loaded {
        /// Resolved extension path loaded by SQLite.
        path: PathBuf,
    },
}

impl ExtensionLoadOutcome {
    /// Emit the same operational warning semantics as the legacy loader.
    pub(crate) fn log_if_needed(&self) {
        match self {
            Self::NotConfigured | Self::Loaded { .. } => {}
            Self::Disabled { path } => warn!(
                path = %path,
                "{SQLITE_EXT_ENV} ignored because {SQLITE_EXT_ENABLE_ENV} is not set; \
                 refusing to load arbitrary SQLite extensions without explicit opt-in."
            ),
            Self::UnsafeUnavailable { path } => warn!(
                path = %path,
                "{SQLITE_EXT_UNSAFE_ENV} ignored because unsafe SQLite extension loading \
                 is disabled at compile time. Rebuild with the `sqlite-ext-unsafe` \
                 cargo feature to allow it."
            ),
            Self::AllowlistUnavailable { path, reason } | Self::Unavailable { path, reason } => {
                warn!(
                    path = %path,
                    "SQLite extension load skipped: {reason}"
                );
            }
            Self::Blocked { path, reason } => warn!(
                path = %path.display(),
                "SQLite extension load blocked: {reason}"
            ),
        }
    }
}

/// Policy decision for optional extension loading.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum ExtensionPolicyDecision {
    /// Skip extension loading with a structured outcome.
    Skip(ExtensionLoadOutcome),
    /// Load one validated extension path.
    Load(ExtensionLoadPlan),
}

/// Validated extension load plan produced by policy.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ExtensionLoadPlan {
    path: PathBuf,
    unsafe_mode: bool,
}

impl ExtensionLoadPlan {
    /// Resolved extension path to load.
    pub(crate) fn path(&self) -> &Path {
        &self.path
    }

    /// Returns true when policy intentionally bypassed safety checks.
    pub(crate) fn unsafe_mode(&self) -> bool {
        self.unsafe_mode
    }
}

/// Resolve the current process environment into one policy decision.
pub(crate) fn extension_policy_from_env() -> ExtensionPolicyDecision {
    let Ok(path) = std::env::var(SQLITE_EXT_ENV) else {
        return ExtensionPolicyDecision::Skip(ExtensionLoadOutcome::NotConfigured);
    };
    if path.trim().is_empty() {
        return ExtensionPolicyDecision::Skip(ExtensionLoadOutcome::NotConfigured);
    }
    if !env_flag_set(SQLITE_EXT_ENABLE_ENV) {
        return ExtensionPolicyDecision::Skip(ExtensionLoadOutcome::Disabled { path });
    }

    let unsafe_requested = env_flag_set(SQLITE_EXT_UNSAFE_ENV);
    let unsafe_mode = unsafe_requested && unsafe_mode_allowed();
    if unsafe_requested && !unsafe_mode {
        return ExtensionPolicyDecision::Skip(ExtensionLoadOutcome::UnsafeUnavailable { path });
    }

    extension_policy_for_path(path, unsafe_mode)
}

fn extension_policy_for_path(path: String, unsafe_mode: bool) -> ExtensionPolicyDecision {
    let allowlist_dir = if unsafe_mode {
        None
    } else {
        match allowlisted_extension_dir() {
            Ok(dir) => Some(dir),
            Err(reason) => {
                return ExtensionPolicyDecision::Skip(ExtensionLoadOutcome::AllowlistUnavailable {
                    path,
                    reason,
                });
            }
        }
    };
    let resolved = match resolve_extension_path(&path, allowlist_dir.as_deref(), unsafe_mode) {
        Ok(resolved) => resolved,
        Err(reason) => {
            return ExtensionPolicyDecision::Skip(ExtensionLoadOutcome::Unavailable {
                path,
                reason,
            });
        }
    };

    if !unsafe_mode && let Err(reason) = validate_extension_file(&resolved) {
        return ExtensionPolicyDecision::Skip(ExtensionLoadOutcome::Blocked {
            path: resolved,
            reason,
        });
    }

    ExtensionPolicyDecision::Load(ExtensionLoadPlan {
        path: resolved,
        unsafe_mode,
    })
}

fn env_flag_set(name: &str) -> bool {
    let Ok(value) = std::env::var(name) else {
        return false;
    };
    matches!(
        value.trim().to_ascii_lowercase().as_str(),
        "1" | "true" | "yes" | "on"
    )
}

fn unsafe_mode_allowed() -> bool {
    cfg!(feature = "sqlite-ext-unsafe")
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
        Ok(())
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
    use crate::test_runtime::TestRuntimeGuard;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn missing_env_var_is_structured_noop() {
        let mut runtime = TestRuntimeGuard::acquire();
        runtime.remove_var(SQLITE_EXT_ENV);
        runtime.remove_var(SQLITE_EXT_ENABLE_ENV);
        runtime.remove_var(SQLITE_EXT_UNSAFE_ENV);

        assert_eq!(
            extension_policy_from_env(),
            ExtensionPolicyDecision::Skip(ExtensionLoadOutcome::NotConfigured)
        );
    }

    #[test]
    fn configured_extension_requires_explicit_enable_flag() {
        let mut runtime = TestRuntimeGuard::acquire();
        runtime.set_var(SQLITE_EXT_ENV, "test_ext");
        runtime.remove_var(SQLITE_EXT_ENABLE_ENV);
        runtime.remove_var(SQLITE_EXT_UNSAFE_ENV);

        assert_eq!(
            extension_policy_from_env(),
            ExtensionPolicyDecision::Skip(ExtensionLoadOutcome::Disabled {
                path: String::from("test_ext"),
            })
        );
    }

    #[test]
    fn rejects_extension_outside_allowlist() {
        let allowlist = tempdir().unwrap();
        let allowlist_dir = allowlist.path().canonicalize().unwrap();
        let outside = tempdir().unwrap();
        let ext_path = outside.path().join("test_ext.so");
        fs::write(&ext_path, b"not a sqlite extension").unwrap();
        let err = resolve_extension_path(ext_path.to_str().unwrap(), Some(&allowlist_dir), false)
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
            resolve_extension_path(ext_path.to_str().unwrap(), Some(&allowlist_dir), true).unwrap();
        assert_eq!(resolved, ext_path.canonicalize().unwrap());
    }

    #[cfg(feature = "sqlite-ext-unsafe")]
    #[test]
    fn unsafe_mode_uses_cwd_for_relative_paths() {
        let mut runtime = TestRuntimeGuard::acquire();
        let temp = tempdir().unwrap();
        let ext_path = temp.path().join("test_ext.so");
        fs::write(&ext_path, b"not a sqlite extension").unwrap();

        runtime.set_current_dir(temp.path()).unwrap();
        let resolved = resolve_extension_path("test_ext.so", None, true).unwrap();

        assert_eq!(resolved, ext_path.canonicalize().unwrap());
    }

    #[cfg(not(feature = "sqlite-ext-unsafe"))]
    #[test]
    fn unsafe_env_is_structured_unavailable_without_feature() {
        let mut runtime = TestRuntimeGuard::acquire();
        runtime.set_var(SQLITE_EXT_ENV, "test_ext");
        runtime.set_var(SQLITE_EXT_ENABLE_ENV, "1");
        runtime.set_var(SQLITE_EXT_UNSAFE_ENV, "1");

        assert_eq!(
            extension_policy_from_env(),
            ExtensionPolicyDecision::Skip(ExtensionLoadOutcome::UnsafeUnavailable {
                path: String::from("test_ext"),
            })
        );
    }
}
