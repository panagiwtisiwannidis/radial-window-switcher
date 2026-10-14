use std::sync::mpsc::{channel, RecvTimeoutError, Sender};
use std::sync::Arc;
use std::time::{Duration, Instant};

use radial_core::model::{Icon, RingItem, RingModel, Target};
use radial_core::render;
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
use crate::ring::{debug, Rect, Ring};
use crate::settings;
use crate::snapshots::Snapshots;
use crate::surface::Surface;
use crate::tabs::{Tabs, WindowInfo};
use crate::tray;
use crate::windows::{self, Apps, Snapshot};
use crate::x::{self, Atoms};

const TICK: Duration = Duration::from_millis(8);
const IDLE: Duration = Duration::from_millis(500);
const LIVE_INTERVAL: Duration = Duration::from_millis(100);

enum Message {
    X(Event),
    Tray(tray::Command),
    TabsReady,
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
    ring: Ring,
    backdrop: Option<Pixmap>,
    open: bool,
    last_cursor: (i32, i32),
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
    let _tray = tray::start(
        tray_sender(sender.clone()),
        &format!("Hold {label} to switch windows"),
    );
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
        ring: Ring::new([53, 132, 228]),
        backdrop: None,
        open: false,
        last_cursor: (0, 0),
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

fn window_infos(snapshot: &Snapshot) -> Vec<WindowInfo> {
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
        .collect()
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
                    self.ring.model.move_selection(!shift);
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
                        self.ring.model.selected_item().map(|i| i.full_title)
                    )
                });
                self.time = key.time;
                if key.detail == self.keys.space
                    && self.open
                    && self.ring.model.selected_item().is_some()
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
        let mut model = RingModel::build(snapshot.front.as_ref(), &snapshot.others);
        model.tabs = self.tabs.known();
        self.tabs.refresh(window_infos(&snapshot));

        let cursor = self.cursor();
        let work = self.monitor_at(cursor);
        let scale = settings::scale(self.conn.as_ref(), self.root());
        self.ring
            .begin(model, cursor, work, scale, settings::dark_mode());

        let size = render::frame_size(scale);
        let (x, y) = self.ring.frame_origin(size);
        let screen_size = (self.screen.width_in_pixels, self.screen.height_in_pixels);
        self.backdrop = Capturer::region(self.conn.as_ref(), self.root(), screen_size, x, y, size)
            .map(|p| render::blur_backdrop(&p));

        self.last_cursor = cursor;
        self.live_image = None;
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
                self.ring.center,
                self.ring.model.inner.len(),
                self.ring.model.outer.len()
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
        self.ring.end();
        self.backdrop = None;
        let _ = conn.flush();
    }

    fn tick(&mut self) {
        let cursor = self.cursor();
        self.last_cursor = cursor;
        let tick = self.ring.tick(cursor);
        let mut changed = tick.changed;
        if tick.reload {
            self.reload();
            changed = true;
        }
        if self.ring.take_selection_changed() {
            self.live_image = None;
            self.last_live = Instant::now() - LIVE_INTERVAL;
        }
        let now = Instant::now();
        if now.duration_since(self.last_live) >= LIVE_INTERVAL {
            self.last_live = now;
            changed |= self.refresh_live();
        }
        let animating = self.ring.animate();
        if changed || animating {
            self.redraw();
        }
    }

    fn tabs_ready(&mut self) {
        let tabs = self.tabs.known();
        debug(|| format!("tabs for {} windows", tabs.len()));
        if !self.open {
            self.ring.model.tabs = tabs;
            return;
        }
        self.ring.tabs_updated(tabs);
        self.redraw();
    }

    fn refresh_live(&mut self) -> bool {
        let Some(item) = self.ring.model.selected_item() else {
            return false;
        };
        if item.tab().is_some_and(|tab| !tab.selected) {
            return false;
        }
        let handle = item.preview_window().handle;
        let Some(capturer) = &self.capturer else {
            return false;
        };
        let width = (render::CARD_IMAGE.0 * self.ring.style.scale) as u16;
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

    fn redraw(&mut self) {
        let snapshots = &self.snapshots;
        let thumbnail = |item: &RingItem| thumbnail_from(snapshots, item);
        let (frame, (x, y)) = self
            .ring
            .render(&self.text, &thumbnail, self.backdrop.as_ref());
        self.overlay.show(self.conn.as_ref(), &frame.pixmap, x, y);
        self.draw_card();
    }

    fn draw_card(&mut self) {
        let Some(item) = self.ring.model.selected_item() else {
            self.card.hide(self.conn.as_ref());
            return;
        };
        let handle = item.preview_window().handle;
        let background_tab = item.tab().is_some_and(|tab| !tab.selected);
        let image = match &self.live_image {
            Some((h, image)) if *h == handle && !background_tab => Some(image.clone()),
            _ => thumbnail_from(&self.snapshots, &item),
        };
        let card = render::render_card(
            &self.text,
            self.ring.style,
            &item.full_title,
            image.as_deref(),
            item.icon.as_deref(),
        );
        let (x, y) = self
            .ring
            .card_origin(card.width() as f32, card.height() as f32);
        self.card.show(self.conn.as_ref(), &card, x, y);
    }

    fn clicked(&mut self) {
        let cursor = self.cursor();
        if self.ring.has_moved() {
            self.ring.update_selection(cursor, true);
        }
        match self.ring.model.selected_item() {
            Some(item) if self.ring.over_close_button(cursor) => self.close_target(&item),
            Some(_) => self.confirm(),
            None => self.close(),
        }
    }

    fn close_window(&self, window: Window) {
        let event = ClientMessageEvent::new(
            32,
            window,
            self.atoms._NET_CLOSE_WINDOW,
            [self.time, 2, 0, 0, 0],
        );
        let _ = self.conn.send_event(
            false,
            self.root(),
            EventMask::SUBSTRUCTURE_REDIRECT | EventMask::SUBSTRUCTURE_NOTIFY,
            event,
        );
    }

    fn close_target(&mut self, item: &RingItem) {
        debug(|| format!("close {}", item.full_title));
        match &item.target {
            Target::Window(window) => self.close_window(window.handle as Window),
            Target::App { front } => {
                let snapshot =
                    windows::snapshot(self.conn.as_ref(), self.root(), &self.atoms, &mut self.apps);
                for window in snapshot
                    .front
                    .iter()
                    .chain(&snapshot.others)
                    .flat_map(|app| &app.windows)
                    .filter(|w| w.pid == front.pid)
                {
                    self.close_window(window.handle as Window);
                }
            }
            Target::Tab { window, index, .. } => {
                self.tabs.close(window.handle, *index);
                if let Some(tabs) = self.ring.model.tabs.get_mut(&window.handle) {
                    if *index < tabs.len() {
                        tabs.remove(*index);
                    }
                }
            }
        }
        let _ = self.conn.flush();
        self.live_image = None;
        let cursor = self.cursor();
        self.ring.closed_target(item, cursor);
        self.redraw();
    }

    fn reload(&mut self) {
        let snapshot =
            windows::snapshot(self.conn.as_ref(), self.root(), &self.atoms, &mut self.apps);
        self.ring
            .rebuild(RingModel::build(snapshot.front.as_ref(), &snapshot.others));
        self.ring.model.tabs = self.tabs.known();
        self.tabs.refresh(window_infos(&snapshot));
    }

    fn confirm(&mut self) {
        let Some(item) = self.ring.model.selected_item() else {
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

fn thumbnail_from(snapshots: &Snapshots, item: &RingItem) -> Option<Icon> {
    let handle = item.preview_window().handle;
    match &item.target {
        Target::Tab { tab, .. } => snapshots.tab(handle, &tab.title).or_else(|| {
            if tab.selected {
                snapshots.window(handle)
            } else {
                None
            }
        }),
        _ => snapshots.window(handle),
    }
}
