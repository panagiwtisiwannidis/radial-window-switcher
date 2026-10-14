use tiny_skia::Pixmap;
use x11rb::connection::Connection;
use x11rb::errors::ReplyOrIdError;
use x11rb::protocol::xproto::{
    ColormapAlloc, ConfigureWindowAux, ConnectionExt, CreateGCAux, CreateWindowAux, Gcontext,
    ImageFormat, Screen, StackMode, VisualClass, Visualid, Window, WindowClass,
};

pub struct Surface {
    pub window: Window,
    gc: Gcontext,
    depth: u8,
    size: (u16, u16),
    position: (i32, i32),
    mapped: bool,
}

fn argb_visual(screen: &Screen) -> Option<Visualid> {
    screen
        .allowed_depths
        .iter()
        .filter(|d| d.depth == 32)
        .flat_map(|d| &d.visuals)
        .find(|v| v.class == VisualClass::TRUE_COLOR)
        .map(|v| v.visual_id)
}

impl Surface {
    pub fn new(conn: &impl Connection, screen: &Screen) -> Result<Self, ReplyOrIdError> {
        let window = conn.generate_id()?;
        let (depth, visual, colormap) = match argb_visual(screen) {
            Some(visual) => {
                let colormap = conn.generate_id()?;
                conn.create_colormap(ColormapAlloc::NONE, colormap, screen.root, visual)?;
                (32, visual, colormap)
            }
            None => (
                screen.root_depth,
                screen.root_visual,
                screen.default_colormap,
            ),
        };
        conn.create_window(
            depth,
            window,
            screen.root,
            0,
            0,
            1,
            1,
            0,
            WindowClass::INPUT_OUTPUT,
            visual,
            &CreateWindowAux::new()
                .override_redirect(1)
                .background_pixel(0)
                .border_pixel(0)
                .colormap(colormap),
        )?;
        let gc = conn.generate_id()?;
        conn.create_gc(gc, window, &CreateGCAux::new())?;
        Ok(Self {
            window,
            gc,
            depth,
            size: (1, 1),
            position: (0, 0),
            mapped: false,
        })
    }

    pub fn show(&mut self, conn: &impl Connection, pixmap: &Pixmap, x: i32, y: i32) {
        let size = (pixmap.width() as u16, pixmap.height() as u16);
        if size != self.size || (x, y) != self.position || !self.mapped {
            let _ = conn.configure_window(
                self.window,
                &ConfigureWindowAux::new()
                    .x(x)
                    .y(y)
                    .width(size.0 as u32)
                    .height(size.1 as u32)
                    .stack_mode(StackMode::ABOVE),
            );
            self.size = size;
            self.position = (x, y);
        }
        if !self.mapped {
            let _ = conn.map_window(self.window);
            self.mapped = true;
        }
        self.draw(conn, pixmap);
        let _ = conn.flush();
    }

    pub fn hide(&mut self, conn: &impl Connection) {
        if self.mapped {
            let _ = conn.unmap_window(self.window);
            let _ = conn.flush();
            self.mapped = false;
        }
    }

    fn draw(&self, conn: &impl Connection, pixmap: &Pixmap) {
        let width = pixmap.width() as usize;
        let row_bytes = width * 4;
        let max_rows = ((conn.maximum_request_bytes().saturating_sub(64)) / row_bytes).max(1);
        let opaque = self.depth != 32;
        let mut converted = Vec::with_capacity(pixmap.data().len());
        for p in pixmap.data().chunks(4) {
            converted.extend_from_slice(&[p[2], p[1], p[0], if opaque { 255 } else { p[3] }]);
        }
        for (strip, rows) in converted.chunks(max_rows * row_bytes).enumerate() {
            let height = (rows.len() / row_bytes) as u16;
            let _ = conn.put_image(
                ImageFormat::Z_PIXMAP,
                self.window,
                self.gc,
                width as u16,
                height,
                0,
                (strip * max_rows) as i16,
                0,
                self.depth,
                rows,
            );
        }
    }
}
