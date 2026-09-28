//! Group-boundary investigation using native layout, not a proposed group scheduler.
//! These distinguish local ownership from layout-context isolation.
use super::super::*;
use super::common::*;
use dimensions::{AxisFootprint, capture_axis, set_axis_sample};

fn id(value: u64) -> NodeId {
    NodeId::from_wire_u64(value)
}
fn attrs(axis: Axis, length: Length) -> Attrs {
    let mut attrs = fixed_box_attrs(40.0, 40.0);
    match axis {
        Axis::Width => attrs.width = Some(length),
        Axis::Height => attrs.height = Some(length),
    }
    attrs
}
fn extent(tree: &ElementTree, node: u64, axis: Axis, scale: f32) -> f32 {
    let frame = tree.get(&id(node)).unwrap().layout.frame.unwrap();
    match axis {
        Axis::Width => frame.width / scale,
        Axis::Height => frame.height / scale,
    }
}
fn run(tree: &mut ElementTree, axis: Axis, scale: f32) {
    let constraint = match axis {
        Axis::Width => Constraint::new(600.0 * scale, 40.0 * scale),
        Axis::Height => Constraint::new(40.0 * scale, 600.0 * scale),
    };
    layout_tree(tree, constraint, scale, &MockTextMeasurer);
}
fn pool(axis: Axis, lengths: &[Length], scale: f32) -> ElementTree {
    let kind = match axis {
        Axis::Width => ElementKind::Row,
        Axis::Height => ElementKind::Column,
    };
    let mut root = Element::with_attrs(id(1), kind, vec![], attrs(axis, Length::Px(600.0)));
    root.children = (0..lengths.len())
        .map(|index| id(index as u64 + 2))
        .collect();
    let mut tree = ElementTree::new();
    tree.set_root_id(root.id);
    std::iter::once(root)
        .chain(lengths.iter().enumerate().map(|(index, length)| {
            Element::with_attrs(
                id(index as u64 + 2),
                ElementKind::El,
                vec![],
                attrs(axis, length.clone()),
            )
        }))
        .for_each(|node| tree.insert(node));
    run(&mut tree, axis, scale);
    tree
}
fn fp(tree: &ElementTree, node: u64, axis: Axis) -> AxisFootprint {
    capture_axis(tree, &id(node), axis).unwrap()
}
fn near(actual: f32, expected: f32) {
    assert!((actual - expected).abs() < 0.002, "{actual} != {expected}");
}

#[test]
fn sibling_group_holds_early_finisher_and_releases_jointly_without_allocation_jump() {
    for axis in [Axis::Width, Axis::Height] {
        for scale in [0.5, 1.0, 2.0] {
            let source = pool(axis, &[Length::Px(40.0), Length::Px(40.0)], scale);
            let target = pool(axis, &[Length::Fill, Length::Fill], scale);
            let from = [fp(&source, 2, axis), fp(&source, 3, axis)];
            let to = [fp(&target, 2, axis), fp(&target, 3, axis)];
            for hz in [30, 60, 120] {
                let mut live = target.clone();
                for step in 0..=2 * hz {
                    let seconds = step as f32 / hz as f32;
                    let progress = [seconds.min(1.0), (seconds / 2.0).min(1.0)];
                    for index in 0..2 {
                        set_axis_sample(
                            &mut live,
                            &id(index as u64 + 2),
                            axis,
                            Some(from[index].interpolate(to[index], progress[index]).unwrap()),
                        )
                        .unwrap();
                    }
                    run(&mut live, axis, scale);
                    near(extent(&live, 2, axis, scale), 40.0 + 260.0 * progress[0]);
                    near(extent(&live, 3, axis, scale), 40.0 + 260.0 * progress[1]);
                    if step == hz {
                        // Releasing just A still reproduces the original 130px jump.
                        let mut early_release = live.clone();
                        set_axis_sample(&mut early_release, &id(2), axis, None).unwrap();
                        run(&mut early_release, axis, scale);
                        near(extent(&early_release, 2, axis, scale), 430.0);
                    }
                }
                let held = [fp(&live, 2, axis), fp(&live, 3, axis)];
                for node in [2, 3] {
                    set_axis_sample(&mut live, &id(node), axis, None).unwrap();
                }
                run(&mut live, axis, scale);
                assert_eq!([fp(&live, 2, axis), fp(&live, 3, axis)], held);
            }
        }
    }
}

#[test]
fn sibling_group_must_keep_reservations_for_unanimated_siblings() {
    for axis in [Axis::Width, Axis::Height] {
        let weighted = Length::FillWeighted(3.0);
        let source = pool(
            axis,
            &[Length::Px(40.0), Length::Px(40.0), Length::Fill],
            1.0,
        );
        let mut target = pool(axis, &[weighted, Length::Fill, Length::Fill], 1.0);
        let a = fp(&target, 2, axis);
        let b = fp(&target, 3, axis);
        near(a.visible, 360.0);
        near(a.charge, 360.0);
        set_axis_sample(&mut target, &id(2), axis, Some(a)).unwrap();
        set_axis_sample(
            &mut target,
            &id(3),
            axis,
            Some(fp(&source, 3, axis).interpolate(b, 0.5).unwrap()),
        )
        .unwrap();
        run(&mut target, axis, 1.0);
        near(extent(&target, 4, axis, 1.0), 160.0);
        let mut pixels = target.clone();
        set_axis_sample(&mut pixels, &id(2), axis, None).unwrap();
        let node = pixels.get_mut(&id(2)).unwrap();
        match axis {
            Axis::Width => node.spec.declared.width = Some(Length::Px(50.0)),
            Axis::Height => node.spec.declared.height = Some(Length::Px(50.0)),
        }
        pixels.mark_measure_dirty(&id(2));
        run(&mut pixels, axis, 1.0);
        near(extent(&pixels, 4, axis, 1.0), 470.0);
        set_axis_sample(&mut target, &id(3), axis, Some(b)).unwrap();
        run(&mut target, axis, 1.0);
        near(extent(&target, 4, axis, 1.0), 120.0);
        for node in [2, 3] {
            set_axis_sample(&mut target, &id(node), axis, None).unwrap();
        }
        run(&mut target, axis, 1.0);
        near(extent(&target, 4, axis, 1.0), 120.0);
    }
}

