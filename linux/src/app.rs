use std::sync::mpsc::{channel, RecvTimeoutError, Sender};
use std::sync::Arc;
use std::time::{Duration, Instant};

use radial_core::geometry::{Area, Geometry, Level, Selection, DEAD_ZONE, INNER, OUTER, TAB_ARC};
use radial_core::model::{Icon, RingItem, RingModel, Target};
use radial_core::render::{self, Motion, Scene, Style};
use radial_core::text::Text;
use tiny_skia::Pixmap;
use x11rb::connection::Connection;
use x11rb::protocol::randr::ConnectionExt as RandrExt;
use x11rb::protocol::xproto::{
    AtomEnum, ClientMessageEvent, ConnectionExt, CreateWindowAux, EventMask, GrabMode, KeyButMask,
    Screen, Window, WindowClass,
};
use x11rb::protocol::Event;
use x11rb::rust_connection::RustConnection;
use x11rb::CURRENT_TIME;

use crate::capture::Capturer;
use crate::hotkey::{self, Keys};
use crate::settings;
use crate::snapshots::Snapshots;
use crate::surface::Surface;
use crate::tabs::{Tabs, WindowInfo};
use crate::tray;
use crate::windows::{self, Apps};
use crate::x::{self, Atoms};

const TICK: Duration = Duration::from_millis(8);
const IDLE: Duration = Duration::from_millis(500);
const LIVE_INTERVAL: Duration = Duration::from_millis(100);
const MOVE_THRESHOLD: f32 = 4.0;
const EDGE_MARGIN: f32 = 8.0;
const RING_MARGIN: f32 = 44.0;
const PREVIEW_GAP: f32 = 16.0;
const TABS_STILLNESS: Duration = Duration::from_millis(150);

enum Message {
    X(Event),
    Tray(tray::Command),
    TabsReady,
}

#[derive(Clone, Copy)]
struct Rect {
    x: i32,
    y: i32,
    width: i32,
    height: i32,
}

struct App {
    conn: Arc<RustConnection>,
    screen: Screen,
    atoms: Atoms,
    keys: Keys,
    overlay: Surface,
    card: Surface,
    capturer: Option<Capturer>,
    snapshots: Snapshots,
    tabs: Tabs,
    apps: Apps,
    text: Text,
    model: RingModel,
    geometry: Geometry,
    style: Style,
    motion: Motion,
    targets: Vec<(String, f32)>,
    backdrop: Option<Pixmap>,
    open: bool,
    center: (f32, f32),
    work: Rect,
    open_cursor: (i32, i32),
    last_cursor: (i32, i32),
    has_moved: bool,
    last_move: Instant,
    dwell: Option<Selection>,
    last_frame: Instant,
    last_live: Instant,
    live_image: Option<(isize, Icon)>,
    time: u32,
}

