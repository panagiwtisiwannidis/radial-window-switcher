use std::collections::HashMap;
use std::sync::Arc;

use radial_core::geometry::{Geometry, Selection, TAB_ARC};
use radial_core::model::{AppWindows, Icon, RingItem, RingModel, TabInfo, Target, WindowRef};
use radial_core::render::{self, Motion, Scene, Style};
use radial_core::text::Text;
use tiny_skia::{Color, FillRule, Paint, Path, PathBuilder, Pixmap, PixmapPaint, Rect, Transform};

const SCALE: f32 = 2.0;
const PREVIEW_GAP: f32 = 16.0;
const MARGIN: f32 = 24.0;

const FRONT: (&str, &[&str]) = (
    "Safari",
    &["Trip to Lisbon", "Weekend recipes", "Swift documentation"],
);
const FRONT_TABS: &[&str] = &[
    "Trip to Lisbon",
    "Flights to LIS",
    "Hotels in Alfama",
    "Weather in Lisbon",
    "Tram 28 guide",
];
const OTHERS: &[(&str, &[&str])] = &[
    ("Notes", &["Groceries", "Book list", "Meeting notes"]),
    ("Mail", &["Inbox"]),
    ("Music", &["Music"]),
    ("Maps", &["Lisbon"]),
    ("Photos", &["Library"]),
    ("Calendar", &["Calendar"]),
    ("Messages", &["Messages"]),
    ("Finder", &["Documents", "Downloads"]),
    ("System Settings", &["Appearance"]),
];
const PALETTES: &[([u8; 3], [u8; 3])] = &[
    ([51, 120, 242], [237, 242, 252]),
    ([250, 158, 51], [252, 245, 232]),
    ([77, 191, 115], [237, 250, 240]),
    ([217, 77, 115], [252, 237, 242]),
    ([140, 102, 230], [242, 237, 252]),
    ([38, 166, 191], [235, 247, 250]),
];

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let [_, icons, platform, scene, out] = args.as_slice() else {
        eprintln!("usage: demo <icons dir> <windows|linux> <windows|tabs> <out.png>");
        std::process::exit(2);
    };
    let text = Text::from_path("/System/Library/Fonts/Supplemental/Arial.ttf")
        .or_else(Text::system)
        .expect("font");
    let style = Style {
        scale: SCALE,
        dark: true,
        accent: if platform == "windows" {
            [0, 103, 192]
        } else {
            [53, 132, 228]
        },
    };

    let icon = |name: &str| -> Option<Icon> {
        Pixmap::load_png(format!("{icons}/{name}.png"))
            .ok()
            .map(Arc::new)
    };
    let mut next = 900_000isize;
    let mut app = |name: &str, pid: u32, titles: &[&str]| -> AppWindows {
        AppWindows {
            name: name.into(),
            icon: icon(name),
            windows: titles
                .iter()
                .map(|title| {
                    next += 1;
                    WindowRef {
                        handle: next,
                        pid,
                        title: title.to_string(),
                        minimized: false,
                    }
                })
                .collect(),
        }
    };
    let front = app(FRONT.0, 1000, FRONT.1);
    let others: Vec<AppWindows> = OTHERS
        .iter()
        .enumerate()
        .map(|(i, (name, titles))| app(name, 1001 + i as u32, titles))
        .collect();

    let mut model = RingModel::build(Some(&front), &others);
    let tabbed = front.windows[0].handle;
    model.tabs.insert(
        tabbed,
        FRONT_TABS
            .iter()
            .enumerate()
            .map(|(i, title)| TabInfo {
                title: title.to_string(),
                selected: i == 0,
            })
            .collect(),
    );
    match scene.as_str() {
        "windows" => model.selection = Some(Selection::outer(0)),
        "tabs" => {
            model.inner_tabs_window = Some(0);
            model.selection = Some(Selection::outer(2));
        }
        _ => {
            eprintln!("unknown scene {scene}");
            std::process::exit(2);
        }
    }

    let mut cache: HashMap<isize, Icon> = HashMap::new();
    let mut seed_for = |item: &RingItem| -> isize {
        match &item.target {
            Target::Tab { window, index, .. } => window.handle + *index as isize * 7,
            _ => item.preview_window().handle,
        }
    };
    let seeds: Vec<isize> = model
        .inner
        .iter()
        .chain(&model.outer)
        .flat_map(|item| std::iter::once(item).chain(&item.windows))
        .map(&mut seed_for)
        .chain((0..FRONT_TABS.len()).map(|i| tabbed + i as isize * 7))
        .collect();
    for seed in seeds {
        cache
            .entry(seed)
            .or_insert_with(|| Arc::new(mock_window(seed)));
    }
    let thumbnail = |item: &RingItem| -> Option<Icon> {
        let seed = match &item.target {
            Target::Tab { window, index, .. } => window.handle + *index as isize * 7,
            _ => item.preview_window().handle,
        };
        cache.get(&seed).cloned()
    };

    let geometry = Geometry::default();
    let mut motion = Motion::default();
    let scene_for = |motion: &Motion| -> render::Frame {
        render::render(&Scene {
            model: &model,
            geometry: &geometry,
            text: &text,
            style,
            thumbnail: &thumbnail,
            backdrop: None,
            motion,
            close_hover: false,
        })
    };
    let first = scene_for(&motion);
    motion.settle(&first.targets);
    let frame = scene_for(&motion);

    let item = model.selected_item().expect("selection");
    let card = render::render_card(
        &text,
        style,
        &item.full_title,
        thumbnail(&item).as_deref(),
        item.icon.as_deref(),
    );

    let ring = frame.pixmap.width() as f32;
    let center = ring / 2.0;
    let distance = (TAB_ARC.1 + PREVIEW_GAP) * SCALE + card.width() as f32 / 2.0;
    let width = (center + distance + card.width() as f32 / 2.0 + MARGIN * SCALE).ceil() as u32;
    let mut canvas = Pixmap::new(width, ring as u32).unwrap();
    canvas.draw_pixmap(
        0,
        0,
        frame.pixmap.as_ref(),
        &PixmapPaint::default(),
        Transform::identity(),
        None,
    );
    canvas.draw_pixmap(
        (center + distance - card.width() as f32 / 2.0) as i32,
        (center - card.height() as f32 / 2.0) as i32,
        card.as_ref(),
        &PixmapPaint::default(),
        Transform::identity(),
        None,
    );
    trim(&canvas).save_png(out).expect("write png");
    println!("wrote {out}");
}

