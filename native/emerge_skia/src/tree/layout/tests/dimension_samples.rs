//! Allocation-aware replay gates; the pixel-only negative controls stay separate.
use super::super::*;
use super::common::*;
use dimensions::{AxisFootprint, capture_axis, set_axis_sample};

fn pooled_tree(axis: Axis, lengths: &[Length], scale: f32, extent: f64) -> ElementTree {
    let (kind, width, height) = match axis {
        Axis::Width => (ElementKind::Row, extent, 40.0),
        Axis::Height => (ElementKind::Column, 40.0, extent),
    };
    let children: Vec<_> = lengths
        .iter()
        .enumerate()
        .map(|(index, length)| {
            let mut attrs = fixed_box_attrs(40.0, 40.0);
            match axis {
                Axis::Width => attrs.width = Some(length.clone()),
                Axis::Height => attrs.height = Some(length.clone()),
            }
            Element::with_attrs(
                NodeId::from_wire_u64(index as u64 + 2),
                ElementKind::El,
                vec![],
                attrs,
            )
        })
        .collect();
    let mut root = Element::with_attrs(
        NodeId::from_wire_u64(1),
        kind,
        vec![],
        fixed_box_attrs(width, height),
    );
    root.children = children.iter().map(|child| child.id).collect();
    let mut tree = ElementTree::new();
    tree.set_root_id(root.id);
    std::iter::once(root)
        .chain(children)
        .for_each(|node| tree.insert(node));
    run(&mut tree, axis, extent, scale);
    tree
}

fn run(tree: &mut ElementTree, axis: Axis, extent: f64, scale: f32) {
    let constraint = match axis {
        Axis::Width => Constraint::new(extent as f32 * scale, 40.0 * scale),
        Axis::Height => Constraint::new(40.0 * scale, extent as f32 * scale),
    };
    layout_tree(tree, constraint, scale, &MockTextMeasurer);
}

fn id(index: usize) -> NodeId {
    NodeId::from_wire_u64(index as u64 + 2)
}
fn footprint(tree: &ElementTree, index: usize, axis: Axis) -> AxisFootprint {
    capture_axis(tree, &id(index), axis).unwrap()
}
fn bound(value: f64, weight: f64, minimum: bool) -> Length {
    let args = (
        Box::new(Length::Px(value)),
        Box::new(Length::FillWeighted(weight)),
    );
    if minimum {
        Length::Min(args.0, args.1)
    } else {
        Length::Max(args.0, args.1)
    }
}
type Geometry = Vec<(NodeId, Frame, Option<Frame>, Option<Frame>, f32, f32)>;
fn geometry(tree: &ElementTree) -> Geometry {
    let mut nodes: Vec<_> = tree
        .iter_nodes()
        .map(|node| {
            (
                node.id,
                node.layout.frame.unwrap(),
                node.layout.render_frame,
                node.layout.measured_frame,
                node.layout.scroll_x_max,
                node.layout.scroll_y_max,
            )
        })
        .collect();
    nodes.sort_by_key(|node| node.0.to_wire_u64());
    nodes
}
fn assert_near(actual: f32, expected: f32) {
    assert!((actual - expected).abs() < 0.002, "{actual} != {expected}");
}

#[test]
fn footprint_replay_preserves_bounded_allocation_for_every_subset() {
    for axis in [Axis::Width, Axis::Height] {
        for scale in [0.5, 1.0, 2.0] {
            for extent in [100.0, 600.0] {
                for lengths in [
                    vec![bound(50.0, 1.0, true), Length::Fill],
                    vec![bound(50.0, 3.0, true), Length::Fill],
                    vec![bound(700.0, 1.0, false), Length::Fill],
                    vec![
                        Length::Max(Box::new(Length::Px(30.0)), Box::new(bound(50.0, 1.0, true))),
                        Length::Fill,
                    ],
                    vec![
                        bound(50.0, 0.5, true),
                        bound(75.0, 1.5, false),
                        Length::Fill,
                        Length::Px(20.0),
                    ],
                    vec![Length::Px(700.0), bound(50.0, 0.25, true), Length::Fill],
                ] {
                    let reference = pooled_tree(axis, &lengths, scale, extent);
                    let expected = geometry(&reference);
                    for subset in 1..(1 << lengths.len()) {
                        let mut replay = pooled_tree(axis, &lengths, scale, extent);
                        for index in 0..lengths.len() {
                            if subset & (1 << index) != 0 {
                                set_axis_sample(
                                    &mut replay,
                                    &id(index),
                                    axis,
                                    Some(footprint(&reference, index, axis)),
                                )
                                .unwrap();
                            }
                        }
                        run(&mut replay, axis, extent, scale);
                        assert_geometry(&geometry(&replay), &expected);
                        // A warm cache must retain charge and initial-size facts.
                        let before = footprint(&replay, 0, axis);
                        run(&mut replay, axis, extent, scale);
                        assert_eq!(footprint(&replay, 0, axis), before);
                        assert_geometry(&geometry(&replay), &expected);
                        for index in 0..lengths.len() {
                            set_axis_sample(&mut replay, &id(index), axis, None).unwrap();
                        }
                        run(&mut replay, axis, extent, scale);
                        assert_geometry(&geometry(&replay), &expected);
                    }
                }
            }
        }
    }
}