pub fn run() -> Result<(), String> {
    let (conn, screen_number) =
        x11rb::connect(None).map_err(|e| format!("can't connect to the X server: {e}"))?;
    let conn = Arc::new(conn);
    let screen = conn.setup().roots[screen_number].clone();
    let root = screen.root;
    let atoms = Atoms::new(conn.as_ref())
        .map_err(|e| e.to_string())?
        .reply()
        .map_err(|e| e.to_string())?;

    let marker = conn.generate_id().map_err(|e| e.to_string())?;
    conn.create_window(
        0,
        marker,
        root,
        0,
        0,
        1,
        1,
        0,
        WindowClass::INPUT_ONLY,
        0,
        &CreateWindowAux::new(),
    )
    .map_err(|e| e.to_string())?;
    let owner = conn
        .get_selection_owner(atoms._RADIAL_SWITCHER_RUNNING)
        .map_err(|e| e.to_string())?
        .reply()
        .map_err(|e| e.to_string())?;
    if owner.owner != x11rb::NONE {
        return Err("Radial Switcher is already running".into());
    }
    conn.set_selection_owner(marker, atoms._RADIAL_SWITCHER_RUNNING, CURRENT_TIME)
        .map_err(|e| e.to_string())?;

    let keys = Keys::load(conn.as_ref()).ok_or("can't read the keyboard mapping")?;
    hotkey::enable_detectable_autorepeat(conn.as_ref());
    let label = hotkey::register(conn.as_ref(), root, &keys)
        .ok_or("Alt+Space, Ctrl+Alt+Space and Super+Alt+Space are all taken by other programs")?;
    conn.flush().map_err(|e| e.to_string())?;

    let text = settings::font_path()
        .and_then(|path| Text::from_path(&path))
        .or_else(Text::system)
        .ok_or("no usable font found (install fonts-dejavu or fonts-noto)")?;

    let (sender, receiver) = channel::<Message>();
    let _tray = tray::start(tray_sender(sender.clone()), label);
    let tabs_sender = sender.clone();
    let tabs = Tabs::start(move || {
        let _ = tabs_sender.send(Message::TabsReady);
    });
    let reader = conn.clone();
    let events = sender.clone();
    std::thread::spawn(move || {
        while let Ok(event) = reader.wait_for_event() {
            if events.send(Message::X(event)).is_err() {
                break;
            }
        }
    });
    println!("Radial Switcher is running. Hold {label} to switch windows.");

    let mut app = App {
        overlay: Surface::new(conn.as_ref(), &screen).map_err(|e| e.to_string())?,
        card: Surface::new(conn.as_ref(), &screen).map_err(|e| e.to_string())?,
        capturer: Capturer::new(conn.as_ref()),
        snapshots: Snapshots::start(),
        tabs,
        apps: Apps::load(),
        text,
        model: RingModel::default(),
        geometry: Geometry::default(),
        style: Style {
            scale: 1.0,
            dark: true,
            accent: [53, 132, 228],
        },
        motion: Motion::default(),
        targets: Vec::new(),
        backdrop: None,
        open: false,
        center: (0.0, 0.0),
        work: Rect {
            x: 0,
            y: 0,
            width: screen.width_in_pixels as i32,
            height: screen.height_in_pixels as i32,
        },
        open_cursor: (0, 0),
        last_cursor: (0, 0),
        has_moved: false,
        last_move: Instant::now(),
        dwell: None,
        last_frame: Instant::now(),
        last_live: Instant::now(),
        live_image: None,
        time: CURRENT_TIME,
        conn,
        screen,
        atoms,
        keys,
    };

    loop {
        let timeout = if app.open { TICK } else { IDLE };
        match receiver.recv_timeout(timeout) {
            Ok(Message::X(event)) => app.handle(event),
            Ok(Message::Tray(tray::Command::Quit)) => break,
            Ok(Message::TabsReady) => app.tabs_ready(),
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => break,
        }
        if app.open {
            app.tick();
        }
    }
    app.close();
    Ok(())
}

fn debug(message: impl FnOnce() -> String) {
    if std::env::var_os("RADIAL_SWITCHER_DEBUG").is_some() {
        eprintln!("[debug] {}", message());
    }
}

fn tray_sender(sender: Sender<Message>) -> Sender<tray::Command> {
    let (tray_sender, tray_receiver) = channel::<tray::Command>();
    std::thread::spawn(move || {
        while let Ok(command) = tray_receiver.recv() {
            if sender.send(Message::Tray(command)).is_err() {
                break;
            }
        }
    });
    tray_sender
}

impl App {
    fn root(&self) -> Window {
        self.screen.root
    }

    fn handle(&mut self, event: Event) {
        match event {
            Event::KeyPress(key) => {
                debug(|| {
                    format!(
                        "key press {} state {:?} open {}",
                        key.detail, key.state, self.open
                    )
                });
                self.time = key.time;
                let shift = u16::from(key.state) & u16::from(KeyButMask::SHIFT) != 0;
                if key.detail == self.keys.space {
                    if !self.open {
                        self.open_ring();
                    }
                } else if self.open && self.keys.tab.contains(&key.detail) {
                    self.model.move_selection(!shift);
                    self.redraw();
                } else if self.open && self.keys.enter.contains(&key.detail) {
                    self.confirm();
                } else if self.open && self.keys.escape.contains(&key.detail) {
                    self.close();
                }
            }
            Event::KeyRelease(key) => {
                debug(|| {
                    format!(
                        "key release {} open {} selected {:?}",
                        key.detail,
                        self.open,
                        self.model.selected_item().map(|i| i.full_title)
                    )
                });
                self.time = key.time;
                if key.detail == self.keys.space
                    && self.open
                    && self.model.selected_item().is_some()
                {
                    self.confirm();
                }
            }
            Event::ButtonPress(button) if self.open => {
                self.time = button.time;
                match button.detail {
                    1 => self.clicked(),
                    3 => self.close(),
                    _ => {}
                }
            }
            _ => {}
        }
    }

