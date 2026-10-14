use std::collections::HashMap;
use std::sync::mpsc::{channel, Sender};
use std::sync::{Arc, Mutex};

use atspi::proxy::accessible::{AccessibleProxy, ObjectRefExt};
use atspi::proxy::proxy_ext::ProxyExt;
use atspi::{
    AccessibilityConnection, MatchType, ObjectMatchRule, ObjectRefOwned, Role, SortOrder, State,
};
use radial_core::model::TabInfo;
use zbus::names::BusName;

#[derive(Clone)]
pub struct WindowInfo {
    pub handle: isize,
    pub pid: u32,
    pub title: String,
}

enum Request {
    Refresh(Vec<WindowInfo>),
    Select(isize, usize),
}

#[derive(Clone)]
pub struct Tabs {
    requests: Sender<Request>,
    known: Arc<Mutex<HashMap<isize, Vec<TabInfo>>>>,
}

type Found = HashMap<isize, (Vec<TabInfo>, Vec<ObjectRefOwned>)>;

fn debug(message: impl FnOnce() -> String) {
    if std::env::var_os("RADIAL_SWITCHER_DEBUG").is_some() {
        eprintln!("[debug] tabs: {}", message());
    }
}

impl Tabs {
    pub fn start(ready: impl Fn() + Send + 'static) -> Self {
        let (requests, receiver) = channel::<Request>();
        let known = Arc::new(Mutex::new(HashMap::new()));
        let shared = known.clone();
        std::thread::spawn(move || {
            let connection = match zbus::block_on(AccessibilityConnection::new()) {
                Ok(connection) => connection,
                Err(error) => {
                    debug(|| format!("no accessibility bus: {error}"));
                    return;
                }
            };
            let mut elements: HashMap<isize, Vec<ObjectRefOwned>> = HashMap::new();
            let mut pids: HashMap<String, u32> = HashMap::new();
            while let Ok(request) = receiver.recv() {
                match request {
                    Request::Refresh(windows) => {
                        let found = zbus::block_on(read_all(&connection, &windows, &mut pids))
                            .unwrap_or_else(|error| {
                                debug(|| format!("reading failed: {error}"));
                                Found::new()
                            });
                        elements = found
                            .iter()
                            .map(|(w, (_, refs))| (*w, refs.clone()))
                            .collect();
                        if let Ok(mut known) = shared.lock() {
                            *known = found.into_iter().map(|(w, (tabs, _))| (w, tabs)).collect();
                        }
                        ready();
                    }
                    Request::Select(window, index) => {
                        if let Some(tab) = elements.get(&window).and_then(|tabs| tabs.get(index)) {
                            let _ = zbus::block_on(select(connection.connection(), tab));
                        }
                    }
                }
            }
        });
        Self { requests, known }
    }

    pub fn refresh(&self, windows: Vec<WindowInfo>) {
        let _ = self.requests.send(Request::Refresh(windows));
    }

    pub fn select(&self, window: isize, index: usize) {
        let _ = self.requests.send(Request::Select(window, index));
    }

    pub fn known(&self) -> HashMap<isize, Vec<TabInfo>> {
        self.known.lock().map(|k| k.clone()).unwrap_or_default()
    }
}

async fn read_all(
    accessibility: &AccessibilityConnection,
    windows: &[WindowInfo],
    pids: &mut HashMap<String, u32>,
) -> Result<Found, Box<dyn std::error::Error>> {
    let conn = accessibility.connection();
    let root = accessibility.root_accessible_on_registry().await?;
    let dbus = zbus::fdo::DBusProxy::new(conn).await?;
    let mut found = Found::new();
    let apps = root.get_children().await?;
    debug(|| {
        format!(
            "{} accessible apps; window pids {:?}",
            apps.len(),
            windows.iter().map(|w| w.pid).collect::<Vec<_>>()
        )
    });
    for app in apps {
        let Some(name) = app.name_as_str() else {
            continue;
        };
        let pid = match pids.get(name) {
            Some(pid) => *pid,
            None => {
                let Ok(bus_name) = BusName::try_from(name) else {
                    continue;
                };
                let Ok(pid) = dbus.get_connection_unix_process_id(bus_name).await else {
                    continue;
                };
                pids.insert(name.to_string(), pid);
                pid
            }
        };
        let candidates: Vec<&WindowInfo> = windows.iter().filter(|w| w.pid == pid).collect();
        debug(|| {
            format!(
                "app {name} pid {pid}: {} matching windows",
                candidates.len()
            )
        });
        if candidates.is_empty() {
            continue;
        }
        let Ok(app) = app.as_accessible_proxy(conn).await else {
            continue;
        };
        let Ok(frames) = app.get_children().await else {
            continue;
        };
        for frame in frames {
            let Ok(frame) = frame.as_accessible_proxy(conn).await else {
                continue;
            };
            let title = frame.name().await.unwrap_or_default();
            let window = candidates
                .iter()
                .find(|w| {
                    !title.is_empty()
                        && (w.title == title
                            || w.title.starts_with(&title)
                            || title.starts_with(&w.title))
                })
                .or_else(|| (candidates.len() == 1).then(|| &candidates[0]));
            debug(|| format!("frame {title:?} -> window {:?}", window.map(|w| &w.title)));
            let Some(window) = window else { continue };
            let result = read_tabs(conn, &frame).await;
            debug(|| match &result {
                Ok((tabs, _)) => format!("{:?}", tabs.iter().map(|t| &t.title).collect::<Vec<_>>()),
                Err(error) => format!("no tabs: {error}"),
            });
            if let Ok(tabs) = result {
                if tabs.0.len() >= 2 {
                    found.insert(window.handle, tabs);
                }
            }
        }
    }
    Ok(found)
}

async fn read_tabs(
    conn: &zbus::Connection,
    frame: &AccessibleProxy<'_>,
) -> Result<(Vec<TabInfo>, Vec<ObjectRefOwned>), Box<dyn std::error::Error>> {
    let collection = frame.proxies().await?.collection().await?;
    let strips = collection
        .get_matches(
            ObjectMatchRule::builder()
                .roles(&[Role::PageTabList], MatchType::All)
                .build(),
            SortOrder::Canonical,
            1,
            false,
        )
        .await?;
    let strip = strips.into_iter().next().ok_or("no tab strip")?;
    let strip = strip.as_accessible_proxy(conn).await?;
    let mut tabs = Vec::new();
    let mut refs = Vec::new();
    for child in strip.get_children().await? {
        let Ok(tab) = child.as_accessible_proxy(conn).await else {
            continue;
        };
        if tab.get_role().await.ok() != Some(Role::PageTab) {
            continue;
        }
        let title = tab.name().await.unwrap_or_default();
        let selected = tab
            .get_state()
            .await
            .is_ok_and(|state| state.contains(State::Selected));
        tabs.push(TabInfo { title, selected });
        refs.push(child);
    }
    Ok((tabs, refs))
}

async fn select(
    conn: &zbus::Connection,
    tab: &ObjectRefOwned,
) -> Result<(), Box<dyn std::error::Error>> {
    let tab = tab.as_accessible_proxy(conn).await?;
    let proxies = tab.proxies().await?;
    if let Ok(action) = proxies.action().await {
        if action.do_action(0).await.unwrap_or(false) {
            return Ok(());
        }
    }
    let index = tab.get_index_in_parent().await?;
    let parent = tab.parent().await?;
    let parent = parent.as_accessible_proxy(conn).await?;
    parent
        .proxies()
        .await?
        .selection()
        .await?
        .select_child(index)
        .await?;
    Ok(())
}
