use super::*;
use crate::raster_assets::{BitmapRef, Loader, Settings, Source};
use skia_safe::{Color4f, surfaces};
const W: i32 = 160;
const H: i32 = 120;
fn viewport() -> Rect {
    Rect {
        x: 0.,
        y: 0.,
        width: W as f32,
        height: H as f32,
    }
}
fn image(source: Source) -> Command {
    let (w, h) = source.dimensions().unwrap();
    Command::Image(image::Sample {
        fit: ImageFit::Contain,
        source,
        src: Rect {
            width: w,
            height: h,
            ..Rect::default()
        },
        dst: SkRect::from_xywh(20., 16., 40., 24.),
        clip: sk(viewport()),
        relaxed_masks: 1,
        alpha: 255,
        tint: None,
    })
}
fn render(draw: &Drawing, bg: Color, isolated: Option<u8>) -> (Vec<u8>, Calls) {
    let info = ImageInfo::new((W, H), ColorType::RGBA8888, AlphaType::Premul, None);
    let mut pixels = vec![0; (W * H * 4) as usize];
    let mut s = surfaces::wrap_pixels(&info, &mut pixels, 4 * W as usize, None).unwrap();
    let c = s.canvas();
    c.clear(bg);
    CALLS.take();
    if let Some(a) = isolated {
        c.save_layer_alpha(None, u32::from(a));
    }
    replay(c, draw, |_, _, _| unreachable!()).unwrap();
    if isolated.is_some() {
        c.restore();
    }
    drop(s);
    (pixels, CALLS.take())
}
fn pixel(bytes: &[u8], x: usize, y: usize) -> [u8; 4] {
    bytes[4 * (y * W as usize + x)..][..4].try_into().unwrap()
}
fn png() -> Source {
    Loader::new(Settings {
        sources: vec![format!("{}/../../priv", env!("CARGO_MANIFEST_DIR"))],
        decode_at_size: false,
        cache_max_entries: 1,
        cache_max_bytes: 1024,
        ..Default::default()
    })
    .unwrap()
    .load("test_assets/alpha_grid.png".into())
}
#[test]
fn sampled_png_and_precomposited_snapshot_take_one_draw_without_an_opacity_layer() {
    let png = png();
    let mut surface = surfaces::raster_n32_premul((10, 6)).unwrap();
    surface.canvas().clear(Color::TRANSPARENT);
    surface.canvas().draw_rect(
        SkRect::from_xywh(1., 1., 6., 4.),
        &Paint::new(Color4f::new(1., 0., 0., 0.5), None),
    );
    surface.canvas().draw_rect(
        SkRect::from_xywh(5., 1., 4., 4.),
        &Paint::new(Color4f::new(0., 1., 1., 1.), None),
    );
    let snapshot = Source::Ready(BitmapRef::snapshot(surface.image_snapshot()));
    for source in [png, snapshot] {
        let mut original = Drawing::default();
        original.push(image(source.clone()));
        let mut direct = Drawing::default();
        let start = direct.begin_group(128);
        direct.push(image(source));
        direct.end_group(start, viewport());
        for bg in [Color::TRANSPARENT, Color::BLUE] {
            let (result, calls) = render(&direct, bg, None);
            assert_eq!((calls.layers, calls.images), (0, 1));
            let (oracle, _) = render(&original, bg, Some(128));
            assert!(
                result == oracle,
                "complete direct and isolated buffers differ"
            );
            if bg == Color::TRANSPARENT {
                assert_eq!(pixel(&result, 30, 26)[3], 64);
            }
        }
    }
}
#[test]
fn rounded_bitmap_clip_precedes_direct_alpha_without_an_intermediate_surface() {
    let png = png();
    let render_at = |alpha: u8, bg, isolate: bool| {
        let mut drawing = Drawing::default();
        let mask = Rounded::new(
            Rect {
                x: 18.,
                y: 17.,
                width: 24.,
                height: 19.,
            },
            [5.; 4],
        );
        drawing.mask = drawing.clips.add(OPEN, mask);
        let start = (!isolate && alpha != 255).then(|| drawing.begin_group(alpha));
        drawing.push(image(png.clone()));
        if let Some(start) = start {
            drawing.end_group(start, viewport());
        }
        render(&drawing, bg, (isolate && alpha != 255).then_some(alpha))
    };
    for bg in [Color::TRANSPARENT, Color::from_rgb(13, 70, 100)] {
        for alpha in [128, 255] {
            let (direct, calls) = render_at(alpha, bg, false);
            let (isolated, _) = render_at(alpha, bg, true);
            assert_eq!((calls.images, calls.layers), (1, 0));
            // Isolation quantizes the clipped sample to 8 bits before applying
            // opacity. Check every channel, not just an interior texel.
            let max_error = direct
                .iter()
                .zip(isolated)
                .map(|(a, b)| a.abs_diff(b))
                .max()
                .unwrap();
            assert!(max_error <= u8::from(alpha != 255));
        }
    }
}
#[test]
fn overlapping_draws_take_one_content_sized_layer_and_blend_once() {
    let fill = |x, c| {
        Command::Fill(
            Rect {
                x,
                y: 7.,
                width: 18.,
                height: 11.,
            },
            Brush::Solid(c),
        )
    };
    let mut group = Drawing::default();
    let start = group.begin_group(128);
    let mut original = Drawing::default();
    for cmd in [fill(8., 0xff0000ff), fill(16., 0x00ff00ff)] {
        original.push(cmd.clone());
        group.push(cmd);
    }
    group.end_group(start, viewport());
    let (bytes, calls) = render(&group, Color::TRANSPARENT, None);
    assert_eq!((calls.layers, calls.images), (1, 0));
    assert_eq!(calls.last, SkRect::from_ltrb(7., 6., 35., 19.));
    assert_eq!(pixel(&bytes, 18, 10), [0, 128, 0, 128]);
    assert!(bytes == render(&original, Color::TRANSPARENT, Some(128)).0);
    let outside = sk(Rect {
        x: 1000.,
        y: -10.,
        width: 15.,
        height: 5.,
    });
    let mut c = surfaces::raster_n32_premul((100, 80)).unwrap();
    let canvas = c.canvas();
    assert!(visible(canvas, outside).is_none());
    canvas.clip_rect(SkRect::from_ltrb(4., 3., 18., 19.), None, false);
    canvas.scale((2., 2.));
    assert_eq!(
        visible(canvas, SkRect::from_ltrb(1.5, 1., 4.1, 6.2)),
        Some(SkRect::from_ltrb(2., 1.5, 4.5, 6.5))
    );
}
