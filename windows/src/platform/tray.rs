use windows::core::w;
use windows::Win32::Foundation::{HWND, POINT};
use windows::Win32::UI::Shell::{
    Shell_NotifyIconW, NIF_ICON, NIF_INFO, NIF_MESSAGE, NIF_TIP, NIIF_INFO, NIM_ADD, NIM_DELETE, NOTIFYICONDATAW,
};
use windows::Win32::UI::WindowsAndMessaging::{
    AppendMenuW, CreatePopupMenu, DestroyMenu, DestroyWindow, GetCursorPos, SetForegroundWindow, TrackPopupMenu, HICON,
    MF_CHECKED, MF_SEPARATOR, MF_STRING, MF_UNCHECKED, TPM_NONOTIFY, TPM_RETURNCMD, TPM_RIGHTBUTTON,
};

use super::settings;
use super::util::WM_TRAY;

const ICON_ID: u32 = 1;
const MENU_START_AT_LOGIN: usize = 1;
const MENU_QUIT: usize = 2;

fn copy_into(target: &mut [u16], text: &str) {
    for (slot, unit) in target.iter_mut().zip(text.encode_utf16().chain(Some(0))) {
        *slot = unit;
    }
    if let Some(last) = target.last_mut() {
        *last = 0;
    }
}

fn data(hwnd: HWND) -> NOTIFYICONDATAW {
    NOTIFYICONDATAW {
        cbSize: std::mem::size_of::<NOTIFYICONDATAW>() as u32,
        hWnd: hwnd,
        uID: ICON_ID,
        ..Default::default()
    }
}

pub fn add(hwnd: HWND, icon: HICON, announce: bool) {
    let mut data = data(hwnd);
    data.uFlags = NIF_ICON | NIF_MESSAGE | NIF_TIP;
    data.uCallbackMessage = WM_TRAY;
    data.hIcon = icon;
    copy_into(&mut data.szTip, "Radial Switcher — hold Alt+Space");
    if announce {
        data.uFlags |= NIF_INFO;
        data.dwInfoFlags = NIIF_INFO;
        copy_into(&mut data.szInfoTitle, "Radial Switcher is running");
        copy_into(&mut data.szInfo, "Hold Alt+Space to switch windows. Right-click this icon for options.");
    }
    unsafe {
        let _ = Shell_NotifyIconW(NIM_ADD, &data);
    }
}

pub fn remove(hwnd: HWND) {
    unsafe {
        let _ = Shell_NotifyIconW(NIM_DELETE, &data(hwnd));
    }
}

pub fn show_menu(hwnd: HWND) {
    unsafe {
        let Ok(menu) = CreatePopupMenu() else { return };
        let login_state = if settings::start_at_login() { MF_CHECKED } else { MF_UNCHECKED };
        let _ = AppendMenuW(menu, MF_STRING | login_state, MENU_START_AT_LOGIN, w!("Start with Windows"));
        let _ = AppendMenuW(menu, MF_SEPARATOR, 0, None);
        let _ = AppendMenuW(menu, MF_STRING, MENU_QUIT, w!("Quit Radial Switcher"));

        let mut cursor = POINT::default();
        let _ = GetCursorPos(&mut cursor);
        let _ = SetForegroundWindow(hwnd);
        let choice = TrackPopupMenu(menu, TPM_RETURNCMD | TPM_RIGHTBUTTON | TPM_NONOTIFY, cursor.x, cursor.y, 0, hwnd, None);
        let _ = DestroyMenu(menu);
        match choice.0 as usize {
            MENU_START_AT_LOGIN => settings::set_start_at_login(!settings::start_at_login()),
            MENU_QUIT => {
                let _ = DestroyWindow(hwnd);
            }
            _ => {}
        }
    }
}