fn trim(pixmap: &Pixmap) -> Pixmap {
    let (w, h) = (pixmap.width() as i32, pixmap.height() as i32);
    let (mut x0, mut y0, mut x1, mut y1) = (w, h, 0, 0);
    for y in 0..h {
        for x in 0..w {
            if pixmap
                .pixel(x as u32, y as u32)
                .is_some_and(|p| p.alpha() > 8)
            {
                x0 = x0.min(x);
                y0 = y0.min(y);
                x1 = x1.max(x);
                y1 = y1.max(y);
            }
        }
    }
    let pad = (MARGIN * SCALE) as i32;
    let (x0, y0) = ((x0 - pad).max(0), (y0 - pad).max(0));
    let (x1, y1) = ((x1 + pad).min(w - 1), (y1 + pad).min(h - 1));
    let mut out = Pixmap::new((x1 - x0 + 1) as u32, (y1 - y0 + 1) as u32).unwrap();
    out.draw_pixmap(
        -x0,
        -y0,
        pixmap.as_ref(),
        &PixmapPaint::default(),
        Transform::identity(),
        None,
    );
    out
}

fn rounded(x: f32, y: f32, w: f32, h: f32, r: f32) -> Option<Path> {
    let r = r.min(w / 2.0).min(h / 2.0);
    let mut pb = PathBuilder::new();
    pb.move_to(x + r, y);
    pb.line_to(x + w - r, y);
    pb.quad_to(x + w, y, x + w, y + r);
    pb.line_to(x + w, y + h - r);
    pb.quad_to(x + w, y + h, x + w - r, y + h);
    pb.line_to(x + r, y + h);
    pb.quad_to(x, y + h, x, y + h - r);
    pb.line_to(x, y + r);
    pb.quad_to(x, y, x + r, y);
    pb.close();
    pb.finish()
}

fn fill(pixmap: &mut Pixmap, path: Option<Path>, rgb: [u8; 3], alpha: f32) {
    let Some(path) = path else { return };
    let mut paint = Paint {
        anti_alias: true,
        ..Paint::default()
    };
    paint.set_color(Color::from_rgba8(
        rgb[0],
        rgb[1],
        rgb[2],
        (alpha * 255.0) as u8,
    ));
    pixmap.fill_path(
        &path,
        &paint,
        FillRule::Winding,
        Transform::identity(),
        None,
    );
}

fn mock_window(seed: isize) -> Pixmap {
    let (accent, paper) = PALETTES[seed.unsigned_abs() % PALETTES.len()];
    let mut p = Pixmap::new(600, 400).unwrap();
    fill(
        &mut p,
        rounded(0.0, 0.0, 600.0, 400.0, 14.0),
        [255, 255, 255],
        1.0,
    );
    fill(
        &mut p,
        Rect::from_xywh(0.0, 0.0, 600.0, 44.0).map(PathBuilder::from_rect),
        [240, 240, 240],
        1.0,
    );
    for (i, color) in [[255, 95, 87], [254, 188, 46], [40, 200, 64]]
        .iter()
        .enumerate()
    {
        fill(
            &mut p,
            PathBuilder::from_circle(25.0 + i as f32 * 22.0, 22.0, 7.0),
            *color,
            1.0,
        );
    }
    fill(
        &mut p,
        Rect::from_xywh(0.0, 44.0, 150.0, 356.0).map(PathBuilder::from_rect),
        paper,
        1.0,
    );
    for row in 0..5 {
        let w = 110.0 - ((row + seed) % 3) as f32 * 18.0;
        fill(
            &mut p,
            rounded(18.0, 66.0 + row as f32 * 34.0, w, 14.0, 7.0),
            accent,
            0.35,
        );
    }
    fill(
        &mut p,
        rounded(172.0, 66.0, 404.0, 140.0, 12.0),
        accent,
        1.0,
    );
    let title = 180.0 + (seed % 4) as f32 * 30.0;
    fill(
        &mut p,
        rounded(196.0, 92.0, title, 18.0, 9.0),
        [255, 255, 255],
        0.85,
    );
    fill(
        &mut p,
        rounded(196.0, 122.0, 120.0, 12.0, 6.0),
        [255, 255, 255],
        0.85,
    );
    for column in 0..3 {
        let x = 172.0 + column as f32 * 138.0;
        fill(&mut p, rounded(x, 226.0, 128.0, 150.0, 10.0), paper, 1.0);
        fill(
            &mut p,
            rounded(x + 12.0, 240.0, 104.0, 70.0, 8.0),
            accent,
            0.55,
        );
        fill(
            &mut p,
            rounded(x + 12.0, 324.0, 90.0, 10.0, 5.0),
            [191, 191, 191],
            1.0,
        );
        fill(
            &mut p,
            rounded(x + 12.0, 344.0, 64.0, 10.0, 5.0),
            [191, 191, 191],
            1.0,
        );
    }
    p
}
