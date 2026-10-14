use std::collections::HashMap;
use std::sync::Arc;

use tiny_skia::Pixmap;

use crate::geometry::{
    self, Aim, Geometry, Level, Selection, MAX_INNER_ITEMS, TAB_POPUP, WINDOW_ARC,
};

pub type Icon = Arc<Pixmap>;

#[derive(Clone, Debug, PartialEq)]
pub struct WindowRef {
    pub handle: isize,
    pub pid: u32,
    pub title: String,
    pub minimized: bool,
}

#[derive(Clone, Debug)]
pub struct AppWindows {
    pub name: String,
    pub icon: Option<Icon>,
    pub windows: Vec<WindowRef>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct TabInfo {
    pub title: String,
    pub selected: bool,
}

#[derive(Clone, Debug)]
pub enum Target {
    Window(WindowRef),
    App {
        front: WindowRef,
    },
    Tab {
        window: WindowRef,
        index: usize,
        tab: TabInfo,
    },
}

#[derive(Clone, Debug)]
pub struct RingItem {
    pub id: String,
    pub icon: Option<Icon>,
    pub short_title: String,
    pub full_title: String,
    pub target: Target,
    pub windows: Vec<RingItem>,
}

impl RingItem {
    pub fn preview_window(&self) -> &WindowRef {
        match &self.target {
            Target::Window(window) => window,
            Target::App { front } => front,
            Target::Tab { window, .. } => window,
        }
    }

    pub fn tab(&self) -> Option<&TabInfo> {
        match &self.target {
            Target::Tab { tab, .. } => Some(tab),
            _ => None,
        }
    }

    fn window(window: &WindowRef, app: &AppWindows) -> Self {
        let title = if window.title.is_empty() {
            app.name.clone()
        } else {
            window.title.clone()
        };
        Self {
            id: format!("w{}", window.handle),
            icon: app.icon.clone(),
            short_title: title.clone(),
            full_title: title,
            target: Target::Window(window.clone()),
            windows: Vec::new(),
        }
    }

    fn app(app: &AppWindows) -> Option<Self> {
        let front = app
            .windows
            .iter()
            .find(|w| !w.minimized)
            .or(app.windows.first())?
            .clone();
        let count = app.windows.len();
        let detail = if count == 1 {
            "1 window".to_string()
        } else {
            format!("{count} windows")
        };
        let windows = if count >= 2 {
            app.windows
                .iter()
                .take(MAX_INNER_ITEMS)
                .map(|w| Self::window(w, app))
                .collect()
        } else {
            Vec::new()
        };
        Some(Self {
            id: format!("a{}", front.pid),
            icon: app.icon.clone(),
            short_title: app.name.clone(),
            full_title: format!("{} — {}", app.name, detail),
            target: Target::App { front },
            windows,
        })
    }

    fn tab_item(window: &WindowRef, icon: &Option<Icon>, index: usize, tab: &TabInfo) -> Self {
        Self {
            id: format!("t{}:{index}", window.handle),
            icon: icon.clone(),
            short_title: tab.title.clone(),
            full_title: tab.title.clone(),
            target: Target::Tab {
                window: window.clone(),
                index,
                tab: tab.clone(),
            },
            windows: Vec::new(),
        }
    }
}

#[derive(Default)]
pub struct RingModel {
    pub inner: Vec<RingItem>,
    pub outer: Vec<RingItem>,
    pub selection: Option<Selection>,
    pub tabs: HashMap<isize, Vec<TabInfo>>,
    pub inner_tabs_window: Option<usize>,
}

impl RingModel {
    pub fn build(frontmost: Option<&AppWindows>, others: &[AppWindows]) -> Self {
        let inner = frontmost
            .map(|app| {
                app.windows
                    .iter()
                    .take(MAX_INNER_ITEMS)
                    .map(|w| RingItem::window(w, app))
                    .collect()
            })
            .unwrap_or_default();
        let outer = others.iter().filter_map(RingItem::app).collect();
        Self {
            inner,
            outer,
            ..Self::default()
        }
    }

    pub fn outer_window_counts(&self) -> Vec<usize> {
        self.outer.iter().map(|item| item.windows.len()).collect()
    }

