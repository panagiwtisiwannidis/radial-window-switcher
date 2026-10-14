use smithay_client_toolkit::dispatch2::Dispatch2;
use wayland_client::{Connection, QueueHandle};
use wayland_protocols_wlr::foreign_toplevel::v1::client::zwlr_foreign_toplevel_handle_v1::{
    self, ZwlrForeignToplevelHandleV1,
};
use wayland_protocols_wlr::foreign_toplevel::v1::client::zwlr_foreign_toplevel_manager_v1::{
    self, ZwlrForeignToplevelManagerV1,
};

use super::State;

const MINIMIZED: u32 = 1;
const ACTIVATED: u32 = 2;

pub struct Toplevel {
    pub handle: ZwlrForeignToplevelHandleV1,
    pub id: isize,
    pub title: String,
    pub app_id: String,
    pub minimized: bool,
    pub activated: bool,
    pub recency: u64,
    pub ready: bool,
}

#[derive(Default)]
pub struct Toplevels {
    pub list: Vec<Toplevel>,
    next_id: isize,
    clock: u64,
}

impl Toplevels {
    fn get(&mut self, handle: &ZwlrForeignToplevelHandleV1) -> Option<&mut Toplevel> {
        self.list.iter_mut().find(|t| &t.handle == handle)
    }

    pub fn by_id(&self, id: isize) -> Option<&Toplevel> {
        self.list.iter().find(|t| t.id == id)
    }

    fn tick(&mut self) -> u64 {
        self.clock += 1;
        self.clock
    }
}

pub struct ManagerData;
pub struct HandleData;

impl Dispatch2<ZwlrForeignToplevelManagerV1, State> for ManagerData {
    fn event(
        &self,
        state: &mut State,
        _: &ZwlrForeignToplevelManagerV1,
        event: zwlr_foreign_toplevel_manager_v1::Event,
        _: &Connection,
        _: &QueueHandle<State>,
    ) {
        if let zwlr_foreign_toplevel_manager_v1::Event::Toplevel { toplevel } = event {
            let toplevels = &mut state.toplevels;
            toplevels.next_id += 1;
            let recency = toplevels.tick();
            let id = toplevels.next_id;
            toplevels.list.push(Toplevel {
                handle: toplevel,
                id,
                title: String::new(),
                app_id: String::new(),
                minimized: false,
                activated: false,
                recency,
                ready: false,
            });
        }
    }

    wayland_client::event_created_child!(State, ZwlrForeignToplevelManagerV1, [
        zwlr_foreign_toplevel_manager_v1::EVT_TOPLEVEL_OPCODE => (ZwlrForeignToplevelHandleV1, HandleData)
    ]);
}

impl Dispatch2<ZwlrForeignToplevelHandleV1, State> for HandleData {
    fn event(
        &self,
        state: &mut State,
        handle: &ZwlrForeignToplevelHandleV1,
        event: zwlr_foreign_toplevel_handle_v1::Event,
        _: &Connection,
        _: &QueueHandle<State>,
    ) {
        use zwlr_foreign_toplevel_handle_v1::Event;
        match event {
            Event::Title { title } => {
                if let Some(t) = state.toplevels.get(handle) {
                    t.title = title;
                }
            }
            Event::AppId { app_id } => {
                if let Some(t) = state.toplevels.get(handle) {
                    t.app_id = app_id;
                }
            }
            Event::State { state: raw } => {
                let flags: Vec<u32> = raw
                    .as_chunks::<4>()
                    .0
                    .iter()
                    .map(|c| u32::from_ne_bytes(*c))
                    .collect();
                let toplevels = &mut state.toplevels;
                let clock = toplevels.clock + 1;
                if let Some(t) = toplevels.get(handle) {
                    let activated = flags.contains(&ACTIVATED);
                    let newly_active = activated && !t.activated;
                    t.minimized = flags.contains(&MINIMIZED);
                    t.activated = activated;
                    if newly_active {
                        t.recency = clock;
                        toplevels.clock = clock;
                    }
                }
            }
            Event::Done => {
                if let Some(t) = state.toplevels.get(handle) {
                    t.ready = true;
                }
            }
            Event::Closed => {
                state.toplevels.list.retain(|t| &t.handle != handle);
                handle.destroy();
            }
            _ => {}
        }
    }
}
