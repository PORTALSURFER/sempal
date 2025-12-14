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
    use std::sync::OnceLock;
    use windows::Win32::Foundation::{GlobalFree, HANDLE, HGLOBAL};
    use windows::Win32::System::DataExchange::{
        CloseClipboard, EmptyClipboard, OpenClipboard, RegisterClipboardFormatW, SetClipboardData,
    };
    use windows::Win32::System::Memory::{
        GMEM_MOVEABLE, GMEM_ZEROINIT, GlobalAlloc, GlobalLock, GlobalUnlock,
    };
    use windows::Win32::System::Ole::{CF_HDROP, DROPEFFECT_COPY};
    use windows::Win32::UI::Shell::DROPFILES;
    use windows::core::w;

    struct Clipboard;

    impl Clipboard {
        fn new() -> Result<Self, String> {
            unsafe { OpenClipboard(None) }.map_err(|err| format!("OpenClipboard failed: {err}"))?;
            unsafe { EmptyClipboard() }.map_err(|err| format!("EmptyClipboard failed: {err}"))?;
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

    struct OwnedHGlobal {
        handle: HGLOBAL,
        released: bool,
    }

    impl OwnedHGlobal {
        fn new(bytes: usize) -> Result<Self, String> {
            let handle = unsafe { GlobalAlloc(GMEM_MOVEABLE | GMEM_ZEROINIT, bytes) }
                .map_err(|_| "GlobalAlloc failed".to_string())?;
            Ok(Self {
                handle,
                released: false,
            })
        }

        fn handle(&self) -> HGLOBAL {
            self.handle
        }

        fn release(mut self) -> HGLOBAL {
            self.released = true;
            self.handle
        }
    }

    impl Drop for OwnedHGlobal {
        fn drop(&mut self) {
            if !self.released {
                unsafe {
                    let _ = GlobalFree(Some(self.handle));
                }
            }
        }
    }

    struct GlobalLockGuard {
        handle: HGLOBAL,
        ptr: *mut core::ffi::c_void,
    }

    impl GlobalLockGuard {
        unsafe fn new(handle: HGLOBAL) -> Result<Self, String> {
            let ptr = unsafe { GlobalLock(handle) };
            if ptr.is_null() {
                return Err("GlobalLock failed".into());
            }
            Ok(Self { handle, ptr })
        }

        fn ptr(&self) -> *mut core::ffi::c_void {
            self.ptr
        }

        fn as_mut_ptr<T>(&self) -> *mut T {
            self.ptr as *mut T
        }
    }

    impl Drop for GlobalLockGuard {
        fn drop(&mut self) {
            unsafe {
                let _ = GlobalUnlock(self.handle);
            }
        }
    }

    pub fn copy_file_paths(paths: &[PathBuf]) -> Result<(), String> {
        let _clipboard = Clipboard::new()?;
        let hdrop = create_hdrop(paths)?;
        let drop_effect = create_drop_effect(DROPEFFECT_COPY.0)?;
        let effect_format = preferred_drop_effect_format()?;
        // SAFETY: clipboard is open; ownership of the HGLOBAL transfers to the system on success.
        unsafe { SetClipboardData(effect_format as u32, Some(HANDLE(drop_effect.handle().0))) }
            .map_err(|err| format!("SetClipboardData(Preferred DropEffect) failed: {err}"))?;
        let _ = drop_effect.release();
        // SAFETY: clipboard is open; ownership of the HGLOBAL transfers to the system on success.
        unsafe { SetClipboardData(CF_HDROP.0 as u32, Some(HANDLE(hdrop.handle().0))) }
            .map_err(|err| format!("SetClipboardData failed: {err}"))?;
        let _ = hdrop.release();
        Ok(())
    }

    fn preferred_drop_effect_format() -> Result<u16, String> {
        static FORMAT: OnceLock<Result<u16, String>> = OnceLock::new();
        FORMAT
            .get_or_init(|| {
                let fmt = unsafe { RegisterClipboardFormatW(w!("Preferred DropEffect")) };
                if fmt == 0 {
                    Err("RegisterClipboardFormatW failed".to_string())
                } else {
                    Ok(fmt as u16)
                }
            })
            .clone()
    }

    fn create_drop_effect(effect: u32) -> Result<OwnedHGlobal, String> {
        let owned = OwnedHGlobal::new(std::mem::size_of::<u32>())?;
        let lock = unsafe { GlobalLockGuard::new(owned.handle) }?;
        unsafe {
            *lock.as_mut_ptr::<u32>() = effect;
        }
        Ok(owned)
    }

    fn create_hdrop(paths: &[PathBuf]) -> Result<OwnedHGlobal, String> {
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
        let owned = OwnedHGlobal::new(bytes_needed)?;
        let lock = unsafe { GlobalLockGuard::new(owned.handle) }?;
        unsafe {
            let header = lock.as_mut_ptr::<DROPFILES>();
            *header = DROPFILES {
                pFiles: std::mem::size_of::<DROPFILES>() as u32,
                pt: windows::Win32::Foundation::POINT { x: 0, y: 0 },
                fNC: false.into(),
                fWide: true.into(),
            };
            let data_ptr = (lock.ptr() as *mut u8).add(std::mem::size_of::<DROPFILES>());
            copy_nonoverlapping(
                utf16_paths.as_ptr() as *const u8,
                data_ptr,
                utf16_paths.len() * std::mem::size_of::<u16>(),
            );
        }
        Ok(owned)
    }
}