    fn cursor(&self) -> (i32, i32) {
        self.conn
            .query_pointer(self.root())
            .ok()
            .and_then(|c| c.reply().ok())
            .map(|p| (p.root_x as i32, p.root_y as i32))
            .unwrap_or(self.last_cursor)
    }

    fn monitor_at(&self, (x, y): (i32, i32)) -> Rect {
        let full = Rect {
            x: 0,
            y: 0,
            width: self.screen.width_in_pixels as i32,
            height: self.screen.height_in_pixels as i32,
        };
        let monitor = self
            .conn
            .randr_get_monitors(self.root(), true)
            .ok()
            .and_then(|c| c.reply().ok())
            .and_then(|reply| {
                reply
                    .monitors
                    .iter()
                    .map(|m| Rect {
                        x: m.x as i32,
                        y: m.y as i32,
                        width: m.width as i32,
                        height: m.height as i32,
                    })
                    .find(|m| x >= m.x && x < m.x + m.width && y >= m.y && y < m.y + m.height)
            })
            .unwrap_or(full);
        let desktop = x::values32(
            self.conn.as_ref(),
            self.root(),
            self.atoms._NET_CURRENT_DESKTOP,
            AtomEnum::CARDINAL,
        )
        .first()
        .copied()
        .unwrap_or(0) as usize;
        let areas = x::values32(
            self.conn.as_ref(),
            self.root(),
            self.atoms._NET_WORKAREA,
            AtomEnum::CARDINAL,
        );
        let Some(area) = areas.get(desktop * 4..desktop * 4 + 4) else {
            return monitor;
        };
        let left = monitor.x.max(area[0] as i32);
        let top = monitor.y.max(area[1] as i32);
        let right = (monitor.x + monitor.width).min(area[0] as i32 + area[2] as i32);
        let bottom = (monitor.y + monitor.height).min(area[1] as i32 + area[3] as i32);
        if right > left && bottom > top {
            Rect {
                x: left,
                y: top,
                width: right - left,
                height: bottom - top,
            }
        } else {
            monitor
        }
    }

    fn open_ring(&mut self) {
        let snapshot =
            windows::snapshot(self.conn.as_ref(), self.root(), &self.atoms, &mut self.apps);
        self.model = RingModel::build(snapshot.front.as_ref(), &snapshot.others);
        self.model.tabs = self.tabs.known();
        self.tabs.refresh(
            snapshot
                .front
                .iter()
                .chain(&snapshot.others)
                .flat_map(|app| &app.windows)
                .map(|w| WindowInfo {
                    handle: w.handle,
                    pid: w.pid,
                    title: w.title.clone(),
                })
                .collect(),
        );
        self.dwell = None;

        let cursor = self.cursor();
        let work = self.monitor_at(cursor);
        let scale = settings::scale(self.conn.as_ref(), self.root());
        self.work = work;
        self.style = Style {
            scale,
            dark: settings::dark_mode(),
            accent: self.style.accent,
        };

        let reach = (OUTER.1 + RING_MARGIN) * scale;
        let clamp = |value: f32, low: f32, high: f32| {
            if low <= high {
                value.clamp(low, high)
            } else {
                (low + high) / 2.0
            }
        };
        let (left, top, right, bottom) = (
            work.x as f32,
            work.y as f32,
            (work.x + work.width) as f32,
            (work.y + work.height) as f32,
        );
        self.center = (
            clamp(cursor.0 as f32, left + reach, right - reach),
            clamp(cursor.1 as f32, top + reach, bottom - reach),
        );
        self.geometry = Geometry {
            visible: Some(Area {
                min_x: (left - self.center.0) / scale + EDGE_MARGIN,
                max_x: (right - self.center.0) / scale - EDGE_MARGIN,
                min_y: (self.center.1 - bottom) / scale + EDGE_MARGIN,
                max_y: (self.center.1 - top) / scale - EDGE_MARGIN,
            }),
        };

        let size = render::frame_size(scale);
        let (x, y) = self.frame_origin(size);
        let screen_size = (self.screen.width_in_pixels, self.screen.height_in_pixels);
        self.backdrop = Capturer::region(self.conn.as_ref(), self.root(), screen_size, x, y, size)
            .map(|p| render::blur_backdrop(&p));

        self.open_cursor = cursor;
        self.last_cursor = cursor;
        self.has_moved = false;
        self.live_image = None;
        self.motion.reset();
        self.targets.clear();
        self.last_frame = Instant::now();
        self.open = true;
        self.redraw();

        let conn = self.conn.as_ref();
        let keyboard = conn
            .grab_keyboard(
                false,
                self.root(),
                self.time,
                GrabMode::ASYNC,
                GrabMode::ASYNC,
            )
            .ok()
            .and_then(|c| c.reply().ok())
            .map(|r| r.status);
        let mask = EventMask::BUTTON_PRESS | EventMask::BUTTON_RELEASE | EventMask::POINTER_MOTION;
        let pointer = conn
            .grab_pointer(
                false,
                self.overlay.window,
                mask,
                GrabMode::ASYNC,
                GrabMode::ASYNC,
                x11rb::NONE,
                x11rb::NONE,
                self.time,
            )
            .ok()
            .and_then(|c| c.reply().ok())
            .map(|r| r.status);
        debug(|| {
            format!(
                "opened at {:?}: {} inner, {} outer; keyboard grab {keyboard:?}, pointer grab {pointer:?}",
                self.center,
                self.model.inner.len(),
                self.model.outer.len()
            )
        });
        let _ = conn.flush();
    }