#[test]
fn mixed_box_and_charge_samples_have_continuous_peer_allocation() {
    for axis in [Axis::Width, Axis::Height] {
        for scale in [0.5, 1.0, 2.0] {
            let source = pooled_tree(axis, &[Length::Px(40.0), Length::Fill], scale, 600.0);
            let mut destination =
                pooled_tree(axis, &[bound(50.0, 1.0, true), Length::Fill], scale, 600.0);
            let from = footprint(&source, 0, axis);
            let to = footprint(&destination, 0, axis);
            assert_eq!(
                (from.visible, from.charge, to.visible, to.charge),
                (40.0, 40.0, 50.0, 300.0)
            );
            for t in [0.0, 0.00001, 0.25, 0.5, 0.75, 0.99999, 1.0, 0.5, 0.0] {
                let sample = from.interpolate(to, t).unwrap();
                set_axis_sample(&mut destination, &id(0), axis, Some(sample)).unwrap();
                run(&mut destination, axis, 600.0, scale);
                let actual = footprint(&destination, 0, axis);
                assert_near(actual.visible, 40.0 + 10.0 * t);
                assert_near(actual.charge, 40.0 + 260.0 * t);
                assert_near(footprint(&destination, 1, axis).visible, 560.0 - 260.0 * t);
                assert_eq!(actual.policy, sample.policy);
            }
        }
    }
}

#[test]
fn same_box_different_charge_invalidates_retained_layout() {
    for axis in [Axis::Width, Axis::Height] {
        let source = pooled_tree(axis, &[bound(50.0, 1.0, true), Length::Fill], 1.0, 600.0);
        let mut destination =
            pooled_tree(axis, &[bound(50.0, 3.0, true), Length::Fill], 1.0, 600.0);
        let from = footprint(&source, 0, axis);
        let to = footprint(&destination, 0, axis);
        for (sample, peer) in [(from, 300.0), (to, 150.0), (from, 300.0)] {
            set_axis_sample(&mut destination, &id(0), axis, Some(sample)).unwrap();
            run(&mut destination, axis, 600.0, 1.0);
            assert_eq!(footprint(&destination, 0, axis).visible, 50.0);
            assert_eq!(footprint(&destination, 1, axis).visible, peer);
        }
    }
}

#[test]
fn interruption_captures_the_whole_sample_and_scale_is_applied_once() {
    let axis = Axis::Width;
    let source = pooled_tree(axis, &[Length::Px(40.0), Length::Fill], 1.0, 600.0);
    let mut target = pooled_tree(axis, &[bound(50.0, 1.0, true), Length::Fill], 1.0, 600.0);
    let from = footprint(&source, 0, axis);
    let to = footprint(&target, 0, axis);
    let midpoint = from.interpolate(to, 0.5).unwrap();
    set_axis_sample(&mut target, &id(0), axis, Some(midpoint)).unwrap();
    run(&mut target, axis, 600.0, 1.0);
    let captured = footprint(&target, 0, axis);
    assert_eq!(captured, midpoint);
    for scale in [2.0, 0.5, 1.0] {
        run(&mut target, axis, 600.0, scale);
        assert_eq!(footprint(&target, 0, axis), captured);
        assert_eq!(footprint(&target, 1, axis).visible, 430.0);
    }
    let reverse = captured.interpolate(from, 0.5).unwrap();
    assert_eq!((reverse.visible, reverse.charge), (42.5, 105.0));
}

