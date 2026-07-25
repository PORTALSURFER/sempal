use windows::Win32::Foundation::{GlobalFree, HGLOBAL, HWND};
use windows::Win32::System::DataExchange::{CloseClipboard, EmptyClipboard, OpenClipboard};
use windows::Win32::System::Memory::{
    GMEM_MOVEABLE, GMEM_ZEROINIT, GlobalAlloc, GlobalLock, GlobalUnlock,
};

pub(super) struct ClipboardWriter {
    _clipboard: ClipboardOpenGuard,
}

impl ClipboardWriter {
    pub(super) fn new() -> Result<Self, String> {
        let owner = active_window_owner()?;
        Self::new_with_owner(owner)
    }

    fn new_with_owner(owner: HWND) -> Result<Self, String> {
        unsafe { OpenClipboard(Some(owner)) }
            .map_err(|err| format!("OpenClipboard failed: {err}"))?;
        Self::after_opened(close_clipboard, || {
            unsafe { EmptyClipboard() }.map_err(|err| err.to_string())
        })
    }

    fn after_opened<E>(close: fn(), empty: E) -> Result<Self, String>
    where
        E: FnOnce() -> Result<(), String>,
    {
        let writer = Self {
            _clipboard: ClipboardOpenGuard { close },
        };
        empty().map_err(|err| format!("EmptyClipboard failed: {err}"))?;
        Ok(writer)
    }

    #[cfg(test)]
    fn test_after_opened<E>(close: fn(), empty: E) -> Result<Self, String>
    where
        E: FnOnce() -> Result<(), String>,
    {
        Self::after_opened(close, empty)
    }
}

fn active_window_owner() -> Result<HWND, String> {
    let owner = unsafe { windows::Win32::UI::WindowsAndMessaging::GetActiveWindow() };
    if owner.0.is_null() {
        Err("GetActiveWindow returned no clipboard owner".to_string())
    } else {
        Ok(owner)
    }
}

fn close_clipboard() {
    unsafe {
        let _ = CloseClipboard();
    }
}

struct ClipboardOpenGuard {
    close: fn(),
}

impl Drop for ClipboardOpenGuard {
    fn drop(&mut self) {
        (self.close)();
    }
}

pub(super) struct ClipboardReader {
    _clipboard: ClipboardOpenGuard,
}

impl ClipboardReader {
    pub(super) fn new() -> Result<Self, String> {
        unsafe { OpenClipboard(None) }.map_err(|err| format!("OpenClipboard failed: {err}"))?;
        Ok(Self {
            _clipboard: ClipboardOpenGuard {
                close: close_clipboard,
            },
        })
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::ClipboardWriter;

    static CLOSE_CALLS: AtomicUsize = AtomicUsize::new(0);

    fn record_close() {
        CLOSE_CALLS.fetch_add(1, Ordering::SeqCst);
    }

    #[test]
    fn writer_closes_exactly_once_after_empty_failure_or_normal_drop() {
        CLOSE_CALLS.store(0, Ordering::SeqCst);
        let empty_failure = ClipboardWriter::test_after_opened(record_close, || {
            Err("injected EmptyClipboard failure".to_string())
        });
        assert!(empty_failure.is_err());
        assert_eq!(CLOSE_CALLS.load(Ordering::SeqCst), 1);

        CLOSE_CALLS.store(0, Ordering::SeqCst);
        let writer = ClipboardWriter::test_after_opened(record_close, || Ok(()))
            .expect("injected clipboard setup should succeed");
        drop(writer);
        assert_eq!(CLOSE_CALLS.load(Ordering::SeqCst), 1);
    }
}

pub(super) struct OwnedHGlobal {
    handle: HGLOBAL,
    released: bool,
}

impl OwnedHGlobal {
    pub(super) fn new(bytes: usize) -> Result<Self, String> {
        let handle = unsafe { GlobalAlloc(GMEM_MOVEABLE | GMEM_ZEROINIT, bytes) }
            .map_err(|_| "GlobalAlloc failed".to_string())?;
        Ok(Self {
            handle,
            released: false,
        })
    }

    pub(super) fn handle(&self) -> HGLOBAL {
        self.handle
    }

    pub(super) fn release(mut self) -> HGLOBAL {
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

pub(super) struct GlobalLockGuard {
    handle: HGLOBAL,
    ptr: *mut core::ffi::c_void,
}

impl GlobalLockGuard {
    pub(super) unsafe fn new(handle: HGLOBAL) -> Result<Self, String> {
        let ptr = unsafe { GlobalLock(handle) };
        if ptr.is_null() {
            return Err("GlobalLock failed".into());
        }
        Ok(Self { handle, ptr })
    }

    pub(super) fn ptr(&self) -> *mut core::ffi::c_void {
        self.ptr
    }

    pub(super) fn as_mut_ptr<T>(&self) -> *mut T {
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
