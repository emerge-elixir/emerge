use super::*;
use crate::tree::motion::Property;

fn shadow(x: f64, y: f64) -> Bytes {
    [52, 1]
        .into_iter()
        .chain([x, y, 0., 0.].into_iter().flat_map(f64::to_be_bytes))
        .chain([0, 0, 255, 0, 0])
        .collect()
}
fn tile(w: f64, h: f64, c: [u8; 3], extra: &[Bytes]) -> Bytes {
    block(
        &[px(1, w), px(2, h), color(c), vec![40, 1]]
            .into_iter()
            .chain(extra.iter().cloned())
            .collect::<Vec<_>>(),
    )
}
#[test]
fn a_sampled_shadowed_wrapper_does_not_repaint_or_steal_the_fixed_header() {
    for (angle, alpha) in [(0., 1.), (4., 1.), (0., 0.5), (4., 0.5)] {
        let bytes = upload(&[
            node(1, 3, block(&[px(1, 100.), px(2, 100.)]), &[2, 3]),
            node(2, 4, tile(100., 20., [255, 255, 0], &[]), &[]),
            node(
                3,
                3,
                tile(100., 80., [0, 0, 255], &[vec![7, 1], fraction(86, 0.5)]),
                &[4, 5, 7],
            ),
            node(4, 4, box_attrs(100., 10.), &[]),
            node(
                5,
                4,
                block(&[px(1, 40.), px(2, 40.), number(33, angle), number(35, alpha)]),
                &[6],
            ),
            node(6, 4, tile(40., 40., [255, 0, 0], &[shadow(0., 6.)]), &[]),
            node(7, 4, block(&[px(1, 100.), px(2, 90.)]), &[]),
        ]);
        let mut t = Tree::for_session(0);
        Publisher::new(0).apply(&bytes, 1.).unwrap().install(&mut t);
        let start = Instant::now();
        let mut motion = Motion::default();
        motion.linear(
            t.elements.ids[&5],
            Property::OffsetX(4., 16.),
            start,
            Duration::from_millis(48),
            false,
        );
        let paint = |c: [u8; 3]| {
            if alpha == 1. {
                [c[0], c[1], c[2], 255]
            } else {
                [
                    c[0] / 2 + u8::from(c[0] != 0),
                    c[1] / 2 + u8::from(c[1] != 0),
                    127,
                    255,
                ]
            }
        };
        for ms in [0, 24, 48] {
            motion.sample(&mut t, start + Duration::from_millis(ms));
            t.refresh(100., 100., 1.).unwrap();
            let scene = t.seal().unwrap();
            let pixels = scene.rasterize().unwrap();
            let xy = |x, y| &pixels[4 * (100 * y + x)..4 * (100 * y + x + 1)];
            assert_eq!(xy(30usize, 19usize), [255, 255, 0, 255]);
            assert_eq!(xy(30, 30), paint([255, 0, 0]));
            assert_eq!(xy(30, 43), paint([0, 255, 0]));
            assert_eq!(
                scene
                    .hits
                    .target_at(scene.viewport, 30., 19.)
                    .map(|(_, id, x, y)| (id, x, y)),
                Some((2, 30., 19.))
            );
            assert_eq!(scene.hits.target_at(scene.viewport, 30., 30.).unwrap().1, 6);
            if ms > 0 {
                assert!(
                    t.layers
                        .trace
                        .iter()
                        .all(|(p, _, _)| matches!(p, PaintContext | Compose | Project))
                );
            }
        }
    }
}

#[test]
fn an_offscreen_anchor_hides_its_menu_but_ordinary_shadow_reach_is_unchanged() {
    let bytes = upload(&[
        node(
            1,
            3,
            block(&[px(1, 100.), px(2, 40.), vec![7, 1]]),
            &[2, 3, 4],
        ),
        node(2, 4, tile(100., 40., [255, 255, 0], &[]), &[]),
        mounted(node(3, 4, block(&[px(1, 50.), px(2, 30.)]), &[]), &[(2, 5)]),
        node(4, 4, tile(30., 20., [255, 0, 0], &[shadow(55., -60.)]), &[]),
        node(5, 4, tile(35., 18., [0, 0, 255], &[]), &[]),
    ]);
    let mut r = Raster::default();
    r.tree.upload(&bytes).unwrap();
    let mut reference = Raster::default();
    reference.tree.layers.scroll.exhaustive = true;
    reference.tree.upload(&bytes).unwrap();
    for pos in [0., 0.4, 1., 0.] {
        let a = set(
            1,
            3,
            block(&[px(1, 100.), px(2, 40.), vec![7, 1], fraction(86, pos)]),
        );
        for t in [&mut r, &mut reference] {
            t.tree.patch(&a).unwrap();
            t.draw(100., 80., 1.).unwrap();
        }
        assert!(r.pixels() == reference.pixels());
        for (x, y) in [(10., 28.), (65., 17.), (50., 50.)] {
            assert_eq!(r.hit_at(x, y), reference.hit_at(x, y));
        }
        if pos == 0. {
            assert_eq!(r.hit_at(10., 28.), Some((2, 10., 28.)));
            assert_eq!(&r.pixels()[4 * 2810..4 * 2811], [255, 255, 0, 255]);
            assert_eq!(&r.pixels()[4 * 1765..4 * 1766], [0, 255, 0, 255]);
            assert_eq!(r.hit_at(65., 17.), Some((2, 65., 17.)));
        }
    }
}
