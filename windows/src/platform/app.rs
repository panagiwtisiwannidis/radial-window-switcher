use std::cell::RefCell;
use std::ffi::c_void;

use windows::core::{w, Result};
use windows::Win32::Foundation::{GetLastError, ERROR_ALREADY_EXISTS, HINSTANCE, HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::{GetMonitorInfoW, MonitorFromPoint, MONITORINFO, MONITOR_DEFAULTTONEAREST};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Threading::{CreateMutexW, GetCurrentProcessId};
use windows::Win32::UI::HiDpi::{GetDpiForMonitor, SetProcessDpiAwarenessContext, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2, MDT_EFFECTIVE_DPI};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DispatchMessageW, GetCursorPos, GetMessageW, KillTimer, PostQuitMessage,
    RegisterClassExW, RegisterWindowMessageW, SetTimer, TranslateMessage, HICON, MSG, WINDOW_EX_STYLE, WM_DESTROY,
    WM_LBUTTONUP, WM_RBUTTONUP, WM_TIMER, WNDCLASSEXW, WS_OVERLAPPED,
};

use super::util::{
    WM_RING_CANCEL, WM_RING_CLICK, WM_RING_CONFIRM, WM_RING_PRESS, WM_RING_RELEASE, WM_RING_TAB, WM_TRAY,
};
use super::{activate, enumerate, hooks, icons, overlay::Overlay, preview, preview::Preview, settings, tray};
use crate::geometry::{Area, Geometry, OUTER, WINDOW_ARC};
use crate::icon;
use crate::model::RingModel;
use crate::render::{self, Style};
use crate::text::Text;

const TRACK_TIMER: usize = 1;
const TRACK_INTERVAL_MS: u32 = 8;
const MOVE_THRESHOLD: f32 = 4.0;
const EDGE_MARGIN: f32 = 8.0;
const RING_MARGIN: f32 = 44.0;
const PREVIEW_GAP: f32 = 16.0;

struct App {
    main: HWND,
    overlay: Overlay,
    preview: Preview,
    text: Text,
    model: RingModel,
    geometry: Geometry,
    style: Style,
    own_pid: u32,
    open: bool,
    center: (f32, f32),
    work: RECT,
    open_cursor: POINT,
    last_cursor: POINT,
    has_moved: bool,
}

thread_local! {
    static APP: RefCell<Option<App>> = const { RefCell::new(None) };
    static TRAY_ICON: RefCell<HICON> = RefCell::new(HICON::default());
    static TASKBAR_CREATED: RefCell<u32> = const { RefCell::new(0) };
}

pub fn run() {
    unsafe {
        let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
        let _mutex = CreateMutexW(None, true, w!("RadialSwitcher.SingleInstance"));
        if GetLastError() == ERROR_ALREADY_EXISTS {
            return;
        }
    }
    if let Err(error) = start() {
        eprintln!("Radial Switcher failed to start: {error}");
        return;
    }
    unsafe {
        let mut message = MSG::default();
        while GetMessageW(&mut message, None, 0, 0).as_bool() {
            let _ = TranslateMessage(&message);
            DispatchMessageW(&message);
        }
    }
    hooks::uninstall();
}

fn start() -> Result<()> {
    unsafe {
        let instance: HINSTANCE = GetModuleHandleW(None)?.into();
        let class = WNDCLASSEXW {
            cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
            lpfnWndProc: Some(main_proc),
            hInstance: instance,
            lpszClassName: w!("RadialSwitcherMain"),
            ..Default::default()
        };
        RegisterClassExW(&class);
        let main = CreateWindowExW(
            WINDOW_EX_STYLE(0),
            w!("RadialSwitcherMain"),
            w!("Radial Switcher"),
            WS_OVERLAPPED,
            0,
            0,
            0,
            0,
            None,
            None,
            instance,
            None,
        )?;
        TASKBAR_CREATED.with(|t| *t.borrow_mut() = RegisterWindowMessageW(w!("TaskbarCreated")));

        let app = App {
            main,
            overlay: Overlay::new(instance)?,
            preview: Preview::new(instance)?,
            text: Text::system().ok_or_else(windows::core::Error::from_win32)?,
            model: RingModel::default(),
            geometry: Geometry::default(),
            style: Style { scale: 1.0, dark: true, accent: settings::accent() },
            own_pid: GetCurrentProcessId(),
            open: false,
            center: (0.0, 0.0),
            work: RECT::default(),
            open_cursor: POINT::default(),
            last_cursor: POINT::default(),
            has_moved: false,
        };
        APP.with(|a| *a.borrow_mut() = Some(app));

        let tray_icon = icons::to_hicon(&icon::app_icon(32)).unwrap_or_default();
        TRAY_ICON.with(|t| *t.borrow_mut() = tray_icon);
        tray::add(main, tray_icon, true);
        hooks::install_keyboard(main)?;
    }
    Ok(())
}

