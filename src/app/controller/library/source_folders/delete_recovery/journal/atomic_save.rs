use std::path::Path;

#[cfg(test)]
use std::{collections::HashMap, path::PathBuf};

#[cfg(test)]
use std::sync::{Mutex, OnceLock};

#[cfg(test)]
#[derive(Default)]
struct FailSaveTargets {
    next_token: u64,
    armed: HashMap<PathBuf, u64>,
}

#[cfg(test)]
fn fail_save_targets() -> &'static Mutex<FailSaveTargets> {
    static TARGETS: OnceLock<Mutex<FailSaveTargets>> = OnceLock::new();
    TARGETS.get_or_init(|| Mutex::new(FailSaveTargets::default()))
}

pub(super) fn replace_journal_file(tmp_path: &Path, path: &Path) -> Result<(), String> {
    #[cfg(target_os = "windows")]
    {
        use windows::{
            Win32::Storage::FileSystem::{
                MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH, MoveFileExW,
            },
            core::PCWSTR,
        };

        let from = wide_path(tmp_path);
        let to = wide_path(path);
        let flags = MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH;
        unsafe { MoveFileExW(PCWSTR(from.as_ptr()), PCWSTR(to.as_ptr()), flags) }
            .map_err(|err| format!("Failed to save delete journal: {err}"))?;
        Ok(())
    }
    #[cfg(not(target_os = "windows"))]
    {
        std::fs::rename(tmp_path, path)
            .map_err(|err| format!("Failed to save delete journal: {err}"))
    }
}

#[cfg(target_os = "windows")]
fn wide_path(path: &Path) -> Vec<u16> {
    let mut wide: Vec<u16> =
        <std::ffi::OsStr as std::os::windows::ffi::OsStrExt>::encode_wide(path.as_os_str())
            .collect();
    wide.push(0);
    wide
}

#[cfg(not(test))]
pub(crate) fn fail_save_before_replace(_path: &Path) -> Result<(), String> {
    Ok(())
}

#[cfg(test)]
pub(crate) fn fail_save_before_replace(path: &Path) -> Result<(), String> {
    let mut guard = fail_save_targets()
        .lock()
        .map_err(|_| "Delete journal test hook lock poisoned".to_string())?;
    if guard.armed.remove(path).is_some() {
        return Err("Injected delete journal save failure before replace".into());
    }
    Ok(())
}

#[cfg(test)]
pub(crate) struct FailSaveBeforeReplaceGuard {
    path: PathBuf,
    token: u64,
}

#[cfg(test)]
impl Drop for FailSaveBeforeReplaceGuard {
    fn drop(&mut self) {
        let mut targets = fail_save_targets()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if targets.armed.get(&self.path) == Some(&self.token) {
            targets.armed.remove(&self.path);
        }
    }
}

#[cfg(test)]
pub(crate) fn fail_next_save_before_replace_for_tests(path: PathBuf) -> FailSaveBeforeReplaceGuard {
    let mut guard = fail_save_targets()
        .lock()
        .expect("delete journal test hook lock");
    assert!(
        !guard.armed.contains_key(&path),
        "delete journal failure already armed for {}",
        path.display()
    );
    guard.next_token = guard.next_token.wrapping_add(1);
    let token = guard.next_token;
    guard.armed.insert(path.clone(), token);
    FailSaveBeforeReplaceGuard { path, token }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn failure_scope_cleans_up_on_unwind_and_isolates_paths() {
        let dir = tempfile::tempdir().expect("tempdir");
        let first = dir.path().join("first.json");
        let second = dir.path().join("second.json");

        let unwind = std::panic::catch_unwind({
            let first = first.clone();
            move || {
                let _failure = fail_next_save_before_replace_for_tests(first);
                panic!("exercise failure-scope cleanup");
            }
        });
        assert!(unwind.is_err());
        assert!(fail_save_before_replace(&first).is_ok());

        let _first_failure = fail_next_save_before_replace_for_tests(first.clone());
        let _second_failure = fail_next_save_before_replace_for_tests(second.clone());
        assert!(fail_save_before_replace(&first).is_err());
        assert!(fail_save_before_replace(&second).is_err());

        let consumed = fail_next_save_before_replace_for_tests(first.clone());
        assert!(fail_save_before_replace(&first).is_err());
        let successor = fail_next_save_before_replace_for_tests(first.clone());
        drop(consumed);
        assert!(fail_save_before_replace(&first).is_err());
        drop(successor);
    }
}
