use std::fs;
use std::path::Path;

use crate::paths;

#[cfg(target_os = "windows")]
use windows::{
    Win32::{
        System::Com::{
            CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED, CoCreateInstance, CoInitializeEx,
            CoUninitialize, IPersistFile,
        },
        UI::Shell::{IShellLinkW, ShellLink},
    },
    core::{HSTRING, Interface, PCWSTR},
};

pub(crate) fn create_start_menu_shortcut(install_dir: &Path) -> Result<(), String> {
    #[cfg(target_os = "windows")]
    {
        let start_menu = paths::start_menu_dir().ok_or_else(|| "APPDATA not set".to_string())?;
        fs::create_dir_all(&start_menu)
            .map_err(|err| format!("Failed to create Start Menu folder: {err}"))?;

        let shortcut_path = start_menu.join("SemPal.lnk");
        let target_path = install_dir.join("sempal.exe");
        let icon_path = install_dir.join("sempal.ico");

        struct ComGuard;
        impl Drop for ComGuard {
            fn drop(&mut self) {
                unsafe { CoUninitialize() };
            }
        }

        unsafe {
            CoInitializeEx(None, COINIT_APARTMENTTHREADED)
                .ok()
                .map_err(|err| format!("Failed to init COM: {err}"))?;
            let _guard = ComGuard;
            let link: IShellLinkW = CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER)
                .map_err(|err| format!("Failed to create IShellLink: {err}"))?;

            let target = HSTRING::from(target_path.display().to_string());
            link.SetPath(PCWSTR::from_raw(target.as_ptr()))
                .map_err(|err| format!("Failed to set shortcut target: {err}"))?;

            let icon = HSTRING::from(icon_path.display().to_string());
            let _ = link.SetIconLocation(PCWSTR::from_raw(icon.as_ptr()), 0);

            let persist: IPersistFile = link
                .cast()
                .map_err(|err| format!("Failed to cast IPersistFile: {err}"))?;
            let shortcut = HSTRING::from(shortcut_path.display().to_string());
            persist
                .Save(PCWSTR::from_raw(shortcut.as_ptr()), true)
                .map_err(|err| format!("Failed to save shortcut: {err}"))?;
        }
        return Ok(());
    }
    #[allow(unreachable_code)]
    Err("Start Menu shortcut is only supported on Windows.".to_string())
}
