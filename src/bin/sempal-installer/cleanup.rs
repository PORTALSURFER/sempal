use std::{env, fs};

use crate::{UNINSTALL_KEY, paths};

#[cfg(target_os = "windows")]
use winreg::RegKey;
#[cfg(target_os = "windows")]
use winreg::enums::{HKEY_CURRENT_USER, KEY_WRITE};

pub(crate) fn run_uninstall() -> Result<(), String> {
    #[cfg(target_os = "windows")]
    {
        let hkcu = RegKey::predef(HKEY_CURRENT_USER);
        let uninstall_key = hkcu
            .open_subkey_with_flags(UNINSTALL_KEY, KEY_WRITE)
            .map_err(|err| format!("Failed to open uninstall registry key: {err}"))?;
        let install_location: String = uninstall_key
            .get_value("InstallLocation")
            .map_err(|err| format!("Failed to read InstallLocation: {err}"))?;
        schedule_delete_after_exit(&install_location)?;
        hkcu.delete_subkey_all(UNINSTALL_KEY)
            .map_err(|err| format!("Failed to remove uninstall registry key: {err}"))?;
        return Ok(());
    }
    #[allow(unreachable_code)]
    Err("Uninstall is only supported on Windows.".to_string())
}

#[cfg(target_os = "windows")]
fn schedule_delete_after_exit(install_location: &str) -> Result<(), String> {
    let temp_dir = env::temp_dir();
    let script_path = temp_dir.join("sempal_uninstall.cmd");
    let start_menu_dir = paths::start_menu_dir()
        .ok_or_else(|| "Failed to resolve Start Menu path".to_string())?
        .display()
        .to_string();
    let script = format!(
        "@echo off\r\n\
        ping 127.0.0.1 -n 3 > nul\r\n\
        rmdir /s /q \"{install_location}\"\r\n\
        rmdir /s /q \"{start_menu_dir}\"\r\n\
        del \"%~f0\"\r\n"
    );
    fs::write(&script_path, script)
        .map_err(|err| format!("Failed to write uninstall script: {err}"))?;
    std::process::Command::new("cmd")
        .args(["/C", "start", "", script_path.to_string_lossy().as_ref()])
        .spawn()
        .map_err(|err| format!("Failed to launch uninstall script: {err}"))?;
    Ok(())
}
