//! Platform helpers for copying file paths to the system clipboard as file drops.
//!
//! On Windows this publishes `CF_HDROP` so paste targets (e.g., Explorer) receive
//! a list of real files. Other platforms return an unsupported error.

use std::path::PathBuf;

/// Copy the given file paths to the system clipboard for pasting elsewhere.
pub fn copy_file_paths(paths: &[PathBuf]) -> Result<(), String> {
    if paths.is_empty() {
        return Err("No files to copy".into());
    }
    platform::copy_file_paths(paths)
}

#[cfg(not(target_os = "windows"))]
mod platform {
    use super::*;

    pub fn copy_file_paths(_paths: &[PathBuf]) -> Result<(), String> {
        Err("Clipboard file copy is only implemented on Windows in this build".into())
    }
}

#[cfg(target_os = "windows")]
mod platform {
    use super::*;
    use std::os::windows::ffi::OsStrExt;
    use std::ptr::copy_nonoverlapping;
    use windows::Win32::Foundation::HGLOBAL;
    use windows::Win32::System::DataExchange::{
        CloseClipboard, EmptyClipboard, OpenClipboard, SetClipboardData,
    };
    use windows::Win32::System::Memory::{
        GMEM_MOVEABLE, GMEM_ZEROINIT, GlobalAlloc, GlobalFree, GlobalLock, GlobalUnlock,
    };
    use windows::Win32::System::Ole::CF_HDROP;
    use windows::Win32::UI::Shell::DROPFILES;

    struct Clipboard;

    impl Clipboard {
        fn new() -> Result<Self, String> {
            unsafe { OpenClipboard(None) }
                .ok()
                .map_err(|err| format!("OpenClipboard failed: {err}"))?;
            unsafe { EmptyClipboard() }
                .ok()
                .map_err(|err| format!("EmptyClipboard failed: {err}"))?;
            Ok(Self)
        }
    }

    impl Drop for Clipboard {
        fn drop(&mut self) {
            unsafe {
                let _ = CloseClipboard();
            }
        }
    }

    pub fn copy_file_paths(paths: &[PathBuf]) -> Result<(), String> {
        let _clipboard = Clipboard::new()?;
        let hglobal = create_hdrop(paths)?;
        // SAFETY: clipboard is open; ownership of the HGLOBAL transfers to the system on success.
        let set_result = unsafe { SetClipboardData(CF_HDROP.0 as u32, Some(hglobal)) };
        set_result
            .ok()
            .map_err(|err| format!("SetClipboardData failed: {err}"))
            .map(|_| ())
    }

    fn create_hdrop(paths: &[PathBuf]) -> Result<HGLOBAL, String> {
        let mut utf16_paths = Vec::new();
        for path in paths {
            let wide: Vec<u16> = path
                .as_os_str()
                .encode_wide()
                .chain(std::iter::once(0))
                .collect();
            utf16_paths.extend_from_slice(&wide);
        }
        utf16_paths.push(0); // Double null-terminator.
        let bytes_needed =
            std::mem::size_of::<DROPFILES>() + utf16_paths.len() * std::mem::size_of::<u16>();
        // SAFETY: allocating movable global memory for shell clipboard.
        let handle = unsafe { GlobalAlloc(GMEM_MOVEABLE | GMEM_ZEROINIT, bytes_needed) }
            .map_err(|_| "GlobalAlloc failed".to_string())?;
        // SAFETY: lock global memory to populate DROPFILES header and path list.
        let ptr = unsafe { GlobalLock(handle) };
        if ptr.is_null() {
            unsafe {
                let _ = GlobalUnlock(handle);
                let _ = GlobalFree(handle);
            }
            return Err("GlobalLock failed".into());
        }
        unsafe {
            let header = ptr as *mut DROPFILES;
            *header = DROPFILES {
                pFiles: std::mem::size_of::<DROPFILES>() as u32,
                pt: windows::Win32::Foundation::POINT { x: 0, y: 0 },
                fNC: false.into(),
                fWide: true.into(),
            };
            let data_ptr = (ptr as *mut u8).add(std::mem::size_of::<DROPFILES>());
            copy_nonoverlapping(
                utf16_paths.as_ptr() as *const u8,
                data_ptr,
                utf16_paths.len() * std::mem::size_of::<u16>(),
            );
            let _ = GlobalUnlock(handle);
        }
        Ok(handle)
    }
}
