use std::collections::HashMap;
use std::sync::Arc;

use radial_core::model::{AppWindows, Icon, WindowRef};
use tiny_skia::Pixmap;
use x11rb::connection::Connection;
use x11rb::protocol::xproto::{AtomEnum, Window};

use crate::desktop::DesktopApps;
use crate::x::{self, Atoms};

const ICON_TARGET: u32 = 64;

pub struct Snapshot {
    pub front: Option<AppWindows>,
    pub others: Vec<AppWindows>,
}

pub struct Apps {
    desktop: DesktopApps,
    icons: HashMap<String, Option<Icon>>,
}

impl Apps {
    pub fn load() -> Self {
        Self {
            desktop: DesktopApps::load(),
            icons: HashMap::new(),
        }
    }

    fn name(&self, instance: &str, class: &str) -> String {
        self.desktop.name(&[class, instance]).unwrap_or_else(|| {
            if class.is_empty() {
                instance.to_string()
            } else {
                class.to_string()
            }
        })
    }
}

pub fn client_windows(conn: &impl Connection, root: Window, atoms: &Atoms) -> Vec<Window> {
    let mut stacking = x::values32(
        conn,
        root,
        atoms._NET_CLIENT_LIST_STACKING,
        AtomEnum::WINDOW,
    );
    if stacking.is_empty() {
        stacking = x::values32(conn, root, atoms._NET_CLIENT_LIST, AtomEnum::WINDOW);
    }
    stacking.reverse();
    stacking
}

pub fn is_switchable(conn: &impl Connection, window: Window, atoms: &Atoms) -> bool {
    let types = x::values32(conn, window, atoms._NET_WM_WINDOW_TYPE, AtomEnum::ATOM);
    if !types.is_empty() && !types.contains(&atoms._NET_WM_WINDOW_TYPE_NORMAL) {
        return false;
    }
    let state = x::values32(conn, window, atoms._NET_WM_STATE, AtomEnum::ATOM);
    !state.contains(&atoms._NET_WM_STATE_SKIP_TASKBAR)
}

pub fn snapshot(conn: &impl Connection, root: Window, atoms: &Atoms, apps: &mut Apps) -> Snapshot {
    let active = x::values32(conn, root, atoms._NET_ACTIVE_WINDOW, AtomEnum::WINDOW)
        .first()
        .copied();
    let mut order: Vec<String> = Vec::new();
    let mut groups: HashMap<String, AppWindows> = HashMap::new();
    let mut front_key = None;

    for window in client_windows(conn, root, atoms) {
        if !is_switchable(conn, window, atoms) {
            continue;
        }
        let (instance, class) = x::class(conn, window);
        let key = if class.is_empty() {
            instance.to_lowercase()
        } else {
            class.to_lowercase()
        };
        if Some(window) == active {
            front_key = Some(key.clone());
        }
        let state = x::values32(conn, window, atoms._NET_WM_STATE, AtomEnum::ATOM);
        let reference = WindowRef {
            handle: window as isize,
            pid: x::values32(conn, window, atoms._NET_WM_PID, AtomEnum::CARDINAL)
                .first()
                .copied()
                .unwrap_or(0),
            title: x::text(conn, window, atoms),
            minimized: state.contains(&atoms._NET_WM_STATE_HIDDEN),
        };
        if !groups.contains_key(&key) {
            order.push(key.clone());
            let desktop = &apps.desktop;
            let icon = apps
                .icons
                .entry(key.clone())
                .or_insert_with(|| {
                    window_icon(conn, window, atoms)
                        .or_else(|| desktop.icon(&[&class, &instance]))
                        .map(Arc::new)
                })
                .clone();
            groups.insert(
                key.clone(),
                AppWindows {
                    name: apps.name(&instance, &class),
                    icon,
                    windows: Vec::new(),
                },
            );
        }
        if let Some(group) = groups.get_mut(&key) {
            group.windows.push(reference);
        }
    }

    let front = front_key.as_ref().and_then(|k| groups.remove(k));
    let others = order
        .into_iter()
        .filter_map(|k| groups.remove(&k))
        .collect();
    Snapshot { front, others }
}

pub fn window_icon(conn: &impl Connection, window: Window, atoms: &Atoms) -> Option<Pixmap> {
    let data = x::values32(conn, window, atoms._NET_WM_ICON, AtomEnum::CARDINAL);
    let mut best: Option<(u32, u32, usize)> = None;
    let mut offset = 0;
    while offset + 2 <= data.len() {
        let (width, height) = (data[offset], data[offset + 1]);
        let length = (width as usize) * (height as usize);
        if width == 0 || height == 0 || offset + 2 + length > data.len() {
            break;
        }
        let better = match best {
            None => true,
            Some((w, _, _)) => {
                let distance = |s: u32| {
                    if s >= ICON_TARGET {
                        s - ICON_TARGET
                    } else {
                        (ICON_TARGET - s) * 4
                    }
                };
                distance(width) < distance(w)
            }
        };
        if better {
            best = Some((width, height, offset + 2));
        }
        offset += 2 + length;
    }
    let (width, height, start) = best?;
    let mut pixmap = Pixmap::new(width, height)?;
    for (pixel, out) in data[start..start + (width * height) as usize]
        .iter()
        .zip(pixmap.data_mut().chunks_mut(4))
    {
        let a = (pixel >> 24) & 0xff;
        let premultiply = |c: u32| (c * a / 255) as u8;
        out[0] = premultiply((pixel >> 16) & 0xff);
        out[1] = premultiply((pixel >> 8) & 0xff);
        out[2] = premultiply(pixel & 0xff);
        out[3] = a as u8;
    }
    Some(pixmap)
}
