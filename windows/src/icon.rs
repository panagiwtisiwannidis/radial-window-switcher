use std::f32::consts::PI;

use tiny_skia::{Color, FillRule, GradientStop, LinearGradient, Paint, PathBuilder, Pixmap, Point, SpreadMode, Stroke, Transform};

pub fn app_icon(size: u32) -> Pixmap {
    let mut pixmap = Pixmap::new(size, size).expect("icon size");
    let s = size as f32 / 1024.0;
    let center = (512.0 * s, 512.0 * s);

    let tile = rounded(100.0 * s, 100.0 * s, 824.0 * s, 185.0 * s);
    let mut paint = Paint::default();
    paint.anti_alias = true;
    paint.shader = LinearGradient::new(
        Point::from_xy(0.0, 100.0 * s),
        Point::from_xy(0.0, 924.0 * s),
        vec![
            GradientStop::new(0.0, Color::from_rgba8(31, 36, 71, 255)),
            GradientStop::new(1.0, Color::from_rgba8(8, 10, 26, 255)),
        ],
        SpreadMode::Pad,
        Transform::identity(),
    )
    .unwrap();
    pixmap.fill_path(&tile, &paint, FillRule::Winding, Transform::identity(), None);

    ring(&mut pixmap, center, 8, 232.0 * s, 340.0 * s, Some(1), s);
    ring(&mut pixmap, center, 4, 118.0 * s, 220.0 * s, None, s);

    let mut hub = PathBuilder::new();
    hub.push_circle(center.0, center.1, 82.0 * s);
    let mut paint = Paint::default();
    paint.anti_alias = true;
    paint.set_color(Color::from_rgba8(255, 255, 255, 230));
    pixmap.fill_path(&hub.finish().unwrap(), &paint, FillRule::Winding, Transform::identity(), None);
    pixmap
}

fn rounded(x: f32, y: f32, side: f32, radius: f32) -> tiny_skia::Path {
    let k = 0.552_284_8 * radius;
    let (r, b) = (x + side, y + side);
    let mut pb = PathBuilder::new();
    pb.move_to(x + radius, y);
    pb.line_to(r - radius, y);
    pb.cubic_to(r - radius + k, y, r, y + radius - k, r, y + radius);
    pb.line_to(r, b - radius);
    pb.cubic_to(r, b - radius + k, r - radius + k, b, r - radius, b);
    pb.line_to(x + radius, b);
    pb.cubic_to(x + radius - k, b, x, b - radius + k, x, b - radius);
    pb.line_to(x, y + radius);
    pb.cubic_to(x, y + radius - k, x + radius - k, y, x + radius, y);
    pb.close();
    pb.finish().unwrap()
}

fn ring(pixmap: &mut Pixmap, center: (f32, f32), count: usize, inner: f32, outer: f32, highlighted: Option<usize>, s: f32) {
    let step = 2.0 * PI / count as f32;
    let gap = 7.0 * s;
    for i in 0..count {
        let start = i as f32 * step - step / 2.0;
        let end = start + step;
        let mut pb = PathBuilder::new();
        let steps = 48;
        for k in 0..=steps {
            let a = start + gap / outer + (end - start - 2.0 * gap / outer) * k as f32 / steps as f32;
            let p = (center.0 + outer * a.sin(), center.1 - outer * a.cos());
            if k == 0 { pb.move_to(p.0, p.1) } else { pb.line_to(p.0, p.1) }
        }
        for k in 0..=steps {
            let a = end - gap / inner - (end - start - 2.0 * gap / inner) * k as f32 / steps as f32;
            pb.line_to(center.0 + inner * a.sin(), center.1 - inner * a.cos());
        }
        pb.close();
        let path = pb.finish().unwrap();
        let mut paint = Paint::default();
        paint.anti_alias = true;
        paint.set_color(if Some(i) == highlighted {
            Color::from_rgba8(77, 143, 255, 255)
        } else {
            Color::from_rgba8(255, 255, 255, 41)
        });
        pixmap.fill_path(&path, &paint, FillRule::Winding, Transform::identity(), None);
        paint.set_color(Color::from_rgba8(255, 255, 255, 90));
        pixmap.stroke_path(&path, &paint, &Stroke { width: (2.0 * s).max(0.75), ..Stroke::default() }, Transform::identity(), None);
    }
}

pub fn ico_file(sizes: &[u32]) -> Vec<u8> {
    let images: Vec<Vec<u8>> = sizes.iter().map(|&size| app_icon(size).encode_png().expect("png")).collect();
    let mut out = Vec::new();
    out.extend_from_slice(&[0, 0, 1, 0]);
    out.extend_from_slice(&(sizes.len() as u16).to_le_bytes());
    let mut offset = 6 + 16 * sizes.len() as u32;
    for (size, png) in sizes.iter().zip(&images) {
        let side = if *size >= 256 { 0 } else { *size as u8 };
        out.extend_from_slice(&[side, side, 0, 0]);
        out.extend_from_slice(&1u16.to_le_bytes());
        out.extend_from_slice(&32u16.to_le_bytes());
        out.extend_from_slice(&(png.len() as u32).to_le_bytes());
        out.extend_from_slice(&offset.to_le_bytes());
        offset += png.len() as u32;
    }
    for png in images {
        out.extend_from_slice(&png);
    }
    out
}
