use std::sync::Arc;

use radial_switcher::geometry::{Geometry, Selection};
use radial_switcher::model::{AppWindows, RingModel, WindowRef};
use radial_switcher::render::{self, Style};
use radial_switcher::text::Text;
use tiny_skia::{Color, FillRule, Paint, PathBuilder, Pixmap, Transform};

fn icon(r: u8, g: u8, b: u8) -> Arc<Pixmap> {
    let mut pixmap = Pixmap::new(64, 64).unwrap();
    let mut pb = PathBuilder::new();
    pb.push_rect(tiny_skia::Rect::from_xywh(6.0, 6.0, 52.0, 52.0).unwrap());
    let mut paint = Paint::default();
    paint.set_color(Color::from_rgba8(r, g, b, 255));
    paint.anti_alias = true;
    pixmap.fill_path(&pb.finish().unwrap(), &paint, FillRule::Winding, Transform::identity(), None);
    let mut pb = PathBuilder::new();
    pb.push_circle(32.0, 32.0, 14.0);
    paint.set_color(Color::from_rgba8(255, 255, 255, 230));
    pixmap.fill_path(&pb.finish().unwrap(), &paint, FillRule::Winding, Transform::identity(), None);
    Arc::new(pixmap)
}

fn app(name: &str, pid: u32, titles: &[&str], color: (u8, u8, u8)) -> AppWindows {
    AppWindows {
        name: name.into(),
        icon: Some(icon(color.0, color.1, color.2)),
        windows: titles
            .iter()
            .enumerate()
            .map(|(i, t)| WindowRef { handle: (pid * 10 + i as u32) as isize, pid, title: t.to_string(), minimized: false })
            .collect(),
    }
}

fn main() {
    let out = std::env::args().nth(1).unwrap_or_else(|| ".".into());
    let text = Text::system().expect("no usable system font");
    let front = app("Code", 1, &["main.rs — radial-switcher", "README.md — radial-switcher", "Settings"], (40, 120, 220));
    let others = vec![
        app("Edge", 2, &["GitHub — radial-window-switcher", "YouTube — How to choose a string", "Mail"], (20, 160, 140)),
        app("Explorer", 3, &["Downloads"], (230, 180, 40)),
        app("Terminal", 4, &["PowerShell", "Ubuntu"], (60, 60, 70)),
        app("Teams", 5, &["Chat"], (90, 80, 200)),
        app("Spotify", 6, &["Spotify Premium"], (30, 200, 90)),
        app("Outlook", 7, &["Inbox"], (20, 110, 200)),
        app("Notepad", 8, &["notes.txt"], (120, 160, 200)),
        app("Photos", 9, &["IMG_2041.jpg"], (220, 80, 120)),
    ];
    let geometry = Geometry::default();
    let style = |dark| Style { scale: 1.5, dark, accent: [0, 103, 192] };

    let cases: Vec<(&str, Option<Selection>, bool)> = vec![
        ("idle", None, true),
        ("inner", Some(Selection::inner(1)), true),
        ("outer", Some(Selection::outer(0)), true),
        ("window", Some(Selection::window(0, 1)), true),
        ("light", Some(Selection::outer(2)), false),
    ];
    for (name, selection, dark) in cases {
        let mut model = RingModel::build(Some(&front), &others);
        model.selection = selection;
        let pixmap = render::render(&model, &geometry, &text, style(dark));
        let mut canvas = Pixmap::new(pixmap.width(), pixmap.height()).unwrap();
        canvas.fill(if dark { Color::from_rgba8(70, 90, 120, 255) } else { Color::from_rgba8(200, 210, 225, 255) });
        canvas.draw_pixmap(0, 0, pixmap.as_ref(), &tiny_skia::PixmapPaint::default(), Transform::identity(), None);
        let path = format!("{out}/ring-{name}.png");
        canvas.save_png(&path).unwrap();
        println!("wrote {path}");
    }
}
