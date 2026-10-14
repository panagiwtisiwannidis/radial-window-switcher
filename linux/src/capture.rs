use tiny_skia::Pixmap;
use x11rb::connection::Connection;
use x11rb::protocol::composite::ConnectionExt as CompositeExt;
use x11rb::protocol::render::{
    self, ConnectionExt as RenderExt, CreatePictureAux, PictOp, PictType, Pictformat,
    QueryPictFormatsReply,
};
use x11rb::protocol::xproto::{ConnectionExt, ImageFormat, MapState, Window};

pub struct Capturer {
    formats: QueryPictFormatsReply,
    rgb24: Option<Pictformat>,
    composite: bool,
}

fn fixed(value: f64) -> i32 {
    (value * 65536.0) as i32
}

impl Capturer {
    pub fn new(conn: &impl Connection) -> Option<Self> {
        let composite = conn
            .composite_query_version(0, 4)
            .ok()
            .and_then(|c| c.reply().ok())
            .is_some();
        let formats = conn.render_query_pict_formats().ok()?.reply().ok()?;
        let rgb24 = formats
            .formats
            .iter()
            .find(|f| {
                f.type_ == PictType::DIRECT
                    && f.depth == 24
                    && f.direct.red_mask == 0xff
                    && f.direct.red_shift == 16
            })
            .map(|f| f.id);
        Some(Self {
            formats,
            rgb24,
            composite,
        })
    }

    fn format_for_visual(&self, visual: u32) -> Option<Pictformat> {
        self.formats
            .screens
            .iter()
            .flat_map(|s| &s.depths)
            .flat_map(|d| &d.visuals)
            .find(|v| v.visual == visual)
            .map(|v| v.format)
    }

    pub fn window(
        &self,
        conn: &impl Connection,
        root: Window,
        window: Window,
        max_width: u16,
    ) -> Option<Pixmap> {
        let attributes = conn.get_window_attributes(window).ok()?.reply().ok()?;
        if attributes.map_state != MapState::VIEWABLE {
            return None;
        }
        let geometry = conn.get_geometry(window).ok()?.reply().ok()?;
        let (width, height) = (geometry.width, geometry.height);
        if width < 50 || height < 50 {
            return None;
        }
        let format = self.format_for_visual(attributes.visual)?;
        let rgb24 = self.rgb24?;

        let mut named = None;
        if self.composite {
            let pixmap = conn.generate_id().ok()?;
            if conn
                .composite_name_window_pixmap(window, pixmap)
                .ok()?
                .check()
                .is_ok()
            {
                named = Some(pixmap);
            }
        }
        let source = named.unwrap_or(window);

        let target_width = width.min(max_width).max(1);
        let target_height = ((height as u32 * target_width as u32) / width as u32).max(1) as u16;
        let factor = width as f64 / target_width as f64;

        let result = (|| {
            let source_picture = conn.generate_id().ok()?;
            conn.render_create_picture(source_picture, source, format, &CreatePictureAux::new())
                .ok()?
                .check()
                .ok()?;
            let transform = render::Transform {
                matrix11: fixed(factor),
                matrix12: 0,
                matrix13: 0,
                matrix21: 0,
                matrix22: fixed(factor),
                matrix23: 0,
                matrix31: 0,
                matrix32: 0,
                matrix33: fixed(1.0),
            };
            let _ = conn.render_set_picture_transform(source_picture, transform);
            let _ = conn.render_set_picture_filter(source_picture, b"bilinear", &[]);

            let target = conn.generate_id().ok()?;
            conn.create_pixmap(24, target, root, target_width, target_height)
                .ok()?;
            let target_picture = conn.generate_id().ok()?;
            conn.render_create_picture(target_picture, target, rgb24, &CreatePictureAux::new())
                .ok()?;
            let _ = conn.render_composite(
                PictOp::SRC,
                source_picture,
                0u32,
                target_picture,
                0,
                0,
                0,
                0,
                0,
                0,
                target_width,
                target_height,
            );
            let image = conn
                .get_image(
                    ImageFormat::Z_PIXMAP,
                    target,
                    0,
                    0,
                    target_width,
                    target_height,
                    !0,
                )
                .ok()
                .and_then(|c| c.reply().ok());
            let _ = conn.render_free_picture(target_picture);
            let _ = conn.free_pixmap(target);
            let _ = conn.render_free_picture(source_picture);
            let image = image?;
            to_pixmap(&image.data, target_width as u32, target_height as u32)
        })();

        if let Some(pixmap) = named {
            let _ = conn.free_pixmap(pixmap);
        }
        result
    }

    pub fn region(
        conn: &impl Connection,
        root: Window,
        screen: (u16, u16),
        x: i32,
        y: i32,
        size: u32,
    ) -> Option<Pixmap> {
        let left = x.max(0);
        let top = y.max(0);
        let right = (x + size as i32).min(screen.0 as i32);
        let bottom = (y + size as i32).min(screen.1 as i32);
        if right <= left || bottom <= top {
            return None;
        }
        let (width, height) = ((right - left) as u16, (bottom - top) as u16);
        let image = conn
            .get_image(
                ImageFormat::Z_PIXMAP,
                root,
                left as i16,
                top as i16,
                width,
                height,
                !0,
            )
            .ok()?
            .reply()
            .ok()?;
        let part = to_pixmap(&image.data, width as u32, height as u32)?;
        let mut full = Pixmap::new(size, size)?;
        full.draw_pixmap(
            left - x,
            top - y,
            part.as_ref(),
            &tiny_skia::PixmapPaint::default(),
            tiny_skia::Transform::identity(),
            None,
        );
        Some(full)
    }
}

fn to_pixmap(data: &[u8], width: u32, height: u32) -> Option<Pixmap> {
    let mut pixmap = Pixmap::new(width, height)?;
    if data.len() < (width * height * 4) as usize {
        return None;
    }
    for (src, dst) in data.chunks(4).zip(pixmap.data_mut().chunks_mut(4)) {
        dst[0] = src[2];
        dst[1] = src[1];
        dst[2] = src[0];
        dst[3] = 255;
    }
    Some(pixmap)
}
