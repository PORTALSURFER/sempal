use std::path::{Path, PathBuf};

use directories::BaseDirs;

use super::profile::{ProfileSelection, ResolvedPersistence, persistence_mode_from_selection};
use super::state::{
    APP_ROOT_OVERRIDE, CONFIG_BASE_OVERRIDE, IGNORE_GLOBAL_APP_ROOT_OVERRIDE,
    SCOPED_APP_ROOT_OVERRIDE, SCOPED_PROFILE_OVERRIDE, TEST_CONFIG_BASE, TEST_CONFIG_OVERRIDE,
};
use super::{
    APP_DIR_NAME, AUTOMATED_PROFILE_NAME, AppDirError, CONFIG_PROFILE_ENV, PROFILE_DIR_NAME,
    SANDBOX_PROFILE_NAME, TEST_EXECUTABLE_DIR_NAME,
};

/// Ensure tests do not touch real user config directories.
pub fn ensure_test_config_base() {
    if std::env::var_os("WAVECRATE_CONFIG_HOME").is_some() {
        return;
    }
    let test_base = std::sync::LazyLock::force(&TEST_CONFIG_BASE).clone();
    let mut guard = CONFIG_BASE_OVERRIDE
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    if guard.is_none() {
        *guard = Some(test_base);
    }
}

/// Return the root `.wavecrate` directory, creating it if needed.
pub fn app_root_dir() -> Result<PathBuf, AppDirError> {
    if should_auto_isolate_test_config_base() {
        ensure_test_config_base();
    }

    if let Some(path) = scoped_or_global_app_root_override()? {
        return Ok(path);
    }
    Ok(resolve_persistence()?.app_root)
}

/// Resolve the current config base, app root, and high-level persistence mode.
pub fn resolve_persistence() -> Result<ResolvedPersistence, AppDirError> {
    if should_auto_isolate_test_config_base() {
        ensure_test_config_base();
    }
    if let Some(path) = scoped_or_global_app_root_override()? {
        return Ok(ResolvedPersistence {
            config_base: path.parent().map_or_else(|| path.clone(), PathBuf::from),
            app_root: path,
            mode: persistence_mode_from_selection(&effective_profile()),
        });
    }

    let base = config_base_dir().ok_or(AppDirError::NoBaseDir)?;
    let selection = effective_profile();
    let app_root = resolve_profile_app_root(&base, &selection)?;
    create_dir(&app_root)?;
    Ok(ResolvedPersistence {
        config_base: base,
        app_root,
        mode: persistence_mode_from_selection(&selection),
    })
}

/// Return the high-level persistence mode that applies to the current run.
pub fn persistence_mode() -> super::PersistenceMode {
    persistence_mode_from_selection(&effective_profile())
}

/// Override the resolved application root directory (the `.wavecrate` folder).
pub fn set_app_root_override(path: PathBuf) -> Result<(), AppDirError> {
    create_dir(&path)?;
    if TEST_CONFIG_OVERRIDE.with(|override_path| override_path.borrow().is_some())
        || SCOPED_PROFILE_OVERRIDE.with(|override_profile| override_profile.borrow().is_some())
    {
        SCOPED_APP_ROOT_OVERRIDE.with(|override_path| {
            *override_path.borrow_mut() = Some(path);
        });
        return Ok(());
    }
    let mut guard = APP_ROOT_OVERRIDE
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    *guard = Some(path);
    Ok(())
}

/// Return the base directory used for `.wavecrate` when no override is set.
pub fn config_base_dir_path() -> Option<PathBuf> {
    config_base_dir()
}

pub(super) fn config_base_dir() -> Option<PathBuf> {
    if let Some(path) = TEST_CONFIG_OVERRIDE.with(|override_path| override_path.borrow().clone()) {
        return Some(path);
    }
    if let Some(path) = CONFIG_BASE_OVERRIDE
        .lock()
        .ok()
        .and_then(|guard| guard.clone())
    {
        return Some(path);
    }
    if let Ok(path) = std::env::var("WAVECRATE_CONFIG_HOME") {
        return Some(PathBuf::from(path));
    }
    BaseDirs::new().map(|dirs| dirs.config_dir().to_path_buf())
}

