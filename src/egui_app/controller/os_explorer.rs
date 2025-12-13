use std::path::Path;

#[cfg(any(test, target_os = "windows"))]
use std::ffi::OsString;

#[cfg(any(test, target_os = "windows"))]
fn windows_explorer_target(path: &Path) -> OsString {
    let rendered = path.to_string_lossy();
    if rendered.contains('/') {
        OsString::from(rendered.replace('/', "\\"))
    } else {
        path.as_os_str().to_owned()
    }
}

#[cfg(any(test, target_os = "windows"))]
fn windows_explorer_select_args(path: &Path) -> [OsString; 2] {
    [OsString::from("/select,"), windows_explorer_target(path)]
}

pub(super) fn reveal_in_file_explorer(path: &Path) -> Result<(), String> {
    if !path.exists() {
        return Err(format!("File not found: {}", path.display()));
    }
    #[cfg(target_os = "windows")]
    {
        let status = std::process::Command::new("explorer.exe")
            .args(windows_explorer_select_args(path))
            .status()
            .map_err(|err| format!("Failed to launch explorer: {err}"))?;
        if status.success() {
            return Ok(());
        }
        return Err(format!(
            "Explorer exited unsuccessfully for {}",
            path.display()
        ));
    }
    #[cfg(target_os = "macos")]
    {
        let status = std::process::Command::new("open")
            .arg("-R")
            .arg(path)
            .status()
            .map_err(|err| format!("Failed to launch Finder: {err}"))?;
        if status.success() {
            return Ok(());
        }
        return Err(format!("Finder exited unsuccessfully for {}", path.display()));
    }
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    {
        let parent = path
            .parent()
            .ok_or_else(|| "Unable to resolve parent directory".to_string())?;
        open::that(parent)
            .map_err(|err| format!("Could not open folder {}: {err}", parent.display()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::OsStr;

    #[test]
    fn windows_explorer_args_use_select_and_path() {
        let path = Path::new("C:\\samples\\kick.wav");
        let args = windows_explorer_select_args(path);
        assert_eq!(args[0], OsStr::new("/select,"));
        assert_eq!(args[1], path.as_os_str());
    }

    #[test]
    fn windows_explorer_args_normalize_forward_slashes() {
        let path = Path::new("C:/samples/kick.wav");
        let args = windows_explorer_select_args(path);
        assert_eq!(args[1], OsStr::new("C:\\samples\\kick.wav"));
    }
}
