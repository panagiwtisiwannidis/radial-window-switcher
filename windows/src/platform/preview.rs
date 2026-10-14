use std::cell::{Cell, RefCell};
use std::ffi::c_void;

use windows::core::{w, Result};
use windows::Win32::Foundation::{BOOL, COLORREF, HINSTANCE, HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows::Win32::Graphics::Dwm::{
    DwmQueryThumbnailSourceSize, DwmRegisterThumbnail, DwmSetWindowAttribute, DwmUnregisterThumbnail,
    DwmUpdateThumbnailProperties, DWMWA_USE_IMMERSIVE_DARK_MODE, DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_ROUND,
    DWM_THUMBNAIL_PROPERTIES, DWM_TNP_OPACITY, DWM_TNP_RECTDESTINATION, DWM_TNP_SOURCECLIENTAREAONLY, DWM_TNP_VISIBLE,
};
use windows::Win32::Graphics::Gdi::{
    BeginPaint, CreateFontW, CreateSolidBrush, DeleteObject, DrawTextW, EndPaint, FillRect, InvalidateRect,
    SelectObject, SetBkMode, SetTextColor, CLEARTYPE_QUALITY, CLIP_DEFAULT_PRECIS, DEFAULT_CHARSET, DT_CENTER,
    DT_END_ELLIPSIS, DT_SINGLELINE, DT_VCENTER, OUT_DEFAULT_PRECIS, PAINTSTRUCT, TRANSPARENT,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, GetClientRect, RegisterClassExW, SetWindowPos, ShowWindow, HTTRANSPARENT,
    HWND_TOPMOST, SWP_NOACTIVATE, SWP_SHOWWINDOW, SW_HIDE, WM_NCHITTEST, WM_PAINT, WNDCLASSEXW, WS_EX_NOACTIVATE,
    WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_POPUP,
};

pub const IMAGE_SIZE: (f32, f32) = (440.0, 290.0);
pub const PADDING: f32 = 12.0;
pub const TITLE_HEIGHT: f32 = 30.0;

pub fn card_size() -> (f32, f32) {
    (IMAGE_SIZE.0 + 2.0 * PADDING, IMAGE_SIZE.1 + 2.0 * PADDING + TITLE_HEIGHT)
}

thread_local! {
    static CARD: RefCell<(Vec<u16>, f32, bool)> = const { RefCell::new((Vec::new(), 1.0, true)) };
}

pub struct Preview {
    hwnd: HWND,
    thumbnail: Cell<Option<isize>>,
    source: Cell<isize>,
}

impl Preview {
    pub fn new(instance: HINSTANCE) -> Result<Self> {
        unsafe {
            let class = WNDCLASSEXW {
                cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
                lpfnWndProc: Some(window_proc),
                hInstance: instance,
                lpszClassName: w!("RadialSwitcherPreview"),
                ..Default::default()
            };
            RegisterClassExW(&class);
            let hwnd = CreateWindowExW(
                WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE,
                w!("RadialSwitcherPreview"),
                w!("Radial Switcher Preview"),
                WS_POPUP,
                0,
                0,
                1,
                1,
                None,
                None,
                instance,
                None,
            )?;
            let corner = DWMWCP_ROUND;
            let _ = DwmSetWindowAttribute(hwnd, DWMWA_WINDOW_CORNER_PREFERENCE, &corner as *const _ as *const c_void, 4);
            let dark = BOOL::from(true);
            let _ = DwmSetWindowAttribute(hwnd, DWMWA_USE_IMMERSIVE_DARK_MODE, &dark as *const _ as *const c_void, 4);
            Ok(Self { hwnd, thumbnail: Cell::new(None), source: Cell::new(0) })
        }
    }

    pub fn show(&self, source: isize, title: &str, frame: RECT, scale: f32, dark: bool) {
        CARD.with(|card| *card.borrow_mut() = (title.encode_utf16().collect(), scale, dark));
        unsafe {
            let _ = SetWindowPos(
                self.hwnd,
                HWND_TOPMOST,
                frame.left,
                frame.top,
                frame.right - frame.left,
                frame.bottom - frame.top,
                SWP_NOACTIVATE | SWP_SHOWWINDOW,
            );
            let _ = InvalidateRect(self.hwnd, None, true);
        }
        if self.source.get() != source {
            self.release_thumbnail();
            if let Ok(id) = unsafe { DwmRegisterThumbnail(self.hwnd, HWND(source as *mut c_void)) } {
                self.thumbnail.set(Some(id));
                self.source.set(source);
            }
        }
        self.place_thumbnail(scale);
    }

    pub fn hide(&self) {
        self.release_thumbnail();
        unsafe {
            let _ = ShowWindow(self.hwnd, SW_HIDE);
        }
    }

    fn release_thumbnail(&self) {
        if let Some(id) = self.thumbnail.take() {
            unsafe {
                let _ = DwmUnregisterThumbnail(id);
            }
        }
        self.source.set(0);
    }

    fn place_thumbnail(&self, scale: f32) {
        let Some(id) = self.thumbnail.get() else { return };
        let Ok(source) = (unsafe { DwmQueryThumbnailSourceSize(id) }) else { return };
        if source.cx <= 0 || source.cy <= 0 {
            return;
        }
        let (box_w, box_h) = (IMAGE_SIZE.0 * scale, IMAGE_SIZE.1 * scale);
        let fit = (box_w / source.cx as f32).min(box_h / source.cy as f32).min(1.0);
        let (w, h) = (source.cx as f32 * fit, source.cy as f32 * fit);
        let left = PADDING * scale + (box_w - w) / 2.0;
        let top = PADDING * scale + (box_h - h) / 2.0;
        let properties = DWM_THUMBNAIL_PROPERTIES {
            dwFlags: DWM_TNP_RECTDESTINATION | DWM_TNP_VISIBLE | DWM_TNP_OPACITY | DWM_TNP_SOURCECLIENTAREAONLY,
            rcDestination: RECT { left: left as i32, top: top as i32, right: (left + w) as i32, bottom: (top + h) as i32 },
            opacity: 255,
            fVisible: true.into(),
            fSourceClientAreaOnly: false.into(),
            ..Default::default()
        };
        unsafe {
            let _ = DwmUpdateThumbnailProperties(id, &properties);
        }
    }
}

fn rgb(r: u8, g: u8, b: u8) -> COLORREF {
    COLORREF(r as u32 | (g as u32) << 8 | (b as u32) << 16)
}

unsafe fn paint(hwnd: HWND) {
    let mut ps = PAINTSTRUCT::default();
    let dc = BeginPaint(hwnd, &mut ps);
    let (title, scale, dark) = CARD.with(|card| card.borrow().clone());
    let mut client = RECT::default();
    let _ = GetClientRect(hwnd, &mut client);

    let background = if dark { CreateSolidBrush(rgb(32, 33, 40)) } else { CreateSolidBrush(rgb(243, 244, 248)) };
    FillRect(dc, &client, background);
    let _ = DeleteObject(background);

    let font = CreateFontW(
        -(14.0 * scale) as i32,
        0,
        0,
        0,
        600,
        0,
        0,
        0,
        DEFAULT_CHARSET.0 as u32,
        OUT_DEFAULT_PRECIS.0 as u32,
        CLIP_DEFAULT_PRECIS.0 as u32,
        CLEARTYPE_QUALITY.0 as u32,
        0,
        w!("Segoe UI"),
    );
    let previous = SelectObject(dc, font);
    SetBkMode(dc, TRANSPARENT);
    SetTextColor(dc, if dark { rgb(245, 245, 248) } else { rgb(24, 24, 28) });
    let mut text_rect = RECT {
        left: client.left + (PADDING * scale) as i32,
        top: client.bottom - ((TITLE_HEIGHT + PADDING / 2.0) * scale) as i32,
        right: client.right - (PADDING * scale) as i32,
        bottom: client.bottom - (PADDING / 2.0 * scale) as i32,
    };
    let mut title = title;
    DrawTextW(dc, &mut title, &mut text_rect, DT_CENTER | DT_SINGLELINE | DT_VCENTER | DT_END_ELLIPSIS);
    SelectObject(dc, previous);
    let _ = DeleteObject(font);
    let _ = EndPaint(hwnd, &ps);
}

unsafe extern "system" fn window_proc(hwnd: HWND, message: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match message {
        WM_PAINT => {
            paint(hwnd);
            LRESULT(0)
        }
        WM_NCHITTEST => LRESULT(HTTRANSPARENT as isize),
        _ => DefWindowProcW(hwnd, message, wparam, lparam),
    }
}
