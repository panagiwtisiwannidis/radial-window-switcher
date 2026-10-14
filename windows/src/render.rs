use std::collections::HashMap;
use std::f32::consts::PI;

use tiny_skia::{
    Color, FillRule, FilterQuality, GradientStop, LinearGradient, Paint, Path, PathBuilder,
    Pattern, Pixmap, PixmapPaint, Point, Shader, SpreadMode, Stroke, Transform,
};

use crate::geometry::{
    self, Bounds, Geometry, Level, Radii, Selection, INNER, OUTER, TAB_ARC, TAB_POPUP, WINDOW_ARC,
};
use crate::model::{Icon, RingItem, RingModel};
use crate::text::{Text, TextStyle};

pub const FRAME_RADIUS: f32 = TAB_ARC.1 + 44.0;

const BACKDROP_ALPHA: f32 = 0.35;
const SETTLE_TIME: f32 = 0.07;
const OPEN_START_SCALE: f32 = 0.82;

#[derive(Clone, Copy)]
pub struct Style {
    pub scale: f32,
    pub dark: bool,
    pub accent: [u8; 3],
}

pub struct Scene<'a> {
    pub model: &'a RingModel,
    pub geometry: &'a Geometry,
    pub text: &'a Text,
    pub style: Style,
    pub thumbnail: &'a dyn Fn(&RingItem) -> Option<Icon>,
    pub backdrop: Option<&'a Pixmap>,
    pub motion: &'a Motion,
}

pub struct Frame {
    pub pixmap: Pixmap,
    pub targets: Vec<(String, f32)>,
}

#[derive(Default)]
pub struct Motion {
    pub open: f32,
    values: HashMap<String, f32>,
}

impl Motion {
    pub fn reset(&mut self) {
        self.open = 0.0;
        self.values.clear();
    }

    pub fn value(&self, key: &str) -> f32 {
        self.values.get(key).copied().unwrap_or(0.0)
    }

    pub fn step(&mut self, targets: &[(String, f32)], dt: f32) -> bool {
        let blend = 1.0 - (-dt / SETTLE_TIME).exp();
        let mut moving = false;
        for (key, target) in targets {
            let value = self.values.entry(key.clone()).or_insert(0.0);
            *value += (target - *value) * blend;
            if (target - *value).abs() < 0.01 {
                *value = *target;
            } else {
                moving = true;
            }
        }
        self.values.retain(|key, value| {
            if targets.iter().any(|(k, _)| k == key) {
                return true;
            }
            *value -= *value * blend;
            if *value < 0.01 {
                return false;
            }
            moving = true;
            true
        });
        if self.open < 1.0 {
            self.open = (self.open + dt / 0.18).min(1.0);
            moving = true;
        }
        moving
    }

    pub fn settle(&mut self, targets: &[(String, f32)]) {
        self.open = 1.0;
        self.values = targets.iter().cloned().collect();
    }
}

#[derive(Clone, Copy, PartialEq)]
enum Emphasis {
    Normal,
    Tab,
}

impl Emphasis {
    fn pop(self) -> f32 {
        if self == Emphasis::Tab {
            10.0
        } else {
            7.0
        }
    }

    fn label_scale(self) -> f32 {
        if self == Emphasis::Tab {
            1.12
        } else {
            1.15
        }
    }

    fn segment_scale(self) -> f32 {
        if self == Emphasis::Tab {
            1.2
        } else {
            1.0
        }
    }
}

enum Label {
    Icon,
    IconAndTitle,
    Thumbnail {
        width: f32,
        thumb_height: f32,
        image: Option<Icon>,
        bold: bool,
    },
}

struct Segment<'a> {
    key: String,
    item: &'a RingItem,
    bounds: Bounds,
    radii: Radii,
    gap: f32,
    selected: bool,
    parent: bool,
    alpha: f32,
    arc_scale: f32,
    emphasis: Emphasis,
    label: Label,
    icon_size: f32,
}

pub fn frame_size(scale: f32) -> u32 {
    (2.0 * FRAME_RADIUS * scale).ceil() as u32
}

