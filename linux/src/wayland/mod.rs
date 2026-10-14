mod control;
mod ipc;
mod keymap;
mod toplevels;

use std::collections::{HashMap, HashSet};
use std::sync::mpsc;
use std::sync::Arc;
use std::time::{Duration, Instant};

use radial_core::model::{AppWindows, Icon, RingItem, RingModel, Target, WindowRef};
use radial_core::render;
use radial_core::text::Text;
use smithay_client_toolkit::compositor::{CompositorHandler, CompositorState, Region};
use smithay_client_toolkit::dispatch2::Dispatch2;
use smithay_client_toolkit::output::{OutputHandler, OutputState};
use smithay_client_toolkit::reexports::calloop::channel::{self, Sender};
use smithay_client_toolkit::reexports::calloop::timer::{TimeoutAction, Timer};
use smithay_client_toolkit::reexports::calloop::{EventLoop, LoopHandle};
use smithay_client_toolkit::reexports::calloop_wayland_source::WaylandSource;
use smithay_client_toolkit::registry::{ProvidesRegistryState, RegistryState};
use smithay_client_toolkit::seat::pointer::{PointerEvent, PointerEventKind, PointerHandler};
use smithay_client_toolkit::seat::{Capability, SeatHandler, SeatState};
use smithay_client_toolkit::shell::wlr_layer::{
    Anchor, KeyboardInteractivity, Layer, LayerShell, LayerShellHandler, LayerSurface,
    LayerSurfaceConfigure,
};
use smithay_client_toolkit::shell::WaylandSurface;
use smithay_client_toolkit::shm::slot::SlotPool;
use smithay_client_toolkit::shm::{Shm, ShmHandler};
use smithay_client_toolkit::subcompositor::SubcompositorState;
use smithay_client_toolkit::{delegate_registry, registry_handlers};
use tiny_skia::Pixmap;
use wayland_client::globals::registry_queue_init;
use wayland_client::protocol::{
    wl_keyboard, wl_output, wl_pointer, wl_seat, wl_shm, wl_subsurface, wl_surface,
};
use wayland_client::{Connection, QueueHandle, WEnum};
use wayland_protocols_wlr::foreign_toplevel::v1::client::zwlr_foreign_toplevel_manager_v1::ZwlrForeignToplevelManagerV1;

use crate::desktop::DesktopApps;
use crate::ring::{debug, Rect, Ring};
use crate::settings;
use crate::tabs::{Tabs, WindowInfo};
use crate::tray;

pub use control::send;
use keymap::{Key, Keymap};
use toplevels::{ManagerData, Toplevels};

const TICK: Duration = Duration::from_millis(8);
const POINTER_WAIT: Duration = Duration::from_millis(150);
const BTN_LEFT: u32 = 0x110;
const BTN_RIGHT: u32 = 0x111;

enum Message {
    Show,
    Quit,
    TabsReady,
}

struct Listed {
    id: isize,
    app_id: String,
    title: String,
    minimized: bool,
    activated: bool,
    pid: u32,
    rank: i64,
    recency: u64,
}

struct Layered {
    surface: wl_surface::WlSurface,
    subsurface: wl_subsurface::WlSubsurface,
    visible: bool,
}

struct Overlay {
    layer: LayerSurface,
    ring: Layered,
    card: Layered,
    size: Option<(u32, u32)>,
    opened: Instant,
}

pub struct State {
    registry_state: RegistryState,
    seat_state: SeatState,
    output_state: OutputState,
    compositor: CompositorState,
    subcompositor: SubcompositorState,
    layer_shell: LayerShell,
    shm: Shm,
    pool: SlotPool,
    pub toplevels: Toplevels,
    seat: Option<wl_seat::WlSeat>,
    pointer: Option<wl_pointer::WlPointer>,
    keyboard: Option<wl_keyboard::WlKeyboard>,
    overlay: Option<Overlay>,
    ring: Ring,
    text: Text,
    desktop: DesktopApps,
    icons: HashMap<String, Option<Icon>>,
    tabs: Tabs,
    started: bool,
    cursor: Option<(f64, f64)>,
    scale: i32,
    shift: bool,
    keymap: Keymap,
    held: HashSet<u32>,
    pressed: HashSet<u32>,
    pending: Option<RingModel>,
    qh: QueueHandle<State>,
    loop_handle: LoopHandle<'static, State>,
    exit: bool,
}

