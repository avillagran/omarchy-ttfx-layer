//! Preserve the direct raster's pixel boundaries on scaled displays.
use gtk4::{gdk, glib, prelude::*, subclass::prelude::*};

mod imp {
    use super::*;
    use gtk4::glib;
    use std::cell::OnceCell;

    #[derive(Default)]
    pub struct PixelPaintable {
        pub texture: OnceCell<gdk::Texture>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for PixelPaintable {
        const NAME: &'static str = "TtfxPixelPaintable";
        type Type = super::PixelPaintable;
        type Interfaces = (gdk::Paintable,);
    }

    impl ObjectImpl for PixelPaintable {}

    impl gdk::subclass::prelude::PaintableImpl for PixelPaintable {
        fn current_image(&self) -> gdk::Paintable {
            self.obj().clone().upcast()
        }

        fn flags(&self) -> gdk::PaintableFlags {
            gdk::PaintableFlags::SIZE | gdk::PaintableFlags::CONTENTS
        }

        fn intrinsic_width(&self) -> i32 {
            self.texture.get().map_or(0, |texture| texture.width())
        }

        fn intrinsic_height(&self) -> i32 {
            self.texture.get().map_or(0, |texture| texture.height())
        }

        fn intrinsic_aspect_ratio(&self) -> f64 {
            self.texture.get().map_or(0.0, |texture| {
                texture.width() as f64 / texture.height().max(1) as f64
            })
        }

        fn snapshot(&self, snapshot: &gdk::Snapshot, width: f64, height: f64) {
            let (Some(texture), Some(snapshot)) = (
                self.texture.get(),
                snapshot.downcast_ref::<gtk4::Snapshot>(),
            ) else {
                return;
            };
            if width <= 0.0 || height <= 0.0 || !width.is_finite() || !height.is_finite() {
                return;
            }
            snapshot.append_scaled_texture(
                texture,
                gtk4::gsk::ScalingFilter::Nearest,
                &gtk4::graphene::Rect::new(0.0, 0.0, width as f32, height as f32),
            );
        }
    }
}

glib::wrapper! {
    pub struct PixelPaintable(ObjectSubclass<imp::PixelPaintable>)
        @implements gdk::Paintable;
}

impl PixelPaintable {
    pub fn new(texture: &gdk::Texture) -> Self {
        let paintable: Self = glib::Object::new();
        paintable
            .imp()
            .texture
            .set(texture.clone())
            .expect("immutable texture initialized once");
        paintable
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "requires a GTK display; run this exact test in its own Xvfb process"]
    fn nearest_snapshot_preserves_colors_at_fractional_scale() {
        gtk4::init().unwrap();
        let source = [
            0, 0, 255, 255, 0, 255, 0, 255, 255, 0, 0, 255, 255, 255, 255, 255,
        ];
        let texture: gdk::Texture = gdk::MemoryTexture::new(
            2,
            2,
            gdk::MemoryFormat::B8g8r8a8Premultiplied,
            &glib::Bytes::from_owned(source.to_vec()),
            8,
        )
        .upcast();
        let paintable = PixelPaintable::new(&texture);
        drop(texture);
        assert_eq!(paintable.intrinsic_aspect_ratio(), 1.0);
        assert!(paintable
            .flags()
            .contains(gdk::PaintableFlags::SIZE | gdk::PaintableFlags::CONTENTS));
        let snapshot = gtk4::Snapshot::new();
        paintable.current_image().snapshot(&snapshot, 5.0, 3.0);
        let renderer = gtk4::gsk::CairoRenderer::new();
        renderer.realize(None::<&gdk::Surface>).unwrap();
        let output = renderer.render_texture(
            &snapshot.to_node().unwrap(),
            Some(&gtk4::graphene::Rect::new(0.0, 0.0, 5.0, 3.0)),
        );
        let mut pixels = vec![0; 5 * 3 * 4];
        output.download(&mut pixels, 5 * 4);
        renderer.unrealize();
        // Nearest sampling preserves colors. At a center exactly between two
        // source pixels, either neighbor is valid: Cairo rounds that tie down.
        for y in 0..3 {
            for x in 0..5 {
                let pixel = &pixels[(y * 5 + x) * 4..][..4];
                assert!(source.chunks_exact(4).any(|color| pixel == color));
                if x != 2 && y != 1 {
                    let source_x = usize::from(x > 2);
                    let source_y = usize::from(y > 1);
                    let offset = (source_y * 2 + source_x) * 4;
                    assert_eq!(pixel, &source[offset..offset + 4]);
                }
            }
        }
    }

