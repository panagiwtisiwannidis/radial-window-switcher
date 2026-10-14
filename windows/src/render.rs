use tiny_skia::{
    Color, FillRule, FilterQuality, Paint, PathBuilder, Pixmap, PixmapPaint, Stroke, Transform,
};

use crate::geometry::{self, Bounds, Geometry, Level, Radii, Selection, INNER, OUTER, WINDOW_ARC};
use crate::model::{RingItem, RingModel};
use crate::text::{Text, TextStyle};

pub const FRAME_RADIUS: f32 = WINDOW_ARC.1 + 44.0;

const POP_DISTANCE: f32 = 7.0;
const LABEL_SCALE: f32 = 1.15;
const BACKDROP_ALPHA: f32 = 0.35;

#[derive(Clone, Copy)]
pub struct Style {
    pub scale: f32,
    pub dark: bool,
    pub accent: [u8; 3],
}

#[derive(Clone, Copy, PartialEq)]
enum State {
    Normal,
    Parent,
    Selected,
}

struct Painter<'a> {
    pixmap: Pixmap,
    center: (f32, f32),
    style: Style,
    text: &'a Text,
}

pub fn frame_size(scale: f32) -> u32 {
    (2.0 * FRAME_RADIUS * scale).ceil() as u32
}

pub fn render(model: &RingModel, geometry: &Geometry, text: &Text, style: Style) -> Pixmap {
    let size = frame_size(style.scale);
    let pixmap = Pixmap::new(size, size).expect("ring frame size");
    let mut painter = Painter { pixmap, center: (size as f32 / 2.0, size as f32 / 2.0), style, text };

    let backdrop = if model.is_past_apps_ring() { BACKDROP_ALPHA } else { 1.0 };
    painter.ring(model, &model.outer, Level::Outer, OUTER, backdrop);
    painter.ring(model, &model.inner, Level::Inner, INNER, backdrop);
    if let Some(app) = model.expanded_app_index() {
        painter.window_arc(model, geometry, app);
    }
    painter.hub(model);
    painter.pixmap
}