    fn tabs_of(&self, window: &WindowRef, icon: &Option<Icon>) -> Vec<RingItem> {
        self.tabs
            .get(&window.handle)
            .filter(|tabs| tabs.len() >= 2)
            .map(|tabs| {
                tabs.iter()
                    .enumerate()
                    .map(|(i, tab)| RingItem::tab_item(window, icon, i, tab))
                    .collect()
            })
            .unwrap_or_default()
    }

    pub fn tab_window(&self, app: usize, window: Option<usize>) -> Option<&WindowRef> {
        let item = self.outer.get(app)?;
        if item.windows.len() < 2 {
            return Some(item.preview_window());
        }
        Some(item.windows.get(window?)?.preview_window())
    }

    pub fn tab_items(&self, app: usize, window: Option<usize>) -> Vec<RingItem> {
        match self.tab_window(app, window) {
            Some(target) => self.tabs_of(target, &self.outer[app].icon),
            None => Vec::new(),
        }
    }

    pub fn inner_tab_items(&self, window: usize) -> Vec<RingItem> {
        self.inner
            .get(window)
            .map(|item| self.tabs_of(item.preview_window(), &item.icon))
            .unwrap_or_default()
    }

    pub fn inner_window_has_tabs(&self, window: usize) -> bool {
        !self.inner_tab_items(window).is_empty()
    }

    pub fn outer_items(&self) -> Vec<RingItem> {
        match self.inner_tabs_window {
            Some(window) => self.inner_tab_items(window),
            None => self.outer.clone(),
        }
    }

    pub fn target_item(&self) -> Option<RingItem> {
        let selection = self.selection?;
        if let (Some(window), Level::Outer) = (self.inner_tabs_window, selection.level) {
            return self
                .inner_tab_items(window)
                .into_iter()
                .nth(selection.index);
        }
        let item = match selection.level {
            Level::Inner => self.inner.get(selection.index)?,
            Level::Outer => self.outer.get(selection.index)?,
        };
        if let Some(tab) = selection.tab {
            return self
                .tab_items(selection.index, selection.window)
                .into_iter()
                .nth(tab);
        }
        match selection.window {
            Some(window) => item.windows.get(window).cloned(),
            None => Some(item.clone()),
        }
    }

    pub fn selected_item(&self) -> Option<RingItem> {
        if self.selection?.pointing {
            self.target_item()
        } else {
            None
        }
    }

    pub fn expanded_app_index(&self) -> Option<usize> {
        if self.inner_tabs_window.is_some() {
            return None;
        }
        let selection = self.selection?;
        let app = self.outer.get(selection.index)?;
        (selection.level == Level::Outer && app.windows.len() >= 2).then_some(selection.index)
    }

    pub fn open_tab_arc(&self) -> Option<(usize, Option<usize>, Vec<RingItem>)> {
        if self.inner_tabs_window.is_some() {
            return None;
        }
        let selection = self.selection.filter(|s| s.level == Level::Outer)?;
        let items = self.tab_items(selection.index, selection.window);
        (items.len() >= 2).then_some((selection.index, selection.window, items))
    }

    pub fn aim(&self, geometry: &Geometry) -> Option<Aim> {
        let selection = self.selection?;
        if let Some(window) = self.inner_tabs_window {
            if selection.level != Level::Inner || selection.index != window {
                return None;
            }
            let bounds = geometry.tab_popup_bounds(
                window,
                self.inner.len(),
                self.inner_tab_items(window).len(),
            );
            return Some(Aim {
                arc: geometry::span(&bounds)?,
                arc_inner: TAB_POPUP.0,
            });
        }
        if selection.level != Level::Outer || selection.tab.is_some() {
            return None;
        }
        let app = selection.index;
        let app_count = self.outer.len();
        let window_count = self.outer.get(app)?.windows.len();
        let window_bounds = if window_count >= 2 {
            geometry.window_arc_bounds(app, app_count, window_count)
        } else {
            Vec::new()
        };
        if selection.window.is_none() && window_count >= 2 {
            return Some(Aim {
                arc: geometry::span(&window_bounds)?,
                arc_inner: WINDOW_ARC.0,
            });
        }
        let arc = geometry.tab_arc(
            app,
            app_count,
            selection.window,
            &window_bounds,
            self.tab_items(app, selection.window).len(),
        )?;
        Some(Aim {
            arc: geometry::span(&arc.bounds)?,
            arc_inner: arc.radii.0,
        })
    }