pub fn render(scene: &Scene) -> Frame {
    let size = frame_size(scene.style.scale);
    let mut painter = Painter {
        pixmap: Pixmap::new(size, size).expect("ring frame size"),
        center: (size as f32 / 2.0, size as f32 / 2.0),
        scene,
        targets: Vec::new(),
    };
    painter.draw();
    let targets = std::mem::take(&mut painter.targets);
    let pixmap = painter.pixmap;
    let open = scene.motion.open.clamp(0.0, 1.0);
    if open >= 1.0 {
        return Frame { pixmap, targets };
    }
    let eased = 1.0 - (1.0 - open).powi(3);
    let scale = OPEN_START_SCALE + (1.0 - OPEN_START_SCALE) * eased;
    let mut out = Pixmap::new(size, size).unwrap();
    let c = size as f32 / 2.0;
    let transform = Transform::from_translate(-c, -c)
        .post_scale(scale, scale)
        .post_translate(c, c);
    let paint = PixmapPaint {
        opacity: eased,
        quality: FilterQuality::Bilinear,
        ..PixmapPaint::default()
    };
    out.draw_pixmap(0, 0, pixmap.as_ref(), &paint, transform, None);
    Frame {
        pixmap: out,
        targets,
    }
}

struct Painter<'a, 'b> {
    pixmap: Pixmap,
    center: (f32, f32),
    scene: &'b Scene<'a>,
    targets: Vec<(String, f32)>,
}

impl Painter<'_, '_> {
    fn draw(&mut self) {
        let model = self.scene.model;
        let dim_target = if model.is_past_apps_ring() { 1.0 } else { 0.0 };
        let dim = self.track("dim", dim_target);
        let ring_alpha = 1.0 - (1.0 - BACKDROP_ALPHA) * dim;

        if let Some(window) = model.inner_tabs_window {
            let outer = model.outer.clone();
            self.ring(&outer, Level::Outer, OUTER, BACKDROP_ALPHA, false);
            let inner = model.inner.clone();
            self.ring(&inner, Level::Inner, INNER, 1.0, true);
            let tabs = model.inner_tab_items(window);
            let bounds =
                self.scene
                    .geometry
                    .tab_popup_bounds(window, model.inner.len(), tabs.len());
            let appear = self.track(&format!("arc-p{window}"), 1.0);
            self.fan(
                &tabs,
                &bounds,
                TAB_POPUP,
                &format!("p{window}"),
                appear,
                66.0,
                |s| s.level == Level::Outer,
                |s, i| s.index == i,
            );
        } else {
            let outer = model.outer.clone();
            self.ring(&outer, Level::Outer, OUTER, ring_alpha, true);
            let inner = model.inner.clone();
            self.ring(&inner, Level::Inner, INNER, ring_alpha, true);
            if let Some(app) = model.expanded_app_index() {
                let windows = model.outer[app].windows.clone();
                let bounds =
                    self.scene
                        .geometry
                        .window_arc_bounds(app, model.outer.len(), windows.len());
                let appear = self.track(&format!("arc-w{app}"), 1.0);
                self.fan(
                    &windows,
                    &bounds,
                    WINDOW_ARC,
                    &format!("w{app}"),
                    appear,
                    66.0,
                    |s| s.window.is_some(),
                    |s, i| s.window == Some(i) && s.tab.is_none(),
                );
            }
            if let Some((app, window, tabs)) = model.open_tab_arc() {
                let window_bounds = match model.outer[app].windows.len() {
                    n if n >= 2 => self
                        .scene
                        .geometry
                        .window_arc_bounds(app, model.outer.len(), n),
                    _ => Vec::new(),
                };
                if let Some(arc) = self.scene.geometry.tab_arc(
                    app,
                    model.outer.len(),
                    window,
                    &window_bounds,
                    tabs.len(),
                ) {
                    let key = format!("t{app}:{}", window.map_or(-1, |w| w as i64));
                    let appear = self.track(&format!("arc-{key}"), 1.0);
                    self.fan(
                        &tabs,
                        &arc.bounds,
                        arc.radii,
                        &key,
                        appear,
                        66.0,
                        |s| s.tab.is_some(),
                        |s, i| s.tab == Some(i),
                    );
                }
            }
        }
        self.hub();
    }

    fn track(&mut self, key: &str, target: f32) -> f32 {
        self.targets.push((key.to_string(), target));
        self.scene.motion.value(key)
    }

