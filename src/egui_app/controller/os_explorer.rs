use std::path::Path;

pub(super) fn reveal_in_file_explorer(path: &Path) -> Result<(), String> {
    if !path.exists() {
        return Err(format!("File not found: {}", path.display()));
    }
    #[cfg(target_os = "windows")]
    {
        let quoted = format!("/select,\"{}\"", path.display());
        let status = std::process::Command::new("explorer.exe")
            .arg(quoted)
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
