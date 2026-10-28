//! Small Win32 helpers: single instance guard and bringing the window forward.

use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use windows_sys::Win32::Foundation::{ERROR_ALREADY_EXISTS, GetLastError, HANDLE, HWND, POINT};
use windows_sys::Win32::Graphics::Gdi::{GetMonitorInfoW, MONITOR_DEFAULTTONEAREST, MONITORINFO, MonitorFromPoint};
use windows_sys::Win32::System::Threading::CreateMutexW;
use windows_sys::Win32::UI::HiDpi::{GetDpiForMonitor, MDT_EFFECTIVE_DPI};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    GA_ROOTOWNER, GetAncestor, GetCursorPos, GetForegroundWindow, IsIconic, MB_ICONINFORMATION, MB_OK, MessageBoxW,
    SW_RESTORE, SetForegroundWindow, ShowWindow,
};

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

/// Holds the named mutex for the lifetime of the process.
pub struct InstanceGuard(#[allow(dead_code)] HANDLE);

/// Returns `None` when another instance already owns the mutex.
pub fn single_instance() -> Option<InstanceGuard> {
    let name = wide("Local\\TotpTray.SingleInstance");
    // SAFETY: plain Win32 call with a valid, NUL-terminated name.
    unsafe {
        let handle = CreateMutexW(std::ptr::null(), 0, name.as_ptr());
        if handle.is_null() || GetLastError() == ERROR_ALREADY_EXISTS {
            return None;
        }
        Some(InstanceGuard(handle))
    }
}

pub fn info_box(title: &str, text: &str) {
    let (title, text) = (wide(title), wide(text));
    // SAFETY: both strings are NUL-terminated and outlive the call.
    unsafe {
        MessageBoxW(std::ptr::null_mut(), text.as_ptr(), title.as_ptr(), MB_OK | MB_ICONINFORMATION);
    }
}

fn hwnd(window: &slint::Window) -> Option<HWND> {
    let handle = window.window_handle();
    let handle = handle.window_handle().ok()?;
    match handle.as_raw() {
        RawWindowHandle::Win32(h) => Some(h.hwnd.get() as HWND),
        _ => None,
    }
}

/// Restores and focuses a Slint window.
pub fn bring_to_front(window: &slint::Window) {
    let Some(hwnd) = hwnd(window) else { return };
    // SAFETY: hwnd belongs to a live window owned by this thread.
    unsafe {
        if IsIconic(hwnd) != 0 {
            ShowWindow(hwnd, SW_RESTORE);
        }
        SetForegroundWindow(hwnd);
    }
}

/// True when the window, or a window it owns (file dialog, popup), is in front.
pub fn is_foreground(window: &slint::Window) -> bool {
    let Some(hwnd) = hwnd(window) else { return false };
    // SAFETY: read-only window queries.
    unsafe {
        let fg = GetForegroundWindow();
        fg == hwnd || (!fg.is_null() && GetAncestor(fg, GA_ROOTOWNER) == hwnd)
    }
}

/// Work area (screen minus taskbar) of a monitor in physical pixels, plus its scale factor.
pub struct WorkArea {
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
    pub scale: f32,
}

pub fn work_area_at(x: i32, y: i32) -> WorkArea {
    // SAFETY: MONITORINFO is plain data with cbSize set as required.
    unsafe {
        let monitor = MonitorFromPoint(POINT { x, y }, MONITOR_DEFAULTTONEAREST);
        let mut info: MONITORINFO = std::mem::zeroed();
        info.cbSize = std::mem::size_of::<MONITORINFO>() as u32;
        GetMonitorInfoW(monitor, &mut info);
        let (mut dpi_x, mut dpi_y) = (96u32, 96u32);
        if GetDpiForMonitor(monitor, MDT_EFFECTIVE_DPI, &mut dpi_x, &mut dpi_y) != 0 {
            dpi_x = 96;
        }
        let r = info.rcWork;
        WorkArea { left: r.left, top: r.top, right: r.right, bottom: r.bottom, scale: dpi_x as f32 / 96.0 }
    }
}

pub fn cursor_pos() -> (i32, i32) {
    let mut p = POINT { x: 0, y: 0 };
    // SAFETY: p is a valid out pointer.
    unsafe { GetCursorPos(&mut p) };
    (p.x, p.y)
}