fn assert_frame(actual: Frame, expected: Frame) {
    for (actual, expected) in [
        actual.x,
        actual.y,
        actual.width,
        actual.height,
        actual.content_width,
        actual.content_height,
    ]
    .into_iter()
    .zip([
        expected.x,
        expected.y,
        expected.width,
        expected.height,
        expected.content_width,
        expected.content_height,
    ]) {
        assert!(
            (actual - expected).abs() < 0.0002,
            "frame mismatch {actual} != {expected}"
        );
    }
}
fn assert_geometry(actual: &Geometry, expected: &Geometry) {
    assert_eq!(actual.len(), expected.len());
    for (actual, expected) in actual.iter().zip(expected) {
        assert_eq!(actual.0, expected.0);
        assert_frame(actual.1, expected.1);
        for (actual, expected) in [(actual.2, expected.2), (actual.3, expected.3)] {
            match (actual, expected) {
                (Some(actual), Some(expected)) => assert_frame(actual, expected),
                (None, None) => (),
                _ => panic!("different frame presence"),
            }
        }
        assert_near(actual.4, expected.4);
        assert_near(actual.5, expected.5);
    }
}

fn content_pool(axis: Axis, length: Length) -> ElementTree {
    let mut tree = pooled_tree(axis, &[Length::Fill, Length::Fill], 1.0, 600.0);
    for index in 0..2 {
        let inner = Element::with_attrs(
            NodeId::from_wire_u64(10 + index as u64),
            ElementKind::El,
            vec![],
            fixed_box_attrs(20.0, 20.0),
        );
        tree.get_mut(&id(index)).unwrap().children = vec![inner.id];
        tree.insert(inner);
    }
    let root = tree.get_mut(&NodeId::from_wire_u64(1)).unwrap();
    match axis {
        Axis::Width => root.spec.declared.width = Some(length),
        Axis::Height => root.spec.declared.height = Some(length),
    }
    tree.mark_all_measure_dirty();
    run(&mut tree, axis, 600.0, 1.0);
    tree
}

#[test]
fn content_to_definite_container_blends_child_distribution_without_a_start_jump() {
    let root = NodeId::from_wire_u64(1);
    for axis in [Axis::Width, Axis::Height] {
        let source = content_pool(axis, Length::Content);
        let mut target = content_pool(axis, Length::Px(600.0));
        let from = capture_axis(&source, &root, axis).unwrap();
        let to = capture_axis(&target, &root, axis).unwrap();
        let source_geometry = geometry(&source);
        let target_geometry = geometry(&target);
        assert_eq!((from.visible, from.policy.children_fill), (40.0, 0.0));
        assert_eq!((to.visible, to.policy.children_fill), (600.0, 1.0));
        for t in [0.0, 0.00001, 0.25, 0.5, 0.75, 0.99999, 1.0] {
            let sample = from.interpolate(to, t).unwrap();
            set_axis_sample(&mut target, &root, axis, Some(sample)).unwrap();
            run(&mut target, axis, 600.0, 1.0);
            assert_near(
                capture_axis(&target, &root, axis).unwrap().visible,
                40.0 + 560.0 * t,
            );
            let expected_child = 20.0 + 280.0 * t * t;
            assert_near(footprint(&target, 0, axis).visible, expected_child);
            assert_near(footprint(&target, 1, axis).visible, expected_child);
            if t == 0.0 {
                assert_geometry(&geometry(&target), &source_geometry);
            }
            if t == 1.0 {
                assert_geometry(&geometry(&target), &target_geometry);
            }
            assert_eq!(capture_axis(&target, &root, axis).unwrap(), sample);
        }
        set_axis_sample(&mut target, &root, axis, None).unwrap();
        run(&mut target, axis, 600.0, 1.0);
        assert_geometry(&geometry(&target), &target_geometry);
    }
}

