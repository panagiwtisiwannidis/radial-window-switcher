use std::cell::Cell;

use windows::core::Result;
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetAsyncKeyState, SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYBD_EVENT_FLAGS,
    KEYEVENTF_KEYUP, VIRTUAL_KEY, VK_CONTROL, VK_ESCAPE, VK_LWIN, VK_MENU, VK_RETURN, VK_RWIN,
    VK_SHIFT, VK_SPACE, VK_TAB,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, PostMessageW, SetWindowsHookExW, UnhookWindowsHookEx, HC_ACTION, HHOOK,
    KBDLLHOOKSTRUCT, MSLLHOOKSTRUCT, WH_KEYBOARD_LL, WH_MOUSE_LL, WM_KEYDOWN, WM_KEYUP,
    WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MBUTTONDOWN, WM_MBUTTONUP, WM_RBUTTONDOWN, WM_RBUTTONUP,
    WM_SYSKEYDOWN, WM_SYSKEYUP,
};

use super::util::{
    INJECTED_MARKER, WM_RING_CANCEL, WM_RING_CLICK, WM_RING_CONFIRM, WM_RING_PRESS,
    WM_RING_RELEASE, WM_RING_TAB,
};

const MASK_KEY: u16 = 0xE8;

thread_local! {
    static TARGET: Cell<HWND> = Cell::new(HWND::default());
    static KEYBOARD: Cell<HHOOK> = Cell::new(HHOOK::default());
    static MOUSE: Cell<HHOOK> = Cell::new(HHOOK::default());
    static SPACE_HELD: Cell<bool> = const { Cell::new(false) };
    static RING_OPEN: Cell<bool> = const { Cell::new(false) };
}

pub fn install_keyboard(target: HWND) -> Result<()> {
    TARGET.with(|t| t.set(target));
    unsafe {
        let module = GetModuleHandleW(None)?;
        let hook = SetWindowsHookExW(WH_KEYBOARD_LL, Some(keyboard_proc), module, 0)?;
        KEYBOARD.with(|k| k.set(hook));
    }
    Ok(())
}

pub fn uninstall() {
    set_ring_open(false);
    KEYBOARD.with(|k| unsafe {
        if !k.get().is_invalid() {
            let _ = UnhookWindowsHookEx(k.get());
            k.set(HHOOK::default());
        }
    });
}

pub fn set_ring_open(open: bool) {
    RING_OPEN.with(|r| r.set(open));
    MOUSE.with(|m| unsafe {
        if open && m.get().is_invalid() {
            if let Ok(module) = GetModuleHandleW(None) {
                if let Ok(hook) = SetWindowsHookExW(WH_MOUSE_LL, Some(mouse_proc), module, 0) {
                    m.set(hook);
                }
            }
        } else if !open && !m.get().is_invalid() {
            let _ = UnhookWindowsHookEx(m.get());
            m.set(HHOOK::default());
        }
    });
}

pub fn send_alt_tap() {
    send_keys(&[(VK_MENU.0, false), (VK_MENU.0, true)]);
}

fn send_keys(keys: &[(u16, bool)]) {
    let inputs: Vec<INPUT> = keys
        .iter()
        .map(|&(vk, up)| INPUT {
            r#type: INPUT_KEYBOARD,
            Anonymous: INPUT_0 {
                ki: KEYBDINPUT {
                    wVk: VIRTUAL_KEY(vk),
                    wScan: 0,
                    dwFlags: if up {
                        KEYEVENTF_KEYUP
                    } else {
                        KEYBD_EVENT_FLAGS(0)
                    },
                    time: 0,
                    dwExtraInfo: INJECTED_MARKER,
                },
            },
        })
        .collect();
    unsafe {
        SendInput(&inputs, std::mem::size_of::<INPUT>() as i32);
    }
}

fn post(message: u32, wparam: usize) {
    let target = TARGET.with(|t| t.get());
    unsafe {
        let _ = PostMessageW(target, message, WPARAM(wparam), LPARAM(0));
    }
}

fn is_pressed(key: VIRTUAL_KEY) -> bool {
    unsafe { GetAsyncKeyState(key.0 as i32) < 0 }
}

fn handle_key(key: VIRTUAL_KEY, down: bool, up: bool) -> bool {
    let open = RING_OPEN.with(|r| r.get());
    match key {
        VK_SPACE => {
            let held = SPACE_HELD.with(|s| s.get());
            if down {
                if held {
                    return true;
                }
                let only_alt = is_pressed(VK_MENU)
                    && !is_pressed(VK_CONTROL)
                    && !is_pressed(VK_SHIFT)
                    && !is_pressed(VK_LWIN)
                    && !is_pressed(VK_RWIN);
                if only_alt {
                    SPACE_HELD.with(|s| s.set(true));
                    send_keys(&[(MASK_KEY, false), (MASK_KEY, true)]);
                    post(WM_RING_PRESS, 0);
                    return true;
                }
            } else if up && held {
                SPACE_HELD.with(|s| s.set(false));
                post(WM_RING_RELEASE, 0);
                return true;
            }
            false
        }
        VK_TAB if open => {
            if down {
                post(WM_RING_TAB, is_pressed(VK_SHIFT) as usize);
            }
            true
        }
        VK_RETURN if open => {
            if down {
                post(WM_RING_CONFIRM, 0);
            }
            true
        }
        VK_ESCAPE if open => {
            if down {
                post(WM_RING_CANCEL, 0);
            }
            true
        }
        _ => false,
    }
}

unsafe extern "system" fn keyboard_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code == HC_ACTION as i32 {
        let event = &*(lparam.0 as *const KBDLLHOOKSTRUCT);
        if event.dwExtraInfo != INJECTED_MARKER {
            let message = wparam.0 as u32;
            let down = message == WM_KEYDOWN || message == WM_SYSKEYDOWN;
            let up = message == WM_KEYUP || message == WM_SYSKEYUP;
            if handle_key(VIRTUAL_KEY(event.vkCode as u16), down, up) {
                return LRESULT(1);
            }
        }
    }
    CallNextHookEx(None, code, wparam, lparam)
}

unsafe extern "system" fn mouse_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code == HC_ACTION as i32 && RING_OPEN.with(|r| r.get()) {
        let event = &*(lparam.0 as *const MSLLHOOKSTRUCT);
        if event.dwExtraInfo != INJECTED_MARKER {
            match wparam.0 as u32 {
                WM_LBUTTONDOWN => {
                    post(WM_RING_CLICK, 0);
                    return LRESULT(1);
                }
                WM_RBUTTONDOWN => {
                    post(WM_RING_CANCEL, 0);
                    return LRESULT(1);
                }
                WM_LBUTTONUP | WM_RBUTTONUP | WM_MBUTTONDOWN | WM_MBUTTONUP => return LRESULT(1),
                _ => {}
            }
        }
    }
    CallNextHookEx(None, code, wparam, lparam)
}
