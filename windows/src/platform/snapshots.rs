use std::collections::HashMap;
use std::ffi::c_void;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use tiny_skia::Pixmap;
use windows::Win32::Foundation::{BOOL, HWND, LPARAM, RECT, TRUE};
use windows::Win32::Graphics::Gdi::{
    BitBlt, CreateCompatibleBitmap, CreateCompatibleDC, CreateDIBSection, DeleteDC, DeleteObject,
    GetDC, ReleaseDC, SelectObject, SetStretchBltMode, StretchBlt, BITMAPINFO, BITMAPINFOHEADER,
    BI_RGB, DIB_RGB_COLORS, HALFTONE, HDC, SRCCOPY,
};
use windows::Win32::Storage::Xps::{PrintWindow, PRINT_WINDOW_FLAGS};
use windows::Win32::UI::WindowsAndMessaging::{
    EnumWindows, GetWindowRect, GetWindowThreadProcessId, IsIconic, IsWindow,
};

use super::enumerate;
use radial_core::model::Icon;

const INTERVAL: Duration = Duration::from_secs(5);
const MAX_WIDTH: i32 = 480;
const TITLES_PER_WINDOW: usize = 40;
const PW_RENDERFULLCONTENT: u32 = 2;

#[derive(Default)]
struct Store {
    windows: HashMap<isize, Icon>,
    titles: HashMap<isize, Vec<(String, Icon)>>,
}

#[derive(Clone)]
pub struct Snapshots {
    store: Arc<Mutex<Store>>,
}

impl Snapshots {
    pub fn start(own_pid: u32) -> Self {
        let store = Arc::new(Mutex::new(Store::default()));
        let shared = store.clone();
        std::thread::spawn(move || loop {
            capture_all(&shared, own_pid);
            std::thread::sleep(INTERVAL);
        });
        Self { store }
    }

    pub fn window(&self, handle: isize) -> Option<Icon> {
        self.store.lock().ok()?.windows.get(&handle).cloned()
    }

    pub fn tab(&self, handle: isize, tab_title: &str) -> Option<Icon> {
        if tab_title.is_empty() {
            return None;
        }
        let store = self.store.lock().ok()?;
        store
            .titles
            .get(&handle)?
            .iter()
            .rev()
            .find(|(title, _)| title.starts_with(tab_title))
            .map(|(_, image)| image.clone())
    }
}

fn capture_all(store: &Arc<Mutex<Store>>, own_pid: u32) {
    let mut handles: Vec<HWND> = Vec::new();
    unsafe {
        let _ = EnumWindows(Some(collect), LPARAM(&mut handles as *mut _ as isize));
    }
    for hwnd in handles {
        let eligible = unsafe {
            let mut pid = 0u32;
            GetWindowThreadProcessId(hwnd, Some(&mut pid));
            pid != own_pid && !IsIconic(hwnd).as_bool() && enumerate::is_switchable(hwnd)
        };
        if !eligible {
            continue;
        }
        let Some(image) = (unsafe { capture_window(hwnd) }) else {
            continue;
        };
        let image = Arc::new(image);
        let title = unsafe { enumerate::title(hwnd) };
        if let Ok(mut store) = store.lock() {
            let key = hwnd.0 as isize;
            store.windows.insert(key, image.clone());
            let titles = store.titles.entry(key).or_default();
            titles.retain(|(t, _)| *t != title);
            titles.push((title, image));
            if titles.len() > TITLES_PER_WINDOW {
                titles.remove(0);
            }
        }
    }
    if let Ok(mut store) = store.lock() {
        store
            .windows
            .retain(|handle, _| unsafe { IsWindow(HWND(*handle as *mut c_void)).as_bool() });
        let live: Vec<isize> = store.windows.keys().copied().collect();
        store.titles.retain(|handle, _| live.contains(handle));
    }
}

unsafe extern "system" fn collect(hwnd: HWND, lparam: LPARAM) -> BOOL {
    (*(lparam.0 as *mut Vec<HWND>)).push(hwnd);
    TRUE
}

unsafe fn capture_window(hwnd: HWND) -> Option<Pixmap> {
    let mut rect = RECT::default();
    GetWindowRect(hwnd, &mut rect).ok()?;
    let (width, height) = (rect.right - rect.left, rect.bottom - rect.top);
    if width < 50 || height < 50 {
        return None;
    }
    let screen = GetDC(None);
    let full_dc = CreateCompatibleDC(screen);
    let full = CreateCompatibleBitmap(screen, width, height);
    let previous_full = SelectObject(full_dc, full);
    let printed = PrintWindow(hwnd, full_dc, PRINT_WINDOW_FLAGS(PW_RENDERFULLCONTENT)).as_bool();

    let target_width = width.min(MAX_WIDTH);
    let target_height = (height as i64 * target_width as i64 / width as i64).max(1) as i32;
    let result = if printed {
        scaled_copy(
            screen,
            full_dc,
            (width, height),
            (target_width, target_height),
        )
    } else {
        None
    };

    SelectObject(full_dc, previous_full);
    let _ = DeleteObject(full);
    let _ = DeleteDC(full_dc);
    ReleaseDC(None, screen);
    result
}

pub unsafe fn capture_screen(left: i32, top: i32, width: i32, height: i32) -> Option<Pixmap> {
    let screen = GetDC(None);
    let result = read_region(
        screen,
        screen,
        (left, top),
        (width, height),
        (width, height),
    );
    ReleaseDC(None, screen);
    result
}

unsafe fn scaled_copy(
    screen: HDC,
    source: HDC,
    size: (i32, i32),
    target: (i32, i32),
) -> Option<Pixmap> {
    read_region(screen, source, (0, 0), size, target)
}

unsafe fn read_region(
    screen: HDC,
    source: HDC,
    origin: (i32, i32),
    size: (i32, i32),
    target: (i32, i32),
) -> Option<Pixmap> {
    let info = BITMAPINFO {
        bmiHeader: BITMAPINFOHEADER {
            biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: target.0,
            biHeight: -target.1,
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB.0,
            ..Default::default()
        },
        ..Default::default()
    };
    let mut bits: *mut c_void = std::ptr::null_mut();
    let bitmap = CreateDIBSection(screen, &info, DIB_RGB_COLORS, &mut bits, None, 0).ok()?;
    let dc = CreateCompatibleDC(screen);
    let previous = SelectObject(dc, bitmap);
    let copied = if size == target {
        BitBlt(
            dc, 0, 0, target.0, target.1, source, origin.0, origin.1, SRCCOPY,
        )
        .is_ok()
    } else {
        SetStretchBltMode(dc, HALFTONE);
        StretchBlt(
            dc, 0, 0, target.0, target.1, source, origin.0, origin.1, size.0, size.1, SRCCOPY,
        )
        .as_bool()
    };
    let mut pixmap = None;
    if copied {
        let data =
            std::slice::from_raw_parts(bits as *const u8, (target.0 * target.1 * 4) as usize);
        if let Some(mut p) = Pixmap::new(target.0 as u32, target.1 as u32) {
            for (src, dst) in data.chunks(4).zip(p.data_mut().chunks_mut(4)) {
                dst[0] = src[2];
                dst[1] = src[1];
                dst[2] = src[0];
                dst[3] = 255;
            }
            pixmap = Some(p);
        }
    }
    SelectObject(dc, previous);
    let _ = DeleteObject(bitmap);
    let _ = DeleteDC(dc);
    pixmap
}
