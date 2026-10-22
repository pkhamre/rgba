//! Host X11 transport: upload native pixels and scale with nearest-neighbor RENDER.

use std::error::Error;

use x11rb::connection::Connection;
use x11rb::errors::ConnectionError;
use x11rb::protocol::render::{ConnectionExt as _, CreatePictureAux, PictOp, Picture, Transform};
use x11rb::protocol::xproto::{ConnectionExt as _, Gcontext, Pixmap, Visualid, Window};

pub(crate) struct Presentation {
    pub(crate) pixmap: Pixmap,
    pub(crate) upload_scale: u16,
    pictures: Option<(Picture, Picture)>,
    window: Window,
    gc: Gcontext,
    width: u16,
    height: u16,
}

impl Presentation {
    pub(crate) fn create(
        connection: &impl Connection,
        window: Window,
        gc: Gcontext,
        depth: u8,
        visual: Visualid,
        scale: u16,
    ) -> Result<Self, Box<dyn Error>> {
        let mut format = None;
        if scale > 1 && connection.extension_information("RENDER")?.is_some() {
            let version = connection.render_query_version(0, 11)?.reply()?;
            if version.major_version > 0 || version.minor_version >= 6 {
                let formats = connection.render_query_pict_formats()?.reply()?;
                format = formats
                    .screens
                    .iter()
                    .flat_map(|screen| &screen.depths)
                    .flat_map(|depth| &depth.visuals)
                    .find(|entry| entry.visual == visual)
                    .map(|entry| entry.format);
            }
        }
        let pixmap = connection.generate_id()?;
        let upload_scale = if format.is_some() { 1 } else { scale };
        connection
            .create_pixmap(
                depth,
                pixmap,
                window,
                240 * upload_scale,
                160 * upload_scale,
            )?
            .check()?;
        let pictures = if let Some(format) = format {
            let source = connection.generate_id()?;
            let destination = connection.generate_id()?;
            connection
                .render_create_picture(source, pixmap, format, &CreatePictureAux::new())?
                .check()?;
            connection
                .render_create_picture(destination, window, format, &CreatePictureAux::new())?
                .check()?;
            connection
                .render_set_picture_transform(source, nearest_transform(scale))?
                .check()?;
            connection
                .render_set_picture_filter(source, b"nearest", &[])?
                .check()?;
            Some((source, destination))
        } else {
            None
        };
        eprintln!(
            "Video: {}; upload={}x{} window={}x{}",
            if pictures.is_some() {
                "XRender nearest-neighbor"
            } else {
                "X11 image transfer"
            },
            240 * upload_scale,
            160 * upload_scale,
            240 * scale,
            160 * scale
        );
        Ok(Self {
            pixmap,
            upload_scale,
            pictures,
            window,
            gc,
            width: 240 * scale,
            height: 160 * scale,
        })
    }

    pub(crate) fn present(&self, connection: &impl Connection) -> Result<(), ConnectionError> {
        if let Some((source, destination)) = self.pictures {
            connection.render_composite(
                PictOp::SRC,
                source,
                x11rb::NONE,
                destination,
                0,
                0,
                0,
                0,
                0,
                0,
                self.width,
                self.height,
            )?;
        } else {
            connection.copy_area(
                self.pixmap,
                self.window,
                self.gc,
                0,
                0,
                0,
                0,
                self.width,
                self.height,
            )?;
        }
        Ok(())
    }

    pub(crate) fn close(self, connection: &impl Connection) -> Result<(), ConnectionError> {
        if let Some((source, destination)) = self.pictures {
            connection.render_free_picture(source)?;
            connection.render_free_picture(destination)?;
        }
        connection.free_pixmap(self.pixmap)?;
        Ok(())
    }
}

fn nearest_transform(scale: u16) -> Transform {
    let step = 65536 / i32::from(scale);
    Transform {
        matrix11: step,
        matrix12: 0,
        matrix13: 0,
        matrix21: 0,
        matrix22: step,
        matrix23: 0,
        matrix31: 0,
        matrix32: 0,
        matrix33: 65536,
    }
}

