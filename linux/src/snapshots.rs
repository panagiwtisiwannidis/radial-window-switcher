use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use radial_core::model::Icon;
use x11rb::connection::Connection as _;

use crate::capture::Capturer;
use crate::windows;
use crate::x::{self, Atoms};

const INTERVAL: Duration = Duration::from_secs(5);
const MAX_WIDTH: u16 = 480;
const TITLES_PER_WINDOW: usize = 40;

#[derive(Default)]
struct Store {
    windows: HashMap<isize, Icon>,
    titles: HashMap<isize, Vec<(String, Icon)>>,
}

impl Store {
    fn record(&mut self, handle: isize, title: String, image: Icon) {
        self.windows.insert(handle, image.clone());
        let titles = self.titles.entry(handle).or_default();
        titles.retain(|(t, _)| *t != title);
        titles.push((title, image));
        if titles.len() > TITLES_PER_WINDOW {
            titles.remove(0);
        }
    }
}

#[derive(Clone, Default)]
pub struct Snapshots {
    store: Arc<Mutex<Store>>,
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
                        let title = x::text(&conn, window, &atoms);
                        if let Ok(mut store) = store.lock() {
                            store.record(window as isize, title, Arc::new(image));
                        }
                    }
                }
                if let Ok(mut store) = store.lock() {
                    store
                        .windows
                        .retain(|handle, _| windows.contains(&(*handle as u32)));
                    store
                        .titles
                        .retain(|handle, _| windows.contains(&(*handle as u32)));
                }
                std::thread::sleep(INTERVAL);
            }
        });
        snapshots
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

    pub fn update(&self, handle: isize, title: String, image: Icon) {
        if let Ok(mut store) = self.store.lock() {
            store.record(handle, title, image);
        }
    }
}
