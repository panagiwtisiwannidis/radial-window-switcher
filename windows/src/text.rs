use std::cell::RefCell;
use std::collections::HashMap;

use fontdue::{Font, FontSettings, Metrics};
use tiny_skia::Pixmap;

const FONT_PATHS: &[&str] = &[
    "C:\\Windows\\Fonts\\segoeui.ttf",
    "C:\\Windows\\Fonts\\arial.ttf",
    "/System/Library/Fonts/Supplemental/Arial.ttf",
    "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf",
];

pub struct Text {
    font: Font,
    glyphs: RefCell<HashMap<(char, u32), (Metrics, Vec<u8>)>>,
}

#[derive(Clone, Copy)]
pub struct TextStyle {
    pub size: f32,
    pub color: [u8; 4],
    pub max_width: f32,
    pub max_lines: usize,
    pub shadow: bool,
}

impl Text {
    pub fn system() -> Option<Self> {
        FONT_PATHS.iter().find_map(|path| std::fs::read(path).ok()).and_then(Self::from_bytes)
    }

    pub fn from_bytes(bytes: Vec<u8>) -> Option<Self> {
        let font = Font::from_bytes(bytes, FontSettings::default()).ok()?;
        Some(Self { font, glyphs: RefCell::new(HashMap::new()) })
    }

    pub fn draw_centered(&self, pixmap: &mut Pixmap, text: &str, center_x: f32, top_y: f32, style: TextStyle) {
        let line_height = style.size * 1.25;
        for (i, line) in self.wrap(text, style).iter().enumerate() {
            let width = self.width(line, style.size);
            let x = center_x - width / 2.0;
            let baseline = top_y + i as f32 * line_height + self.ascent(style.size);
            if style.shadow {
                self.draw_line(pixmap, line, x, baseline + style.size * 0.08, style.size, [0, 0, 0, 110]);
            }
            self.draw_line(pixmap, line, x, baseline, style.size, style.color);
        }
    }

    pub fn block_height(&self, text: &str, style: TextStyle) -> f32 {
        self.wrap(text, style).len() as f32 * style.size * 1.25
    }

    fn ascent(&self, size: f32) -> f32 {
        self.font.horizontal_line_metrics(size).map(|m| m.ascent).unwrap_or(size * 0.8)
    }

    fn width(&self, text: &str, size: f32) -> f32 {
        text.chars().map(|c| self.font.metrics(c, size).advance_width).sum()
    }

    fn wrap(&self, text: &str, style: TextStyle) -> Vec<String> {
        let mut lines: Vec<String> = Vec::new();
        let mut current = String::new();
        for word in text.split_whitespace() {
            let candidate = if current.is_empty() { word.to_string() } else { format!("{current} {word}") };
            if self.width(&candidate, style.size) <= style.max_width {
                current = candidate;
                continue;
            }
            if !current.is_empty() {
                lines.push(std::mem::take(&mut current));
            }
            for c in word.chars() {
                let next = format!("{current}{c}");
                if self.width(&next, style.size) > style.max_width && !current.is_empty() {
                    lines.push(std::mem::take(&mut current));
                }
                current.push(c);
            }
        }
        if !current.is_empty() {
            lines.push(current);
        }
        if lines.len() > style.max_lines {
            lines.truncate(style.max_lines);
            let last = lines.last_mut().unwrap();
            while !last.is_empty() && self.width(&format!("{last}…"), style.size) > style.max_width {
                last.pop();
            }
            last.push('…');
        }
        lines
    }

    fn draw_line(&self, pixmap: &mut Pixmap, line: &str, mut x: f32, baseline: f32, size: f32, color: [u8; 4]) {
        let mut glyphs = self.glyphs.borrow_mut();
        for c in line.chars() {
            let (metrics, coverage) = glyphs
                .entry((c, size.to_bits()))
                .or_insert_with(|| self.font.rasterize(c, size));
            let left = (x + metrics.xmin as f32).round() as i32;
            let top = (baseline - metrics.height as f32 - metrics.ymin as f32).round() as i32;
            blend_coverage(pixmap, left, top, metrics.width, metrics.height, coverage, color);
            x += metrics.advance_width;
        }
    }
}

fn blend_coverage(pixmap: &mut Pixmap, left: i32, top: i32, width: usize, height: usize, coverage: &[u8], color: [u8; 4]) {
    let (pw, ph) = (pixmap.width() as i32, pixmap.height() as i32);
    let data = pixmap.data_mut();
    for row in 0..height as i32 {
        let y = top + row;
        if y < 0 || y >= ph {
            continue;
        }
        for col in 0..width as i32 {
            let x = left + col;
            if x < 0 || x >= pw {
                continue;
            }
            let a = coverage[(row as usize) * width + col as usize] as u32 * color[3] as u32 / 255;
            if a == 0 {
                continue;
            }
            let i = ((y * pw + x) * 4) as usize;
            for (k, channel) in color.iter().take(3).enumerate() {
                let src = *channel as u32 * a / 255;
                data[i + k] = (src + data[i + k] as u32 * (255 - a) / 255) as u8;
            }
            data[i + 3] = (a + data[i + 3] as u32 * (255 - a) / 255) as u8;
        }
    }
}
