use std::sync::Arc;

use tiny_skia::Pixmap;

use crate::geometry::{Level, Selection, MAX_INNER_ITEMS};

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

#[derive(Clone, Debug)]
pub enum Target {
    Window(WindowRef),
    App { front: WindowRef },
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
        }
    }

    fn window(window: &WindowRef, app: &AppWindows) -> Self {
        let title = if window.title.is_empty() { app.name.clone() } else { window.title.clone() };
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
        let front = app.windows.iter().find(|w| !w.minimized).or(app.windows.first())?.clone();
        let count = app.windows.len();
        let detail = if count == 1 { "1 window".to_string() } else { format!("{count} windows") };
        let windows = if count >= 2 {
            app.windows.iter().take(MAX_INNER_ITEMS).map(|w| Self::window(w, app)).collect()
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
}

#[derive(Default)]
pub struct RingModel {
    pub inner: Vec<RingItem>,
    pub outer: Vec<RingItem>,
    pub selection: Option<Selection>,
}

impl RingModel {
    pub fn build(frontmost: Option<&AppWindows>, others: &[AppWindows]) -> Self {
        let inner = frontmost
            .map(|app| app.windows.iter().take(MAX_INNER_ITEMS).map(|w| RingItem::window(w, app)).collect())
            .unwrap_or_default();
        let outer = others.iter().filter_map(RingItem::app).collect();
        Self { inner, outer, selection: None }
    }

    pub fn outer_window_counts(&self) -> Vec<usize> {
        self.outer.iter().map(|item| item.windows.len()).collect()
    }

    pub fn target_item(&self) -> Option<&RingItem> {
        let selection = self.selection?;
        let item = match selection.level {
            Level::Inner => self.inner.get(selection.index)?,
            Level::Outer => self.outer.get(selection.index)?,
        };
        match selection.window {
            Some(window) => item.windows.get(window),
            None => Some(item),
        }
    }

    pub fn selected_item(&self) -> Option<&RingItem> {
        if self.selection?.pointing { self.target_item() } else { None }
    }

    pub fn expanded_app_index(&self) -> Option<usize> {
        let selection = self.selection?;
        let app = self.outer.get(selection.index)?;
        (selection.level == Level::Outer && app.windows.len() >= 2).then_some(selection.index)
    }

    pub fn is_past_apps_ring(&self) -> bool {
        self.selection.is_some_and(|s| s.level == Level::Outer && (s.window.is_some() || s.tab.is_some()))
    }

    pub fn keyboard_order(&self) -> Vec<Selection> {
        let inner = (0..self.inner.len()).map(Selection::inner);
        let outer = self.outer.iter().enumerate().flat_map(|(i, app)| {
            if app.windows.len() >= 2 {
                (0..app.windows.len()).map(|w| Selection::window(i, w)).collect::<Vec<_>>()
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
        self.selection = Some(match position.and_then(|p| order.iter().position(|o| *o == p)) {
            Some(current) if forward => order[(current + 1) % order.len()],
            Some(current) => order[(current + order.len() - 1) % order.len()],
            None if forward => order[if !self.inner.is_empty() && order.len() > 1 { 1 } else { 0 }],
            None => order[order.len() - 1],
        });
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
                .map(|i| WindowRef { handle: (pid * 100 + i as u32) as isize, pid, title: format!("{name} {i}"), minimized: false })
                .collect(),
        }
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
        assert_eq!(model.keyboard_order(), vec![Selection::window(0, 0), Selection::window(0, 1), Selection::outer(1)]);
    }

    #[test]
    fn only_pointed_selections_can_be_opened() {
        let mut model = RingModel::build(None, &[app("Browser", 2, 2)]);
        model.selection = Some(Selection { pointing: false, ..Selection::outer(0) });
        assert!(model.selected_item().is_none());
        assert!(model.target_item().is_some());
    }
}