fn scoped_or_global_app_root_override() -> Result<Option<PathBuf>, AppDirError> {
    if let Some(path) =
        SCOPED_APP_ROOT_OVERRIDE.with(|override_path| override_path.borrow().clone())
    {
        create_dir(&path)?;
        return Ok(Some(path));
    }
    if IGNORE_GLOBAL_APP_ROOT_OVERRIDE.with(std::cell::Cell::get) {
        return Ok(None);
    }
    if let Some(path) = APP_ROOT_OVERRIDE
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .clone()
    {
        create_dir(&path)?;
        return Ok(Some(path));
    }
    Ok(None)
}

fn resolve_profile_app_root(
    base: &Path,
    selection: &ProfileSelection,
) -> Result<PathBuf, AppDirError> {
    match selection {
        ProfileSelection::Live => Ok(base.join(APP_DIR_NAME)),
        ProfileSelection::Named(profile) => {
            let sanitized = sanitize_profile_name(profile)?;
            Ok(base
                .join(APP_DIR_NAME)
                .join(PROFILE_DIR_NAME)
                .join(sanitized))
        }
    }
}

fn effective_profile() -> ProfileSelection {
    current_profile_override()
        .or_else(profile_from_env)
        .unwrap_or_else(default_profile)
}

fn should_auto_isolate_test_config_base() -> bool {
    std::env::var_os("WAVECRATE_CONFIG_HOME").is_none()
        && effective_profile() != ProfileSelection::Live
        && running_under_test_harness()
}

fn current_profile_override() -> Option<ProfileSelection> {
    SCOPED_PROFILE_OVERRIDE.with(|override_profile| override_profile.borrow().clone())
}

fn profile_from_env() -> Option<ProfileSelection> {
    let raw = std::env::var(CONFIG_PROFILE_ENV).ok()?;
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }
    if trimmed.eq_ignore_ascii_case("live") {
        return Some(ProfileSelection::Live);
    }
    if trimmed.eq_ignore_ascii_case("automated") {
        return Some(ProfileSelection::Named(String::from(
            AUTOMATED_PROFILE_NAME,
        )));
    }
    if trimmed.eq_ignore_ascii_case(SANDBOX_PROFILE_NAME) {
        return Some(ProfileSelection::Named(String::from(SANDBOX_PROFILE_NAME)));
    }
    Some(ProfileSelection::Named(trimmed.to_string()))
}

fn default_profile() -> ProfileSelection {
    if running_under_test_harness() {
        return ProfileSelection::Named(String::from(AUTOMATED_PROFILE_NAME));
    }
    ProfileSelection::Live
}

fn running_under_test_harness() -> bool {
    cfg!(test)
        || std::env::current_exe().ok().is_some_and(|path| {
            path.parent()
                .and_then(Path::file_name)
                .is_some_and(|name| name == TEST_EXECUTABLE_DIR_NAME)
        })
}

fn sanitize_profile_name(profile: &str) -> Result<String, AppDirError> {
    let trimmed = profile.trim();
    if trimmed.is_empty() {
        return Err(AppDirError::InvalidProfileName {
            profile: profile.to_string(),
        });
    }
    let mut sanitized = String::with_capacity(trimmed.len());
    for ch in trimmed.chars() {
        if ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.') {
            sanitized.push(ch);
        } else {
            return Err(AppDirError::InvalidProfileName {
                profile: profile.to_string(),
            });
        }
    }
    Ok(sanitized)
}

fn create_dir(path: &Path) -> Result<(), AppDirError> {
    std::fs::create_dir_all(path).map_err(|source| AppDirError::CreateDir {
        path: path.to_path_buf(),
        source,
    })
}