fn nested(parent: Length, child: Length) -> ElementTree {
    let mut tree = pool(Axis::Width, &[parent], 1.0);
    tree.get_mut(&id(2)).unwrap().spec.kind = ElementKind::Row;
    tree.insert(Element::with_attrs(
        id(3),
        ElementKind::El,
        vec![],
        attrs(Axis::Width, child),
    ));
    tree.set_children(&id(2), vec![id(3)]).unwrap();
    tree.mark_measure_dirty(&id(2));
    run(&mut tree, Axis::Width, 1.0);
    tree
}

#[test]
fn content_parent_endpoint_needs_current_descendant_output_not_only_group_members() {
    // G(root) owns P's width; G(P) owns C's width. C grows during P's run.
    let initial = nested(Length::Content, Length::Px(40.0));
    let stale = fp(&initial, 2, Axis::Width);
    let mut current = nested(Length::Content, Length::Px(120.0));
    let refreshed = fp(&current, 2, Axis::Width);
    near(refreshed.visible, 120.0);
    set_axis_sample(&mut current, &id(2), Axis::Width, Some(stale)).unwrap();
    run(&mut current, Axis::Width, 1.0);
    near(extent(&current, 2, Axis::Width, 1.0), 40.0);
    set_axis_sample(&mut current, &id(2), Axis::Width, None).unwrap();
    run(&mut current, Axis::Width, 1.0);
    near(extent(&current, 2, Axis::Width, 1.0), 120.0);
    // Refreshing against the exported current child state restores replay parity.
    set_axis_sample(&mut current, &id(2), Axis::Width, Some(refreshed)).unwrap();
    run(&mut current, Axis::Width, 1.0);
    near(extent(&current, 2, Axis::Width, 1.0), 120.0);
}

#[test]
fn nested_fill_endpoint_needs_current_parent_constraint_not_its_original_box() {
    let initial = nested(Length::Px(200.0), Length::Fill);
    let mut current = nested(Length::Px(400.0), Length::Fill);
    set_axis_sample(
        &mut current,
        &id(3),
        Axis::Width,
        Some(fp(&initial, 3, Axis::Width)),
    )
    .unwrap();
    run(&mut current, Axis::Width, 1.0);
    near(extent(&current, 3, Axis::Width, 1.0), 200.0);
    set_axis_sample(&mut current, &id(3), Axis::Width, None).unwrap();
    run(&mut current, Axis::Width, 1.0);
    near(extent(&current, 3, Axis::Width, 1.0), 400.0);
}

#[test]
fn legacy_weight_sampling_cannot_switch_early_to_a_joint_final_box_without_a_jump() {
    let mut target = pool(Axis::Width, &[Length::FillWeighted(3.0), Length::Fill], 1.0);
    let a = fp(&target, 2, Axis::Width);
    let b = fp(&target, 3, Axis::Width);
    near(a.visible, 450.0);
    let source = pool(
        Axis::Width,
        &[Length::FillWeighted(1.0), Length::Px(40.0)],
        1.0,
    );
    set_axis_sample(
        &mut target,
        &id(3),
        Axis::Width,
        Some(fp(&source, 3, Axis::Width).interpolate(b, 0.5).unwrap()),
    )
    .unwrap();
    run(&mut target, Axis::Width, 1.0);
    // A's symbolic weight reaches 3 at 1s while B is halfway to its 2s destination.
    near(extent(&target, 3, Axis::Width, 1.0), 95.0);
    near(extent(&target, 2, Axis::Width, 1.0), 505.0);
    set_axis_sample(&mut target, &id(2), Axis::Width, Some(a)).unwrap();
    run(&mut target, Axis::Width, 1.0);
    near(extent(&target, 2, Axis::Width, 1.0), 450.0);
}

#[test]
fn separate_child_groups_exchange_context_through_unanimated_ancestors() {
    let scene = |first_child_width| {
        let mut tree = pool(Axis::Width, &[Length::Content, Length::Fill], 1.0);
        for node in [2, 3] {
            tree.get_mut(&id(node)).unwrap().spec.kind = ElementKind::Row;
        }
        tree.insert(Element::with_attrs(
            id(4),
            ElementKind::El,
            vec![],
            attrs(Axis::Width, Length::Px(first_child_width)),
        ));
        tree.insert(Element::with_attrs(
            id(5),
            ElementKind::El,
            vec![],
            attrs(Axis::Width, Length::Fill),
        ));
        tree.set_children(&id(2), vec![id(4)]).unwrap();
        tree.set_children(&id(3), vec![id(5)]).unwrap();
        tree.mark_measure_dirty(&id(2));
        tree.mark_measure_dirty(&id(3));
        run(&mut tree, Axis::Width, 1.0);
        tree
    };
    // C belongs to G(P), D belongs to G(Q); neither P nor Q needs an animation.
    let before = scene(40.0);
    let after = scene(200.0);
    near(extent(&before, 5, Axis::Width, 1.0), 560.0);
    near(extent(&after, 5, Axis::Width, 1.0), 400.0);
    assert_eq!(
        fp(&before, 5, Axis::Width).scope,
        fp(&after, 5, Axis::Width).scope
    );
}