#[cfg(test)]
mod tests {
    #[test]
    #[ignore = "requires a dedicated Xvfb display of at least 1440x960, depth 24"]
    fn x11_scaled_pixels_match_cpu_reference() -> Result<(), Box<dyn std::error::Error>> {
        use x11rb::connection::{Connection, RequestConnection};
        use x11rb::protocol::xproto::{
            ConnectionExt as _, CreateWindowAux, ImageFormat, ImageOrder, WindowClass,
        };
        let (connection, index) = x11rb::connect(None)?;
        let setup = connection.setup();
        let screen = &setup.roots[index];
        let format = setup
            .pixmap_formats
            .iter()
            .find(|entry| entry.depth == screen.root_depth)
            .expect("format");
        let visual = screen
            .allowed_depths
            .iter()
            .flat_map(|entry| &entry.visuals)
            .find(|entry| entry.visual_id == screen.root_visual)
            .expect("visual");
        assert_eq!(format.bits_per_pixel, 32, "capture fixture requires 32 bpp");
        assert_eq!(
            [visual.red_mask, visual.green_mask, visual.blue_mask],
            [0xff0000, 0xff00, 0xff]
        );
        let pixels: Vec<u32> = (0..240 * 160)
            .map(|index| {
                let x = index % 240;
                let y = index / 240;
                (((x * 37 + y * 17) & 255) << 16)
                    | (((x * 13 + y * 71) & 255) << 8)
                    | ((x * 97 + y * 29) & 255)
            })
            .collect();
        for scale in 1..=6 {
            let window = connection.generate_id()?;
            let gc = connection.generate_id()?;
            let width = 240 * scale;
            let height = 160 * scale;
            connection
                .create_window(
                    screen.root_depth,
                    window,
                    screen.root,
                    0,
                    0,
                    width,
                    height,
                    0,
                    WindowClass::INPUT_OUTPUT,
                    screen.root_visual,
                    &CreateWindowAux::new(),
                )?
                .check()?;
            connection
                .create_gc(gc, window, &Default::default())?
                .check()?;
            connection.map_window(window)?.check()?;
            let presentation = super::Presentation::create(
                &connection,
                window,
                gc,
                screen.root_depth,
                screen.root_visual,
                scale,
            )?;
            let upload_width = 240 * presentation.upload_scale;
            let upload_height = 160 * presentation.upload_scale;
            let stride = usize::from(upload_width) * 4;
            let image = crate::desktop::pack_pixels(
                &pixels,
                presentation.upload_scale,
                4,
                stride,
                setup.image_byte_order == ImageOrder::LSB_FIRST,
                [0xff0000, 0xff00, 0xff],
            );
            let rows = ((connection.maximum_request_bytes() - 64) / stride).max(1);
            for y in (0..usize::from(upload_height)).step_by(rows) {
                let count = rows.min(usize::from(upload_height) - y);
                connection
                    .put_image(
                        ImageFormat::Z_PIXMAP,
                        presentation.pixmap,
                        gc,
                        upload_width,
                        count as u16,
                        0,
                        y as i16,
                        0,
                        screen.root_depth,
                        &image[y * stride..(y + count) * stride],
                    )?
                    .check()?;
            }
            presentation.present(&connection)?;
            let captured = connection
                .get_image(ImageFormat::Z_PIXMAP, window, 0, 0, width, height, u32::MAX)?
                .reply()?;
            for y in 0..usize::from(height) {
                for x in 0..usize::from(width) {
                    let start = (y * usize::from(width) + x) * 4;
                    let bytes = captured.data[start..start + 4].try_into().expect("pixel");
                    let actual = if setup.image_byte_order == ImageOrder::LSB_FIRST {
                        u32::from_le_bytes(bytes)
                    } else {
                        u32::from_be_bytes(bytes)
                    };
                    let expected = pixels[y / usize::from(scale) * 240 + x / usize::from(scale)];
                    assert_eq!(actual & 0xffffff, expected, "scale={scale}, x={x}, y={y}");
                }
            }
            presentation.close(&connection)?;
            connection.free_gc(gc)?;
            connection.destroy_window(window)?;
        }
        Ok(())
    }

    #[test]
    fn fixed_point_transform_maps_every_destination_center_to_nearest_source_pixel() {
        for scale in 1..=6 {
            let transform = super::nearest_transform(scale);
            for x in 0..240 * i32::from(scale) {
                let source = ((2 * x + 1) * transform.matrix11 / 2) / 65536;
                assert_eq!(source, x / i32::from(scale), "scale={scale}, x={x}");
            }
        }
    }
}