pub fn run() -> Result<(), String> {
    let conn = Connection::connect_to_env()
        .map_err(|e| format!("can't connect to the Wayland compositor: {e}"))?;
    let (globals, event_queue) = registry_queue_init(&conn).map_err(|e| e.to_string())?;
    let qh = event_queue.handle();
    let mut event_loop: EventLoop<State> = EventLoop::try_new().map_err(|e| e.to_string())?;
    let loop_handle = event_loop.handle();
    WaylandSource::new(conn.clone(), event_queue)
        .insert(loop_handle.clone())
        .map_err(|e| e.to_string())?;

    let unsupported = |what: &str| {
        format!(
            "{} doesn't support {what}, which Radial Switcher needs on Wayland. Supported: Hyprland, Sway, \
             river, Wayfire, labwc, niri and other wlroots-based compositors. On GNOME or KDE, use an X11 session.",
            ipc::compositor()
        )
    };
    let compositor = CompositorState::bind(&globals, &qh).map_err(|e| e.to_string())?;
    let subcompositor = SubcompositorState::bind(compositor.wl_compositor().clone(), &globals, &qh)
        .map_err(|e| e.to_string())?;
    let layer_shell =
        LayerShell::bind(&globals, &qh).map_err(|_| unsupported("wlr-layer-shell"))?;
    let shm = Shm::bind(&globals, &qh).map_err(|e| e.to_string())?;
    let _manager: ZwlrForeignToplevelManagerV1 = globals
        .bind(&qh, 1..=3, ManagerData)
        .map_err(|_| unsupported("wlr-foreign-toplevel-management"))?;
    let pool = SlotPool::new(4 * 1024 * 1024, &shm).map_err(|e| e.to_string())?;

    let (sender, messages) = channel::channel::<Message>();
    loop_handle
        .insert_source(messages, |event, _, state: &mut State| {
            if let channel::Event::Msg(message) = event {
                match message {
                    Message::Show => state.show(),
                    Message::Quit => state.exit = true,
                    Message::TabsReady => state.tabs_ready(),
                }
            }
        })
        .map_err(|e| e.to_string())?;
    let show = sender.clone();
    control::listen(move |control::Request::Show| {
        let _ = show.send(Message::Show);
    })?;
    let _tray = tray::start(
        tray_bridge(sender.clone()),
        &format!(
            "Bind a key in {} to `radial-switcher show`",
            ipc::compositor()
        ),
    );
    let tabs_sender = sender.clone();
    let tabs = Tabs::start(move || {
        let _ = tabs_sender.send(Message::TabsReady);
    });

    let text = settings::font_path()
        .and_then(|path| Text::from_path(&path))
        .or_else(Text::system)
        .ok_or("no usable font found (install fonts-dejavu or fonts-noto)")?;

    let mut state = State {
        registry_state: RegistryState::new(&globals),
        seat_state: SeatState::new(&globals, &qh),
        output_state: OutputState::new(&globals, &qh),
        compositor,
        subcompositor,
        layer_shell,
        shm,
        pool,
        toplevels: Toplevels::default(),
        seat: None,
        pointer: None,
        keyboard: None,
        overlay: None,
        ring: Ring::new([53, 132, 228]),
        text,
        desktop: DesktopApps::load(),
        icons: HashMap::new(),
        tabs,
        started: false,
        cursor: None,
        scale: 1,
        shift: false,
        keymap: Keymap::default(),
        held: HashSet::new(),
        pressed: HashSet::new(),
        pending: None,
        qh,
        loop_handle,
        exit: false,
    };
    println!(
        "Radial Switcher is running on {}. Bind a key to `radial-switcher show` (see the README).",
        ipc::compositor()
    );
    while !state.exit {
        event_loop
            .dispatch(None, &mut state)
            .map_err(|e| e.to_string())?;
    }
    state.close();
    Ok(())
}

fn tray_bridge(sender: Sender<Message>) -> mpsc::Sender<tray::Command> {
    let (tray_sender, tray_receiver) = mpsc::channel::<tray::Command>();
    std::thread::spawn(move || {
        while let Ok(tray::Command::Quit) = tray_receiver.recv() {
            if sender.send(Message::Quit).is_err() {
                break;
            }
        }
    });
    tray_sender
}

fn to_argb(pixmap: &Pixmap, out: &mut [u8]) {
    for (src, dst) in pixmap
        .data()
        .as_chunks::<4>()
        .0
        .iter()
        .zip(out.as_chunks_mut::<4>().0.iter_mut())
    {
        dst[0] = src[2];
        dst[1] = src[1];
        dst[2] = src[0];
        dst[3] = src[3];
    }
}