    fn ring(
        &mut self,
        items: &[RingItem],
        level: Level,
        radii: Radii,
        alpha: f32,
        interactive: bool,
    ) {
        let selection = self.scene.model.selection;
        let in_tab_mode = self.scene.model.inner_tabs_window;
        let mut segments = Vec::new();
        for (index, item) in items.iter().enumerate() {
            let exact = Selection {
                level,
                ..Selection::inner(index)
            };
            let selected = interactive && selection == Some(exact);
            let parent = interactive
                && (selection.is_some_and(|s| {
                    s.level == level
                        && s.index == index
                        && (s.window.is_some() || s.tab.is_some() || !s.pointing)
                }) || (level == Level::Inner
                    && in_tab_mode == Some(index)
                    && selection.is_some_and(|s| s.level == Level::Outer)));
            let bounds = geometry::segment_bounds(index, items.len());
            let mid = (radii.0 + radii.1) / 2.0;
            let arc = 2.0 * PI * mid / items.len().max(1) as f32;
            segments.push(Segment {
                key: format!("{}{index}", if level == Level::Inner { "i" } else { "o" }),
                item,
                bounds,
                radii,
                gap: if items.len() > 1 { 3.0 } else { 0.0 },
                selected,
                parent,
                alpha,
                arc_scale: 1.0,
                emphasis: Emphasis::Normal,
                label: if level == Level::Inner {
                    Label::IconAndTitle
                } else {
                    Label::Icon
                },
                icon_size: (arc - 16.0).clamp(18.0, 42.0),
            });
        }
        self.segments(segments);
    }

    #[allow(clippy::too_many_arguments)]
    fn fan(
        &mut self,
        items: &[RingItem],
        bounds: &[Bounds],
        radii: Radii,
        key: &str,
        appear: f32,
        thumb_height: f32,
        on_level: impl Fn(&Selection) -> bool,
        is_item: impl Fn(&Selection, usize) -> bool,
    ) {
        let selection = self.scene.model.selection;
        let mid = (radii.0 + radii.1) / 2.0;
        let segments = items
            .iter()
            .zip(bounds)
            .enumerate()
            .map(|(index, (item, b))| {
                let on_it = selection.is_some_and(|s| on_level(&s) && is_item(&s, index));
                let pointing = selection.is_some_and(|s| s.pointing);
                let deeper = selection.is_some_and(|s| s.window == Some(index) && s.tab.is_some());
                Segment {
                    key: format!("{key}:{index}"),
                    item,
                    bounds: *b,
                    radii,
                    gap: 3.0,
                    selected: on_it && pointing,
                    parent: (on_it && !pointing) || deeper,
                    alpha: appear,
                    arc_scale: 0.92 + 0.08 * appear,
                    emphasis: Emphasis::Tab,
                    label: Label::Thumbnail {
                        width: (b.end - b.start) * mid - 10.0,
                        thumb_height,
                        image: (self.scene.thumbnail)(item),
                        bold: item.tab().is_some_and(|t| t.selected),
                    },
                    icon_size: 18.0,
                }
            })
            .collect();
        self.segments(segments);
    }

