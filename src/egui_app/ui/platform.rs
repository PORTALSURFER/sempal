#[cfg(target_os = "windows")]
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
#[cfg(target_os = "windows")]
use windows::Win32::Foundation::HWND;
#[cfg(target_os = "windows")]
use windows::Win32::Foundation::{POINT, RECT};
#[cfg(target_os = "windows")]
use windows::Win32::UI::Input::KeyboardAndMouse::{GetAsyncKeyState, VK_LBUTTON};
#[cfg(target_os = "windows")]
use windows::Win32::UI::WindowsAndMessaging::{GetCursorPos, GetWindowRect};

#[cfg(target_os = "windows")]
pub(super) fn hwnd_from_frame(frame: &eframe::Frame) -> Option<HWND> {
    let handle = frame.window_handle().ok()?;
    match handle.as_raw() {
        RawWindowHandle::Win32(win) => Some(HWND(win.hwnd.get() as *mut _)),
        _ => None,
    }
}

#[cfg(target_os = "windows")]
pub(super) fn cursor_inside_hwnd(hwnd: HWND) -> Option<bool> {
    unsafe {
        let mut cursor = POINT::default();
        if GetCursorPos(&mut cursor).is_err() {
            return None;
        }
        let mut rect = RECT::default();
        if GetWindowRect(hwnd, &mut rect).is_err() {
            return None;
        }
        Some(
            cursor.x >= rect.left
                && cursor.x < rect.right
                && cursor.y >= rect.top
                && cursor.y < rect.bottom,
        )
    }
}

#[cfg(target_os = "windows")]
pub(super) fn left_mouse_button_down() -> bool {
    unsafe { (GetAsyncKeyState(VK_LBUTTON.0 as i32) & 0x8000) != 0 }
}
