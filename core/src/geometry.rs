use std::f32::consts::PI;

pub type Radii = (f32, f32);

pub const DEAD_ZONE: f32 = 45.0;
pub const INNER: Radii = (50.0, 125.0);
pub const OUTER: Radii = (132.0, 210.0);
pub const WINDOW_ARC: Radii = (216.0, 322.0);
pub const TAB_ARC: Radii = (328.0, 434.0);
pub const SINGLE_WINDOW_TAB_ARC: Radii = (216.0, 322.0);
pub const TAB_POPUP: Radii = (214.0, 320.0);
pub const MAX_RADIUS: f32 = TAB_ARC.1;
pub const OUTSIDE_SLACK: f32 = 20.0;
pub const MAX_INNER_ITEMS: usize = 8;
pub const POP_OUT_ALLOWANCE: f32 = 30.0;

const WINDOW_ARC_SEGMENT: f32 = 0.36;
const TAB_ARC_SEGMENT: f32 = 0.26;
const SINGLE_WINDOW_TAB_ARC_SEGMENT: f32 = 0.36;
const TAB_POPUP_SEGMENT: f32 = 0.36;
const WIDE_FAN_MAX_SPAN: f32 = 1.6 * PI;
const AIM_MARGIN: f32 = 0.2;
const AIM_SAMPLES: usize = 24;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Bounds {
    pub start: f32,
    pub end: f32,
}