impl State {
    fn is_open(&self) -> bool {
        self.overlay.is_some()
    }

    fn output_scale(&self) -> i32 {
        self.output_state
            .outputs()
            .filter_map(|o| self.output_state.info(&o))
            .map(|info| info.scale_factor)
            .max()
            .unwrap_or(1)
            .max(1)
    }

    fn build_model(&mut self) -> (RingModel, Vec<WindowInfo>) {
        let clients = ipc::clients();
        let mut list: Vec<Listed> = self
            .toplevels
            .list
            .iter()
            .filter(|t| t.ready)
            .map(|t| {
                let client = ipc::find(&clients, &t.app_id, &t.title);
                Listed {
                    id: t.id,
                    app_id: t.app_id.clone(),
                    title: t.title.clone(),
                    minimized: t.minimized,
                    activated: t.activated,
                    pid: client.map_or(0, |c| c.pid),
                    rank: client.and_then(|c| c.rank).unwrap_or(i64::MAX),
                    recency: t.recency,
                }
            })
            .collect();
        list.sort_by(|a, b| a.rank.cmp(&b.rank).then(b.recency.cmp(&a.recency)));
        let front_id = list
            .iter()
            .find(|t| t.activated)
            .or(list.first())
            .map(|t| t.id);

        let mut order: Vec<String> = Vec::new();
        let mut groups: HashMap<String, AppWindows> = HashMap::new();
        let mut front_key = None;
        let mut infos = Vec::new();
        for Listed {
            id,
            app_id,
            title,
            minimized,
            pid,
            ..
        } in list
        {
            let key = app_id.to_lowercase();
            if Some(id) == front_id {
                front_key = Some(key.clone());
            }
            infos.push(WindowInfo {
                handle: id,
                pid,
                title: title.clone(),
            });
            if !groups.contains_key(&key) {
                order.push(key.clone());
                let desktop = &self.desktop;
                let icon = self
                    .icons
                    .entry(key.clone())
                    .or_insert_with(|| desktop.icon(&[&app_id]).map(Arc::new))
                    .clone();
                let name = desktop.name(&[&app_id]).unwrap_or_else(|| {
                    if app_id.is_empty() {
                        "Window".into()
                    } else {
                        app_id.clone()
                    }
                });
                groups.insert(
                    key.clone(),
                    AppWindows {
                        name,
                        icon,
                        windows: Vec::new(),
                    },
                );
            }
            if let Some(group) = groups.get_mut(&key) {
                group.windows.push(WindowRef {
                    handle: id,
                    pid,
                    title,
                    minimized,
                });
            }
        }
        let front = front_key.as_ref().and_then(|k| groups.remove(k));
        let others: Vec<AppWindows> = order
            .into_iter()
            .filter_map(|k| groups.remove(&k))
            .collect();
        (RingModel::build(front.as_ref(), &others), infos)
    }

    fn show(&mut self) {
        if self.is_open() {
            return;
        }
        let (mut model, infos) = self.build_model();
        model.tabs = self.tabs.known();
        self.tabs.refresh(infos);
        self.pending = Some(model);
        self.scale = self.output_scale();
        self.started = false;
        self.cursor = None;

        let surface = self.compositor.create_surface(&self.qh);
        let layer = self.layer_shell.create_layer_surface(
            &self.qh,
            surface,
            Layer::Overlay,
            Some("radial-switcher"),
            None,
        );
        layer.set_anchor(Anchor::TOP | Anchor::BOTTOM | Anchor::LEFT | Anchor::RIGHT);
        layer.set_exclusive_zone(-1);
        layer.set_keyboard_interactivity(KeyboardInteractivity::Exclusive);
        layer.set_size(0, 0);
        let parent = layer.wl_surface().clone();
        let make = |state: &State| {
            let (subsurface, surface) = state
                .subcompositor
                .create_subsurface(parent.clone(), &state.qh);
            subsurface.set_desync();
            if let Ok(region) = Region::new(&state.compositor) {
                surface.set_input_region(Some(region.wl_region()));
            }
            Layered {
                surface,
                subsurface,
                visible: false,
            }
        };
        let ring = make(self);
        let card = make(self);
        layer.commit();
        self.overlay = Some(Overlay {
            layer,
            ring,
            card,
            size: None,
            opened: Instant::now(),
        });
        let _ = self
            .loop_handle
            .insert_source(Timer::immediate(), |_, _, state| {
                state.tick();
                if state.is_open() {
                    TimeoutAction::ToDuration(TICK)
                } else {
                    TimeoutAction::Drop
                }
            });
        debug(|| "wayland: overlay requested".into());
    }

