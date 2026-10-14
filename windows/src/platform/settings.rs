use std::ffi::c_void;

use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::{BOOL, ERROR_SUCCESS};
use windows::Win32::Graphics::Dwm::DwmGetColorizationColor;
use windows::Win32::System::Registry::{
    RegCloseKey, RegDeleteValueW, RegGetValueW, RegOpenKeyExW, RegSetValueExW, HKEY,
    HKEY_CURRENT_USER, KEY_SET_VALUE, REG_SZ, RRF_RT_REG_DWORD, RRF_RT_REG_SZ,
};

const RUN_KEY: PCWSTR = w!("Software\\Microsoft\\Windows\\CurrentVersion\\Run");
const RUN_VALUE: PCWSTR = w!("RadialSwitcher");
const DEFAULT_ACCENT: [u8; 3] = [0, 103, 192];

pub fn start_at_login() -> bool {
    unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            RUN_KEY,
            RUN_VALUE,
            RRF_RT_REG_SZ,
            None,
            None,
            None,
        ) == ERROR_SUCCESS
    }
}

pub fn set_start_at_login(enabled: bool) {
    unsafe {
        let mut key = HKEY::default();
        if RegOpenKeyExW(HKEY_CURRENT_USER, RUN_KEY, 0, KEY_SET_VALUE, &mut key) != ERROR_SUCCESS {
            return;
        }
        if enabled {
            if let Ok(exe) = std::env::current_exe() {
                let command: Vec<u16> = format!("\"{}\"", exe.display())
                    .encode_utf16()
                    .chain(Some(0))
                    .collect();
                let bytes =
                    std::slice::from_raw_parts(command.as_ptr() as *const u8, command.len() * 2);
                let _ = RegSetValueExW(key, RUN_VALUE, 0, REG_SZ, Some(bytes));
            }
        } else {
            let _ = RegDeleteValueW(key, RUN_VALUE);
        }
        let _ = RegCloseKey(key);
    }
}

pub fn dark_mode() -> bool {
    let mut value = 1u32;
    let mut size = std::mem::size_of::<u32>() as u32;
    let result = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            w!("Software\\Microsoft\\Windows\\CurrentVersion\\Themes\\Personalize"),
            w!("AppsUseLightTheme"),
            RRF_RT_REG_DWORD,
            None,
            Some(&mut value as *mut _ as *mut c_void),
            Some(&mut size),
        )
    };
    result != ERROR_SUCCESS || value == 0
}

pub fn accent() -> [u8; 3] {
    let mut color = 0u32;
    let mut opaque = BOOL::default();
    match unsafe { DwmGetColorizationColor(&mut color, &mut opaque) } {
        Ok(()) => [(color >> 16) as u8, (color >> 8) as u8, color as u8],
        Err(_) => DEFAULT_ACCENT,
    }
}