fn implicit_parent(length: Length) -> ElementTree {
    let mut root = make_element("root", ElementKind::El, fixed_box_attrs(600.0, 40.0));
    let mut parent = make_element(
        "parent",
        ElementKind::Column,
        Attrs {
            height: Some(Length::Px(40.0)),
            ..Attrs::default()
        },
    );
    let child = Element::with_attrs(
        id(0),
        ElementKind::El,
        vec![],
        Attrs {
            width: Some(length),
            height: Some(Length::Px(40.0)),
            ..Attrs::default()
        },
    );
    parent.children = vec![child.id];
    root.children = vec![parent.id];
    let mut tree = ElementTree::new();
    tree.set_root_id(root.id);
    [root, parent, child]
        .into_iter()
        .for_each(|node| tree.insert(node));
    run(&mut tree, Axis::Width, 600.0, 1.0);
    tree
}

#[test]
fn bounded_child_fill_demand_does_not_snap_its_implicit_parent() {
    let source = implicit_parent(Length::Px(40.0));
    let mut target = implicit_parent(bound(50.0, 1.0, true));
    let from = footprint(&source, 0, Axis::Width);
    let to = footprint(&target, 0, Axis::Width);
    let target_geometry = geometry(&target);
    let parent_id = make_element("parent", ElementKind::Column, Attrs::default()).id;
    for t in [0.0, 0.00001, 0.25, 0.5, 0.75, 0.99999, 1.0] {
        set_axis_sample(
            &mut target,
            &id(0),
            Axis::Width,
            Some(from.interpolate(to, t).unwrap()),
        )
        .unwrap();
        run(&mut target, Axis::Width, 600.0, 1.0);
        let parent = target.get(&parent_id).unwrap();
        // Blend current finite intrinsic and fill candidates, not a bool predicate.
        assert_near(
            parent.layout.frame.unwrap().width,
            dimensions::blend(40.0 * (1.0 - t), 600.0, t),
        );
        assert_near(footprint(&target, 0, Axis::Width).visible, 40.0 + 10.0 * t);
        if t == 0.0 {
            assert_geometry(&geometry(&target), &geometry(&source));
        }
        if t == 1.0 {
            assert_geometry(&geometry(&target), &target_geometry);
        }
    }
}

#[test]
fn invalid_samples_and_foreign_pool_scopes_are_rejected_without_mutation() {
    let mut tree = pooled_tree(Axis::Width, &[Length::Fill, Length::Fill], 1.0, 600.0);
    let sample = footprint(&tree, 0, Axis::Width);
    let before = geometry(&tree);
    for invalid in [
        AxisFootprint {
            charge: f32::NAN,
            ..sample
        },
        AxisFootprint {
            visible: f32::INFINITY,
            ..sample
        },
        AxisFootprint {
            intrinsic: -1.0,
            ..sample
        },
    ] {
        assert!(set_axis_sample(&mut tree, &id(0), Axis::Width, Some(invalid)).is_err());
    }
    assert!(sample.interpolate(sample, f32::NAN).is_err());
    assert!(set_axis_sample(&mut tree, &id(0), Axis::Height, Some(sample)).is_err());
    tree.get_mut(&NodeId::from_wire_u64(1))
        .unwrap()
        .lifecycle
        .mounted_at_revision += 1;
    assert!(set_axis_sample(&mut tree, &id(0), Axis::Width, Some(sample)).is_err());
    assert_geometry(&geometry(&tree), &before);
    assert!(tree.get(&id(0)).unwrap().layout.dimension_samples.is_none());
}

fn image_tree(axis: Axis, length: Length, kind: ElementKind) -> ElementTree {
    let mut tree = pooled_tree(axis, &[length, Length::Fill], 1.0, 600.0);
    let image = tree.get_mut(&id(0)).unwrap();
    image.spec.kind = kind;
    image.spec.declared.image_size = Some((200.0, 100.0));
    match axis {
        Axis::Width => image.spec.declared.height = Some(Length::Content),
        Axis::Height => image.spec.declared.width = Some(Length::Content),
    }
    tree.mark_all_measure_dirty();
    run(&mut tree, axis, 600.0, 1.0);
    tree
}

