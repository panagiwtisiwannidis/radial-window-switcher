use std::cell::RefCell;
use std::ffi::c_void;
use std::time::Instant;

use tiny_skia::Pixmap;
use windows::core::{w, Result};
use windows::Win32::Foundation::{
    GetLastError, ERROR_ALREADY_EXISTS, HINSTANCE, HWND, LPARAM, LRESULT, POINT, RECT, WPARAM,
};
use windows::Win32::Graphics::Gdi::{
    GetMonitorInfoW, MonitorFromPoint, MONITORINFO, MONITOR_DEFAULTTONEAREST,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Threading::{CreateMutexW, GetCurrentProcessId};
use windows::Win32::UI::HiDpi::{
    GetDpiForMonitor, SetProcessDpiAwarenessContext, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2,
    MDT_EFFECTIVE_DPI,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DispatchMessageW, GetCursorPos, GetMessageW, KillTimer,
    PostQuitMessage, RegisterClassExW, RegisterWindowMessageW, SetTimer, TranslateMessage, HICON,
    MSG, WINDOW_EX_STYLE, WM_DESTROY, WM_LBUTTONUP, WM_RBUTTONUP, WM_TIMER, WNDCLASSEXW,
    WS_OVERLAPPED,
};

use super::preview::{Content, Preview};
use super::snapshots::{self, Snapshots};
use super::tabs::Tabs;
use super::util::{
    WM_RING_CANCEL, WM_RING_CLICK, WM_RING_CONFIRM, WM_RING_PRESS, WM_RING_RELEASE, WM_RING_TAB,
    WM_TABS_READY, WM_TRAY,
};
use super::{activate, enumerate, hooks, icons, overlay::Overlay, preview, settings, tray};
use radial_core::geometry::{Area, Geometry, Level, Selection, DEAD_ZONE, INNER, OUTER, TAB_ARC};
use radial_core::icon;
use radial_core::model::{Icon, RingItem, RingModel, Target};
use radial_core::render::{self, Motion, Scene, Style};
use radial_core::text::Text;

const TRACK_TIMER: usize = 1;
const TRACK_INTERVAL_MS: u32 = 8;
const MOVE_THRESHOLD: f32 = 4.0;
const EDGE_MARGIN: f32 = 8.0;
const RING_MARGIN: f32 = 44.0;
const PREVIEW_GAP: f32 = 16.0;
const TABS_STILLNESS: f32 = 0.15;
const RELOAD_DELAYS_MS: [u64; 2] = [250, 1000];

struct App {
    main: HWND,
    overlay: Overlay,
    preview: Preview,
    text: Text,
    snapshots: Snapshots,
    tabs: Tabs,
    model: RingModel,
    geometry: Geometry,
    style: Style,
    motion: Motion,
    targets: Vec<(String, f32)>,
    backdrop: Option<Pixmap>,
    own_pid: u32,
    open: bool,
    center: (f32, f32),
    work: RECT,
    open_cursor: POINT,
    last_cursor: POINT,
    has_moved: bool,
    last_move: Instant,
    last_frame: Instant,
    dwell: Option<(Selection, Instant)>,
    close_button: Option<(f32, f32)>,
    close_hover: bool,
    reloads: Vec<Instant>,
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

        let own_pid = GetCurrentProcessId();
        let app = App {
            main,
            overlay: Overlay::new(instance)?,
            preview: Preview::new(instance)?,
            text: Text::system().ok_or_else(windows::core::Error::from_win32)?,
            snapshots: Snapshots::start(own_pid),
            tabs: Tabs::start(main),
            model: RingModel::default(),
            geometry: Geometry::default(),
            style: Style {
                scale: 1.0,
                dark: true,
                accent: settings::accent(),
            },
            motion: Motion::default(),
            targets: Vec::new(),
            backdrop: None,
            own_pid,
            open: false,
            center: (0.0, 0.0),
            work: RECT::default(),
            open_cursor: POINT::default(),
            last_cursor: POINT::default(),
            has_moved: false,
            last_move: Instant::now(),
            last_frame: Instant::now(),
            dwell: None,
            close_button: None,
            close_hover: false,
            reloads: Vec::new(),
        };
        app.prewarm_tabs();
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
    fn prewarm_tabs(&self) {
        let snapshot = enumerate::snapshot(self.own_pid);
        self.tabs
            .refresh(windows_of(&snapshot.front, &snapshot.others));
    }

    fn open(&mut self) {
        if self.open {
            return;
        }
        let snapshot = enumerate::snapshot(self.own_pid);
        self.model = RingModel::build(snapshot.front.as_ref(), &snapshot.others);
        self.model.tabs = self.tabs.known();
        self.tabs
            .refresh(windows_of(&snapshot.front, &snapshot.others));

        let mut cursor = POINT::default();
        unsafe {
            let _ = GetCursorPos(&mut cursor);
        }
        let (work, scale) = monitor_at(cursor);
        self.work = work;
        self.style = Style {
            scale,
            dark: settings::dark_mode(),
            accent: settings::accent(),
        };

        let reach = (OUTER.1 + RING_MARGIN) * scale;
        let clamp = |value: f32, low: f32, high: f32| {
            if low <= high {
                value.clamp(low, high)
            } else {
                (low + high) / 2.0
            }
        };
        self.center = (
            clamp(
                cursor.x as f32,
                work.left as f32 + reach,
                work.right as f32 - reach,
            ),
            clamp(
                cursor.y as f32,
                work.top as f32 + reach,
                work.bottom as f32 - reach,
            ),
        );
        self.geometry = Geometry {
            visible: Some(Area {
                min_x: (work.left as f32 - self.center.0) / scale + EDGE_MARGIN,
                max_x: (work.right as f32 - self.center.0) / scale - EDGE_MARGIN,
                min_y: (self.center.1 - work.bottom as f32) / scale + EDGE_MARGIN,
                max_y: (self.center.1 - work.top as f32) / scale - EDGE_MARGIN,
            }),
        };

        let size = render::frame_size(scale) as i32;
        let (left, top) = self.frame_origin(size);
        self.backdrop = unsafe { snapshots::capture_screen(left, top, size, size) }
            .map(|p| render::blur_backdrop(&p));

        self.open_cursor = cursor;
        self.last_cursor = cursor;
        self.has_moved = false;
        self.dwell = None;
        self.close_button = None;
        self.close_hover = false;
        self.reloads.clear();
        self.motion.reset();
        self.targets.clear();
        self.last_frame = Instant::now();
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
        self.model.inner_tabs_window = None;
        self.backdrop = None;
        self.reloads.clear();
    }

    fn frame_origin(&self, size: i32) -> (i32, i32) {
        (
            (self.center.0 - size as f32 / 2.0).round() as i32,
            (self.center.1 - size as f32 / 2.0).round() as i32,
        )
    }

    fn tick(&mut self) {
        let now = Instant::now();
        let dt = now.duration_since(self.last_frame).as_secs_f32().min(0.1);
        self.last_frame = now;

        let mut cursor = POINT::default();
        unsafe {
            let _ = GetCursorPos(&mut cursor);
        }
        let mut changed = false;
        if cursor != self.last_cursor {
            self.last_cursor = cursor;
            self.last_move = now;
            let moved = ((cursor.x - self.open_cursor.x) as f32)
                .hypot((cursor.y - self.open_cursor.y) as f32);
            if self.has_moved || moved >= MOVE_THRESHOLD * self.style.scale {
                self.has_moved = true;
                changed |= self.update_selection(cursor);
            }
        }
        changed |= self.check_dwell(now);
        let hover = self.over_close_button(cursor);
        if hover != self.close_hover {
            self.close_hover = hover;
            changed = true;
        }
        if self.reloads.first().is_some_and(|at| now >= *at) {
            self.reloads.remove(0);
            self.reload();
            changed = true;
        }

        let animating = self.motion.step(&self.targets, dt);
        if changed || animating {
            self.redraw();
        }
    }

    fn update_selection(&mut self, cursor: POINT) -> bool {
        let scale = self.style.scale;
        let dx = (cursor.x as f32 - self.center.0) / scale;
        let dy = (self.center.1 - cursor.y as f32) / scale;
        let selection = match self.model.inner_tabs_window {
            Some(window) => {
                let tab_count = self.model.inner_tab_items(window).len();
                let tab_selection = self.geometry.tab_popup_selection(
                    dx,
                    dy,
                    self.model.inner.len(),
                    window,
                    tab_count,
                );
                let distance = dx.hypot(dy);
                let ring_boundary = (INNER.1 + OUTER.0) / 2.0;
                if tab_selection.is_none() && distance > ring_boundary && distance <= OUTER.1 {
                    self.model.inner_tabs_window = None;
                    self.normal_selection(dx, dy, None)
                } else {
                    tab_selection
                }
            }
            None => self.normal_selection(dx, dy, self.model.selection),
        };
        if selection == self.model.selection {
            return false;
        }
        self.model.selection = selection;
        self.update_tab_mode(dx.hypot(dy) <= DEAD_ZONE);
        true
    }

    fn normal_selection(&self, dx: f32, dy: f32, current: Option<Selection>) -> Option<Selection> {
        let model = &self.model;
        self.geometry.selection(
            dx,
            dy,
            model.inner.len(),
            &model.outer_window_counts(),
            &|app, window| model.tab_items(app, window).len(),
            current,
        )
    }

    fn update_tab_mode(&mut self, in_dead_zone: bool) {
        let selection = self.model.selection;
        if let Some(window) = self.model.inner_tabs_window {
            let on_tabs = selection.is_some_and(|s| s.level == Level::Outer);
            let on_window = selection.is_some_and(|s| s.level == Level::Inner && s.index == window);
            if on_tabs || on_window || (selection.is_none() && !in_dead_zone) {
                return;
            }
            self.model.inner_tabs_window = None;
        }
        self.dwell = selection
            .filter(|s| s.level == Level::Inner && self.model.inner_window_has_tabs(s.index))
            .map(|s| (s, Instant::now()));
    }

    fn check_dwell(&mut self, now: Instant) -> bool {
        let Some((selection, _)) = self.dwell else {
            return false;
        };
        if self.model.selection != Some(selection) {
            self.dwell = None;
            return false;
        }
        if now.duration_since(self.last_move).as_secs_f32() < TABS_STILLNESS {
            return false;
        }
        self.dwell = None;
        self.model.inner_tabs_window = Some(selection.index);
        true
    }

    fn thumbnail(&self, item: &RingItem) -> Option<Icon> {
        let window = item.preview_window().handle;
        match &item.target {
            Target::Tab { tab, .. } => self.snapshots.tab(window, &tab.title).or_else(|| {
                if tab.selected {
                    self.snapshots.window(window)
                } else {
                    None
                }
            }),
            _ => self.snapshots.window(window),
        }
    }

    fn redraw(&mut self) {
        let thumbnail = |item: &RingItem| self.thumbnail(item);
        let frame = render::render(&Scene {
            model: &self.model,
            geometry: &self.geometry,
            text: &self.text,
            style: self.style,
            thumbnail: &thumbnail,
            backdrop: self.backdrop.as_ref(),
            motion: &self.motion,
            close_hover: self.close_hover,
        });
        let (left, top) = self.frame_origin(frame.pixmap.width() as i32);
        self.close_button = frame
            .close_button
            .map(|(x, y)| (x + left as f32, y + top as f32));
        self.overlay.show(&frame.pixmap, left, top);
        self.targets = frame.targets;

        match self.model.selected_item() {
            Some(item) => {
                let preview_frame = self.preview_frame();
                let content = self.preview_content(&item);
                self.preview.show(
                    content,
                    &item.full_title,
                    preview_frame,
                    self.style.scale,
                    self.style.dark,
                );
            }
            None => self.preview.hide(),
        }
    }

    fn preview_content(&self, item: &RingItem) -> Content {
        let window = item.preview_window();
        if let Some(tab) = item.tab() {
            if !tab.selected {
                return self.thumbnail(item).map_or(Content::Empty, Content::Image);
            }
        }
        if window.minimized {
            if let Some(image) = self.snapshots.window(window.handle) {
                return Content::Image(image);
            }
        }
        Content::Live(window.handle)
    }

    fn preview_frame(&self) -> RECT {
        let scale = self.style.scale;
        let (w, h) = preview::card_size();
        let (w, h) = (w * scale, h * scale);
        let distance = (TAB_ARC.1 + PREVIEW_GAP) * scale + w / 2.0;
        let work = self.work;
        let margin = EDGE_MARGIN * scale;
        let center_x = if self.center.0 + distance + w / 2.0 <= work.right as f32 - margin {
            self.center.0 + distance
        } else if self.center.0 - distance - w / 2.0 >= work.left as f32 + margin {
            self.center.0 - distance
        } else if work.right as f32 - self.center.0 >= self.center.0 - work.left as f32 {
            work.right as f32 - margin - w / 2.0
        } else {
            work.left as f32 + margin + w / 2.0
        };
        let center_x = center_x.clamp(
            work.left as f32 + margin + w / 2.0,
            (work.right as f32 - margin - w / 2.0).max(work.left as f32 + margin + w / 2.0),
        );
        let center_y = self.center.1.clamp(
            work.top as f32 + margin + h / 2.0,
            (work.bottom as f32 - margin - h / 2.0).max(work.top as f32 + margin + h / 2.0),
        );
        RECT {
            left: (center_x - w / 2.0) as i32,
            top: (center_y - h / 2.0) as i32,
            right: (center_x + w / 2.0) as i32,
            bottom: (center_y + h / 2.0) as i32,
        }
    }

    fn tabs_ready(&mut self) {
        self.model.tabs = self.tabs.known();
        if self.open {
            if self.model.target_item().is_none() {
                self.model.selection = None;
            }
            self.redraw();
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
        match self.model.selected_item() {
            Some(item) if self.over_close_button(cursor_pos()) => self.close_target(&item),
            Some(_) => self.confirm(),
            None => self.close(),
        }
    }

    fn over_close_button(&self, cursor: POINT) -> bool {
        self.close_button.is_some_and(|(x, y)| {
            (cursor.x as f32 - x).hypot(cursor.y as f32 - y)
                <= render::CLOSE_HIT_RADIUS * self.style.scale
        })
    }

    fn close_target(&mut self, item: &RingItem) {
        match &item.target {
            Target::Window(window) => activate::close(HWND(window.handle as *mut c_void)),
            Target::App { front } => activate::close_app(front.pid, self.own_pid),
            Target::Tab { window, index, .. } => {
                self.tabs.close(window.handle, *index);
                if let Some(tabs) = self.model.tabs.get_mut(&window.handle) {
                    if *index < tabs.len() {
                        tabs.remove(*index);
                    }
                }
            }
        }
        if item.tab().is_none() {
            self.model.inner_tabs_window = None;
        }
        self.model.selection = None;
        self.dwell = None;
        self.close_hover = false;
        self.open_cursor = cursor_pos();
        self.has_moved = false;
        let now = Instant::now();
        self.reloads = RELOAD_DELAYS_MS
            .iter()
            .map(|ms| now + std::time::Duration::from_millis(*ms))
            .collect();
        self.redraw();
    }

    fn reload(&mut self) {
        let snapshot = enumerate::snapshot(self.own_pid);
        let tabs_window = self
            .model
            .inner_tabs_window
            .and_then(|w| self.model.inner.get(w))
            .map(|item| item.preview_window().handle);
        self.model = RingModel::build(snapshot.front.as_ref(), &snapshot.others);
        self.model.tabs = self.tabs.known();
        self.model.inner_tabs_window = tabs_window
            .and_then(|handle| {
                self.model
                    .inner
                    .iter()
                    .position(|item| item.preview_window().handle == handle)
            })
            .filter(|w| self.model.inner_window_has_tabs(*w));
        self.tabs
            .refresh(windows_of(&snapshot.front, &snapshot.others));
    }

    fn tab(&mut self, forward: bool) {
        if !self.open {
            return;
        }
        self.model.move_selection(forward);
        self.redraw();
    }

    fn confirm(&mut self) {
        let Some(item) = self.model.selected_item() else {
            self.close();
            return;
        };
        self.close();
        let window = item.preview_window().handle;
        if let Target::Tab { index, .. } = item.target {
            self.tabs.select(window, index);
        }
        activate::activate(HWND(window as *mut c_void));
    }
}

fn cursor_pos() -> POINT {
    let mut cursor = POINT::default();
    unsafe {
        let _ = GetCursorPos(&mut cursor);
    }
    cursor
}

fn windows_of(
    front: &Option<radial_core::model::AppWindows>,
    others: &[radial_core::model::AppWindows],
) -> Vec<(isize, u32)> {
    front
        .iter()
        .chain(others)
        .flat_map(|app| app.windows.iter().map(|w| (w.handle, w.pid)))
        .collect()
}

fn monitor_at(point: POINT) -> (RECT, f32) {
    unsafe {
        let monitor = MonitorFromPoint(point, MONITOR_DEFAULTTONEAREST);
        let mut info = MONITORINFO {
            cbSize: std::mem::size_of::<MONITORINFO>() as u32,
            ..Default::default()
        };
        let _ = GetMonitorInfoW(monitor, &mut info);
        let (mut dpi_x, mut dpi_y) = (96u32, 96u32);
        let _ = GetDpiForMonitor(monitor, MDT_EFFECTIVE_DPI, &mut dpi_x, &mut dpi_y);
        (info.rcWork, dpi_x as f32 / 96.0)
    }
}

unsafe extern "system" fn main_proc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match message {
        WM_RING_PRESS => with_app(|app| app.open()),
        WM_RING_RELEASE => with_app(|app| app.released()),
        WM_RING_TAB => with_app(|app| app.tab(wparam.0 == 0)),
        WM_RING_CONFIRM => with_app(|app| app.confirm()),
        WM_RING_CANCEL => with_app(|app| app.close()),
        WM_RING_CLICK => with_app(|app| app.clicked()),
        WM_TABS_READY => with_app(|app| app.tabs_ready()),
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