impl Bounds {
    pub fn mid(&self) -> f32 {
        (self.start + self.end) / 2.0
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Aim {
    pub arc: Bounds,
    pub arc_inner: f32,
}

impl Aim {
    pub fn heading(&self, from: (f32, f32), to: (f32, f32)) -> bool {
        let (mx, my) = (to.0 - from.0, to.1 - from.1);
        if to.0.hypot(to.1) >= self.arc_inner || mx.hypot(my) <= 0.0 {
            return false;
        }
        let direction = my.atan2(mx);
        let wrap = |a: f32| (a + PI).rem_euclid(2.0 * PI) - PI;
        let relative: Vec<f32> = (0..=AIM_SAMPLES)
            .map(|k| {
                let angle = self.arc.start
                    + (self.arc.end - self.arc.start) * k as f32 / AIM_SAMPLES as f32;
                let (px, py) = (self.arc_inner * angle.sin(), self.arc_inner * angle.cos());
                wrap((py - to.1).atan2(px - to.0) - direction)
            })
            .collect();
        relative.iter().any(|r| r.abs() <= AIM_MARGIN)
            || relative
                .windows(2)
                .any(|w| w[0].signum() != w[1].signum() && (w[0] - w[1]).abs() < PI)
    }
}

pub fn span(bounds: &[Bounds]) -> Option<Bounds> {
    Some(Bounds {
        start: bounds.first()?.start,
        end: bounds.last()?.end,
    })
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Level {
    Inner,
    Outer,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Selection {
    pub level: Level,
    pub index: usize,
    pub window: Option<usize>,
    pub tab: Option<usize>,
    pub pointing: bool,
}

impl Selection {
    pub fn inner(index: usize) -> Self {
        Self {
            level: Level::Inner,
            index,
            window: None,
            tab: None,
            pointing: true,
        }
    }

    pub fn outer(index: usize) -> Self {
        Self {
            level: Level::Outer,
            index,
            window: None,
            tab: None,
            pointing: true,
        }
    }

    pub fn window(app: usize, window: usize) -> Self {
        Self {
            window: Some(window),
            ..Self::outer(app)
        }
    }

    pub fn position(&self) -> Self {
        Self {
            tab: None,
            pointing: true,
            ..*self
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Area {
    pub min_x: f32,
    pub min_y: f32,
    pub max_x: f32,
    pub max_y: f32,
}

impl Area {
    fn contains(&self, x: f32, y: f32) -> bool {
        x >= self.min_x && x <= self.max_x && y >= self.min_y && y <= self.max_y
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct TabArc {
    pub radii: Radii,
    pub inner_edge: f32,
    pub bounds: Vec<Bounds>,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Geometry {
    pub visible: Option<Area>,
}

impl Geometry {
    pub fn selection(
        &self,
        dx: f32,
        dy: f32,
        inner_count: usize,
        outer_window_counts: &[usize],
        tab_count: &dyn Fn(usize, Option<usize>) -> usize,
        current: Option<Selection>,
    ) -> Option<Selection> {
        let distance = dx.hypot(dy);
        let angle = clockwise_angle_from_top(dx, dy);
        let outer_count = outer_window_counts.len();

        if let Some(current) = current.filter(|c| c.level == Level::Outer && c.index < outer_count)
        {
            let app = current.index;
            let window_count = outer_window_counts[app];
            let has_window_arc = window_count >= 2;
            let window_bounds = if has_window_arc {
                self.window_arc_bounds(app, outer_count, window_count)
            } else {
                Vec::new()
            };
            let keep_current = Selection {
                tab: None,
                pointing: false,
                ..current
            };

            if let Some(arc) = self.tab_arc(
                app,
                outer_count,
                current.window,
                &window_bounds,
                tab_count(app, current.window),
            ) {
                if distance > arc.inner_edge && distance <= arc.radii.1 + OUTSIDE_SLACK {
                    return Some(match fan_index(angle, &arc.bounds) {
                        Some(tab) => Selection {
                            tab: Some(tab),
                            pointing: true,
                            ..current
                        },
                        None => keep_current,
                    });
                }
            }

            if has_window_arc && distance > OUTER.1 && distance <= MAX_RADIUS + OUTSIDE_SLACK {
                if distance <= WINDOW_ARC.1 {
                    if let Some(window) = fan_index(angle, &window_bounds) {
                        return Some(Selection::window(app, window));
                    }
                }
                return Some(keep_current);
            }
        }

        if distance <= DEAD_ZONE || distance > OUTER.1 + OUTSIDE_SLACK {
            return None;
        }
        let ring_boundary = (INNER.1 + OUTER.0) / 2.0;
        if inner_count > 0 && (distance < ring_boundary || outer_count == 0) {
            return Some(Selection::inner(segment_index(angle, inner_count)));
        }
        if outer_count > 0 {
            return Some(Selection::outer(segment_index(angle, outer_count)));
        }
        None
    }

    pub fn tab_popup_selection(
        &self,
        dx: f32,
        dy: f32,
        inner_count: usize,
        inner_index: usize,
        tab_count: usize,
    ) -> Option<Selection> {
        let distance = dx.hypot(dy);
        if distance <= DEAD_ZONE || distance > TAB_POPUP.1 + OUTSIDE_SLACK {
            return None;
        }
        let angle = clockwise_angle_from_top(dx, dy);
        if distance < (INNER.1 + OUTER.0) / 2.0 {
            return Some(Selection::inner(segment_index(angle, inner_count)));
        }
        let bounds = self.tab_popup_bounds(inner_index, inner_count, tab_count);
        fan_index(angle, &bounds).map(Selection::outer)
    }

    pub fn window_arc_bounds(
        &self,
        app: usize,
        app_count: usize,
        window_count: usize,
    ) -> Vec<Bounds> {
        let center = app as f32 * 2.0 * PI / app_count as f32;
        self.on_screen(
            fan(center, window_count, WINDOW_ARC_SEGMENT, WIDE_FAN_MAX_SPAN),
            WINDOW_ARC.1,
        )
    }

    pub fn tab_popup_bounds(
        &self,
        inner_index: usize,
        inner_count: usize,
        tab_count: usize,
    ) -> Vec<Bounds> {
        let center = inner_index as f32 * 2.0 * PI / inner_count as f32;
        self.on_screen(
            fan(center, tab_count, TAB_POPUP_SEGMENT, WIDE_FAN_MAX_SPAN),
            TAB_POPUP.1,
        )
    }

    pub fn tab_arc(
        &self,
        app: usize,
        app_count: usize,
        window: Option<usize>,
        window_bounds: &[Bounds],
        tab_count: usize,
    ) -> Option<TabArc> {
        if tab_count < 2 {
            return None;
        }
        if window_bounds.is_empty() {
            let center = app as f32 * 2.0 * PI / app_count as f32;
            let bounds = fan(
                center,
                tab_count,
                SINGLE_WINDOW_TAB_ARC_SEGMENT,
                WIDE_FAN_MAX_SPAN,
            );
            return Some(TabArc {
                radii: SINGLE_WINDOW_TAB_ARC,
                inner_edge: OUTER.1,
                bounds: self.on_screen(bounds, SINGLE_WINDOW_TAB_ARC.1),
            });
        }
        let window = window.filter(|w| *w < window_bounds.len())?;
        let bounds = fan(
            window_bounds[window].mid(),
            tab_count,
            TAB_ARC_SEGMENT,
            WIDE_FAN_MAX_SPAN,
        );
        Some(TabArc {
            radii: TAB_ARC,
            inner_edge: WINDOW_ARC.1,
            bounds: self.on_screen(bounds, TAB_ARC.1),
        })
    }

    fn on_screen(&self, bounds: Vec<Bounds>, outer_radius: f32) -> Vec<Bounds> {
        let Some(area) = self.visible else {
            return bounds;
        };
        if bounds.is_empty() {
            return bounds;
        }
        let radius = outer_radius + POP_OUT_ALLOWANCE;
        let fits = |shift: f32| {
            bounds.iter().all(|b| {
                [b.start, b.mid(), b.end]
                    .iter()
                    .all(|a| area.contains(radius * (a + shift).sin(), radius * (a + shift).cos()))
            })
        };
        if fits(0.0) {
            return bounds;
        }
        for step in 1..=63 {
            let shift = step as f32 * 0.05;
            for candidate in [shift, -shift] {
                if fits(candidate) {
                    return bounds
                        .iter()
                        .map(|b| Bounds {
                            start: b.start + candidate,
                            end: b.end + candidate,
                        })
                        .collect();
                }
            }
        }
        bounds
    }
}

pub fn clockwise_angle_from_top(dx: f32, dy: f32) -> f32 {
    let angle = dx.atan2(dy);
    if angle < 0.0 {
        angle + 2.0 * PI
    } else {
        angle
    }
}

pub fn segment_index(angle: f32, count: usize) -> usize {
    let segment = 2.0 * PI / count as f32;
    ((angle + segment / 2.0) / segment) as usize % count
}

pub fn segment_bounds(index: usize, count: usize) -> Bounds {
    let segment = 2.0 * PI / count as f32;
    let mid = index as f32 * segment;
    Bounds {
        start: mid - segment / 2.0,
        end: mid + segment / 2.0,
    }
}

pub fn fan(center: f32, count: usize, segment: f32, max_span: f32) -> Vec<Bounds> {
    let segment = segment.min(max_span / count as f32);
    let start = center - segment * count as f32 / 2.0;
    (0..count)
        .map(|i| Bounds {
            start: start + i as f32 * segment,
            end: start + (i + 1) as f32 * segment,
        })
        .collect()
}

pub fn fan_index(angle: f32, bounds: &[Bounds]) -> Option<usize> {
    let (first, last) = (bounds.first()?, bounds.last()?);
    let segment = first.end - first.start;
    let span = last.end - first.start;
    let origin = first.start - segment / 2.0;
    let mut relative = (angle - origin) % (2.0 * PI);
    if relative < 0.0 {
        relative += 2.0 * PI;
    }
    if relative > span + segment {
        return None;
    }
    let index = ((relative - segment / 2.0) / segment).max(0.0) as usize;
    Some(index.min(bounds.len() - 1))
}

pub fn offset(angle: f32, radius: f32) -> (f32, f32) {
    (radius * angle.sin(), -radius * angle.cos())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn moving_toward_a_rotated_arc_counts_and_moving_away_does_not() {
        let aim = Aim {
            arc: Bounds {
                start: 0.6,
                end: 2.0,
            },
            arc_inner: TAB_POPUP.0,
        };
        let start = (0.0, 90.0);
        assert!(aim.heading(start, (8.0, 88.0)));
        assert!(aim.heading(start, (6.0, 84.0)));
        assert!(!aim.heading(start, (-8.0, 88.0)));
        assert!(!aim.heading(start, (0.0, 98.0)));
        assert!(!aim.heading((0.0, 200.0), (10.0, 220.0)));
    }

    fn no_tabs(_: usize, _: Option<usize>) -> usize {
        0
    }

    fn select(
        dx: f32,
        dy: f32,
        inner: usize,
        outer: &[usize],
        current: Option<Selection>,
    ) -> Option<Selection> {
        Geometry::default().selection(dx, dy, inner, outer, &no_tabs, current)
    }

    #[test]
    fn segment_zero_is_at_the_top_and_indices_go_clockwise() {
        assert_eq!(segment_index(clockwise_angle_from_top(0.0, 1.0), 4), 0);
        assert_eq!(segment_index(clockwise_angle_from_top(1.0, 0.0), 4), 1);
        assert_eq!(segment_index(clockwise_angle_from_top(0.0, -1.0), 4), 2);
        assert_eq!(segment_index(clockwise_angle_from_top(-1.0, 0.0), 4), 3);
    }

    #[test]
    fn dead_zone_and_outside_select_nothing() {
        assert_eq!(select(0.0, 30.0, 3, &[1, 1], None), None);
        assert_eq!(select(0.0, 300.0, 3, &[1, 1], None), None);
    }

    #[test]
    fn rings_are_picked_by_distance() {
        assert_eq!(
            select(0.0, 90.0, 3, &[1, 1, 1, 1], None),
            Some(Selection::inner(0))
        );
        assert_eq!(
            select(170.0, 0.0, 3, &[1, 1, 1, 1], None),
            Some(Selection::outer(1))
        );
    }

    #[test]
    fn window_arc_picks_the_window_under_the_cursor() {
        let current = Some(Selection::outer(0));
        let picked = select(0.0, 269.0, 2, &[3, 1, 1, 1], current);
        assert_eq!(picked, Some(Selection::window(0, 1)));
    }

    #[test]
    fn off_the_window_arc_keeps_the_app_without_pointing() {
        let current = Some(Selection::outer(0));
        let picked = select(269.0, 0.0, 2, &[3, 1, 1, 1], current).unwrap();
        assert_eq!(
            (picked.level, picked.index, picked.pointing),
            (Level::Outer, 0, false)
        );
    }

    #[test]
    fn far_end_of_a_wide_arc_is_reachable() {
        let bounds = fan(0.0, 13, 0.30, WIDE_FAN_MAX_SPAN);
        let far = bounds[12].mid();
        assert_eq!(fan_index(far, &bounds), Some(12));
        assert_eq!(fan_index(bounds[0].mid(), &bounds), Some(0));
    }

    #[test]
    fn arcs_rotate_back_on_screen_near_an_edge() {
        let geometry = Geometry {
            visible: Some(Area {
                min_x: -900.0,
                min_y: -900.0,
                max_x: 900.0,
                max_y: 120.0,
            }),
        };
        let bounds = geometry.window_arc_bounds(0, 8, 3);
        let radius = WINDOW_ARC.1 + POP_OUT_ALLOWANCE;
        for b in &bounds {
            assert!(radius * b.mid().cos() <= 120.0);
        }
    }

    #[test]
    fn tab_popup_selects_tabs_past_the_inner_ring() {
        let geometry = Geometry::default();
        assert_eq!(
            geometry.tab_popup_selection(0.0, 260.0, 2, 0, 5),
            Some(Selection::outer(2))
        );
        assert_eq!(
            geometry.tab_popup_selection(0.0, 90.0, 2, 0, 5),
            Some(Selection::inner(0))
        );
    }
}