#[test]
fn equal_image_boxes_retain_syntax_dependent_aspect_policy() {
    for axis in [Axis::Width, Axis::Height] {
        for kind in [ElementKind::Image, ElementKind::Video] {
            let source = image_tree(axis, Length::Px(40.0), kind);
            let mut target = image_tree(
                axis,
                Length::Min(Box::new(Length::Px(40.0)), Box::new(Length::Px(100.0))),
                kind,
            );
            let from = footprint(&source, 0, axis);
            let to = footprint(&target, 0, axis);
            let target_geometry = geometry(&target);
            for t in [0.0, 0.00001, 0.25, 0.5, 0.75, 0.99999, 1.0] {
                set_axis_sample(
                    &mut target,
                    &id(0),
                    axis,
                    Some(from.interpolate(to, t).unwrap()),
                )
                .unwrap();
                run(&mut target, axis, 600.0, 1.0);
                if t == 0.0 {
                    assert_geometry(&geometry(&target), &geometry(&source));
                }
                if t == 1.0 {
                    assert_geometry(&geometry(&target), &target_geometry);
                }
                let opposite = match axis {
                    Axis::Width => Axis::Height,
                    Axis::Height => Axis::Width,
                };
                let source_opposite = footprint(&source, 0, opposite).visible;
                let target_opposite = capture_axis(&target, &id(0), opposite).unwrap().visible;
                let to_opposite = match (axis, kind) {
                    (Axis::Width, _) => 100.0,
                    (Axis::Height, _) => 200.0,
                };
                assert_near(
                    target_opposite,
                    dimensions::blend(source_opposite, to_opposite, t),
                );
            }
            set_axis_sample(&mut target, &id(0), axis, None).unwrap();
            run(&mut target, axis, 600.0, 1.0);
            assert_geometry(&geometry(&target), &target_geometry);
        }
    }
}

#[test]
fn allocation_replay_keeps_visible_alignment_with_rotation_insets_and_layout_scale() {
    for axis in [Axis::Width, Axis::Height] {
        for angle in [0.0, 33.0, 90.0] {
            for evenly in [false, true] {
                let mut reference = pooled_tree(
                    axis,
                    &[bound(50.0, 1.0, true), bound(700.0, 2.0, false)],
                    1.0,
                    600.0,
                );
                let root_id = NodeId::from_wire_u64(1);
                let root = reference.get_mut(&root_id).unwrap();
                root.spec.declared.space_evenly = Some(evenly);
                root.spec.declared.padding = Some(Padding::Uniform(5.0));
                root.spec.declared.border_width = Some(BorderWidth::Uniform(2.0));
                root.spec.declared.spacing = Some(7.0);
                for index in 0..2 {
                    let child = reference.get_mut(&id(index)).unwrap();
                    child.spec.declared.align_x = Some(AlignX::Center);
                    child.spec.declared.align_y = Some(AlignY::Center);
                    child.spec.declared.layout_rotate = Some(angle);
                    child.spec.declared.layout_scale = Some(1.5);
                }
                reference.mark_all_measure_dirty();
                run(&mut reference, axis, 600.0, 1.0);
                let expected = geometry(&reference);
                let sample = footprint(&reference, 0, axis);
                set_axis_sample(&mut reference, &id(0), axis, Some(sample)).unwrap();
                run(&mut reference, axis, 600.0, 1.0);
                assert_geometry(&geometry(&reference), &expected);
            }
        }
    }
}

fn overflowing_container(axis: Axis, kind: ElementKind, length: Length) -> ElementTree {
    let mut tree = pooled_tree(axis, &[length, Length::Fill], 1.0, 600.0);
    let owner = tree.get_mut(&id(0)).unwrap();
    owner.spec.kind = kind;
    owner.spec.declared.align_x = Some(AlignX::Center);
    owner.spec.declared.align_y = Some(AlignY::Center);
    let inner_id = NodeId::from_wire_u64(10);
    let nearby_id = NodeId::from_wire_u64(20);
    owner.children = vec![inner_id];
    owner.nearby.push(NearbySlot::InFront, nearby_id);
    let mut inner = Element::with_attrs(
        inner_id,
        ElementKind::El,
        vec![],
        fixed_box_attrs(100.0, 100.0),
    );
    inner.spec.declared.align_x = Some(AlignX::Right);
    inner.spec.declared.align_y = Some(AlignY::Bottom);
    let mut nearby = Element::with_attrs(
        nearby_id,
        ElementKind::El,
        vec![],
        fixed_box_attrs(10.0, 10.0),
    );
    nearby.spec.declared.align_x = Some(AlignX::Right);
    nearby.spec.declared.align_y = Some(AlignY::Bottom);
    [inner, nearby]
        .into_iter()
        .for_each(|node| tree.insert(node));
    tree.mark_all_measure_dirty();
    run(&mut tree, axis, 600.0, 1.0);
    tree
}

