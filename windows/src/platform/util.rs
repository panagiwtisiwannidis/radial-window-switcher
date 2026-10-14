use windows::Win32::UI::WindowsAndMessaging::WM_APP;

pub const WM_RING_PRESS: u32 = WM_APP + 1;
pub const WM_RING_RELEASE: u32 = WM_APP + 2;
pub const WM_RING_TAB: u32 = WM_APP + 3;
pub const WM_RING_CONFIRM: u32 = WM_APP + 4;
pub const WM_RING_CANCEL: u32 = WM_APP + 5;
pub const WM_RING_CLICK: u32 = WM_APP + 6;
pub const WM_TRAY: u32 = WM_APP + 7;
pub const WM_TABS_READY: u32 = WM_APP + 8;

pub const INJECTED_MARKER: usize = 0x5253_5754;

pub fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(Some(0)).collect()
}

pub fn from_wide(buffer: &[u16]) -> String {
    let end = buffer.iter().position(|&c| c == 0).unwrap_or(buffer.len());
    String::from_utf16_lossy(&buffer[..end])
}
