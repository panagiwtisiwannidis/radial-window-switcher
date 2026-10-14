use std::time::{Duration, Instant};

use radial_core::geometry::{Area, Geometry, Level, Selection, DEAD_ZONE, INNER, OUTER, TAB_ARC};
use radial_core::model::{Icon, RingItem, RingModel};
use radial_core::render::{self, Frame, Motion, Scene, Style};
use radial_core::text::Text;
use tiny_skia::Pixmap;

const MOVE_THRESHOLD: f32 = 4.0;
const EDGE_MARGIN: f32 = 8.0;
const RING_MARGIN: f32 = 44.0;
const PREVIEW_GAP: f32 = 16.0;
const TABS_STILLNESS: Duration = Duration::from_millis(150);
const AIM_STILLNESS: Duration = Duration::from_millis(150);
const RELOAD_DELAYS: [Duration; 2] = [Duration::from_millis(250), Duration::from_millis(1000)];

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}

pub fn debug(message: impl FnOnce() -> String) {
    if std::env::var_os("RADIAL_SWITCHER_DEBUG").is_some() {
        eprintln!("[debug] {}", message());
    }
}

pub struct Ring {
    pub model: RingModel,
    pub geometry: Geometry,
    pub style: Style,
    pub motion: Motion,
    pub center: (f32, f32),
    pub work: Rect,
    targets: Vec<(String, f32)>,
    open_cursor: (i32, i32),
    last_cursor: (i32, i32),
    has_moved: bool,
    last_move: Instant,
    dwell: Option<Selection>,
    close_button: Option<(f32, f32)>,
    close_hover: bool,
    last_point: (f32, f32),
    aim_hold: bool,
    reloads: Vec<Instant>,
    last_frame: Instant,
    selection_changed: bool,
}

pub struct Tick {
    pub changed: bool,
    pub reload: bool,
}

impl Ring {
    pub fn new(accent: [u8; 3]) -> Self {
        Self {
            model: RingModel::default(),
            geometry: Geometry::default(),
            style: Style {
                scale: 1.0,
                dark: true,
                accent,
            },
            motion: Motion::default(),
            center: (0.0, 0.0),
            work: Rect {
                x: 0,
                y: 0,
                width: 0,
                height: 0,
            },
            targets: Vec::new(),
            open_cursor: (0, 0),
            last_cursor: (0, 0),
            has_moved: false,
            last_move: Instant::now(),
            dwell: None,
            close_button: None,
            close_hover: false,
            last_point: (0.0, 0.0),
            aim_hold: false,
            reloads: Vec::new(),
            last_frame: Instant::now(),
            selection_changed: false,
        }
    }