    pub fn is_past_apps_ring(&self) -> bool {
        self.inner_tabs_window.is_none()
            && self
                .selection
                .is_some_and(|s| s.level == Level::Outer && (s.window.is_some() || s.tab.is_some()))
    }

    pub fn keyboard_order(&self) -> Vec<Selection> {
        let inner = (0..self.inner.len()).map(Selection::inner);
        if self.inner_tabs_window.is_some() {
            return inner
                .chain((0..self.outer_items().len()).map(Selection::outer))
                .collect();
        }
        let outer = self.outer.iter().enumerate().flat_map(|(i, app)| {
            if app.windows.len() >= 2 {
                (0..app.windows.len())
                    .map(|w| Selection::window(i, w))
                    .collect::<Vec<_>>()
            } else {
                vec![Selection::outer(i)]
            }
        });
        inner.chain(outer).collect()
    }

    pub fn move_selection(&mut self, forward: bool) {
        let order = self.keyboard_order();
        if order.is_empty() {
            return;
        }
        let position = self.selection.map(|s| s.position());
        self.selection = Some(
            match position.and_then(|p| order.iter().position(|o| *o == p)) {
                Some(current) if forward => order[(current + 1) % order.len()],
                Some(current) => order[(current + order.len() - 1) % order.len()],
                None if forward => {
                    order[if !self.inner.is_empty() && order.len() > 1 {
                        1
                    } else {
                        0
                    }]
                }
                None => order[order.len() - 1],
            },
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn app(name: &str, pid: u32, windows: usize) -> AppWindows {
        AppWindows {
            name: name.into(),
            icon: None,
            windows: (0..windows)
                .map(|i| WindowRef {
                    handle: (pid * 100 + i as u32) as isize,
                    pid,
                    title: format!("{name} {i}"),
                    minimized: false,
                })
                .collect(),
        }
    }

    fn tabs(count: usize) -> Vec<TabInfo> {
        (0..count)
            .map(|i| TabInfo {
                title: format!("Tab {i}"),
                selected: i == 0,
            })
            .collect()
    }

    #[test]
    fn first_tab_skips_the_current_window() {
        let front = app("Editor", 1, 3);
        let mut model = RingModel::build(Some(&front), &[app("Browser", 2, 1)]);
        model.move_selection(true);
        assert_eq!(model.selection, Some(Selection::inner(1)));
    }

    #[test]
    fn apps_with_several_windows_contribute_each_window_to_the_tab_order() {
        let model = RingModel::build(None, &[app("Browser", 2, 2), app("Mail", 3, 1)]);
        assert_eq!(
            model.keyboard_order(),
            vec![
                Selection::window(0, 0),
                Selection::window(0, 1),
                Selection::outer(1)
            ]
        );
    }

    #[test]
    fn only_pointed_selections_can_be_opened() {
        let mut model = RingModel::build(None, &[app("Browser", 2, 2)]);
        model.selection = Some(Selection {
            pointing: false,
            ..Selection::outer(0)
        });
        assert!(model.selected_item().is_none());
        assert!(model.target_item().is_some());
    }

    #[test]
    fn a_selected_window_opens_its_tab_arc_and_tabs_can_be_targeted() {
        let mut model = RingModel::build(None, &[app("Browser", 2, 2)]);
        model.tabs.insert(201, tabs(3));
        model.selection = Some(Selection::window(0, 1));
        assert_eq!(
            model.open_tab_arc().map(|(_, _, items)| items.len()),
            Some(3)
        );
        model.selection = Some(Selection {
            tab: Some(2),
            ..Selection::window(0, 1)
        });
        let item = model.selected_item().unwrap();
        assert_eq!(item.tab().unwrap().title, "Tab 2");
        assert_eq!(item.preview_window().handle, 201);
    }

    #[test]
    fn tab_mode_swaps_the_outer_ring_for_the_inner_windows_tabs() {
        let front = app("Browser", 1, 2);
        let mut model = RingModel::build(Some(&front), &[app("Mail", 3, 1)]);
        model.tabs.insert(100, tabs(4));
        assert!(model.inner_window_has_tabs(0));
        model.inner_tabs_window = Some(0);
        assert_eq!(model.outer_items().len(), 4);
        model.selection = Some(Selection::outer(3));
        assert_eq!(model.selected_item().unwrap().short_title, "Tab 3");
        assert!(model.open_tab_arc().is_none());
    }
}