    fn close(&mut self) {
        if !self.open {
            return;
        }
        self.open = false;
        let conn = self.conn.as_ref();
        let _ = conn.ungrab_pointer(self.time);
        let _ = conn.ungrab_keyboard(self.time);
        self.overlay.hide(conn);
        self.card.hide(conn);
        self.model.selection = None;
        self.model.inner_tabs_window = None;
        self.dwell = None;
        self.backdrop = None;
        let _ = conn.flush();
    }

    fn frame_origin(&self, size: u32) -> (i32, i32) {
        (
            (self.center.0 - size as f32 / 2.0).round() as i32,
            (self.center.1 - size as f32 / 2.0).round() as i32,
        )
    }

    fn tick(&mut self) {
        let now = Instant::now();
        let dt = now.duration_since(self.last_frame).as_secs_f32().min(0.1);
        self.last_frame = now;

        let cursor = self.cursor();
        let mut changed = false;
        if cursor != self.last_cursor {
            self.last_cursor = cursor;
            self.last_move = now;
            let moved = ((cursor.0 - self.open_cursor.0) as f32)
                .hypot((cursor.1 - self.open_cursor.1) as f32);
            if self.has_moved || moved >= MOVE_THRESHOLD * self.style.scale {
                self.has_moved = true;
                changed |= self.update_selection(cursor);
            }
        }
        changed |= self.check_dwell(now);
        if now.duration_since(self.last_live) >= LIVE_INTERVAL {
            self.last_live = now;
            changed |= self.refresh_live();
        }
        let animating = self.motion.step(&self.targets, dt);
        if changed || animating {
            self.redraw();
        }
    }

    fn update_selection(&mut self, cursor: (i32, i32)) -> bool {
        let scale = self.style.scale;
        let dx = (cursor.0 as f32 - self.center.0) / scale;
        let dy = (self.center.1 - cursor.1 as f32) / scale;
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
        debug(|| format!("selection {selection:?}"));
        self.model.selection = selection;
        self.update_tab_mode(dx.hypot(dy) <= DEAD_ZONE);
        self.live_image = None;
        self.last_live = Instant::now() - LIVE_INTERVAL;
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
            .filter(|s| s.level == Level::Inner && self.model.inner_window_has_tabs(s.index));
    }

    fn check_dwell(&mut self, now: Instant) -> bool {
        let Some(selection) = self.dwell else {
            return false;
        };
        if self.model.selection != Some(selection) {
            self.dwell = None;
            return false;
        }
        if now.duration_since(self.last_move) < TABS_STILLNESS {
            return false;
        }
        self.dwell = None;
        self.model.inner_tabs_window = Some(selection.index);
        debug(|| format!("tab pop-up for inner window {}", selection.index));
        true
    }

    fn tabs_ready(&mut self) {
        self.model.tabs = self.tabs.known();
        debug(|| format!("tabs for {} windows", self.model.tabs.len()));
        if !self.open {
            return;
        }
        if self.model.target_item().is_none() {
            self.model.selection = None;
        }
        if self.dwell.is_none() && self.model.inner_tabs_window.is_none() {
            self.update_tab_mode(false);
        }
        self.redraw();
    }