    #[test]
    #[ignore = "requires a GTK display; run this exact test in its own Xvfb process"]
    fn nearest_retained_bounded_updates_render_recolors_and_removals() {
        gtk4::init().unwrap();
        let block = |x, y, color| crate::DirectCell {
            x,
            y,
            ch: '█',
            color,
            background: None,
            attrs: 0,
        };
        let mut atlas = crate::DirectGlyphAtlas::new(4, 4, 9.0);
        let renderer = gtk4::gsk::CairoRenderer::new();
        renderer.realize(None::<&gdk::Surface>).unwrap();
        let mut previous: Option<gdk::Texture> = None;
        for cells in [
            vec![block(0, 0, (255, 0, 0)), block(2, 1, (0, 0, 255))],
            vec![block(0, 0, (0, 255, 0)), block(2, 1, (0, 0, 255))],
            vec![block(0, 0, (0, 255, 0))],
            vec![],
        ] {
            let mut frame = atlas
                .rasterize_retained_pixels(&cells, 3, 2, 10, 7, false)
                .unwrap();
            frame.bound_damage(10, 7);
            let bytes = glib::Bytes::from_owned(frame.pixels.clone());
            let mut builder = gdk::MemoryTextureBuilder::new()
                .set_width(10)
                .set_height(7)
                .set_format(gdk::MemoryFormat::B8g8r8x8)
                .set_stride(40)
                .set_bytes(Some(&bytes));
            if !frame.full_redraw {
                assert!(previous.is_some());
                let rectangles: Vec<_> = frame
                    .damage
                    .iter()
                    .map(|r| gtk4::cairo::RectangleInt::new(r.x, r.y, r.width, r.height))
                    .collect();
                let region = gtk4::cairo::Region::create_rectangles(&rectangles);
                builder = builder
                    .set_update_texture(previous.as_ref())
                    .set_update_region(Some(&region));
            }
            let texture = builder.build();
            let snapshot = gtk4::Snapshot::new();
            PixelPaintable::new(&texture).snapshot(&snapshot, 20.0, 14.0);
            let output = renderer.render_texture(
                &snapshot.to_node().unwrap(),
                Some(&gtk4::graphene::Rect::new(0.0, 0.0, 20.0, 14.0)),
            );
            let mut pixels = vec![0; 20 * 14 * 4];
            output.download(&mut pixels, 80);
            for y in 0..14 {
                for x in 0..20 {
                    let source = ((y / 2) * 10 + x / 2) * 4;
                    assert_eq!(
                        &pixels[(y * 20 + x) * 4..][..4],
                        &frame.pixels[source..source + 4]
                    );
                }
            }
            previous = Some(texture);
        }
        renderer.unrealize();
    }