    pub fn begin(
        &mut self,
        model: RingModel,
        cursor: (i32, i32),
        work: Rect,
        scale: f32,
        dark: bool,
    ) {
        self.model = model;
        self.work = work;
        self.style = Style {
            scale,
            dark,
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
        self.open_cursor = cursor;
        self.last_cursor = cursor;
        self.has_moved = false;
        self.last_move = Instant::now();
        self.dwell = None;
        self.close_button = None;
        self.close_hover = false;
        self.last_point = (0.0, 0.0);
        self.aim_hold = false;
        self.reloads.clear();
        self.motion.reset();
        self.targets.clear();
        self.last_frame = Instant::now();
        self.selection_changed = false;
    }

    pub fn end(&mut self) {
        self.model.selection = None;
        self.model.inner_tabs_window = None;
        self.dwell = None;
        self.reloads.clear();
        self.close_button = None;
        self.close_hover = false;
    }

    pub fn has_moved(&self) -> bool {
        self.has_moved
    }

    pub fn take_selection_changed(&mut self) -> bool {
        std::mem::take(&mut self.selection_changed)
    }

    pub fn frame_origin(&self, size: u32) -> (i32, i32) {
        (
            (self.center.0 - size as f32 / 2.0).round() as i32,
            (self.center.1 - size as f32 / 2.0).round() as i32,
        )
    }

    pub fn tick(&mut self, cursor: (i32, i32)) -> Tick {
        let now = Instant::now();
        let mut changed = false;
        if cursor != self.last_cursor {
            self.last_cursor = cursor;
            self.last_move = now;
            let moved = ((cursor.0 - self.open_cursor.0) as f32)
                .hypot((cursor.1 - self.open_cursor.1) as f32);
            if self.has_moved || moved >= MOVE_THRESHOLD * self.style.scale {
                self.has_moved = true;
                changed |= self.update_selection(cursor, false);
            }
        }
        if self.aim_hold && now.duration_since(self.last_move) >= AIM_STILLNESS {
            changed |= self.update_selection(cursor, true);
        }
        changed |= self.check_dwell(now);
        let hover = self.over_close_button(cursor);
        if hover != self.close_hover {
            self.close_hover = hover;
            changed = true;
        }
        let reload = self.reloads.first().is_some_and(|at| now >= *at);
        if reload {
            self.reloads.remove(0);
        }
        Tick { changed, reload }
    }

    pub fn animate(&mut self) -> bool {
        let now = Instant::now();
        let dt = now.duration_since(self.last_frame).as_secs_f32().min(0.1);
        self.last_frame = now;
        self.motion.step(&self.targets, dt)
    }

    pub fn update_selection(&mut self, cursor: (i32, i32), force: bool) -> bool {
        let scale = self.style.scale;
        let dx = (cursor.0 as f32 - self.center.0) / scale;
        let dy = (self.center.1 - cursor.1 as f32) / scale;
        let mut escape = false;
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
                    escape = true;
                    self.normal_selection(dx, dy, None)
                } else {
                    tab_selection
                }
            }
            None => self.normal_selection(dx, dy, self.model.selection),
        };
        let point = (dx, dy);
        let previous = std::mem::replace(&mut self.last_point, point);
        if selection == self.model.selection {
            self.aim_hold = false;
            return false;
        }
        if !force
            && self
                .model
                .aim(&self.geometry)
                .is_some_and(|aim| aim.heading(previous, point))
        {
            self.aim_hold = true;
            return false;
        }
        self.aim_hold = false;
        if escape {
            self.model.inner_tabs_window = None;
        }
        debug(|| format!("selection {selection:?}"));
        self.model.selection = selection;
        self.update_tab_mode(dx.hypot(dy) <= DEAD_ZONE);
        self.selection_changed = true;
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

    pub fn tabs_updated(
        &mut self,
        tabs: std::collections::HashMap<isize, Vec<radial_core::model::TabInfo>>,
    ) {
        self.model.tabs = tabs;
        if self.model.target_item().is_none() {
            self.model.selection = None;
        }
        if self.dwell.is_none() && self.model.inner_tabs_window.is_none() {
            self.update_tab_mode(false);
        }
    }

    pub fn over_close_button(&self, cursor: (i32, i32)) -> bool {
        self.close_button.is_some_and(|(x, y)| {
            (cursor.0 as f32 - x).hypot(cursor.1 as f32 - y)
                <= render::CLOSE_HIT_RADIUS * self.style.scale
        })
    }

    pub fn closed_target(&mut self, item: &RingItem, cursor: (i32, i32)) {
        if item.tab().is_none() {
            self.model.inner_tabs_window = None;
        }
        self.model.selection = None;
        self.dwell = None;
        self.close_hover = false;
        self.open_cursor = cursor;
        self.has_moved = false;
        self.selection_changed = true;
        let now = Instant::now();
        self.reloads = RELOAD_DELAYS.iter().map(|delay| now + *delay).collect();
    }

    pub fn rebuild(&mut self, mut model: RingModel) {
        let tabs_window = self
            .model
            .inner_tabs_window
            .and_then(|w| self.model.inner.get(w))
            .map(|item| item.preview_window().handle);
        model.tabs = std::mem::take(&mut self.model.tabs);
        model.inner_tabs_window = tabs_window
            .and_then(|handle| {
                model
                    .inner
                    .iter()
                    .position(|item| item.preview_window().handle == handle)
            })
            .filter(|w| model.inner_window_has_tabs(*w));
        self.model = model;
        debug(|| {
            format!(
                "reloaded: {} inner, {} outer",
                self.model.inner.len(),
                self.model.outer.len()
            )
        });
    }

    pub fn render(
        &mut self,
        text: &Text,
        thumbnail: &dyn Fn(&RingItem) -> Option<Icon>,
        backdrop: Option<&Pixmap>,
    ) -> (Frame, (i32, i32)) {
        let frame = render::render(&Scene {
            model: &self.model,
            geometry: &self.geometry,
            text,
            style: self.style,
            thumbnail,
            backdrop,
            motion: &self.motion,
            close_hover: self.close_hover,
        });
        let origin = self.frame_origin(frame.pixmap.width());
        self.close_button = frame
            .close_button
            .map(|(x, y)| (x + origin.0 as f32, y + origin.1 as f32));
        self.targets = frame.targets.clone();
        (frame, origin)
    }

    pub fn card_origin(&self, w: f32, h: f32) -> (i32, i32) {
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
}