    fn segments(&mut self, segments: Vec<Segment>) {
        let (selected, rest): (Vec<_>, Vec<_>) = segments.into_iter().partition(|s| s.selected);
        let mut ordered: Vec<(Segment, f32)> = Vec::new();
        for segment in rest.into_iter().chain(selected) {
            let h = self.track(
                &segment.key.clone(),
                if segment.selected { 1.0 } else { 0.0 },
            );
            ordered.push((segment, h));
        }
        ordered.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));
        for (segment, h) in ordered {
            self.segment(&segment, h);
        }
    }

    fn segment(&mut self, s: &Segment, h: f32) {
        let scale = self.scene.style.scale;
        let mid_radius = (s.radii.0 + s.radii.1) / 2.0;
        let pop = geometry::offset(s.bounds.mid(), s.emphasis.pop() * h);
        let seg_center = self.to_pixels(geometry::offset(s.bounds.mid(), mid_radius));
        let seg_scale = 1.0 + (s.emphasis.segment_scale() - 1.0) * h;
        let (cx, cy) = self.center;
        let transform = Transform::from_translate(-cx, -cy)
            .post_scale(s.arc_scale, s.arc_scale)
            .post_translate(cx, cy)
            .post_translate(-seg_center.0, -seg_center.1)
            .post_scale(seg_scale, seg_scale)
            .post_translate(seg_center.0 + pop.0 * scale, seg_center.1 + pop.1 * scale);

        if let Some(path) = self.sector_path(s.bounds, s.radii, s.gap) {
            self.glass(&path, transform, s.alpha, h, s.parent);
        }

        let mut at = Point::from_xy(seg_center.0, seg_center.1);
        transform.map_points(std::slice::from_mut(&mut at));
        let size_scale = s.arc_scale * seg_scale * (1.0 + (s.emphasis.label_scale() - 1.0) * h);
        let alpha = s.alpha;
        let selected_text = h > 0.5;
        match &s.label {
            Label::Icon => self.icon(
                s.item.icon.as_ref(),
                (at.x, at.y),
                s.icon_size * size_scale,
                alpha,
            ),
            Label::IconAndTitle => {
                self.icon(
                    s.item.icon.as_ref(),
                    (at.x, at.y - 12.0 * scale * size_scale),
                    20.0 * size_scale,
                    alpha,
                );
                self.label(
                    &s.item.short_title,
                    (at.x, at.y + scale * size_scale),
                    62.0 * size_scale,
                    2,
                    9.5 * size_scale,
                    false,
                    selected_text,
                    alpha,
                );
            }
            Label::Thumbnail {
                width,
                thumb_height,
                image,
                bold,
            } => {
                let width = width.max(20.0) * size_scale;
                let lines = if image.is_some() { 1 } else { 2 };
                let title_height = 9.0 * 1.25 * lines as f32 * size_scale;
                match image {
                    Some(image) => {
                        let box_h = thumb_height * size_scale;
                        let total = box_h + 3.0 * size_scale + title_height;
                        let top = at.y - total / 2.0 * scale;
                        self.thumbnail(image, (at.x, top), width, box_h, alpha);
                        self.label(
                            &s.item.short_title,
                            (at.x, top + (box_h + 3.0 * size_scale) * scale),
                            width,
                            1,
                            9.0 * size_scale,
                            *bold,
                            selected_text,
                            alpha,
                        );
                    }
                    None => {
                        let total = 18.0 * size_scale + 3.0 * size_scale + title_height;
                        let top = at.y - total / 2.0 * scale;
                        self.icon(
                            s.item.icon.as_ref(),
                            (at.x, top + 9.0 * size_scale * scale),
                            18.0 * size_scale,
                            alpha,
                        );
                        self.label(
                            &s.item.short_title,
                            (at.x, top + 21.0 * size_scale * scale),
                            width,
                            2,
                            9.0 * size_scale,
                            *bold,
                            selected_text,
                            alpha,
                        );
                    }
                }
            }
        }
    }

    fn glass(&mut self, path: &Path, transform: Transform, alpha: f32, h: f32, parent: bool) {
        let style = self.scene.style;
        if let Some(backdrop) = self.scene.backdrop {
            let inverse = transform.invert().unwrap_or_default();
            let mut paint = Paint {
                anti_alias: true,
                ..Paint::default()
            };
            paint.shader = Pattern::new(
                backdrop.as_ref(),
                SpreadMode::Pad,
                FilterQuality::Bilinear,
                alpha,
                inverse,
            );
            self.pixmap
                .fill_path(path, &paint, FillRule::Winding, transform, None);
        }
        let base = if style.dark {
            [24.0, 26.0, 34.0, 150.0]
        } else {
            [248.0, 249.0, 252.0, 165.0]
        };
        let base_alpha = if self.scene.backdrop.is_some() {
            base[3]
        } else {
            215.0
        };
        let [r, g, b] = style.accent.map(|c| c as f32);
        let tint = if parent { 0.35 } else { 0.0 };
        let k = (tint + (1.0 - tint) * h * 0.85).min(1.0);
        let mix = |a: f32, b: f32| a + (b - a) * k;
        let color = Color::from_rgba8(
            mix(base[0], r) as u8,
            mix(base[1], g) as u8,
            mix(base[2], b) as u8,
            ((base_alpha + (235.0 - base_alpha) * k) * alpha) as u8,
        );
        let mut paint = Paint {
            anti_alias: true,
            ..Paint::default()
        };
        paint.set_color(color);
        self.pixmap
            .fill_path(path, &paint, FillRule::Winding, transform, None);

        let size = self.pixmap.width() as f32;
        let rim = if style.dark {
            [150u8, 40, 12]
        } else {
            [110, 30, 10]
        };
        let shine = |a: u8| {
            let mut c = if style.dark {
                Color::from_rgba8(255, 255, 255, a)
            } else {
                Color::from_rgba8(0, 0, 0, a / 2)
            };
            c.apply_opacity(alpha);
            c
        };
        let mut paint = Paint {
            anti_alias: true,
            ..Paint::default()
        };
        paint.shader = LinearGradient::new(
            Point::from_xy(0.0, 0.0),
            Point::from_xy(size, size),
            vec![
                GradientStop::new(0.0, shine(rim[0])),
                GradientStop::new(0.5, shine(rim[1])),
                GradientStop::new(1.0, shine(rim[2])),
            ],
            SpreadMode::Pad,
            Transform::identity(),
        )
        .unwrap_or(Shader::SolidColor(shine(rim[1])));
        let stroke = Stroke {
            width: style.scale.max(1.0),
            ..Stroke::default()
        };
        self.pixmap
            .stroke_path(path, &paint, &stroke, transform, None);
    }

    fn hub(&mut self) {
        let scale = self.scene.style.scale;
        let (cx, cy) = self.center;
        let mut pb = PathBuilder::new();
        pb.push_circle(cx, cy, (geometry::DEAD_ZONE - 3.0) * scale);
        if let Some(path) = pb.finish() {
            self.glass(&path, Transform::identity(), 1.0, 0.0, false);
        }
        if let Some(item) = self.scene.model.selected_item() {
            self.icon(item.icon.as_ref(), (cx, cy), 44.0, 1.0);
        }
    }

    fn sector_path(&self, bounds: Bounds, radii: Radii, gap: f32) -> Option<Path> {
        let outer_inset = gap / 2.0 / radii.1;
        let inner_inset = gap / 2.0 / radii.0;
        let (a0, a1) = (bounds.start + outer_inset, bounds.end - outer_inset);
        let (b0, b1) = (bounds.start + inner_inset, bounds.end - inner_inset);
        let steps = (((a1 - a0).abs() / 0.02).ceil() as usize).max(2);
        let mut pb = PathBuilder::new();
        for i in 0..=steps {
            let angle = a0 + (a1 - a0) * i as f32 / steps as f32;
            let (x, y) = self.to_pixels(geometry::offset(angle, radii.1));
            if i == 0 {
                pb.move_to(x, y)
            } else {
                pb.line_to(x, y)
            }
        }
        for i in 0..=steps {
            let angle = b1 - (b1 - b0) * i as f32 / steps as f32;
            let (x, y) = self.to_pixels(geometry::offset(angle, radii.0));
            pb.line_to(x, y);
        }
        pb.close();
        pb.finish()
    }

    fn icon(&mut self, icon: Option<&Icon>, at: (f32, f32), size: f32, alpha: f32) {
        let size_px = size * self.scene.style.scale;
        match icon {
            Some(icon) => {
                let scale = size_px / icon.width().max(1) as f32;
                let transform = Transform::from_scale(scale, scale)
                    .post_translate(at.0 - size_px / 2.0, at.1 - size_px / 2.0);
                let paint = PixmapPaint {
                    opacity: alpha,
                    quality: FilterQuality::Bicubic,
                    ..PixmapPaint::default()
                };
                self.pixmap
                    .draw_pixmap(0, 0, icon.as_ref().as_ref(), &paint, transform, None);
            }
            None => {
                let mut pb = PathBuilder::new();
                pb.push_circle(at.0, at.1, size_px * 0.4);
                if let Some(path) = pb.finish() {
                    let mut paint = Paint {
                        anti_alias: true,
                        ..Paint::default()
                    };
                    paint.set_color(Color::from_rgba8(150, 150, 160, (200.0 * alpha) as u8));
                    self.pixmap.fill_path(
                        &path,
                        &paint,
                        FillRule::Winding,
                        Transform::identity(),
                        None,
                    );
                }
            }
        }
    }

    fn thumbnail(
        &mut self,
        image: &Icon,
        top_center: (f32, f32),
        width: f32,
        height: f32,
        alpha: f32,
    ) {
        let scale = self.scene.style.scale;
        let (box_w, box_h) = (width * scale, height * scale);
        let fit = (box_w / image.width().max(1) as f32).min(box_h / image.height().max(1) as f32);
        let (w, h) = (image.width() as f32 * fit, image.height() as f32 * fit);
        let left = top_center.0 - w / 2.0;
        let top = top_center.1 + (box_h - h) / 2.0;
        let transform = Transform::from_scale(fit, fit).post_translate(left, top);
        let paint = PixmapPaint {
            opacity: alpha,
            quality: FilterQuality::Bilinear,
            ..PixmapPaint::default()
        };
        self.pixmap
            .draw_pixmap(0, 0, image.as_ref().as_ref(), &paint, transform, None);
    }

    #[allow(clippy::too_many_arguments)]
    fn label(
        &mut self,
        title: &str,
        top_center: (f32, f32),
        width: f32,
        lines: usize,
        size: f32,
        bold: bool,
        selected: bool,
        alpha: f32,
    ) {
        let scale = self.scene.style.scale;
        let dark = self.scene.style.dark;
        let base = if selected || dark {
            [255, 255, 255]
        } else {
            [20, 20, 24]
        };
        let style = TextStyle {
            size: size * scale * if bold { 1.05 } else { 1.0 },
            color: [base[0], base[1], base[2], (255.0 * alpha) as u8],
            max_width: width.max(20.0) * scale,
            max_lines: lines,
            shadow: dark || selected,
        };
        self.scene
            .text
            .draw_centered(&mut self.pixmap, title, top_center.0, top_center.1, style);
    }

    fn to_pixels(&self, point: (f32, f32)) -> (f32, f32) {
        let scale = self.scene.style.scale;
        (
            self.center.0 + point.0 * scale,
            self.center.1 + point.1 * scale,
        )
    }
}