    fn close(&mut self) {
        let Some(overlay) = self.overlay.take() else {
            return;
        };
        overlay.ring.subsurface.destroy();
        overlay.ring.surface.destroy();
        overlay.card.subsurface.destroy();
        overlay.card.surface.destroy();
        drop(overlay.layer);
        self.ring.end();
        debug(|| "wayland: overlay closed".into());
        self.started = false;
        self.pending = None;
    }

    fn cursor_px(&self) -> Option<(i32, i32)> {
        let scale = self.scale as f64;
        self.cursor
            .map(|(x, y)| ((x * scale).round() as i32, (y * scale).round() as i32))
    }

    fn start(&mut self) {
        let Some(overlay) = &self.overlay else { return };
        let Some((w, h)) = overlay.size else { return };
        let Some(model) = self.pending.take() else {
            return;
        };
        let scale = self.scale;
        let work = Rect {
            x: 0,
            y: 0,
            width: w as i32 * scale,
            height: h as i32 * scale,
        };
        let cursor = self
            .cursor_px()
            .unwrap_or((work.width / 2, work.height / 2));
        self.ring
            .begin(model, cursor, work, scale as f32, settings::dark_mode());
        self.started = true;
        debug(|| {
            format!(
                "wayland: opened at {:?} ({}x{} @{scale}): {} inner, {} outer",
                self.ring.center,
                w,
                h,
                self.ring.model.inner.len(),
                self.ring.model.outer.len()
            )
        });
        self.redraw();
    }

    fn tick(&mut self) {
        let Some(overlay) = &self.overlay else { return };
        if !self.started {
            if overlay.size.is_some()
                && (self.cursor.is_some() || overlay.opened.elapsed() >= POINTER_WAIT)
            {
                self.start();
            }
            return;
        }
        let Some(cursor) = self.cursor_px() else {
            let animating = self.ring.animate();
            if animating {
                self.redraw();
            }
            return;
        };
        let tick = self.ring.tick(cursor);
        let mut changed = tick.changed;
        if tick.reload {
            self.reload();
            changed = true;
        }
        self.ring.take_selection_changed();
        let animating = self.ring.animate();
        if changed || animating {
            self.redraw();
        }
    }

    fn attach(&mut self, which: fn(&Overlay) -> &Layered, pixmap: &Pixmap, x: i32, y: i32) {
        let scale = self.scale;
        let (w, h) = (pixmap.width() as i32, pixmap.height() as i32);
        let Ok((buffer, canvas)) = self
            .pool
            .create_buffer(w, h, w * 4, wl_shm::Format::Argb8888)
        else {
            return;
        };
        to_argb(pixmap, canvas);
        let Some(overlay) = &self.overlay else { return };
        let layered = which(overlay);
        layered.surface.set_buffer_scale(scale);
        if buffer.attach_to(&layered.surface).is_err() {
            return;
        }
        layered.surface.damage_buffer(0, 0, w, h);
        layered.subsurface.set_position(x / scale, y / scale);
        layered.surface.commit();
    }

    fn redraw(&mut self) {
        if !self.started || self.overlay.is_none() {
            return;
        }
        let (frame, (x, y)) = self.ring.render(&self.text, &|_: &RingItem| None, None);
        self.attach(|o| &o.ring, &frame.pixmap, x, y);
        match self.ring.model.selected_item() {
            Some(item) => {
                let card = render::render_card(
                    &self.text,
                    self.ring.style,
                    &item.full_title,
                    None,
                    item.icon.as_deref(),
                );
                let (cx, cy) = self
                    .ring
                    .card_origin(card.width() as f32, card.height() as f32);
                self.attach(|o| &o.card, &card, cx, cy);
                if let Some(overlay) = &mut self.overlay {
                    overlay.card.visible = true;
                }
            }
            None => {
                if let Some(overlay) = &mut self.overlay {
                    if overlay.card.visible {
                        overlay.card.surface.attach(None, 0, 0);
                        overlay.card.surface.commit();
                        overlay.card.visible = false;
                    }
                }
            }
        }
        if let Some(overlay) = &mut self.overlay {
            overlay.ring.visible = true;
            overlay.layer.commit();
        }
    }

    fn tabs_ready(&mut self) {
        let tabs = self.tabs.known();
        debug(|| format!("tabs for {} windows", tabs.len()));
        if !self.started {
            if let Some(model) = &mut self.pending {
                model.tabs = tabs;
            } else {
                self.ring.model.tabs = tabs;
            }
            return;
        }
        self.ring.tabs_updated(tabs);
        self.redraw();
    }