impl Painter<'_> {
    fn ring(&mut self, model: &RingModel, items: &[RingItem], level: Level, radii: Radii, alpha: f32) {
        let selection = model.selection;
        let mut deferred = None;
        for (index, item) in items.iter().enumerate() {
            let selected = selection == Some(Selection { level, ..Selection::inner(index) });
            let parent = selection.is_some_and(|s| {
                s.level == level && s.index == index && (s.window.is_some() || s.tab.is_some() || !s.pointing)
            });
            let state = if selected { State::Selected } else if parent { State::Parent } else { State::Normal };
            let bounds = geometry::segment_bounds(index, items.len());
            let gap = if items.len() > 1 { 3.0 } else { 0.0 };
            if state == State::Selected {
                deferred = Some((item, bounds));
                continue;
            }
            self.segment(item, bounds, radii, gap, state, alpha, level, items.len());
        }
        if let Some((item, bounds)) = deferred {
            self.segment(item, bounds, radii, if items.len() > 1 { 3.0 } else { 0.0 }, State::Selected, alpha, level, items.len());
        }
    }

    fn window_arc(&mut self, model: &RingModel, geometry: &Geometry, app: usize) {
        let windows = &model.outer[app].windows;
        let bounds = geometry.window_arc_bounds(app, model.outer.len(), windows.len());
        let selection = model.selection;
        for (index, window) in windows.iter().enumerate() {
            let on_it = selection.is_some_and(|s| s.window == Some(index));
            let pointing = selection.is_some_and(|s| s.pointing && s.tab.is_none());
            let state = match (on_it, pointing) {
                (true, true) => State::Selected,
                (true, false) => State::Parent,
                _ => State::Normal,
            };
            self.window_segment(window, bounds[index], state);
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn segment(&mut self, item: &RingItem, bounds: Bounds, radii: Radii, gap: f32, state: State, alpha: f32, level: Level, count: usize) {
        let pop = self.pop(bounds, state);
        self.sector(bounds, radii, gap, state, alpha, pop);
        let mid_radius = (radii.0 + radii.1) / 2.0;
        let (lx, ly) = geometry::offset(bounds.mid(), mid_radius);
        let label_scale = if state == State::Selected { LABEL_SCALE } else { 1.0 };
        let at = (lx + pop.0, ly + pop.1);
        match level {
            Level::Outer => {
                let arc = 2.0 * std::f32::consts::PI * mid_radius / count.max(1) as f32;
                let size = (arc - 16.0).clamp(18.0, 42.0) * label_scale;
                self.icon(item, at, size, alpha);
            }
            Level::Inner => {
                self.icon(item, (at.0, at.1 - 12.0 * label_scale), 20.0 * label_scale, alpha);
                self.label(&item.short_title, (at.0, at.1 + 1.0), 62.0 * label_scale, 2, 9.5 * label_scale, state, alpha);
            }
        }
    }

    fn window_segment(&mut self, item: &RingItem, bounds: Bounds, state: State) {
        let pop = self.pop(bounds, state);
        self.sector(bounds, WINDOW_ARC, 3.0, state, 1.0, pop);
        let mid_radius = (WINDOW_ARC.0 + WINDOW_ARC.1) / 2.0;
        let (lx, ly) = geometry::offset(bounds.mid(), mid_radius);
        let scale = if state == State::Selected { LABEL_SCALE } else { 1.0 };
        let width = ((bounds.end - bounds.start) * mid_radius - 10.0) * scale;
        let at = (lx + pop.0, ly + pop.1);
        self.icon(item, (at.0, at.1 - 14.0 * scale), 30.0 * scale, 1.0);
        self.label(&item.short_title, (at.0, at.1 + 6.0 * scale), width, 2, 9.0 * scale, state, 1.0);
    }

    fn hub(&mut self, model: &RingModel) {
        let radius = geometry::DEAD_ZONE - 3.0;
        let mut pb = PathBuilder::new();
        let (cx, cy) = self.to_pixels((0.0, 0.0));
        pb.push_circle(cx, cy, radius * self.style.scale);
        let path = pb.finish().unwrap();
        self.pixmap.fill_path(&path, &self.fill_paint(State::Normal, 1.0), FillRule::Winding, Transform::identity(), None);
        self.pixmap.stroke_path(&path, &self.rim_paint(1.0), &self.rim_stroke(), Transform::identity(), None);
        if let Some(item) = model.selected_item() {
            let item = item.clone();
            self.icon(&item, (0.0, 0.0), 44.0, 1.0);
        }
    }

    fn pop(&self, bounds: Bounds, state: State) -> (f32, f32) {
        if state == State::Selected { geometry::offset(bounds.mid(), POP_DISTANCE) } else { (0.0, 0.0) }
    }

    fn sector(&mut self, bounds: Bounds, radii: Radii, gap: f32, state: State, alpha: f32, pop: (f32, f32)) {
        let Some(path) = self.sector_path(bounds, radii, gap, pop) else { return };
        self.pixmap.fill_path(&path, &self.fill_paint(state, alpha), FillRule::Winding, Transform::identity(), None);
        self.pixmap.stroke_path(&path, &self.rim_paint(alpha), &self.rim_stroke(), Transform::identity(), None);
    }

    fn sector_path(&self, bounds: Bounds, radii: Radii, gap: f32, pop: (f32, f32)) -> Option<tiny_skia::Path> {
        let outer_inset = gap / 2.0 / radii.1;
        let inner_inset = gap / 2.0 / radii.0;
        let (a0, a1) = (bounds.start + outer_inset, bounds.end - outer_inset);
        let (b0, b1) = (bounds.start + inner_inset, bounds.end - inner_inset);
        let steps = (((a1 - a0).abs() / 0.02).ceil() as usize).max(2);
        let mut pb = PathBuilder::new();
        for i in 0..=steps {
            let angle = a0 + (a1 - a0) * i as f32 / steps as f32;
            let (x, y) = self.to_pixels(add(geometry::offset(angle, radii.1), pop));
            if i == 0 { pb.move_to(x, y) } else { pb.line_to(x, y) }
        }
        for i in 0..=steps {
            let angle = b1 - (b1 - b0) * i as f32 / steps as f32;
            let (x, y) = self.to_pixels(add(geometry::offset(angle, radii.0), pop));
            pb.line_to(x, y);
        }
        pb.close();
        pb.finish()
    }

    fn icon(&mut self, item: &RingItem, at: (f32, f32), size: f32, alpha: f32) {
        let (x, y) = self.to_pixels(at);
        let size_px = size * self.style.scale;
        match &item.icon {
            Some(icon) => {
                let scale = size_px / icon.width().max(1) as f32;
                let transform = Transform::from_scale(scale, scale)
                    .post_translate(x - size_px / 2.0, y - size_px / 2.0);
                let paint = PixmapPaint { opacity: alpha, quality: FilterQuality::Bicubic, ..PixmapPaint::default() };
                self.pixmap.draw_pixmap(0, 0, icon.as_ref().as_ref(), &paint, transform, None);
            }
            None => {
                let mut pb = PathBuilder::new();
                pb.push_circle(x, y, size_px * 0.4);
                if let Some(path) = pb.finish() {
                    let mut paint = Paint::default();
                    paint.set_color(Color::from_rgba8(150, 150, 160, (200.0 * alpha) as u8));
                    paint.anti_alias = true;
                    self.pixmap.fill_path(&path, &paint, FillRule::Winding, Transform::identity(), None);
                }
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn label(&mut self, title: &str, at: (f32, f32), width: f32, lines: usize, size: f32, state: State, alpha: f32) {
        let (x, y) = self.to_pixels(at);
        let scale = self.style.scale;
        let base = if state == State::Selected || self.style.dark { [255, 255, 255] } else { [20, 20, 24] };
        let style = TextStyle {
            size: size * scale,
            color: [base[0], base[1], base[2], (255.0 * alpha) as u8],
            max_width: width.max(20.0) * scale,
            max_lines: lines,
            shadow: self.style.dark || state == State::Selected,
        };
        self.text.draw_centered(&mut self.pixmap, title, x, y, style);
    }

    fn fill_paint(&self, state: State, alpha: f32) -> Paint<'static> {
        let [r, g, b] = self.style.accent;
        let color = match (state, self.style.dark) {
            (State::Selected, _) => Color::from_rgba8(r, g, b, 225),
            (State::Parent, true) => Color::from_rgba8(mix(30, r, 0.35), mix(32, g, 0.35), mix(40, b, 0.35), 215),
            (State::Parent, false) => Color::from_rgba8(mix(245, r, 0.3), mix(246, g, 0.3), mix(250, b, 0.3), 225),
            (State::Normal, true) => Color::from_rgba8(30, 32, 40, 205),
            (State::Normal, false) => Color::from_rgba8(245, 246, 250, 215),
        };
        let mut paint = Paint::default();
        paint.set_color(with_alpha(color, alpha));
        paint.anti_alias = true;
        paint
    }

    fn rim_paint(&self, alpha: f32) -> Paint<'static> {
        let mut paint = Paint::default();
        let rim = if self.style.dark { Color::from_rgba8(255, 255, 255, 60) } else { Color::from_rgba8(0, 0, 0, 40) };
        paint.set_color(with_alpha(rim, alpha));
        paint.anti_alias = true;
        paint
    }

    fn rim_stroke(&self) -> Stroke {
        Stroke { width: self.style.scale.max(1.0), ..Stroke::default() }
    }

    fn to_pixels(&self, point: (f32, f32)) -> (f32, f32) {
        (self.center.0 + point.0 * self.style.scale, self.center.1 + point.1 * self.style.scale)
    }
}

fn add(a: (f32, f32), b: (f32, f32)) -> (f32, f32) {
    (a.0 + b.0, a.1 + b.1)
}

fn mix(base: u8, tint: u8, amount: f32) -> u8 {
    (base as f32 * (1.0 - amount) + tint as f32 * amount) as u8
}

fn with_alpha(color: Color, alpha: f32) -> Color {
    let mut color = color;
    color.apply_opacity(alpha);
    color
}