#[test]
fn owned_final_boxes_preserve_descendant_and_nearby_alignment_at_both_limits() {
    for axis in [Axis::Width, Axis::Height] {
        for kind in [
            ElementKind::El,
            ElementKind::Row,
            ElementKind::Column,
            ElementKind::WrappedRow,
        ] {
            let source = overflowing_container(axis, kind, Length::Px(40.0));
            let mut target = overflowing_container(axis, kind, Length::Content);
            let from = footprint(&source, 0, axis);
            let to = footprint(&target, 0, axis);
            let expected_target = geometry(&target);
            for t in [0.0, 0.000001, 0.25, 0.5, 0.75, 0.999999, 1.0] {
                set_axis_sample(
                    &mut target,
                    &id(0),
                    axis,
                    Some(from.interpolate(to, t).unwrap()),
                )
                .unwrap();
                run(&mut target, axis, 600.0, 1.0);
                if t <= 0.000001 {
                    assert_geometry(&geometry(&target), &geometry(&source));
                }
                if t >= 0.999999 {
                    assert_geometry(&geometry(&target), &expected_target);
                }
                let current = footprint(&target, 0, axis);
                assert_near(
                    current.visible,
                    dimensions::blend(from.visible, to.visible, t),
                );
            }
            set_axis_sample(&mut target, &id(0), axis, None).unwrap();
            run(&mut target, axis, 600.0, 1.0);
            assert_geometry(&geometry(&target), &expected_target);
        }
    }
}

fn placed_tree(kind: ElementKind, axis: Axis, length: Length) -> ElementTree {
    let mut tree = pooled_tree(axis, &[length, Length::Px(30.0)], 1.0, 100.0);
    tree.get_mut(&NodeId::from_wire_u64(1)).unwrap().spec.kind = kind;
    if matches!(kind, ElementKind::TextColumn | ElementKind::Paragraph) {
        tree.get_mut(&id(0)).unwrap().spec.declared.align_x = Some(AlignX::Right);
        tree.get_mut(&id(1)).unwrap().spec.declared.align_x = Some(AlignX::Left);
    }
    tree.mark_all_measure_dirty();
    run(&mut tree, axis, 100.0, 1.0);
    tree
}

#[test]
fn wrapped_lines_and_float_slots_replay_their_own_planner_facts() {
    for axis in [Axis::Width, Axis::Height] {
        for kind in [
            ElementKind::WrappedRow,
            ElementKind::TextColumn,
            ElementKind::Paragraph,
        ] {
            let source = placed_tree(kind, axis, Length::Px(40.0));
            let mut target = placed_tree(kind, axis, Length::Fill);
            let from = footprint(&source, 0, axis);
            let to = footprint(&target, 0, axis);
            let expected_target = geometry(&target);
            for t in [0.0, 0.000001, 0.25, 0.5, 0.75, 0.999999, 1.0] {
                set_axis_sample(
                    &mut target,
                    &id(0),
                    axis,
                    Some(from.interpolate(to, t).unwrap()),
                )
                .unwrap();
                run(&mut target, axis, 100.0, 1.0);
                if t <= 0.000001 {
                    assert_geometry(&geometry(&target), &geometry(&source));
                }
                if t >= 0.999999 {
                    assert_geometry(&geometry(&target), &expected_target);
                }
            }
            set_axis_sample(&mut target, &id(0), axis, None).unwrap();
            run(&mut target, axis, 100.0, 1.0);
            assert_geometry(&geometry(&target), &expected_target);
        }
    }
}

