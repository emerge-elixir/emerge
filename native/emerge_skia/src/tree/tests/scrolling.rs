use super::nearby::mounted;
use super::*;
use std::time::{Duration, Instant};
mod navigation;
mod scopes;
pub(super) mod visibility;

fn fraction(tag: u8, v: f64) -> Bytes {
    [tag, 0].into_iter().chain(v.to_be_bytes()).collect()
}
fn target(tag: u8, token: u64, id: u64) -> Bytes {
    [tag, 1]
        .into_iter()
        .chain(token.to_be_bytes())
        .chain(id.to_be_bytes())
        .collect()
}
fn view(r: &Raster, id: u64) -> scroll::View {
    r.tree.layers.scroll.view(r.tree.elements.ids[&id])
}
fn box_attrs(w: f64, h: f64) -> Bytes {
    block(&[px(1, w), px(2, h), vec![40, 1], color([1, 2, 3])])
}
pub(super) fn set_input(
    t: &mut Tree,
    id: u64,
    xy: [f64; 2],
    serial: u64,
    at: Option<Instant>,
) -> bool {
    let k = t.elements.ids[&id];
    let mut axes = t.layers.scroll.view(k).axes;
    for (i, a) in axes.iter_mut().enumerate() {
        a.pos = xy[i];
        a.input = serial;
    }
    let mut change = scroll::Update::default();
    change.record(axes, [true; 2], at);
    t.scroll_input(Attachment(k, t.elements.slots[k].attachment), change)
}
#[test]
fn ordinary_extents_include_padding_not_mounts_and_input_never_reopens_layout() {
    let a = |xy| {
        block(&[
            px(1, 100.),
            px(2, 60.),
            [3, 0].into_iter().chain(5f64.to_be_bytes()).collect(),
            vec![7, 1],
            vec![8, 1],
            fraction(85, xy),
            fraction(86, xy),
        ])
    };
    let mut r = Raster::default();
    r.tree
        .upload(&upload(&[
            mounted(node(1, 4, a(0.25), &[2]), &[(3, 3)]),
            node(2, 4, box_attrs(170., 130.), &[]),
            node(3, 4, box_attrs(450., 180.), &[]),
        ]))
        .unwrap();
    draw(&mut r);
    assert_eq!(view(&r, 1).range, [80., 80.]);
    assert_eq!(view(&r, 1).offset(), [20., 20.]);
    assert_eq!(r.hit_at(8., 10.), Some((2, 23., 25.)));
    assert_eq!(r.hit_at(110., 10.), Some((3, 10., 10.)));
    let before = bounds(&r, 2);
    assert!(set_input(&mut r.tree, 1, [0.5, 0.75], 1, None));
    draw(&mut r);
    assert_eq!(bounds(&r, 2), before);
    assert_eq!(r.hit_at(8., 10.), Some((2, 43., 65.)));
    assert_eq!(r.hit_at(110., 10.), Some((3, 10., 10.)));
    assert!(
        r.tree
            .layers
            .trace
            .iter()
            .all(|(p, _, _)| matches!(p, PaintContext | Compose | Project))
    );
    assert!(!ran(&r, PaintContext, 3) && !ran(&r, Project, 3));
    r.tree.patch(&set(2, 4, box_attrs(50., 40.))).unwrap();
    draw(&mut r);
    assert_eq!(view(&r, 1).range, [0., 0.]);
    assert_eq!(view(&r, 1).axes.map(|a| a.pos), [0.5, 0.75]);
    r.tree.patch(&set(2, 4, box_attrs(250., 210.))).unwrap();
    draw(&mut r);
    assert_eq!(view(&r, 1).offset(), [80., 120.]);
    r.tree.patch(&set(1, 4, a(1.))).unwrap();
    draw(&mut r);
    assert_eq!(view(&r, 1).offset(), [160., 160.]);
    check_fresh(&r);
}
#[test]
fn target_reads_settled_ordinary_edges_once_and_not_paint_coordinates() {
    let port = |proposal| {
        block(&[
            px(1, 100.),
            px(2, 60.),
            vec![7, 1],
            [3, 0].into_iter().chain(5f64.to_be_bytes()).collect(),
            proposal,
        ])
    };
    let band = |id, h| node(id, 4, box_attrs(70., h), &[]);
    let inner = |axis| block(&[px(1, 60.), px(2, 50.), vec![axis, 1], number(32, 4.)]);
    let mut r = Raster::default();
    r.tree
        .upload(&upload(&[
            node(1, 3, port(target(86, 100, 4)), &[2, 3, 5]),
            band(2, 30.),
            mounted(node(3, 4, inner(8), &[4]), &[(4, 6)]),
            band(4, 60.),
            band(5, 100.),
            band(6, 50.),
        ]))
        .unwrap();
    draw(&mut r);
    assert_eq!(view(&r, 1).range, [0., 130.]);
    assert_eq!(view(&r, 1).offset(), [0., 30.]);
    for (token, id, expected) in [(101, 6, 30.), (102, 0, 30.), (102, 5, 30.), (103, 5, 80.)] {
        r.tree
            .patch(&set(1, 3, port(target(86, token, id))))
            .unwrap();
        draw(&mut r);
        assert_eq!(view(&r, 1).offset()[1], expected);
    }
    r.tree.patch(&set(3, 4, inner(7))).unwrap();
    r.tree.patch(&set(1, 3, port(target(86, 104, 4)))).unwrap();
    draw(&mut r);
    assert_eq!(view(&r, 1).offset()[1], 80.);
    // The same-axis viewport itself is a legal target, its content is not.
    r.tree.patch(&set(1, 3, port(target(86, 105, 3)))).unwrap();
    draw(&mut r);
    assert_eq!(view(&r, 1).offset()[1], 30.);
}
#[test]
fn scrollable_column_zones_start_at_the_end_of_the_leading_group() {
    let port = |enable| {
        block(&[
            px(1, 80.),
            px(2, 100.),
            vec![7, enable],
            number(4, 3.),
            fraction(86, f64::from(enable)),
        ])
    };
    let band = |id, h, align| {
        node(
            id,
            4,
            block(&[px(1, 70.), px(2, h), vec![6, align], vec![40, 1]]),
            &[],
        )
    };
    let mut r = Raster::default();
    r.tree
        .upload(&upload(&[
            node(1, 3, port(0), &[2, 3, 4]),
            band(2, 20., 0),
            band(3, 10., 2),
            band(4, 8., 1),
        ]))
        .unwrap();
    draw(&mut r);
    assert_eq!(bounds(&r, 3).y, 90.);
    r.tree.patch(&set(1, 3, port(1))).unwrap();
    draw(&mut r);
    assert_eq!([bounds(&r, 3).y, bounds(&r, 4).y], [20., 20.]);
    r.tree.patch(&at(upload(&[band(2, 120., 0)]), 2)).unwrap();
    draw(&mut r);
    assert_eq!([bounds(&r, 3).y, bounds(&r, 4).y], [120., 120.]);
    assert_eq!(view(&r, 1).range, [0., 30.]);
    assert_eq!(r.hit_at(5., 92.), Some((4, 5., 2.)));
    check_fresh(&r);
}
#[test]
fn scroll_content_guard_follows_ordinary_and_nearby_edges_through_a_role_change() {
    let mut r = Raster::default();
    let guard = |clip: bool, round| {
        block(&[
            px(1, 40.),
            px(2, 40.),
            vec![7, 1],
            [13, 0].into_iter().chain(f64::to_be_bytes(round)).collect(),
            vec![77, u8::from(clip)],
            fraction(86, 1.),
        ])
    };
    let shadow = [52, 1]
        .into_iter()
        .chain([0f64, 30., 0., 0.].into_iter().flat_map(f64::to_be_bytes))
        .chain([0, 0, 255, 0, 0])
        .collect();
    let image = block(&[
        images::path(images::WIDE),
        px(1, 40.),
        px(2, 20.),
        number(31, -6.),
        shadow,
        vec![40, 1],
    ]);
    let pad = [3, 0].into_iter().chain(30f64.to_be_bytes()).collect();
    r.tree
        .upload(&upload(&[
            node(
                1,
                4,
                block(&[
                    px(1, 160.),
                    px(2, 120.),
                    pad,
                    color([0, 0, 255]),
                    vec![40, 1],
                ]),
                &[2],
            ),
            node(2, 3, guard(false, 12.), &[3, 4]),
            node(3, 4, box_attrs(40., 40.), &[]),
            mounted(node(4, 4, block(&[px(1, 40.), px(2, 40.)]), &[]), &[(6, 5)]),
            node(5, 9, image, &[]),
        ]))
        .unwrap();
    let mut l = images::loader();
    images::load(&mut r.tree, &mut l);
    let paper = ([0, 0, 255, 255], 1);
    let bitmap = ([255, 0, 0, 255], 5);
    let shadow = ([0, 255, 0, 255], 1); // shadows don't expand the listener
    for nearby in [true, false] {
        if !nearby {
            // Same bitmap, position and scroll offset; only the incoming edge changes.
            r.tree.patch(&patch(4, 4, &[edges(4, 4, &[5])])).unwrap();
        }
        for (clip, radius) in [(false, 12.), (true, 12.), (true, 0.), (false, 12.)] {
            r.tree.patch(&set(2, 3, guard(clip, radius))).unwrap();
            draw(&mut r);
            for (x, y, (color, id)) in [
                (25, 36, paper),
                (31, 31, if radius != 0. { paper } else { bitmap }),
                (46, 36, bitmap),
                (46, 64, shadow),
                (46, 74, paper),
            ] {
                let i = 4 * (180 * y + x);
                assert_eq!(
                    &r.pixels()[i..i + 4],
                    color,
                    "nearby={nearby} clip={clip}/{radius} at {x},{y}"
                );
                assert_eq!(r.hit_at(x as f32, y as f32).unwrap().0, id);
            }
        }
    }
}
#[test]
fn echo_deadlines_app_replacements_and_stale_receipts_are_axis_local() {
    let port = |enabled, prop: Option<f64>| {
        block(
            &[
                Some(px(1, 100.)),
                Some(px(2, 60.)),
                Some(vec![7, enabled]),
                Some(vec![8, 1]),
                prop.map(|v| fraction(86, v)),
            ]
            .into_iter()
            .flatten()
            .collect::<Vec<_>>(),
        )
    };
    let mut r = Raster::default();
    r.tree
        .upload(&upload(&[
            node(1, 4, port(1, Some(0.)), &[2]),
            node(2, 4, box_attrs(200., 160.), &[]),
        ]))
        .unwrap();
    draw(&mut r);
    let k = r.tree.elements.ids[&1];
    let t = Instant::now();
    for (n, y, at) in [
        (1, 0.1, t - Duration::from_secs(6)),
        (2, 0.2, t),
        (3, 0.4, t),
    ] {
        assert!(set_input(&mut r.tree, 1, [0.5, y], n, Some(at)));
    }
    assert!(!set_input(&mut r.tree, 1, [0.1, 0.1], 2, None));
    let old = view(&r, 1).axes;
    for (v, y) in [(Some(0.2), 0.4), (None, 0.4), (Some(0.1), 0.1)] {
        r.tree.patch(&set(1, 4, port(1, v))).unwrap();
        draw(&mut r);
        assert_eq!(view(&r, 1).axes.map(|a| a.pos), [0.5, y]);
    }
    let settled = view(&r, 1).axes;
    r.tree.patch(&set(1, 4, port(1, None))).unwrap();
    draw(&mut r);
    r.tree.patch(&set(1, 4, port(1, Some(0.1)))).unwrap();
    draw(&mut r);
    assert_eq!(view(&r, 1).axes, settled, "a no-op must not cancel capture");
    let mut late = scroll::Update::default();
    let mut axes = old;
    axes[1].input = 4;
    late.record(axes, [false, true], None);
    assert!(
        !r.tree
            .scroll_input(Attachment(k, r.tree.elements.slots[k].attachment), late)
    );
    r.tree.patch(&set(1, 4, port(0, Some(0.1)))).unwrap();
    draw(&mut r);
    assert_eq!(view(&r, 1).axes.map(|a| a.pos), [0.5, 0.]);
    r.tree.patch(&set(1, 4, port(1, Some(0.1)))).unwrap();
    draw(&mut r);
    assert_eq!(view(&r, 1).offset(), [50., 0.]);
}
#[test]
fn legacy_pixels_are_subordinate_to_proposals_and_first_final_edits_do_not_rearm() {
    let fields = |xy: [Bytes; 2]| {
        block(&[
            px(1, 100.),
            px(2, 60.),
            vec![7, 1],
            xy[0].clone(),
            xy[1].clone(),
        ])
    };
    let proposal = |y| fields([fraction(86, y), number(39, 10.)]);
    let mut r = Raster::default();
    r.tree
        .upload(&upload(&[
            node(1, 4, proposal(0.25), &[2]),
            node(2, 4, box_attrs(120., 160.), &[]),
        ]))
        .unwrap();
    draw(&mut r);
    assert_eq!(view(&r, 1).offset()[1], 25.);
    assert!(set_input(&mut r.tree, 1, [0., 0.7], 1, None));
    draw(&mut r);
    r.tree.patch(&set(1, 4, proposal(0.5))).unwrap();
    r.tree
        .patch(&set(1, 4, fields([number(39, 10.), fraction(86, 0.25)])))
        .unwrap();
    draw(&mut r);
    assert_eq!(view(&r, 1).offset()[1], 70.);
    assert!(!ran(&r, PaintContext, 1));
    r.tree
        .patch(&set(
            1,
            4,
            block(&[px(1, 100.), px(2, 60.), vec![7, 1], number(39, 10.)]),
        ))
        .unwrap();
    draw(&mut r);
    assert_eq!(view(&r, 1).offset()[1], 10.);
}
#[test]
fn malformed_positions_refuse_both_ingresses_and_growth_keeps_live_progress() {
    let root = block(&[px(1, 100.), px(2, 60.), vec![7, 1]]);
    let wire = upload(&[
        node(1, 3, root, &[2]),
        node(2, 4, box_attrs(100., 160.), &[]),
    ]);
    let mut w = Publisher::new(0);
    let mut t = Tree::for_session(0);
    let mut r = Raster::default();
    r.tree.upload(&wire).unwrap();
    w.apply(&wire, 1.).unwrap().install(&mut t);
    t.refresh(180., 170., 1.).unwrap();
    let old = t.seal().unwrap();
    assert!(set_input(&mut t, 1, [0., 0.75], 1, None));
    for bad in [
        vec![7, 2],
        vec![86, 3],
        vec![85, 1, 0],
        fraction(86, -0.1),
        fraction(85, 1.1),
        fraction(86, f64::NAN),
    ] {
        let p = set(1, 3, block(&[bad]));
        assert!(w.apply(&p, 1.).is_err());
        assert!(r.tree.patch(&p).is_err());
        assert_eq!(r.tree.encode(), wire);
    }
    let inline = upload(&[
        node(10, 7, block(&[px(1, 80.)]), &[11]),
        node(11, 4, block(&[vec![7, 1]]), &[12]),
        node(12, 5, text("word"), &[]),
    ]);
    let mut empty = Tree::for_session(0);
    let mut writer = Publisher::new(0);
    assert!(
        empty
            .upload(&inline)
            .unwrap_err()
            .contains("scrollbars require a block")
    );
    assert!(
        writer
            .apply(&inline, 1.)
            .err()
            .unwrap()
            .contains("scrollbars require a block")
    );
    w.apply(
        &patch(
            1,
            1,
            &[edges(1, 3, &[2, 3]), node(3, 4, box_attrs(100., 100.), &[])],
        ),
        1.,
    )
    .unwrap()
    .install(&mut t);
    t.refresh(180., 170., 1.).unwrap();
    let fresh = t.seal().unwrap();
    assert_eq!(old.hits.target_at(old.viewport, 4., 8.).unwrap().3, 8.);
    assert_eq!(
        fresh.hits.target_at(fresh.viewport, 4., 8.).unwrap().3,
        158.
    );
    assert_eq!(t.layers.scroll.view(t.elements.ids[&1]).axes[1].pos, 0.75);
}
