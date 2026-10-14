use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
use windows::Win32::System::Threading::{AttachThreadInput, GetCurrentThreadId};
use windows::Win32::UI::WindowsAndMessaging::{
    BringWindowToTop, GetForegroundWindow, GetWindowThreadProcessId, IsIconic, PostMessageW,
    SetForegroundWindow, ShowWindow, SW_RESTORE, WM_CLOSE,
};

use super::{enumerate, hooks};

pub fn close(hwnd: HWND) {
    unsafe {
        let _ = PostMessageW(hwnd, WM_CLOSE, WPARAM(0), LPARAM(0));
    }
}

pub fn close_app(pid: u32, own_pid: u32) {
    let snapshot = enumerate::snapshot(own_pid);
    for window in snapshot
        .front
        .iter()
        .chain(&snapshot.others)
        .flat_map(|app| &app.windows)
        .filter(|w| w.pid == pid)
    {
        close(HWND(window.handle as *mut std::ffi::c_void));
    }
}

pub fn activate(hwnd: HWND) {
    unsafe {
        if IsIconic(hwnd).as_bool() {
            let _ = ShowWindow(hwnd, SW_RESTORE);
        }
        let foreground_thread = GetWindowThreadProcessId(GetForegroundWindow(), None);
        let current_thread = GetCurrentThreadId();
        let attached = foreground_thread != 0
            && foreground_thread != current_thread
            && AttachThreadInput(current_thread, foreground_thread, true).as_bool();
        let mut focused = SetForegroundWindow(hwnd).as_bool();
        let _ = BringWindowToTop(hwnd);
        if attached {
            let _ = AttachThreadInput(current_thread, foreground_thread, false);
        }
        if !focused {
            hooks::send_alt_tap();
            focused = SetForegroundWindow(hwnd).as_bool();
        }
        if !focused {
            let _ = BringWindowToTop(hwnd);
        }
    }
}