    fn reload(&mut self) {
        let (model, infos) = self.build_model();
        self.ring.rebuild(model);
        self.ring.model.tabs = self.tabs.known();
        self.tabs.refresh(infos);
    }

    fn clicked(&mut self) {
        if !self.started {
            return;
        }
        let Some(cursor) = self.cursor_px() else {
            return;
        };
        if self.ring.has_moved() {
            self.ring.update_selection(cursor, true);
        }
        match self.ring.model.selected_item() {
            Some(item) if self.ring.over_close_button(cursor) => self.close_target(&item),
            Some(_) => self.confirm(),
            None => self.close(),
        }
    }

    fn confirm(&mut self) {
        let item = self.ring.model.selected_item();
        self.close();
        let Some(item) = item else { return };
        let window = item.preview_window().handle;
        if let Target::Tab { index, .. } = item.target {
            self.tabs.select(window, index);
        }
        let (Some(toplevel), Some(seat)) = (self.toplevels.by_id(window), &self.seat) else {
            return;
        };
        toplevel.handle.activate(seat);
        debug(|| format!("wayland: activate {}", item.full_title));
    }

    fn close_target(&mut self, item: &RingItem) {
        debug(|| format!("close {}", item.full_title));
        match &item.target {
            Target::Window(window) => {
                if let Some(toplevel) = self.toplevels.by_id(window.handle) {
                    toplevel.handle.close();
                }
            }
            Target::App { front } => {
                let app_id = self
                    .toplevels
                    .by_id(front.handle)
                    .map(|t| t.app_id.to_lowercase());
                for toplevel in &self.toplevels.list {
                    if Some(toplevel.app_id.to_lowercase()) == app_id {
                        toplevel.handle.close();
                    }
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
        if let Some(cursor) = self.cursor_px() {
            self.ring.closed_target(item, cursor);
        }
        self.redraw();
    }

    fn key(&mut self, code: u32, pressed: bool) {
        let key = self.keymap.key(code);
        let press_seen = if pressed {
            self.pressed.insert(code);
            true
        } else {
            self.pressed.remove(&code) || self.held.remove(&code)
        };
        debug(|| {
            format!("wayland: key {code} {key:?} pressed {pressed} (press seen {press_seen})")
        });
        let acts = pressed || !press_seen;
        match key {
            Some(Key::Shift) => self.shift = pressed,
            _ if !self.started => {}
            Some(Key::Space) if !pressed => {
                if self.ring.model.selected_item().is_some() {
                    self.confirm();
                }
            }
            Some(Key::Tab) if acts => {
                self.ring.model.move_selection(!self.shift);
                self.redraw();
            }
            Some(Key::Enter) if acts => self.confirm(),
            Some(Key::Escape) if acts => self.close(),
            _ => {}
        }
    }
}

pub struct KeyboardData;

impl Dispatch2<wl_keyboard::WlKeyboard, State> for KeyboardData {
    fn event(
        &self,
        state: &mut State,
        _: &wl_keyboard::WlKeyboard,
        event: wl_keyboard::Event,
        _: &Connection,
        _: &QueueHandle<State>,
    ) {
        match event {
            wl_keyboard::Event::Keymap { fd, size, .. } => {
                let file = std::fs::File::from(fd);
                let mut data = vec![0u8; size as usize];
                if std::os::unix::fs::FileExt::read_exact_at(&file, &mut data, 0).is_ok() {
                    let text = String::from_utf8_lossy(&data);
                    let text = text.trim_end_matches('\0');
                    match Keymap::parse(text) {
                        Some(map) => state.keymap = map,
                        None => {
                            debug(|| "wayland: keymap not understood, using evdev codes".into())
                        }
                    }
                }
            }
            wl_keyboard::Event::Enter { keys, .. } => {
                state.held = keys
                    .as_chunks::<4>()
                    .0
                    .iter()
                    .map(|c| u32::from_ne_bytes(*c))
                    .collect();
                state.pressed.clear();
                state.shift = state
                    .held
                    .iter()
                    .any(|code| state.keymap.key(*code) == Some(Key::Shift));
            }
            wl_keyboard::Event::Key {
                key,
                state: WEnum::Value(key_state),
                ..
            } => {
                state.key(key, key_state == wl_keyboard::KeyState::Pressed);
            }
            _ => {}
        }
    }
}

impl CompositorHandler for State {
    fn scale_factor_changed(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_surface::WlSurface,
        _: i32,
    ) {
    }

    fn transform_changed(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_surface::WlSurface,
        _: wl_output::Transform,
    ) {
    }

    fn frame(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_surface::WlSurface, _: u32) {}

    fn surface_enter(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_surface::WlSurface,
        _: &wl_output::WlOutput,
    ) {
    }

    fn surface_leave(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_surface::WlSurface,
        _: &wl_output::WlOutput,
    ) {
    }
}

impl OutputHandler for State {
    fn output_state(&mut self) -> &mut OutputState {
        &mut self.output_state
    }

    fn new_output(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}

    fn update_output(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}

    fn output_destroyed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}
}

impl LayerShellHandler for State {
    fn closed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &LayerSurface) {
        self.close();
    }

    fn configure(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        layer: &LayerSurface,
        configure: LayerSurfaceConfigure,
        _: u32,
    ) {
        let scale = self.scale;
        let Some(overlay) = &mut self.overlay else {
            return;
        };
        if &overlay.layer != layer || overlay.size.is_some() {
            return;
        }
        let (w, h) = configure.new_size;
        let (w, h) = (w.max(1), h.max(1));
        overlay.size = Some((w, h));
        let (bw, bh) = (w as i32 * scale, h as i32 * scale);
        let Ok((buffer, canvas)) =
            self.pool
                .create_buffer(bw, bh, bw * 4, wl_shm::Format::Argb8888)
        else {
            return;
        };
        canvas.fill(0);
        let surface = overlay.layer.wl_surface();
        surface.set_buffer_scale(scale);
        if buffer.attach_to(surface).is_ok() {
            surface.damage_buffer(0, 0, bw, bh);
        }
        overlay.layer.commit();
        debug(|| format!("wayland: overlay configured {w}x{h}"));
        if self.cursor.is_some() {
            self.start();
        }
    }
}

impl SeatHandler for State {
    fn seat_state(&mut self) -> &mut SeatState {
        &mut self.seat_state
    }

