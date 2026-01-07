use std::{env, path::Path};

use crate::{APP_NAME, APP_PUBLISHER, UNINSTALL_KEY};

#[cfg(target_os = "windows")]
use winreg::RegKey;
#[cfg(target_os = "windows")]
use winreg::enums::HKEY_CURRENT_USER;

pub(crate) fn register_uninstall_entry(install_dir: &Path) -> Result<(), String> {
    #[cfg(target_os = "windows")]
    {
        let hkcu = RegKey::predef(HKEY_CURRENT_USER);
        let (key, _) = hkcu
            .create_subkey(UNINSTALL_KEY)
            .map_err(|err| format!("Failed to create uninstall registry key: {err}"))?;
        let exe_path = install_dir.join("sempal-installer.exe");
        let uninstall = format!("\"{}\" --uninstall", exe_path.display());
        key.set_value("DisplayName", &APP_NAME)
            .map_err(|err| format!("Failed to set DisplayName: {err}"))?;
        key.set_value("DisplayVersion", &env!("CARGO_PKG_VERSION"))
            .map_err(|err| format!("Failed to set DisplayVersion: {err}"))?;
        key.set_value("Publisher", &APP_PUBLISHER)
            .map_err(|err| format!("Failed to set Publisher: {err}"))?;
        key.set_value("InstallLocation", &install_dir.display().to_string())
            .map_err(|err| format!("Failed to set InstallLocation: {err}"))?;
        key.set_value("UninstallString", &uninstall)
            .map_err(|err| format!("Failed to set UninstallString: {err}"))?;
        key.set_value(
            "DisplayIcon",
            &install_dir.join("sempal.ico").display().to_string(),
        )
        .map_err(|err| format!("Failed to set DisplayIcon: {err}"))?;
        return Ok(());
    }
    #[allow(unreachable_code)]
    Err("Uninstall registry entry is only supported on Windows.".to_string())
}