#[test]
fn parent_imposed_dimensions_take_precedence_over_a_retained_sample() {
    let mut tree = pooled_tree(
        Axis::Width,
        &[Length::Px(10.0), Length::Fill, Length::Px(40.0)],
        1.0,
        600.0,
    );
    let root_id = NodeId::from_wire_u64(1);
    tree.get_mut(&root_id).unwrap().spec.kind = ElementKind::Slider;
    tree.mark_all_measure_dirty();
    run(&mut tree, Axis::Width, 600.0, 1.0);
    let sample = footprint(&tree, 0, Axis::Width);
    let reference = geometry(&tree);
    assert_eq!(sample.visible, 560.0);
    set_axis_sample(&mut tree, &id(0), Axis::Width, Some(sample)).unwrap();
    run(&mut tree, Axis::Width, 600.0, 1.0);
    assert_geometry(&geometry(&tree), &reference);
    tree.get_mut(&root_id).unwrap().spec.declared.width = Some(Length::Px(500.0));
    tree.mark_all_measure_dirty();
    run(&mut tree, Axis::Width, 500.0, 1.0);
    assert_eq!(footprint(&tree, 0, Axis::Width).visible, 460.0);
    let resized = geometry(&tree);
    set_axis_sample(&mut tree, &id(0), Axis::Width, None).unwrap();
    run(&mut tree, Axis::Width, 500.0, 1.0);
    assert_geometry(&geometry(&tree), &resized);
}

fn interactive_pool(axis: Axis, length: Length) -> ElementTree {
    use crate::tree::attrs::Background;
    let mut tree = pooled_tree(axis, &[length, Length::Fill], 1.0, 600.0);
    let root = tree.get_mut(&NodeId::from_wire_u64(1)).unwrap();
    match axis {
        Axis::Width => root.spec.declared.scrollbar_x = Some(true),
        Axis::Height => root.spec.declared.scrollbar_y = Some(true),
    }
    for (index, color) in [(0, "red"), (1, "blue")] {
        let child = tree.get_mut(&id(index)).unwrap();
        child.spec.declared.on_click = Some(true);
        child.spec.declared.background = Some(Background::Color(Color::Named(color.to_owned())));
    }
    tree.mark_all_measure_dirty();
    run(&mut tree, axis, 600.0, 1.0);
    tree
}

#[test]
fn endpoint_replay_preserves_clipped_pixels_hits_and_scroll_geometry() {
    use crate::events::registry_builder::{
        assert_registry_rebuild_payloads_equivalent, build_registry_rebuild_cached,
    };
    use crate::tree::render::render_tree;
    for axis in [Axis::Width, Axis::Height] {
        let source = interactive_pool(axis, bound(700.0, 1.0, false));
        let mut target = interactive_pool(axis, bound(50.0, 1.0, true));
        let from = footprint(&source, 0, axis);
        let to = footprint(&target, 0, axis);
        let target_reference = target.clone();
        let (width, height) = match axis {
            Axis::Width => (600, 40),
            Axis::Height => (40, 600),
        };
        // Seed registry caches before changing only native dimensions.
        build_registry_rebuild_cached(&mut target);
        for (sample, reference) in [(from, &source), (to, &target_reference), (from, &source)] {
            set_axis_sample(&mut target, &id(0), axis, Some(sample)).unwrap();
            run(&mut target, axis, 600.0, 1.0);
            let expected = render_tree(reference);
            let actual = render_tree(&target);
            assert_geometry(&geometry(&target), &geometry(reference));
            assert_registry_rebuild_payloads_equivalent(
                &actual.event_rebuild,
                &expected.event_rebuild,
            );
            assert_registry_rebuild_payloads_equivalent(
                &build_registry_rebuild_cached(&mut target),
                &expected.event_rebuild,
            );
            assert_eq!(
                render_scene_to_pixels(width, height, actual.scene),
                render_scene_to_pixels(width, height, expected.scene)
            );
        }
    }
}

#[test]
fn dimension_sample_storage_is_sparse_and_identical_installation_is_a_noop() {
    use std::mem::size_of;
    use std::sync::Arc;
    let mut tree = pooled_tree(Axis::Width, &[Length::Fill, Length::Fill], 1.0, 600.0);
    assert!(
        tree.iter_nodes()
            .all(|node| node.layout.dimension_samples.is_none())
    );
    let sample = footprint(&tree, 0, Axis::Width);
    set_axis_sample(&mut tree, &id(0), Axis::Width, Some(sample)).unwrap();
    run(&mut tree, Axis::Width, 600.0, 1.0);
    let before = tree
        .get(&id(0))
        .unwrap()
        .layout
        .dimension_samples
        .clone()
        .unwrap();
    set_axis_sample(&mut tree, &id(0), Axis::Width, Some(sample)).unwrap();
    let node = tree.get(&id(0)).unwrap();
    assert!(Arc::ptr_eq(
        &before,
        node.layout.dimension_samples.as_ref().unwrap()
    ));
    assert!(!node.layout.measure_dirty && !node.layout.resolve_dirty);
    println!(
        "dimension storage: facts={} optional_samples={} cache_key={} footprint={} pair={}",
        size_of::<dimensions::DimensionFacts>(),
        size_of::<Option<Arc<dimensions::DimensionSamples>>>(),
        size_of::<dimensions::DimensionCacheKey>(),
        size_of::<AxisFootprint>(),
        size_of::<dimensions::DimensionSamples>()
    );
}