    #[test]
    #[ignore = "requires a GTK display; run this exact test under Xvfb"]
    fn nearest_snapshot_preserves_2x_pixels_and_renders_the_wordmark() {
        gtk4::init().expect("GTK display required for the offscreen render test");
        let source = [
            0, 0, 0, 255, 255, 255, 255, 255, 255, 255, 255, 255, 0, 0, 0, 255,
        ];
        let texture: gdk::Texture = gdk::MemoryTexture::new(
            2,
            2,
            gdk::MemoryFormat::B8g8r8a8Premultiplied,
            &glib::Bytes::from_owned(source.to_vec()),
            8,
        )
        .upcast();
        let paintable = PixelPaintable::new(&texture);
        assert_eq!(paintable.intrinsic_width(), 2);
        assert_eq!(paintable.intrinsic_height(), 2);
        let snapshot = gtk4::Snapshot::new();
        paintable.snapshot(&snapshot, 4.0, 4.0);
        let node = snapshot.to_node().unwrap();
        assert_eq!(
            node.downcast_ref::<gtk4::gsk::TextureScaleNode>()
                .unwrap()
                .filter(),
            gtk4::gsk::ScalingFilter::Nearest
        );
        let renderer = gtk4::gsk::CairoRenderer::new();
        renderer.realize(None::<&gdk::Surface>).unwrap();
        let bounds = gtk4::graphene::Rect::new(0.0, 0.0, 4.0, 4.0);
        let output = renderer.render_texture(&node, Some(&bounds));
        let mut pixels = vec![0; 64];
        output.download(&mut pixels, 16);
        for y in 0..4 {
            for x in 0..4 {
                let input = ((y / 2) * 2 + x / 2) * 4;
                assert_eq!(&pixels[(y * 4 + x) * 4..][..4], &source[input..input + 4]);
            }
        }

        // Export the actual atlas -> paintable -> GSK render for visual QA.
        let (cols, rows, width, height) = (236, 78, 1280, 800);
        let input = crate::wordmark::canvas_input("OMARCHY", cols, rows, "native", 2.0);
        let cells: Vec<crate::DirectCell> = input
            .lines()
            .enumerate()
            .flat_map(|(y, line)| {
                line.chars().enumerate().filter_map(move |(x, ch)| {
                    (ch != ' ').then_some(crate::DirectCell {
                        x,
                        y,
                        ch,
                        color: (255, 255, 255),
                        background: None,
                        attrs: 0,
                    })
                })
            })
            .collect();
        let mut atlas = crate::DirectGlyphAtlas::new(6, 11, 9.0);
        let raster = atlas
            .rasterize_pixels(&cells, cols, rows, width, height, false)
            .unwrap();
        assert!(raster
            .chunks_exact(4)
            .all(|pixel| pixel == [0, 0, 0, 255] || pixel == [255, 255, 255, 255]));
        let texture: gdk::Texture = gdk::MemoryTexture::new(
            width,
            height,
            gdk::MemoryFormat::B8g8r8a8Premultiplied,
            &glib::Bytes::from_owned(raster),
            width as usize * 4,
        )
        .upcast();
        let paintable = PixelPaintable::new(&texture);
        let snapshot = gtk4::Snapshot::new();
        paintable.snapshot(&snapshot, width as f64 * 2.0, height as f64 * 2.0);
        let bounds = gtk4::graphene::Rect::new(0.0, 0.0, (width * 2) as f32, (height * 2) as f32);
        let output = renderer.render_texture(&snapshot.to_node().unwrap(), Some(&bounds));
        let stride = width as usize * 2 * 4;
        let mut pixels = vec![0; stride * height as usize * 2];
        output.download(&mut pixels, stride);
        renderer.unrealize();
        for y in (0..height as usize * 2).step_by(2) {
            for x in (0..width as usize * 2).step_by(2) {
                let offset = y * stride + x * 4;
                let pixel = &pixels[offset..offset + 4];
                assert!(pixel == [0, 0, 0, 255] || pixel == [255, 255, 255, 255]);
                assert_eq!(pixel, &pixels[offset + 4..offset + 8]);
                assert_eq!(pixel, &pixels[offset + stride..offset + stride + 4]);
                assert_eq!(pixel, &pixels[offset + stride + 4..offset + stride + 8]);
            }
        }
        if let Some(directory) = std::env::var_os("TTFX_PIXEL_TEST_ARTIFACT_DIR") {
            let directory = std::path::PathBuf::from(directory);
            std::fs::create_dir_all(&directory).unwrap();
            output
                .save_to_png(directory.join("wordmark-fixed-gsk-2x.png"))
                .unwrap();
        }
    }
}
