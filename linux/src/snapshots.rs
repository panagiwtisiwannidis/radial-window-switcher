use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use radial_core::model::Icon;
use x11rb::connection::Connection as _;

use crate::capture::Capturer;
use crate::windows;
use crate::x::Atoms;

const INTERVAL: Duration = Duration::from_secs(5);
const MAX_WIDTH: u16 = 480;

#[derive(Clone, Default)]
pub struct Snapshots {
    store: Arc<Mutex<HashMap<isize, Icon>>>,
}

impl Snapshots {
    pub fn start() -> Self {
        let snapshots = Self::default();
        let store = snapshots.store.clone();
        std::thread::spawn(move || {
            let Ok((conn, screen)) = x11rb::connect(None) else {
                return;
            };
            let root = conn.setup().roots[screen].root;
            let Some(atoms) = Atoms::new(&conn).ok().and_then(|c| c.reply().ok()) else {
                return;
            };
            let Some(capturer) = Capturer::new(&conn) else {
                return;
            };
            loop {
                let windows = windows::client_windows(&conn, root, &atoms);
                for &window in &windows {
                    if !windows::is_switchable(&conn, window, &atoms) {
                        continue;
                    }
                    if let Some(image) = capturer.window(&conn, root, window, MAX_WIDTH) {
                        if let Ok(mut store) = store.lock() {
                            store.insert(window as isize, Arc::new(image));
                        }
                    }
                }
                if let Ok(mut store) = store.lock() {
                    store.retain(|handle, _| windows.contains(&(*handle as u32)));
                }
                std::thread::sleep(INTERVAL);
            }
        });
        snapshots
    }

    pub fn window(&self, handle: isize) -> Option<Icon> {
        self.store.lock().ok()?.get(&handle).cloned()
    }

    pub fn update(&self, handle: isize, image: Icon) {
        if let Ok(mut store) = self.store.lock() {
            store.insert(handle, image);
        }
    }
}