fn with_app(action: impl FnOnce(&mut App)) {
    APP.with(|app| {
        if let Ok(mut app) = app.try_borrow_mut() {
            if let Some(app) = app.as_mut() {
                action(app);
            }
        }
    });
}

impl App {
    fn open(&mut self) {
        if self.open {
            return;
        }
        let snapshot = enumerate::snapshot(self.own_pid);
        self.model = RingModel::build(snapshot.front.as_ref(), &snapshot.others);

        let mut cursor = POINT::default();
        unsafe {
            let _ = GetCursorPos(&mut cursor);
        }
        let (work, scale) = monitor_at(cursor);
        self.work = work;
        self.style = Style { scale, dark: settings::dark_mode(), accent: settings::accent() };

        let reach = (OUTER.1 + RING_MARGIN) * scale;
        let clamp = |value: f32, low: f32, high: f32| if low <= high { value.clamp(low, high) } else { (low + high) / 2.0 };
        self.center = (
            clamp(cursor.x as f32, work.left as f32 + reach, work.right as f32 - reach),
            clamp(cursor.y as f32, work.top as f32 + reach, work.bottom as f32 - reach),
        );
        self.geometry = Geometry {
            visible: Some(Area {
                min_x: (work.left as f32 - self.center.0) / scale + EDGE_MARGIN,
                max_x: (work.right as f32 - self.center.0) / scale - EDGE_MARGIN,
                min_y: (self.center.1 - work.bottom as f32) / scale + EDGE_MARGIN,
                max_y: (self.center.1 - work.top as f32) / scale - EDGE_MARGIN,
            }),
        };

        self.open_cursor = cursor;
        self.last_cursor = cursor;
        self.has_moved = false;
        self.open = true;
        hooks::set_ring_open(true);
        self.redraw();
        unsafe {
            SetTimer(self.main, TRACK_TIMER, TRACK_INTERVAL_MS, None);
        }
    }

    fn close(&mut self) {
        if !self.open {
            return;
        }
        self.open = false;
        hooks::set_ring_open(false);
        unsafe {
            let _ = KillTimer(self.main, TRACK_TIMER);
        }
        self.overlay.hide();
        self.preview.hide();
        self.model.selection = None;
    }

    fn tick(&mut self) {
        let mut cursor = POINT::default();
        unsafe {
            let _ = GetCursorPos(&mut cursor);
        }
        if cursor == self.last_cursor {
            return;
        }
        self.last_cursor = cursor;
        if !self.has_moved {
            let moved = ((cursor.x - self.open_cursor.x) as f32).hypot((cursor.y - self.open_cursor.y) as f32);
            if moved < MOVE_THRESHOLD * self.style.scale {
                return;
            }
            self.has_moved = true;
        }
        self.update_selection(cursor);
    }

    fn update_selection(&mut self, cursor: POINT) {
        let scale = self.style.scale;
        let dx = (cursor.x as f32 - self.center.0) / scale;
        let dy = (self.center.1 - cursor.y as f32) / scale;
        let selection = self.geometry.selection(
            dx,
            dy,
            self.model.inner.len(),
            &self.model.outer_window_counts(),
            &|_, _| 0,
            self.model.selection,
        );
        if selection != self.model.selection {
            self.model.selection = selection;
            self.redraw();
        }
    }

    fn redraw(&mut self) {
        let pixmap = render::render(&self.model, &self.geometry, &self.text, self.style);
        let left = (self.center.0 - pixmap.width() as f32 / 2.0).round() as i32;
        let top = (self.center.1 - pixmap.height() as f32 / 2.0).round() as i32;
        self.overlay.show(&pixmap, left, top);

        match self.model.selected_item() {
            Some(item) => {
                let frame = self.preview_frame();
                let window = item.preview_window().handle;
                self.preview.show(window, &item.full_title, frame, self.style.scale, self.style.dark);
            }
            None => self.preview.hide(),
        }
    }

