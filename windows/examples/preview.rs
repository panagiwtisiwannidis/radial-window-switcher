use std::sync::Arc;

use radial_switcher::geometry::{Geometry, Selection};
use radial_switcher::model::{AppWindows, Icon, RingItem, RingModel, TabInfo, WindowRef};
use radial_switcher::render::{self, Motion, Scene, Style};
use radial_switcher::text::Text;
use tiny_skia::{
    Color, FillRule, GradientStop, LinearGradient, Paint, PathBuilder, Pixmap, Point, SpreadMode,
    Transform,
};

fn solid(width: u32, height: u32, r: u8, g: u8, b: u8, mark: bool) -> Arc<Pixmap> {
    let mut pixmap = Pixmap::new(width, height).unwrap();
    pixmap.fill(Color::from_rgba8(r, g, b, 255));
    if mark {
        let mut pb = PathBuilder::new();
        pb.push_circle(
            width as f32 / 2.0,
            height as f32 / 2.0,
            width.min(height) as f32 * 0.22,
        );
        let mut paint = Paint {
            anti_alias: true,
            ..Paint::default()
        };
        paint.set_color(Color::from_rgba8(255, 255, 255, 230));
        pixmap.fill_path(
            &pb.finish().unwrap(),
            &paint,
            FillRule::Winding,
            Transform::identity(),
            None,
        );
    }
    Arc::new(pixmap)
}

fn app(name: &str, pid: u32, titles: &[&str], color: (u8, u8, u8)) -> AppWindows {
    AppWindows {
        name: name.into(),
        icon: Some(solid(64, 64, color.0, color.1, color.2, true)),
        windows: titles
            .iter()
            .enumerate()
            .map(|(i, t)| WindowRef {
                handle: (pid * 10 + i as u32) as isize,
                pid,
                title: t.to_string(),
                minimized: false,
            })
            .collect(),
    }
}

fn wallpaper(size: u32) -> Pixmap {
    let mut pixmap = Pixmap::new(size, size).unwrap();
    let paint = Paint {
        shader: LinearGradient::new(
            Point::from_xy(0.0, 0.0),
            Point::from_xy(size as f32, size as f32),
            vec![
                GradientStop::new(0.0, Color::from_rgba8(40, 90, 200, 255)),
                GradientStop::new(0.5, Color::from_rgba8(200, 70, 140, 255)),
                GradientStop::new(1.0, Color::from_rgba8(240, 170, 60, 255)),
            ],
            SpreadMode::Pad,
            Transform::identity(),
        )
        .unwrap(),
        ..Paint::default()
    };
    pixmap.fill_rect(
        tiny_skia::Rect::from_xywh(0.0, 0.0, size as f32, size as f32).unwrap(),
        &paint,
        Transform::identity(),
        None,
    );
    for i in 0..12 {
        let mut pb = PathBuilder::new();
        pb.push_rect(
            tiny_skia::Rect::from_xywh(
                i as f32 * size as f32 / 12.0,
                size as f32 * 0.3,
                18.0,
                size as f32 * 0.4,
            )
            .unwrap(),
        );
        let mut p = Paint::default();
        p.set_color(Color::from_rgba8(255, 255, 255, 160));
        pixmap.fill_path(
            &pb.finish().unwrap(),
            &p,
            FillRule::Winding,
            Transform::identity(),
            None,
        );
    }
    pixmap
}

fn main() {
    let out = std::env::args().nth(1).unwrap_or_else(|| ".".into());
    let text = Text::system().expect("no usable system font");
    let front = app(
        "Edge",
        1,
        &["GitHub — radial-window-switcher", "YouTube — Music", "Docs"],
        (20, 160, 140),
    );
    let others = vec![
        app(
            "Code",
            2,
            &["main.rs — radial-switcher", "README.md", "Settings"],
            (40, 120, 220),
        ),
        app("Explorer", 3, &["Downloads"], (230, 180, 40)),
        app("Terminal", 4, &["PowerShell", "Ubuntu"], (60, 60, 70)),
        app("Teams", 5, &["Chat"], (90, 80, 200)),
        app("Spotify", 6, &["Spotify Premium"], (30, 200, 90)),
        app("Outlook", 7, &["Inbox"], (20, 110, 200)),
        app("Notepad", 8, &["notes.txt"], (120, 160, 200)),
        app("Photos", 9, &["IMG_2041.jpg"], (220, 80, 120)),
    ];
    let thumb = |item: &RingItem| -> Option<Icon> {
        let seed = item.short_title.len() as u8;
        Some(solid(320, 200, 60 + seed * 7, 90 + seed * 3, 140, false))
    };
    let tabs = |n: usize| -> Vec<TabInfo> {
        [
            "GitHub",
            "YouTube — Music",
            "Weather",
            "News",
            "Maps",
            "Mail",
            "Docs",
        ]
        .iter()
        .take(n)
        .enumerate()
        .map(|(i, t)| TabInfo {
            title: t.to_string(),
            selected: i == 0,
        })
        .collect()
    };
    let geometry = Geometry::default();
    let size = render::frame_size(1.5);
    let background = wallpaper(size);
    let backdrop = render::blur_backdrop(&background);

    type Setup = Box<dyn Fn(&mut RingModel)>;
    let cases: Vec<(&str, Setup, bool, f32)> = vec![
        ("idle", Box::new(|_| {}), true, 1.0),
        (
            "inner",
            Box::new(|m| m.selection = Some(Selection::inner(1))),
            true,
            1.0,
        ),
        (
            "window",
            Box::new(|m| m.selection = Some(Selection::window(0, 1))),
            true,
            1.0,
        ),
        (
            "tab",
            Box::new(move |m| {
                m.tabs.insert(21, tabs(5));
                m.selection = Some(Selection {
                    tab: Some(1),
                    ..Selection::window(0, 1)
                })
            }),
            true,
            1.0,
        ),
        (
            "popup",
            Box::new(move |m| {
                m.tabs.insert(10, tabs(6));
                m.inner_tabs_window = Some(0);
                m.selection = Some(Selection::outer(2))
            }),
            true,
            1.0,
        ),
        (
            "light",
            Box::new(|m| m.selection = Some(Selection::outer(2))),
            false,
            1.0,
        ),
        ("opening", Box::new(|_| {}), true, 0.4),
    ];
    for (name, setup, dark, open) in cases {
        let mut model = RingModel::build(Some(&front), &others);
        setup(&mut model);
        let style = Style {
            scale: 1.5,
            dark,
            accent: [0, 103, 192],
        };
        let mut motion = Motion::default();
        let first = render::render(&Scene {
            model: &model,
            geometry: &geometry,
            text: &text,
            style,
            thumbnail: &thumb,
            backdrop: Some(&backdrop),
            motion: &motion,
        });
        motion.settle(&first.targets);
        motion.open = open;
        let frame = render::render(&Scene {
            model: &model,
            geometry: &geometry,
            text: &text,
            style,
            thumbnail: &thumb,
            backdrop: Some(&backdrop),
            motion: &motion,
        });
        let mut canvas = background.clone();
        canvas.draw_pixmap(
            0,
            0,
            frame.pixmap.as_ref(),
            &tiny_skia::PixmapPaint::default(),
            Transform::identity(),
            None,
        );
        let path = format!("{out}/ring-{name}.png");
        canvas.save_png(&path).unwrap();
        println!("wrote {path}");
    }
}