    fn new_seat(&mut self, _: &Connection, _: &QueueHandle<Self>, seat: wl_seat::WlSeat) {
        self.seat.get_or_insert(seat);
    }

    fn new_capability(
        &mut self,
        _: &Connection,
        qh: &QueueHandle<Self>,
        seat: wl_seat::WlSeat,
        capability: Capability,
    ) {
        self.seat.get_or_insert(seat.clone());
        if capability == Capability::Pointer && self.pointer.is_none() {
            self.pointer = self.seat_state.get_pointer(qh, &seat).ok();
        }
        if capability == Capability::Keyboard && self.keyboard.is_none() {
            self.keyboard = Some(seat.get_keyboard(qh, KeyboardData));
        }
    }

    fn remove_capability(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: wl_seat::WlSeat,
        capability: Capability,
    ) {
        if capability == Capability::Pointer {
            if let Some(pointer) = self.pointer.take() {
                pointer.release();
            }
        }
        if capability == Capability::Keyboard {
            if let Some(keyboard) = self.keyboard.take() {
                keyboard.release();
            }
        }
    }

    fn remove_seat(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_seat::WlSeat) {}
}

impl PointerHandler for State {
    fn pointer_frame(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_pointer::WlPointer,
        events: &[PointerEvent],
    ) {
        for event in events {
            debug(|| format!("wayland: pointer {:?} at {:?}", event.kind, event.position));
            let ours = self
                .overlay
                .as_ref()
                .is_some_and(|o| &event.surface == o.layer.wl_surface());
            if !ours {
                continue;
            }
            match event.kind {
                PointerEventKind::Enter { .. } | PointerEventKind::Motion { .. } => {
                    let first = self.cursor.is_none();
                    self.cursor = Some(event.position);
                    if first && !self.started {
                        self.start();
                    }
                }
                PointerEventKind::Press { button, .. } => match button {
                    BTN_LEFT => self.clicked(),
                    BTN_RIGHT => self.close(),
                    _ => {}
                },
                _ => {}
            }
        }
    }
}

impl ShmHandler for State {
    fn shm_state(&mut self) -> &mut Shm {
        &mut self.shm
    }
}

impl ProvidesRegistryState for State {
    fn registry(&mut self) -> &mut RegistryState {
        &mut self.registry_state
    }

    registry_handlers![OutputState, SeatState];
}

delegate_registry!(State);
smithay_client_toolkit::delegate_dispatch2!(State);