    fn preview_frame(&self) -> RECT {
        let scale = self.style.scale;
        let (w, h) = preview::card_size();
        let (w, h) = (w * scale, h * scale);
        let distance = (WINDOW_ARC.1 + PREVIEW_GAP) * scale + w / 2.0;
        let work = self.work;
        let margin = EDGE_MARGIN * scale;
        let mut center_x = if self.center.0 + distance + w / 2.0 <= work.right as f32 - margin {
            self.center.0 + distance
        } else if self.center.0 - distance - w / 2.0 >= work.left as f32 + margin {
            self.center.0 - distance
        } else if work.right as f32 - self.center.0 >= self.center.0 - work.left as f32 {
            work.right as f32 - margin - w / 2.0
        } else {
            work.left as f32 + margin + w / 2.0
        };
        center_x = center_x.clamp(work.left as f32 + margin + w / 2.0, work.right as f32 - margin - w / 2.0);
        let center_y = self.center.1.clamp(work.top as f32 + margin + h / 2.0, work.bottom as f32 - margin - h / 2.0);
        RECT {
            left: (center_x - w / 2.0) as i32,
            top: (center_y - h / 2.0) as i32,
            right: (center_x + w / 2.0) as i32,
            bottom: (center_y + h / 2.0) as i32,
        }
    }

    fn released(&mut self) {
        if self.open && self.model.selected_item().is_some() {
            self.confirm();
        }
    }

    fn clicked(&mut self) {
        if self.has_moved {
            let mut cursor = POINT::default();
            unsafe {
                let _ = GetCursorPos(&mut cursor);
            }
            self.update_selection(cursor);
        }
        if self.model.selected_item().is_some() { self.confirm() } else { self.close() }
    }

    fn tab(&mut self, forward: bool) {
        if !self.open {
            return;
        }
        self.model.move_selection(forward);
        self.redraw();
    }

    fn confirm(&mut self) {
        let target = self.model.selected_item().map(|item| item.preview_window().handle);
        self.close();
        if let Some(handle) = target {
            activate::activate(HWND(handle as *mut c_void));
        }
    }
}

fn monitor_at(point: POINT) -> (RECT, f32) {
    unsafe {
        let monitor = MonitorFromPoint(point, MONITOR_DEFAULTTONEAREST);
        let mut info = MONITORINFO { cbSize: std::mem::size_of::<MONITORINFO>() as u32, ..Default::default() };
        let _ = GetMonitorInfoW(monitor, &mut info);
        let (mut dpi_x, mut dpi_y) = (96u32, 96u32);
        let _ = GetDpiForMonitor(monitor, MDT_EFFECTIVE_DPI, &mut dpi_x, &mut dpi_y);
        (info.rcWork, dpi_x as f32 / 96.0)
    }
}

unsafe extern "system" fn main_proc(hwnd: HWND, message: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match message {
        WM_RING_PRESS => with_app(|app| app.open()),
        WM_RING_RELEASE => with_app(|app| app.released()),
        WM_RING_TAB => with_app(|app| app.tab(wparam.0 == 0)),
        WM_RING_CONFIRM => with_app(|app| app.confirm()),
        WM_RING_CANCEL => with_app(|app| app.close()),
        WM_RING_CLICK => with_app(|app| app.clicked()),
        WM_TIMER if wparam.0 == TRACK_TIMER => with_app(|app| app.tick()),
        WM_TRAY => {
            let event = (lparam.0 & 0xFFFF) as u32;
            if event == WM_RBUTTONUP || event == WM_LBUTTONUP {
                tray::show_menu(hwnd);
            }
        }
        WM_DESTROY => {
            tray::remove(hwnd);
            PostQuitMessage(0);
        }
        _ => {
            if message != 0 && message == TASKBAR_CREATED.with(|t| *t.borrow()) {
                tray::add(hwnd, TRAY_ICON.with(|t| *t.borrow()), false);
                return LRESULT(0);
            }
            return DefWindowProcW(hwnd, message, wparam, lparam);
        }
    }
    LRESULT(0)
}