pub fn blur_backdrop(source: &Pixmap) -> Pixmap {
    let factor = 4u32;
    let (w, h) = (
        (source.width() / factor).max(1),
        (source.height() / factor).max(1),
    );
    let mut small = Pixmap::new(w, h).unwrap();
    let paint = PixmapPaint {
        quality: FilterQuality::Bilinear,
        ..PixmapPaint::default()
    };
    let shrink = 1.0 / factor as f32;
    small.draw_pixmap(
        0,
        0,
        source.as_ref(),
        &paint,
        Transform::from_scale(shrink, shrink),
        None,
    );
    for _ in 0..3 {
        box_blur(&mut small, 4);
    }
    let mut out = Pixmap::new(source.width(), source.height()).unwrap();
    let grow = factor as f32;
    out.draw_pixmap(
        0,
        0,
        small.as_ref(),
        &paint,
        Transform::from_scale(grow, grow),
        None,
    );
    out
}

fn box_blur(pixmap: &mut Pixmap, radius: i32) {
    let (w, h) = (pixmap.width() as i32, pixmap.height() as i32);
    let mut scratch = pixmap.data().to_vec();
    for pass in 0..2 {
        let src = if pass == 0 {
            pixmap.data().to_vec()
        } else {
            scratch.clone()
        };
        let dst: &mut [u8] = if pass == 0 {
            &mut scratch
        } else {
            pixmap.data_mut()
        };
        let (outer, inner) = if pass == 0 { (h, w) } else { (w, h) };
        for o in 0..outer {
            for i in 0..inner {
                let mut sum = [0u32; 4];
                let mut count = 0;
                for d in -radius..=radius {
                    let j = (i + d).clamp(0, inner - 1);
                    let (x, y) = if pass == 0 { (j, o) } else { (o, j) };
                    let p = ((y * w + x) * 4) as usize;
                    for c in 0..4 {
                        sum[c] += src[p + c] as u32;
                    }
                    count += 1;
                }
                let (x, y) = if pass == 0 { (i, o) } else { (o, i) };
                let p = ((y * w + x) * 4) as usize;
                for c in 0..4 {
                    dst[p + c] = (sum[c] / count) as u8;
                }
            }
        }
    }
}