    fn refresh_live(&mut self) -> bool {
        let Some(item) = self.model.selected_item() else {
            return false;
        };
        if item.tab().is_some_and(|tab| !tab.selected) {
            return false;
        }
        let handle = item.preview_window().handle;
        let Some(capturer) = &self.capturer else {
            return false;
        };
        let width = (render::CARD_IMAGE.0 * self.style.scale) as u16;
        let Some(image) = capturer.window(self.conn.as_ref(), self.root(), handle as Window, width)
        else {
            return false;
        };
        let image = Arc::new(image);
        let title = x::text(self.conn.as_ref(), handle as Window, &self.atoms);
        self.snapshots.update(handle, title, image.clone());
        self.live_image = Some((handle, image));
        true
    }

    fn thumbnail(&self, item: &RingItem) -> Option<Icon> {
        let handle = item.preview_window().handle;
        match &item.target {
            Target::Tab { tab, .. } => self.snapshots.tab(handle, &tab.title).or_else(|| {
                if tab.selected {
                    self.snapshots.window(handle)
                } else {
                    None
                }
            }),
            _ => self.snapshots.window(handle),
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
        });
        let (x, y) = self.frame_origin(frame.pixmap.width());
        self.overlay.show(self.conn.as_ref(), &frame.pixmap, x, y);
        self.targets = frame.targets;
        self.draw_card();
    }

    fn draw_card(&mut self) {
        let Some(item) = self.model.selected_item() else {
            self.card.hide(self.conn.as_ref());
            return;
        };
        let handle = item.preview_window().handle;
        let background_tab = item.tab().is_some_and(|tab| !tab.selected);
        let image = match &self.live_image {
            Some((h, image)) if *h == handle && !background_tab => Some(image.clone()),
            _ => self.thumbnail(&item),
        };
        let card = render::render_card(
            &self.text,
            self.style,
            &item.full_title,
            image.as_deref(),
            item.icon.as_deref(),
        );
        let (x, y) = self.card_origin(card.width() as f32, card.height() as f32);
        self.card.show(self.conn.as_ref(), &card, x, y);
    }

    fn card_origin(&self, w: f32, h: f32) -> (i32, i32) {
        let scale = self.style.scale;
        let distance = (TAB_ARC.1 + PREVIEW_GAP) * scale + w / 2.0;
        let margin = EDGE_MARGIN * scale;
        let (left, right) = (self.work.x as f32, (self.work.x + self.work.width) as f32);
        let (top, bottom) = (self.work.y as f32, (self.work.y + self.work.height) as f32);
        let x = if self.center.0 + distance + w / 2.0 <= right - margin {
            self.center.0 + distance
        } else if self.center.0 - distance - w / 2.0 >= left + margin {
            self.center.0 - distance
        } else if right - self.center.0 >= self.center.0 - left {
            right - margin - w / 2.0
        } else {
            left + margin + w / 2.0
        };
        let y = self.center.1.clamp(
            top + margin + h / 2.0,
            (bottom - margin - h / 2.0).max(top + margin + h / 2.0),
        );
        ((x - w / 2.0) as i32, (y - h / 2.0) as i32)
    }

    fn clicked(&mut self) {
        if self.has_moved {
            let cursor = self.cursor();
            self.update_selection(cursor);
        }
        if self.model.selected_item().is_some() {
            self.confirm();
        } else {
            self.close();
        }
    }

    fn confirm(&mut self) {
        let Some(item) = self.model.selected_item() else {
            self.close();
            return;
        };
        self.close();
        let window = item.preview_window().handle as Window;
        if let Target::Tab { index, .. } = item.target {
            self.tabs.select(window as isize, index);
        }
        let event = ClientMessageEvent::new(
            32,
            window,
            self.atoms._NET_ACTIVE_WINDOW,
            [2, self.time, 0, 0, 0],
        );
        let conn = self.conn.as_ref();
        let sent = conn
            .send_event(
                false,
                self.root(),
                EventMask::SUBSTRUCTURE_REDIRECT | EventMask::SUBSTRUCTURE_NOTIFY,
                event,
            )
            .map(|c| c.check());
        debug(|| {
            format!(
                "activate {} (0x{window:x}) at time {}: {sent:?}",
                item.full_title, self.time
            )
        });
        let _ = conn.flush();
    }
}