#[test]
fn replaying_children_inside_a_mixed_pool_does_not_interpolate_them_twice() {
    for axis in [Axis::Width, Axis::Height] {
        let root = NodeId::from_wire_u64(1);
        let source = content_pool(axis, Length::Content);
        let mut target = content_pool(axis, Length::Px(600.0));
        let from = capture_axis(&source, &root, axis).unwrap();
        let to = capture_axis(&target, &root, axis).unwrap();
        for t in [0.0, 0.25, 0.5, 0.75, 1.0] {
            set_axis_sample(
                &mut target,
                &root,
                axis,
                Some(from.interpolate(to, t).unwrap()),
            )
            .unwrap();
            run(&mut target, axis, 600.0, 1.0);
            let expected = geometry(&target);
            let children = [footprint(&target, 0, axis), footprint(&target, 1, axis)];
            for subset in 1..4 {
                for (index, child) in children.iter().enumerate() {
                    set_axis_sample(
                        &mut target,
                        &id(index),
                        axis,
                        (subset & (1 << index) != 0).then_some(*child),
                    )
                    .unwrap();
                }
                run(&mut target, axis, 600.0, 1.0);
                assert_geometry(&geometry(&target), &expected);
            }
            for index in 0..2 {
                set_axis_sample(&mut target, &id(index), axis, None).unwrap();
            }
        }
    }
}

fn evenly_changing_pool(axis: Axis, length: Length) -> ElementTree {
    let mut tree = pooled_tree(axis, &[Length::Px(20.0), Length::Px(20.0)], 1.0, 600.0);
    let root = tree.get_mut(&NodeId::from_wire_u64(1)).unwrap();
    root.spec.declared.space_evenly = Some(true);
    root.spec.declared.spacing = Some(7.0);
    match axis {
        Axis::Width => root.spec.declared.width = Some(length),
        Axis::Height => root.spec.declared.height = Some(length),
    }
    for index in 0..2 {
        let child = tree.get_mut(&id(index)).unwrap();
        child.spec.declared.align_x = Some(AlignX::Center);
        child.spec.declared.align_y = Some(AlignY::Center);
    }
    tree.mark_all_measure_dirty();
    run(&mut tree, axis, 600.0, 1.0);
    tree
}

#[test]
fn enabling_even_spacing_has_no_terminal_placement_jump() {
    for axis in [Axis::Width, Axis::Height] {
        let root = NodeId::from_wire_u64(1);
        let source = evenly_changing_pool(
            axis,
            Length::Max(Box::new(Length::Content), Box::new(Length::Px(200.0))),
        );
        let mut target = evenly_changing_pool(axis, Length::Px(600.0));
        let from = capture_axis(&source, &root, axis).unwrap();
        let to = capture_axis(&target, &root, axis).unwrap();
        let expected_target = geometry(&target);
        for t in [0.0, 0.0000001, 0.25, 0.5, 0.75, 0.9999999, 1.0] {
            set_axis_sample(
                &mut target,
                &root,
                axis,
                Some(from.interpolate(to, t).unwrap()),
            )
            .unwrap();
            run(&mut target, axis, 600.0, 1.0);
            if t <= 0.0000001 {
                assert_geometry(&geometry(&target), &geometry(&source));
            }
            if t >= 0.9999999 {
                assert_geometry(&geometry(&target), &expected_target);
            }
            let first = target.get(&id(0)).unwrap().layout.frame.unwrap();
            let position = match axis {
                Axis::Width => first.x,
                Axis::Height => first.y,
            };
            assert_near(position, ((200.0 + 400.0 * t - 47.0) / 2.0) * (1.0 - t));
        }
    }
}
