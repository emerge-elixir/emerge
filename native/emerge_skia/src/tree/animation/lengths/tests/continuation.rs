use super::support::{AttemptError, Driver, Event, Injection, Mode};
use super::*;
use crate::tree::animation::change::{ChangePolicy, Field};
use crate::tree::animation::lengths::inspection::QueryKind;
use crate::tree::animation::timing::apply_curve;

fn late_pixel_parent(axis: Axis) -> ElementTree {
    let mut tree = fixture(Length::Px(40.0), Length::Fill, axis);
    let mut parent = spec(Length::Fill, Length::Px(400.0), 2000.0, axis);
    parent.keyframes.push(
        spec(Length::Px(800.0), Length::Px(800.0), 1.0, axis)
            .keyframes
            .remove(0),
    );
    tree.get_mut(&id(1)).unwrap().spec.declared.animate = Some(parent);
    let child = tree
        .get_mut(&id(2))
        .unwrap()
        .spec
        .declared
        .animate
        .as_mut()
        .unwrap();
    child.duration_ms = 2000.0;
    child.keyframes.insert(0, child.keyframes[0].clone());
    tree
}

#[test]
fn later_pixel_segments_preserve_dependent_curve_anchors_through_joint_completion() {
    for mode in [Mode::Full, Mode::Active, Mode::Dirty] {
        for axis in [Axis::Width, Axis::Height] {
            let mut d = Driver::new(late_pixel_parent(axis), Instant::now(), mode);
            for (us, parent, child) in [
                (0, 600.0, 40.0),
                (500_000, 500.0, 40.0),
                (1_000_000, 400.0, 40.0),
                (1_500_000, 600.0, 170.0),
                (2_000_000, 800.0, 400.0),
            ] {
                let out = d.step(us, vec![]);
                assert!(out.result.is_ok(), "{:?}", out.result);
                close(extent(&d.tree, 1, axis), parent);
                close(extent(&d.tree, 2, axis), child);
            }
            assert!(d.runtime.groups.is_empty());
            assert!(d.tree.length_runtime.is_none());
        }
    }
}

#[test]
fn later_pixel_segments_follow_all_curves_rates_and_scales() {
    for axis in [Axis::Width, Axis::Height] {
        for scale in [0.5, 1.0, 2.0] {
            for curve in [
                AnimationCurve::Linear,
                AnimationCurve::EaseIn,
                AnimationCurve::EaseOut,
                AnimationCurve::EaseInOut,
            ] {
                for hz in [30, 60, 120] {
                    let mut tree = late_pixel_parent(axis);
                    tree.get_mut(&id(2))
                        .unwrap()
                        .spec
                        .declared
                        .animate
                        .as_mut()
                        .unwrap()
                        .curve = curve.clone();
                    let mut d = Driver::new(tree, Instant::now(), Mode::Active);
                    assert!(
                        d.step(
                            0,
                            vec![Event::Viewport {
                                width: 600.0,
                                height: 600.0,
                                scale
                            }]
                        )
                        .result
                        .is_ok()
                    );
                    for frame in 1..=hz * 2 {
                        let us = frame * 1_000_000 / hz;
                        let out = d.step(us, vec![]);
                        assert!(out.result.is_ok(), "{:?}", out.result);
                        if let Some(state) = d.tree.length_runtime.as_ref() {
                            assert_eq!(state.resolver.stats().model_copies, 1);
                        }
                        let t = (us as f64 / 1_000_000.0 - 1.0).max(0.0);
                        let expected = 40.0 + (200.0 + 200.0 * t - 40.0) * apply_curve(&curve, t);
                        close(
                            capture_axis(&d.tree, &id(2), axis).unwrap().visible,
                            expected as f32,
                        );
                        assert!(
                            out.inspection.queries.len() <= 4,
                            "{:?}",
                            out.inspection.queries
                        );
                    }
                    assert!(d.tree.length_runtime.is_none());
                }
            }
        }
    }
}

#[test]
fn pixel_continuation_witness_and_output_failures_preserve_publication_and_retry_clocks() {
    for mode in [Mode::Full, Mode::Active, Mode::Dirty] {
        for axis in [Axis::Width, Axis::Height] {
            for failure in [
                Injection::Query(QueryKind::PriorTarget),
                Injection::Query(QueryKind::PixelContinuation),
                Injection::BeforeLayout,
            ] {
                let mut d = Driver::new(late_pixel_parent(axis), Instant::now(), mode);
                d.step(0, vec![]);
                let before = d.step(1_000_000, vec![]).published_after.unwrap();
                let failed = d.attempt(1_500_000, vec![], failure);
                assert_eq!(
                    failed.result,
                    Err(if failure == Injection::BeforeLayout {
                        AttemptError::BeforeLayout
                    } else {
                        AttemptError::Native(ProjectionError::InjectedQuery)
                    })
                );
                failed.published_after.unwrap().assert_same(&before);
                let out = d.step(1_750_000, vec![]);
                assert!(out.result.is_ok(), "{:?}", out.result);
                close(extent(&d.tree, 2, axis), 272.5);
                assert!(d.step(2_000_000, vec![]).result.is_ok());
                assert!(d.tree.length_runtime.is_none());
            }
        }
    }
}

#[test]
fn pixel_continuation_cannot_launder_corrupted_dependent_terminal_footprints() {
    for mode in [Mode::Full, Mode::Active, Mode::Dirty] {
        for axis in [Axis::Width, Axis::Height] {
            for policy in [false, true] {
                let mut d = Driver::new(late_pixel_parent(axis), Instant::now(), mode);
                d.step(0, vec![]);
                let before = d.step(1_000_000, vec![]).published_after.unwrap();
                let track = Arc::make_mut(
                    d.tree
                        .length_runtime
                        .as_mut()
                        .unwrap()
                        .tracks
                        .get_mut(&(id(2), axis))
                        .unwrap(),
                );
                if policy {
                    track.to.policy.fill_request *= 0.5;
                } else {
                    track.to.charge -= 20.0;
                }
                let out = d.step(1_500_000, vec![]);
                assert_eq!(
                    out.result,
                    Err(AttemptError::Native(ProjectionError::ReleaseMismatch(
                        id(2),
                        axis
                    )))
                );
                out.published_after.unwrap().assert_same(&before);
            }
        }
    }
}

#[test]
fn pixel_equivalence_checks_foreign_policy_in_native_layout_before_and_after_the_frame() {
    for axis in [Axis::Width, Axis::Height] {
        for prior_sample in [false, true] {
            let mut d = Driver::new(late_pixel_parent(axis), Instant::now(), Mode::Full);
            d.step(0, vec![]);
            let first = d.step(1_000_000, vec![]).published_after.unwrap();
            let before = if prior_sample {
                d.step(1_500_000, vec![]).published_after.unwrap()
            } else {
                first
            };
            let state = d.tree.length_runtime.as_mut().unwrap();
            let parent = Arc::make_mut(state.tracks.get_mut(&(id(1), axis)).unwrap());
            parent.from.policy.children_fill = 0.0;
            parent.to.policy.children_fill = 0.0;
            if prior_sample {
                let child = Arc::make_mut(state.tracks.get_mut(&(id(2), axis)).unwrap());
                let projection = Arc::make_mut(&mut child.projection);
                let parent = projection.nodes.get_mut(&id(1)).unwrap();
                let sample = if axis == Axis::Width {
                    parent.width.as_mut()
                } else {
                    parent.height.as_mut()
                }
                .unwrap();
                sample.policy.children_fill = 0.0;
            }
            let out = d.step(1_750_000, vec![]);
            assert_eq!(
                out.result,
                Err(AttemptError::Native(ProjectionError::ReleaseMismatch(
                    id(if prior_sample { 1 } else { 2 }),
                    axis
                )))
            );
            assert!(out.inspection.queries.iter().any(|q| q.kind
                == if prior_sample {
                    QueryKind::PriorTarget
                } else {
                    QueryKind::PixelContinuation
                }));
            out.published_after.unwrap().assert_same(&before);
        }
    }
}

#[test]
fn later_pixel_segments_preserve_parent_scoped_charges_with_local_scale() {
    for axis in [Axis::Width, Axis::Height] {
        for scale in [0.5, 1.0, 2.0] {
            let mut tree = late_pixel_parent(axis);
            tree.get_mut(&id(1)).unwrap().spec.declared.layout_scale = Some(scale);
            tree.insert(Element::with_attrs(
                id(4),
                if axis == Axis::Width {
                    ElementKind::Row
                } else {
                    ElementKind::Column
                },
                vec![],
                Attrs {
                    width: Some(Length::Px(600.0)),
                    height: Some(Length::Px(600.0)),
                    ..Default::default()
                },
            ));
            tree.set_children(&id(4), vec![id(1)]).unwrap();
            tree.set_root_id(id(4));
            let mut d = Driver::new(tree, Instant::now(), Mode::Dirty);
            for (us, expected) in [
                (0, None),
                (1_000_000, Some(40.0)),
                (1_500_000, Some(170.0)),
                (1_750_000, Some(272.5)),
                (2_000_000, Some(400.0)),
            ] {
                let out = d.step(us, vec![]);
                assert!(out.result.is_ok(), "{:?}", out.result);
                if let Some(expected) = expected {
                    close(
                        capture_axis(&d.tree, &id(2), axis).unwrap().visible,
                        expected,
                    );
                }
            }
            assert!(d.tree.length_runtime.is_none());
        }
    }
}

#[test]
fn pixel_segment_continuations_reset_across_skipped_cycles_without_replaying_them() {
    for axis in [Axis::Width, Axis::Height] {
        for repeat in [AnimationRepeat::Times(2000), AnimationRepeat::Loop] {
            let mut tree = late_pixel_parent(axis);
            for node in [1, 2] {
                tree.get_mut(&id(node))
                    .unwrap()
                    .spec
                    .declared
                    .animate
                    .as_mut()
                    .unwrap()
                    .repeat = repeat.clone();
            }
            let mut d = Driver::new(tree, Instant::now(), Mode::Active);
            for (us, expected) in [
                (0, 40.0),
                (1_000_000, 40.0),
                (1_500_000, 170.0),
                (2_001_500_000, 170.0),
                (2_001_750_000, 272.5),
            ] {
                let out = d.step(us, vec![]);
                assert!(out.result.is_ok(), "{:?}", out.result);
                close(
                    capture_axis(&d.tree, &id(2), axis).unwrap().visible,
                    expected,
                );
                assert!(out.inspection.queries.len() <= 4);
            }
            if !matches!(repeat, AnimationRepeat::Loop) {
                let out = d.step(4_000_000_000, vec![]);
                assert!(out.result.is_ok(), "{:?}", out.result);
                close(capture_axis(&d.tree, &id(2), axis).unwrap().visible, 400.0);
                assert!(d.runtime.groups.is_empty());
                assert!(d.tree.length_runtime.is_none());
                assert!(out.inspection.queries.len() <= 4);
            }
        }
    }
}

#[test]
fn mixed_parent_clocks_preserve_dependent_anchors_and_native_release() {
    for axis in [Axis::Width, Axis::Height] {
        for mode in [Mode::Full, Mode::Active, Mode::Dirty] {
            for reverse in [false, true] {
                let mut tree = fixture(Length::Px(40.0), Length::Fill, axis);
                tree.get_mut(&id(1)).unwrap().spec.declared.animate = Some(spec(
                    if reverse {
                        Length::Fill
                    } else {
                        Length::Px(200.0)
                    },
                    if reverse {
                        Length::Px(200.0)
                    } else {
                        Length::Fill
                    },
                    2000.0,
                    axis,
                ));
                tree.get_mut(&id(1))
                    .unwrap()
                    .spec
                    .declared
                    .animate
                    .as_mut()
                    .unwrap()
                    .repeat = AnimationRepeat::Loop;
                let mut d = Driver::new(tree, Instant::now(), mode);
                for (us, expected) in [
                    (0, 40.0),
                    (500_000, if reverse { 145.0 } else { 95.0 }),
                    (1_000_000, 200.0),
                    (1_750_000, if reverse { 125.0 } else { 275.0 }),
                ] {
                    let out = d.step(us, vec![]);
                    assert!(
                        out.result.is_ok(),
                        "{us}: {:?} {:?}",
                        out.result,
                        out.inspection.queries
                    );
                    close(
                        capture_axis(&d.tree, &id(2), axis).unwrap().visible,
                        expected,
                    );
                }
                assert!(d.tree.length_runtime.is_some());
                assert!(d.runtime.groups.is_empty());
            }
        }
    }
}

#[test]
fn mixed_ancestor_footprints_follow_curves_and_all_frame_rates_at_each_scale() {
    for axis in [Axis::Width, Axis::Height] {
        for scale in [0.5, 1.0, 2.0] {
            for curve in [
                AnimationCurve::Linear,
                AnimationCurve::EaseIn,
                AnimationCurve::EaseOut,
                AnimationCurve::EaseInOut,
            ] {
                for hz in [30, 60, 120] {
                    let mut tree = fixture(Length::Px(40.0), Length::Fill, axis);
                    let mut parent = spec(Length::Px(200.0), Length::Fill, 2000.0, axis);
                    parent.curve = curve.clone();
                    parent.repeat = AnimationRepeat::Loop;
                    tree.get_mut(&id(1)).unwrap().spec.declared.animate = Some(parent);
                    tree.get_mut(&id(2))
                        .unwrap()
                        .spec
                        .declared
                        .animate
                        .as_mut()
                        .unwrap()
                        .curve = curve.clone();
                    let mut d = Driver::new(tree, Instant::now(), Mode::Active);
                    assert!(
                        d.step(
                            0,
                            vec![Event::Viewport {
                                width: 600.0 * scale,
                                height: 600.0 * scale,
                                scale
                            }]
                        )
                        .result
                        .is_ok()
                    );
                    for frame in 1..hz * 2 {
                        let us = frame * 1_000_000 / hz;
                        let t = us as f64 / 1_000_000.0;
                        let out = d.step(us, vec![]);
                        assert!(out.result.is_ok(), "{us}: {:?}", out.result);
                        let parent = 200.0 + 400.0 * apply_curve(&curve, t / 2.0);
                        let child = if t <= 1.0 {
                            40.0 + (parent / 2.0 - 40.0) * apply_curve(&curve, t)
                        } else {
                            parent / 2.0
                        };
                        close(
                            capture_axis(&d.tree, &id(1), axis).unwrap().visible,
                            parent as f32,
                        );
                        close(
                            capture_axis(&d.tree, &id(2), axis).unwrap().visible,
                            child as f32,
                        );
                        // Exact forecast provenance adds a native old-goal query.
                        assert!(out.inspection.queries.len() <= 5);
                    }
                    assert!(d.tree.length_runtime.is_some());
                    assert!(d.runtime.groups.is_empty());
                }
            }
        }
    }
}

#[test]
fn mixed_ancestor_witnesses_validate_foreign_goals_and_keep_failed_output_retry_safe() {
    for axis in [Axis::Width, Axis::Height] {
        for mode in [Mode::Full, Mode::Active, Mode::Dirty] {
            for failure in 0..4 {
                let mut tree = fixture(Length::Px(40.0), Length::Fill, axis);
                tree.get_mut(&id(1)).unwrap().spec.declared.animate =
                    Some(spec(Length::Px(200.0), Length::Fill, 2000.0, axis));
                tree.get_mut(&id(1))
                    .unwrap()
                    .spec
                    .declared
                    .animate
                    .as_mut()
                    .unwrap()
                    .repeat = AnimationRepeat::Loop;
                let mut d = Driver::new(tree, Instant::now(), mode);
                let before = d.step(0, vec![]).published_after.unwrap();
                let old = d.tree.length_runtime.as_ref().unwrap().tracks[&(id(1), axis)].clone();
                if failure < 2 {
                    let track = Arc::make_mut(
                        d.tree
                            .length_runtime
                            .as_mut()
                            .unwrap()
                            .tracks
                            .get_mut(&(id(1), axis))
                            .unwrap(),
                    );
                    if failure == 0 {
                        track.to.intrinsic += 20.0;
                    } else {
                        track.to.policy.fill_request *= 0.5;
                    }
                }
                let injection = match failure {
                    2 => Injection::Query(QueryKind::ForeignTarget),
                    3 => Injection::BeforeLayout,
                    _ => Injection::None,
                };
                let out = d.attempt(500_000, vec![], injection);
                let expected = match failure {
                    2 => AttemptError::Native(ProjectionError::InjectedQuery),
                    3 => AttemptError::BeforeLayout,
                    _ => AttemptError::Native(ProjectionError::ReleaseMismatch(id(1), axis)),
                };
                assert_eq!(out.result, Err(expected));
                out.published_after.unwrap().assert_same(&before);
                // Restore only test corruption, not a live model/viewport rollback.
                if failure < 2 {
                    d.tree
                        .length_runtime
                        .as_mut()
                        .unwrap()
                        .tracks
                        .insert((id(1), axis), old);
                }
                let out = d.step(750_000, vec![]);
                assert!(out.result.is_ok(), "{:?}", out.result);
                close(capture_axis(&d.tree, &id(2), axis).unwrap().visible, 141.25);
                assert!(d.step(1_000_000, vec![]).result.is_ok());
                assert!(d.step(2_000_000, vec![]).result.is_ok());
                assert!(d.tree.length_runtime.is_some());
                assert!(d.runtime.groups.is_empty());
            }
        }
    }
}

#[test]
fn finite_fill_parents_share_full_intrinsic_destinations_with_children() {
    for axis in [Axis::Width, Axis::Height] {
        for mode in [Mode::Full, Mode::Active, Mode::Dirty] {
            for child_duration in [1000.0, 2000.0] {
                let mut tree = fixture(Length::Px(40.0), Length::Fill, axis);
                tree.get_mut(&id(1)).unwrap().spec.declared.animate =
                    Some(spec(Length::Px(200.0), Length::Fill, 2000.0, axis));
                tree.get_mut(&id(2))
                    .unwrap()
                    .spec
                    .declared
                    .animate
                    .as_mut()
                    .unwrap()
                    .duration_ms = child_duration;
                let mut d = Driver::new(tree, Instant::now(), mode);
                for us in [0, 500_000, 1_000_000, 1_500_000, 2_000_000] {
                    let out = d.step(us, vec![]);
                    assert!(out.result.is_ok(), "{us}: {:?}", out.result);
                    let t = us as f32 / 2_000_000.0;
                    let parent = capture_axis(&d.tree, &id(1), axis).unwrap();
                    close(parent.visible, 200.0 + 400.0 * t);
                    close(parent.intrinsic, 200.0 * (1.0 - t));
                    close(
                        capture_axis(&d.tree, &id(2), axis).unwrap().visible,
                        40.0 + 260.0 * (us as f32 / (child_duration as f32 * 1000.0)).min(1.0),
                    );
                }
                assert!(d.runtime.groups.is_empty());
                assert!(d.tree.length_runtime.is_none());
            }
        }
    }
}

#[test]
fn intrinsic_group_links_cross_static_fill_wrappers_but_not_fixed_axis_boundaries() {
    for axis in [Axis::Width, Axis::Height] {
        for fixed in [false, true] {
            let mut tree = fixture(Length::Px(40.0), Length::Fill, axis);
            tree.get_mut(&id(1)).unwrap().spec.declared.animate =
                Some(spec(Length::Px(200.0), Length::Fill, 2000.0, axis));
            let mut attrs = Attrs {
                width: Some(Length::Fill),
                height: Some(Length::Fill),
                ..Default::default()
            };
            if fixed {
                if axis == Axis::Width {
                    attrs.width = Some(Length::Px(100.0));
                } else {
                    attrs.height = Some(Length::Px(100.0));
                }
            }
            tree.insert(Element::with_attrs(
                id(4),
                if axis == Axis::Width {
                    ElementKind::Row
                } else {
                    ElementKind::Column
                },
                vec![],
                attrs,
            ));
            tree.set_children(&id(1), vec![id(4)]).unwrap();
            tree.set_children(&id(4), vec![id(2), id(3)]).unwrap();
            let mut d = Driver::new(tree, Instant::now(), Mode::Dirty);
            for (us, t) in [(0, 0.0), (500_000, 0.5), (1_000_000, 1.0), (2_000_000, 1.0)] {
                let out = d.step(us, vec![]);
                assert!(out.result.is_ok(), "{us}: {:?}", out.result);
                close(
                    capture_axis(&d.tree, &id(2), axis).unwrap().visible,
                    40.0 + (if fixed { 10.0 } else { 260.0 }) * t,
                );
            }
            assert!(d.tree.length_runtime.is_none());
        }
    }
}

#[test]
fn shared_clock_evidence_checks_each_consumers_dependency_direction() {
    let mut tree = fixture(Length::Px(40.0), Length::Fill, Axis::Width);
    tree.insert(Element::with_attrs(
        id(4),
        ElementKind::El,
        vec![],
        Attrs::default(),
    ));
    tree.set_children(&id(2), vec![id(4)]).unwrap();
    let mut d = Driver::new(tree, Instant::now(), Mode::Full);
    assert!(d.step(0, vec![]).result.is_ok());
    let track = Arc::clone(&d.tree.length_runtime.as_ref().unwrap().tracks[&(id(2), Axis::Width)]);
    for mixed in [false, true] {
        for independent in [false, true] {
            let proof = Arc::new(ClockContinuation {
                own_axis: None,
                independent: independent.then(|| (vec![id(3)], Arc::clone(&track.projection))),
                projection: Arc::clone(&track.projection),
                pixels: vec![],
                next: None,
                foreign: if mixed {
                    vec![((id(2), Axis::Width), Arc::clone(&track.input))]
                } else {
                    vec![]
                },
            });
            for owners in [[1, 2, 3, 4], [4, 3, 2, 1], [3, 1, 4, 2]] {
                for owner in owners {
                    // A mixed-clock cache hit from child 4 cannot authorize upward
                    // feedback in owner 1, nor can rejection suppress child 4's proof.
                    let cached = Arc::clone(&proof);
                    assert_eq!(
                        cached.permits_owner(&d.tree, (id(owner), Axis::Width)),
                        (!mixed || owner == 4) && (!independent || [2, 4].contains(&owner))
                    );
                }
            }
        }
    }
}

fn pixel_wrapper_tree(axis: Axis) -> ElementTree {
    let mut tree = fixture(Length::Px(40.0), Length::Fill, axis);
    tree.get_mut(&id(1)).unwrap().spec.declared.animate =
        Some(spec(Length::Px(200.0), Length::Fill, 1000.0, axis));
    tree.get_mut(&id(2))
        .unwrap()
        .spec
        .declared
        .animate
        .as_mut()
        .unwrap()
        .duration_ms = 2000.0;
    let mut motion = spec(Length::Px(200.0), Length::Px(400.0), 4000.0, axis);
    motion.repeat = AnimationRepeat::Loop;
    tree.insert(Element::with_attrs(
        id(4),
        if axis == Axis::Width {
            ElementKind::Row
        } else {
            ElementKind::Column
        },
        vec![],
        Attrs {
            width: Some(Length::Fill),
            height: Some(Length::Fill),
            animate: Some(motion),
            ..Default::default()
        },
    ));
    tree.set_children(&id(1), vec![id(4)]).unwrap();
    tree.set_children(&id(4), vec![id(2), id(3)]).unwrap();
    tree
}

#[test]
fn looping_pixel_wrappers_drive_both_ancestor_and_descendant_group_members() {
    for axis in [Axis::Width, Axis::Height] {
        for mode in [Mode::Full, Mode::Active, Mode::Dirty] {
            let mut d = Driver::new(pixel_wrapper_tree(axis), Instant::now(), mode);
            for us in [0, 500_000, 1_000_000, 1_500_000, 2_000_000] {
                let out = d.step(us, vec![]);
                assert!(
                    out.result.is_ok(),
                    "{axis:?} {mode:?} {us}: {:?}",
                    out.result
                );
                let t = us as f32 / 1_000_000.0;
                close(
                    capture_axis(&d.tree, &id(1), axis).unwrap().visible,
                    200.0 + 400.0 * t.min(1.0),
                );
                close(
                    capture_axis(&d.tree, &id(4), axis).unwrap().visible,
                    200.0 + 50.0 * t,
                );
                close(
                    capture_axis(&d.tree, &id(2), axis).unwrap().visible,
                    40.0 + (100.0 + 25.0 * t - 40.0) * t / 2.0,
                );
                assert!(out.inspection.queries.len() <= 3);
            }
            close(
                capture_axis(&d.tree, &id(1), axis).unwrap().intrinsic,
                300.0,
            );
            assert!(d.runtime.groups.is_empty());
            assert!(d.tree.length_runtime.is_none());
        }
    }
}

#[test]
fn pixel_wrapper_release_proofs_keep_intrinsic_corruption_checks_and_retry_clocks() {
    for axis in [Axis::Width, Axis::Height] {
        for mode in [Mode::Full, Mode::Active, Mode::Dirty] {
            for failure in 0..3 {
                let mut d = Driver::new(pixel_wrapper_tree(axis), Instant::now(), mode);
                for us in [0, 500_000, 1_000_000, 1_500_000] {
                    assert!(d.step(us, vec![]).result.is_ok());
                }
                let good =
                    Arc::clone(&d.tree.length_runtime.as_ref().unwrap().tracks[&(id(1), axis)]);
                if failure == 0 {
                    let mut bad = (*good).clone();
                    bad.to.intrinsic -= 10.0;
                    d.tree
                        .length_runtime
                        .as_mut()
                        .unwrap()
                        .tracks
                        .insert((id(1), axis), Arc::new(bad));
                }
                let failed = d.attempt(
                    2_000_000,
                    vec![],
                    match failure {
                        0 => Injection::None,
                        1 => Injection::Query(QueryKind::PriorTarget),
                        _ => Injection::BeforeLayout,
                    },
                );
                assert!(failed.result.is_err());
                failed
                    .published_after
                    .as_ref()
                    .unwrap()
                    .assert_same(failed.published_before.as_ref().unwrap());
                if failure == 0 {
                    assert!(
                        matches!(failed.result,Err(AttemptError::Native(ProjectionError::ReleaseMismatch(node,a))) if node==id(1)&&a==axis)
                    );
                    d.tree
                        .length_runtime
                        .as_mut()
                        .unwrap()
                        .tracks
                        .insert((id(1), axis), good);
                }
                let retry = d.step(2_100_000, vec![]);
                assert!(retry.result.is_ok(), "{:?}", retry.result);
                close(
                    capture_axis(&d.tree, &id(1), axis).unwrap().intrinsic,
                    305.0,
                );
                close(capture_axis(&d.tree, &id(2), axis).unwrap().visible, 152.5);
                assert!(d.runtime.groups.is_empty());
                assert!(d.tree.length_runtime.is_none());
            }
        }
    }
}

fn pixel_boundary_parent(axis: Axis, segment: bool) -> ElementTree {
    let mut tree = fixture(Length::Px(40.0), Length::Fill, axis);
    tree.get_mut(&id(2))
        .unwrap()
        .spec
        .declared
        .animate
        .as_mut()
        .unwrap()
        .duration_ms = 2000.0;
    let mut parent = spec(Length::Px(600.0), Length::Px(800.0), 2000.0, axis);
    if segment {
        parent.keyframes.push(parent.keyframes[0].clone());
        parent.duration_ms = 4000.0;
    }
    parent.repeat = AnimationRepeat::Loop;
    tree.get_mut(&id(1)).unwrap().spec.declared.animate = Some(parent);
    tree
}
#[test]
fn finite_release_replays_pixel_clock_segment_and_cycle_boundaries_without_rewinding_them() {
    for axis in [Axis::Width, Axis::Height] {
        for mode in [Mode::Full, Mode::Active, Mode::Dirty] {
            for segment in [false, true] {
                for skipped in [false, true] {
                    for scale in [0.5, 1.0, 2.0] {
                        for repeat in [AnimationRepeat::Loop, AnimationRepeat::Times(2000)] {
                            let mut tree = pixel_boundary_parent(axis, segment);
                            tree.get_mut(&id(1))
                                .unwrap()
                                .spec
                                .declared
                                .animate
                                .as_mut()
                                .unwrap()
                                .repeat = repeat;
                            let mut d = Driver::new(tree, Instant::now(), mode);
                            assert!(
                                d.step(
                                    0,
                                    vec![Event::Viewport {
                                        width: 600.0 * scale,
                                        height: 600.0 * scale,
                                        scale
                                    }]
                                )
                                .result
                                .is_ok()
                            );
                            for us in [500_000, 1_000_000, 1_500_000] {
                                assert!(d.step(us, vec![]).result.is_ok());
                            }
                            let (us, parent) = if skipped {
                                (2_001_750_000, 775.0)
                            } else {
                                (2_000_000, if segment { 800.0 } else { 600.0 })
                            };
                            let out = d.step(us, vec![]);
                            assert!(
                                out.result.is_ok(),
                                "{axis:?} segment={segment} {us}: {:?}",
                                out.result
                            );
                            close(capture_axis(&d.tree, &id(1), axis).unwrap().visible, parent);
                            close(
                                capture_axis(&d.tree, &id(2), axis).unwrap().visible,
                                parent / 2.0,
                            );
                            assert!(out.inspection.queries.len() <= 2);
                            assert!(d.tree.length_runtime.is_none());
                            assert!(d.runtime.groups.is_empty());
                        }
                    }
                }
            }
        }
    }
}

#[test]
fn pixel_boundary_release_keeps_native_corruption_checks_and_failed_output_retry_clocks() {
    for axis in [Axis::Width, Axis::Height] {
        for mode in [Mode::Full, Mode::Active, Mode::Dirty] {
            for failure in 0..3 {
                let mut d = Driver::new(pixel_boundary_parent(axis, false), Instant::now(), mode);
                for us in [0, 500_000, 1_000_000, 1_500_000] {
                    assert!(d.step(us, vec![]).result.is_ok());
                }
                let good =
                    Arc::clone(&d.tree.length_runtime.as_ref().unwrap().tracks[&(id(2), axis)]);
                if failure == 0 {
                    let mut bad = (*good).clone();
                    bad.to.charge -= 10.0;
                    d.tree
                        .length_runtime
                        .as_mut()
                        .unwrap()
                        .tracks
                        .insert((id(2), axis), Arc::new(bad));
                }
                let failed = d.attempt(
                    2_000_000,
                    vec![],
                    match failure {
                        0 => Injection::None,
                        1 => Injection::Query(QueryKind::PriorTarget),
                        _ => Injection::BeforeLayout,
                    },
                );
                assert!(failed.result.is_err());
                failed
                    .published_after
                    .as_ref()
                    .unwrap()
                    .assert_same(failed.published_before.as_ref().unwrap());
                if failure == 0 {
                    assert!(
                        matches!(failed.result,Err(AttemptError::Native(ProjectionError::ReleaseMismatch(node,a))) if node==id(2)&&a==axis)
                    );
                    d.tree
                        .length_runtime
                        .as_mut()
                        .unwrap()
                        .tracks
                        .insert((id(2), axis), good);
                }
                let retry = d.step(2_100_000, vec![]);
                assert!(retry.result.is_ok(), "{:?}", retry.result);
                close(capture_axis(&d.tree, &id(1), axis).unwrap().visible, 610.0);
                close(capture_axis(&d.tree, &id(2), axis).unwrap().visible, 305.0);
                assert!(d.runtime.groups.is_empty());
                assert!(d.tree.length_runtime.is_none());
            }
        }
    }
}
#[test]
fn pixel_cycle_release_exception_does_not_preserve_a_moving_anchor_across_a_reset() {
    for axis in [Axis::Width, Axis::Height] {
        for mode in [Mode::Full, Mode::Active, Mode::Dirty] {
            let mut tree = pixel_boundary_parent(axis, false);
            tree.get_mut(&id(2))
                .unwrap()
                .spec
                .declared
                .animate
                .as_mut()
                .unwrap()
                .duration_ms = 4000.0;
            let mut d = Driver::new(tree, Instant::now(), mode);
            for us in [0, 500_000, 1_000_000, 1_500_000, 2_000_000] {
                assert!(d.step(us, vec![]).result.is_ok());
            }
            close(capture_axis(&d.tree, &id(2), axis).unwrap().visible, 207.5);
            assert!(d.step(2_500_000, vec![]).result.is_ok());
            close(
                capture_axis(&d.tree, &id(2), axis).unwrap().visible,
                236.875,
            );
            assert!(d.step(4_000_000, vec![]).result.is_ok());
            close(capture_axis(&d.tree, &id(2), axis).unwrap().visible, 300.0);
        }
    }
}

fn mixed_boundary_parent(axis: Axis, reverse: bool, segment: bool) -> ElementTree {
    let mut tree = fixture(Length::Px(40.0), Length::Fill, axis);
    tree.get_mut(&id(2))
        .unwrap()
        .spec
        .declared
        .animate
        .as_mut()
        .unwrap()
        .duration_ms = 2000.0;
    let (from, to) = if reverse {
        (Length::Fill, Length::Px(200.0))
    } else {
        (Length::Px(200.0), Length::Fill)
    };
    let mut parent = spec(from, to, 2000.0, axis);
    if segment {
        parent.keyframes.push(parent.keyframes[0].clone());
        parent.duration_ms = 4000.0;
    }
    parent.repeat = AnimationRepeat::Loop;
    tree.get_mut(&id(1)).unwrap().spec.declared.animate = Some(parent);
    tree
}

#[test]
fn finite_release_follows_mixed_parent_segment_and_repeat_boundaries() {
    for axis in [Axis::Width, Axis::Height] {
        for mode in [Mode::Full, Mode::Active, Mode::Dirty] {
            for reverse in [false, true] {
                for segment in [false, true] {
                    for scale in [0.5, 1.0, 2.0] {
                        for curve in [
                            AnimationCurve::Linear,
                            AnimationCurve::EaseIn,
                            AnimationCurve::EaseOut,
                            AnimationCurve::EaseInOut,
                        ] {
                            for deadline in [2_000_000, 2_100_000, 2_001_100_000] {
                                let mut tree = mixed_boundary_parent(axis, reverse, segment);
                                for node in [1, 2] {
                                    tree.get_mut(&id(node))
                                        .unwrap()
                                        .spec
                                        .declared
                                        .animate
                                        .as_mut()
                                        .unwrap()
                                        .curve = curve.clone();
                                }
                                let mut d = Driver::new(tree, Instant::now(), mode);
                                assert!(
                                    d.step(
                                        0,
                                        vec![Event::Viewport {
                                            width: 600.0 * scale,
                                            height: 600.0 * scale,
                                            scale
                                        }]
                                    )
                                    .result
                                    .is_ok()
                                );
                                for us in [500_000, 1_750_000, deadline] {
                                    let out = d.step(us, vec![]);
                                    assert!(
                                        out.result.is_ok(),
                                        "{axis:?} {mode:?} reverse={reverse} segment={segment} {us}: {:?}",
                                        out.result
                                    );
                                    assert!(
                                        out.inspection.queries.len() <= 9,
                                        "{}",
                                        out.inspection.queries.len()
                                    );
                                    assert!(out.inspection.after.model_copies <= 1);
                                }
                                let time = deadline % if segment { 4_000_000 } else { 2_000_000 };
                                let phase =
                                    apply_curve(&curve, (time % 2_000_000) as f64 / 2_000_000.0)
                                        as f32;
                                let descending = reverse ^ (time >= 2_000_000);
                                let parent = if descending {
                                    600.0 - 400.0 * phase
                                } else {
                                    200.0 + 400.0 * phase
                                };
                                close(capture_axis(&d.tree, &id(1), axis).unwrap().visible, parent);
                                close(
                                    capture_axis(&d.tree, &id(2), axis).unwrap().visible,
                                    parent / 2.0,
                                );
                                assert!(d.runtime.groups.is_empty());
                                assert!(
                                    !d.tree
                                        .length_runtime
                                        .as_ref()
                                        .unwrap()
                                        .tracks
                                        .contains_key(&(id(2), axis))
                                );
                            }
                        }
                    }
                }
            }
        }
    }
}

#[test]
fn mixed_boundary_proofs_preserve_corruption_checks_and_failed_attempts() {
    for axis in [Axis::Width, Axis::Height] {
        for mode in [Mode::Full, Mode::Active, Mode::Dirty] {
            for segment in [false, true] {
                for failure in 0..8 {
                    let mut d = Driver::new(
                        mixed_boundary_parent(axis, false, segment),
                        Instant::now(),
                        mode,
                    );
                    assert!(d.step(0, vec![]).result.is_ok());
                    let before = d.step(1_750_000, vec![]).published_after.unwrap();
                    let old =
                        d.tree.length_runtime.as_ref().unwrap().tracks[&(id(1), axis)].clone();
                    if failure < 2 {
                        let track = Arc::make_mut(
                            d.tree
                                .length_runtime
                                .as_mut()
                                .unwrap()
                                .tracks
                                .get_mut(&(id(1), axis))
                                .unwrap(),
                        );
                        if failure == 0 {
                            track.to.intrinsic += 20.0;
                        } else {
                            track.to.policy.fill_request *= 0.5;
                        }
                    }
                    let injection = match failure {
                        0 | 1 => Injection::None,
                        2 => Injection::Query(QueryKind::Target),
                        3 => Injection::Query(QueryKind::Release),
                        4 => Injection::Query(QueryKind::PriorTarget),
                        5 => Injection::Query(QueryKind::ForeignTarget),
                        6 if !segment => Injection::Query(QueryKind::Source),
                        _ => Injection::BeforeLayout,
                    };
                    let out = d.attempt(2_000_000, vec![], injection);
                    assert!(
                        out.result.is_err(),
                        "{axis:?} {mode:?} segment={segment} failure={failure}"
                    );
                    if failure < 2 {
                        assert_eq!(
                            out.result,
                            Err(AttemptError::Native(ProjectionError::ReleaseMismatch(
                                id(1),
                                axis
                            )))
                        );
                    }
                    out.published_after.unwrap().assert_same(&before);
                    if failure < 2 {
                        d.tree
                            .length_runtime
                            .as_mut()
                            .unwrap()
                            .tracks
                            .insert((id(1), axis), old);
                    }
                    let out = d.step(2_100_000, vec![]);
                    assert!(out.result.is_ok(), "{failure}: {:?}", out.result);
                    let parent = if segment { 580.0 } else { 220.0 };
                    close(capture_axis(&d.tree, &id(1), axis).unwrap().visible, parent);
                    close(
                        capture_axis(&d.tree, &id(2), axis).unwrap().visible,
                        parent / 2.0,
                    );
                    assert!(d.runtime.groups.is_empty());
                }
            }
        }
    }
}

#[test]
fn content_boundary_release_matches_native_full_footprints_not_pixel_lowering() {
    for axis in [Axis::Width, Axis::Height] {
        for mode in [Mode::Full, Mode::Active, Mode::Dirty] {
            for reverse in [false, true] {
                let mut tree = mixed_boundary_parent(axis, reverse, false);
                let parent = tree
                    .get_mut(&id(1))
                    .unwrap()
                    .spec
                    .declared
                    .animate
                    .as_mut()
                    .unwrap();
                let index = if reverse { 1 } else { 0 };
                match axis {
                    Axis::Width => parent.keyframes[index].width = Some(Length::Content),
                    Axis::Height => parent.keyframes[index].height = Some(Length::Content),
                };
                let mut d = Driver::new(tree, Instant::now(), mode);
                for us in [0, 500_000, 1_750_000] {
                    let out = d.step(us, vec![]);
                    assert!(out.result.is_ok(), "{us}: {:?}", out.result);
                }
                let out = d.step(2_100_000, vec![]);
                assert!(
                    out.result.is_ok(),
                    "{axis:?} reverse={reverse}: {:?}",
                    out.result
                );
                let native = &out
                    .inspection
                    .queries
                    .iter()
                    .find(|q| q.kind == QueryKind::Release)
                    .unwrap()
                    .result
                    .as_ref()
                    .unwrap()[&(id(2), axis)];
                assert!(
                    capture_axis(&d.tree, &id(2), axis)
                        .unwrap()
                        .release_matches(*native)
                );
                assert!(d.runtime.groups.is_empty());
            }
        }
    }
}

#[test]
fn retained_pixel_intervals_use_full_boundary_witnesses_at_finite_release() {
    for axis in [Axis::Width, Axis::Height] {
        for mode in [Mode::Full, Mode::Active, Mode::Dirty] {
            let mut tree = mixed_boundary_parent(axis, true, false);
            tree.get_mut(&id(2))
                .unwrap()
                .spec
                .declared
                .animate
                .as_mut()
                .unwrap()
                .duration_ms = 4000.0;
            let parent = tree
                .get_mut(&id(1))
                .unwrap()
                .spec
                .declared
                .animate
                .as_mut()
                .unwrap();
            parent.duration_ms = 6000.0;
            parent.keyframes.extend([400.0, 600.0].map(|pixels| {
                let mut attrs = Attrs::default();
                match axis {
                    Axis::Width => attrs.width = Some(Length::Px(pixels)),
                    Axis::Height => attrs.height = Some(Length::Px(pixels)),
                };
                attrs
            }));
            let mut d = Driver::new(tree, Instant::now(), mode);
            for us in [0, 1_000_000, 2_500_000, 3_750_000, 4_100_000] {
                let out = d.step(us, vec![]);
                assert!(out.result.is_ok(), "{us}: {:?}", out.result);
            }
            close(capture_axis(&d.tree, &id(1), axis).unwrap().visible, 410.0);
            close(capture_axis(&d.tree, &id(2), axis).unwrap().visible, 205.0);
            assert!(d.runtime.groups.is_empty());
        }
    }
}

#[test]
fn mixed_boundary_release_does_not_preserve_nonrelease_anchors_across_resets() {
    for axis in [Axis::Width, Axis::Height] {
        for mode in [Mode::Full, Mode::Active, Mode::Dirty] {
            let mut tree = mixed_boundary_parent(axis, false, false);
            tree.get_mut(&id(2))
                .unwrap()
                .spec
                .declared
                .animate
                .as_mut()
                .unwrap()
                .duration_ms = 4000.0;
            let mut d = Driver::new(tree, Instant::now(), mode);
            for us in [0, 1_750_000] {
                assert!(d.step(us, vec![]).result.is_ok());
            }
            for (us, expected) in [(2_000_000, 157.5), (2_500_000, 155.625), (4_000_000, 100.0)] {
                let out = d.step(us, vec![]);
                assert!(out.result.is_ok(), "{us}: {:?}", out.result);
                close(
                    capture_axis(&d.tree, &id(2), axis).unwrap().visible,
                    expected,
                );
            }
        }
    }
}

#[test]
fn weighted_foreign_boundary_releases_preserve_parent_pool_charges() {
    for axis in [Axis::Width, Axis::Height] {
        for mode in [Mode::Full, Mode::Active, Mode::Dirty] {
            for scale in [0.5, 1.0, 2.0] {
                let mut tree = mixed_boundary_parent(axis, false, false);
                let mut parent = spec(
                    Length::FillWeighted(1.0),
                    Length::FillWeighted(3.0),
                    2000.0,
                    axis,
                );
                parent.repeat = AnimationRepeat::Loop;
                tree.get_mut(&id(1)).unwrap().spec.declared.animate = Some(parent);
                tree.insert(Element::with_attrs(
                    id(10),
                    if axis == Axis::Width {
                        ElementKind::Row
                    } else {
                        ElementKind::Column
                    },
                    vec![],
                    Attrs {
                        width: Some(Length::Px(600.0)),
                        height: Some(Length::Px(600.0)),
                        ..Default::default()
                    },
                ));
                tree.insert(Element::with_attrs(
                    id(11),
                    ElementKind::El,
                    vec![],
                    Attrs {
                        width: Some(Length::Fill),
                        height: Some(Length::Fill),
                        ..Default::default()
                    },
                ));
                tree.set_children(&id(10), vec![id(1), id(11)]).unwrap();
                tree.set_root_id(id(10));
                let mut d = Driver::new(tree, Instant::now(), mode);
                assert!(
                    d.step(
                        0,
                        vec![Event::Viewport {
                            width: 600.0 * scale,
                            height: 600.0 * scale,
                            scale
                        }]
                    )
                    .result
                    .is_ok()
                );
                for us in [500_000, 1_750_000, 2_100_000] {
                    let out = d.step(us, vec![]);
                    assert!(out.result.is_ok(), "{axis:?} {us}: {:?}", out.result);
                }
                let parent = capture_axis(&d.tree, &id(1), axis).unwrap();
                close(parent.visible, 307.5);
                close(parent.charge, 307.5);
                close(capture_axis(&d.tree, &id(11), axis).unwrap().visible, 292.5);
                close(capture_axis(&d.tree, &id(2), axis).unwrap().visible, 153.75);
                assert!(d.runtime.groups.is_empty());
            }
        }
    }
}

#[test]
fn mixed_boundary_resolves_both_foreign_axes_before_joint_release() {
    for mode in [Mode::Full, Mode::Active, Mode::Dirty] {
        let mut tree = mixed_boundary_parent(Axis::Width, false, false);
        for node in [1, 2] {
            let spec = tree
                .get_mut(&id(node))
                .unwrap()
                .spec
                .declared
                .animate
                .as_mut()
                .unwrap();
            for attrs in &mut spec.keyframes {
                attrs.height = attrs.width.clone();
            }
        }
        let mut d = Driver::new(tree, Instant::now(), mode);
        for us in [0, 500_000, 1_750_000, 2_100_000] {
            let out = d.step(us, vec![]);
            assert!(out.result.is_ok(), "{us}: {:?}", out.result);
            assert!(
                out.inspection.queries.len() <= 12,
                "{}",
                out.inspection.queries.len()
            );
        }
        close(
            capture_axis(&d.tree, &id(1), Axis::Width).unwrap().visible,
            220.0,
        );
        close(
            capture_axis(&d.tree, &id(1), Axis::Height).unwrap().visible,
            220.0,
        );
        close(
            capture_axis(&d.tree, &id(2), Axis::Width).unwrap().visible,
            110.0,
        );
        close(
            capture_axis(&d.tree, &id(2), Axis::Height).unwrap().visible,
            220.0,
        );
        assert!(d.runtime.groups.is_empty());
    }
}

fn independent_mixed_panels(axis: Axis) -> ElementTree {
    let mut tree = fixture(Length::Px(40.0), Length::Fill, axis);
    let mut parent = spec(Length::Px(200.0), Length::Fill, 2000.0, axis);
    parent.repeat = AnimationRepeat::Loop;
    tree.get_mut(&id(1)).unwrap().spec.declared.animate = Some(parent);
    let mut other = Attrs {
        width: Some(Length::Px(600.0)),
        height: Some(Length::Px(600.0)),
        ..Default::default()
    };
    let mut animation = spec(Length::Px(100.0), Length::Fill, 3000.0, axis);
    animation.repeat = AnimationRepeat::Loop;
    other.animate = Some(animation);
    tree.insert(Element::with_attrs(id(5), ElementKind::El, vec![], other));
    tree.insert(Element::with_attrs(
        id(10),
        if axis == Axis::Width {
            ElementKind::Column
        } else {
            ElementKind::Row
        },
        vec![],
        Attrs {
            width: Some(Length::Px(600.0)),
            height: Some(Length::Px(600.0)),
            ..Default::default()
        },
    ));
    tree.set_children(&id(10), vec![id(1), id(5)]).unwrap();
    tree.set_root_id(id(10));
    tree
}

#[test]
fn unrelated_mixed_panels_do_not_restart_or_block_a_finite_child() {
    for axis in [Axis::Width, Axis::Height] {
        for mode in [Mode::Full, Mode::Active, Mode::Dirty] {
            let mut d = Driver::new(independent_mixed_panels(axis), Instant::now(), mode);
            for (us, expected) in [(0, 40.0), (500_000, 95.0), (1_000_000, 200.0)] {
                let out = d.step(us, vec![]);
                assert!(
                    out.result.is_ok(),
                    "{axis:?} {mode:?} {us}: {:?}",
                    out.result
                );
                close(
                    capture_axis(&d.tree, &id(2), axis).unwrap().visible,
                    expected,
                );
            }
            assert!(d.runtime.groups.is_empty());
        }
    }
}

fn mixed_peer_pool(axis: Axis) -> ElementTree {
    let mut tree = fixture(Length::Px(40.0), Length::Fill, axis);
    let mut peer = spec(Length::Px(200.0), Length::Fill, 2000.0, axis);
    peer.repeat = AnimationRepeat::Loop;
    tree.get_mut(&id(3)).unwrap().spec.declared.animate = Some(peer);
    tree
}

#[test]
fn same_pool_mixed_changes_are_not_mistaken_for_independence() {
    for axis in [Axis::Width, Axis::Height] {
        for mode in [Mode::Full, Mode::Active, Mode::Dirty] {
            let mut d = Driver::new(mixed_peer_pool(axis), Instant::now(), mode);
            assert!(d.step(0, vec![]).result.is_ok());
            let out = d.step(500_000, vec![]);
            assert!(out.result.is_ok(), "{:?}", out.result);
            close(capture_axis(&d.tree, &id(2), axis).unwrap().visible, 175.0);
            let target = out
                .inspection
                .queries
                .iter()
                .find(|q| {
                    q.kind == QueryKind::IndependentContext
                        && q.result
                            .as_ref()
                            .is_ok_and(|v| v.contains_key(&(id(2), axis)))
                })
                .unwrap()
                .result
                .as_ref()
                .unwrap()[&(id(2), axis)];
            close(target.visible, 310.0);
            // Corrupt the original cached target to exactly match that new native
            // target. The original projection must still reject this, rather
            // than laundering it through the independence comparison.
            let mut d = Driver::new(mixed_peer_pool(axis), Instant::now(), mode);
            let before = d.step(0, vec![]).published_after.unwrap();
            Arc::make_mut(
                d.tree
                    .length_runtime
                    .as_mut()
                    .unwrap()
                    .tracks
                    .get_mut(&(id(2), axis))
                    .unwrap(),
            )
            .to = target;
            let out = d.step(500_000, vec![]);
            assert_eq!(
                out.result,
                Err(AttemptError::Native(ProjectionError::ReleaseMismatch(
                    id(2),
                    axis
                )))
            );
            out.published_after.unwrap().assert_same(&before);
        }
    }
}

#[test]
fn independence_witness_failures_preserve_sources_and_retry_clocks() {
    for axis in [Axis::Width, Axis::Height] {
        for mode in [Mode::Full, Mode::Active, Mode::Dirty] {
            for failure in 0..5 {
                let mut d = Driver::new(independent_mixed_panels(axis), Instant::now(), mode);
                let before = d.step(0, vec![]).published_after.unwrap();
                let old = d.tree.length_runtime.as_ref().unwrap().tracks[&(id(1), axis)].clone();
                if failure == 0 {
                    Arc::make_mut(
                        d.tree
                            .length_runtime
                            .as_mut()
                            .unwrap()
                            .tracks
                            .get_mut(&(id(1), axis))
                            .unwrap(),
                    )
                    .to
                    .policy
                    .fill_request *= 0.5;
                }
                let injection = match failure {
                    0 => Injection::None,
                    1 => Injection::Query(QueryKind::PriorTarget),
                    2 => Injection::Query(QueryKind::IndependentContext),
                    3 => Injection::Query(QueryKind::ForeignTarget),
                    _ => Injection::BeforeLayout,
                };
                let out = d.attempt(500_000, vec![], injection);
                assert!(out.result.is_err());
                out.published_after.unwrap().assert_same(&before);
                if failure == 0 {
                    d.tree
                        .length_runtime
                        .as_mut()
                        .unwrap()
                        .tracks
                        .insert((id(1), axis), old);
                }
                let out = d.step(750_000, vec![]);
                assert!(out.result.is_ok(), "{:?}", out.result);
                close(capture_axis(&d.tree, &id(2), axis).unwrap().visible, 141.25);
                assert!(d.step(1_000_000, vec![]).result.is_ok());
                assert!(d.runtime.groups.is_empty());
            }
        }
    }
}

#[test]
fn independent_boundary_panels_follow_curves_at_each_scale_and_query_order() {
    for axis in [Axis::Width, Axis::Height] {
        for mode in [Mode::Full, Mode::Active, Mode::Dirty] {
            for scale in [0.5, 1.0, 2.0] {
                for curve in [
                    AnimationCurve::Linear,
                    AnimationCurve::EaseIn,
                    AnimationCurve::EaseOut,
                    AnimationCurve::EaseInOut,
                ] {
                    for reverse_order in [false, true] {
                        let mut tree = independent_mixed_panels(axis);
                        tree.get_mut(&id(2))
                            .unwrap()
                            .spec
                            .declared
                            .animate
                            .as_mut()
                            .unwrap()
                            .duration_ms = 2000.0;
                        for node in [1, 2, 5] {
                            tree.get_mut(&id(node))
                                .unwrap()
                                .spec
                                .declared
                                .animate
                                .as_mut()
                                .unwrap()
                                .curve = curve.clone();
                        }
                        if reverse_order {
                            tree.set_children(&id(10), vec![id(5), id(1)]).unwrap();
                        }
                        let mut d = Driver::new(tree, Instant::now(), mode);
                        assert!(
                            d.step(
                                0,
                                vec![Event::Viewport {
                                    width: 600.0 * scale,
                                    height: 600.0 * scale,
                                    scale
                                }]
                            )
                            .result
                            .is_ok()
                        );
                        for us in [500_000, 1_750_000, 2_100_000] {
                            let out = d.step(us, vec![]);
                            assert!(
                                out.result.is_ok(),
                                "{axis:?} {mode:?} {curve:?} {us}: {:?}",
                                out.result
                            );
                            assert!(out.inspection.after.model_copies <= 1);
                            let t = us as f64 / 1_000_000.0;
                            let parent = 200.0 + 400.0 * apply_curve(&curve, (t % 2.0) / 2.0);
                            let expected = if t < 2.0 {
                                40.0 + (parent / 2.0 - 40.0) * apply_curve(&curve, t / 2.0)
                            } else {
                                parent / 2.0
                            };
                            close(
                                capture_axis(&d.tree, &id(2), axis).unwrap().visible,
                                expected as f32,
                            );
                        }
                        assert!(d.runtime.groups.is_empty());
                    }
                }
            }
        }
    }
}

#[test]
fn release_checks_the_actual_continued_foreign_sample_before_publication() {
    for axis in [Axis::Width, Axis::Height] {
        for mode in [Mode::Full, Mode::Active, Mode::Dirty] {
            let mut tree = independent_mixed_panels(axis);
            tree.get_mut(&id(1)).unwrap().spec.declared.animate = None;
            let mut parent = spec(Length::Px(100.0), Length::Content, 2000.0, axis);
            parent.repeat = AnimationRepeat::Loop;
            tree.get_mut(&id(5)).unwrap().spec.declared.animate = Some(parent);
            let mut child = spec(Length::Px(200.0), Length::Px(400.0), 2000.0, axis);
            child.repeat = AnimationRepeat::Loop;
            tree.insert(Element::with_attrs(
                id(6),
                ElementKind::El,
                vec![],
                Attrs {
                    animate: Some(child),
                    ..Default::default()
                },
            ));
            tree.set_children(&id(5), vec![id(6)]).unwrap();
            let mut d = Driver::new(tree, Instant::now(), mode);
            assert!(d.step(0, vec![]).result.is_ok());
            let before = d.step(500_000, vec![]).published_after.unwrap();
            close(capture_axis(&d.tree, &id(5), axis).unwrap().visible, 137.5);
            let out = d.attempt(
                1_000_000,
                vec![],
                Injection::Query(QueryKind::CurrentRelease),
            );
            assert_eq!(
                out.result,
                Err(AttemptError::Native(ProjectionError::InjectedQuery))
            );
            out.published_after.unwrap().assert_same(&before);
            let out = d.step(1_100_000, vec![]);
            assert!(out.result.is_ok(), "{:?}", out.result);
            close(capture_axis(&d.tree, &id(5), axis).unwrap().visible, 215.5);
            close(capture_axis(&d.tree, &id(2), axis).unwrap().visible, 300.0);
            assert!(d.runtime.groups.is_empty());
        }
    }
}

#[test]
fn retired_query_work_includes_terminal_queries_without_retaining_a_workspace() {
    for mode in [Mode::Full, Mode::Active, Mode::Dirty] {
        let mut d = Driver::new(
            fixture(Length::Px(40.0), Length::Fill, Axis::Width),
            Instant::now(),
            mode,
        );
        assert!(d.step(0, vec![]).result.is_ok());
        assert!(d.step(500_000, vec![]).result.is_ok());
        let before = d.tree.length_runtime.as_ref().unwrap().query_stats();
        assert_eq!(d.tree.animation_query_retirement.count, 0);
        assert!(d.step(1_000_000, vec![]).result.is_ok());
        assert!(d.tree.length_runtime.is_none());
        let work = &d.tree.animation_query_retirement;
        assert_eq!(work.count, 1);
        assert_eq!(work.model_copies, 1);
        assert_eq!(work.copied_nodes, d.tree.len() as u64);
        assert!(work.layout_queries > before.layout_queries);
        assert_eq!(work.layout_queries, work.last.layout_queries);
        let queries = work.layout_queries;
        let mut attrs = d.attrs(id(2));
        attrs.animate = Some(spec(Length::Fill, Length::Px(40.0), 1000.0, Axis::Width));
        assert!(
            d.step(1_000_001, vec![Event::Attrs(id(2), Box::new(attrs))])
                .result
                .is_ok()
        );
        assert!(d.step(2_000_001, vec![]).result.is_ok());
        assert!(d.tree.length_runtime.is_none());
        let work = &d.tree.animation_query_retirement;
        assert_eq!(work.count, 2);
        assert_eq!(work.layout_queries, queries + work.last.layout_queries);
        assert_eq!(work.model_copies, 2);
    }
}

#[test]
fn independent_consumers_share_only_matching_native_input_cohorts() {
    for axis in [Axis::Width, Axis::Height] {
        for mode in [Mode::Full, Mode::Active, Mode::Dirty] {
            let mut tree = independent_mixed_panels(axis);
            let attrs = tree.get(&id(2)).unwrap().spec.declared.clone();
            for node in 20..83 {
                tree.insert(Element::with_attrs(
                    id(node),
                    ElementKind::El,
                    vec![],
                    attrs.clone(),
                ));
            }
            tree.set_children(
                &id(1),
                [id(2), id(3)].into_iter().chain((20..83).map(id)).collect(),
            )
            .unwrap();
            let mut d = Driver::new(tree, Instant::now(), mode);
            for (us, parent, p) in [
                (0, 200.0, 0.0),
                (500_000, 300.0, 0.5),
                (1_000_000, 400.0, 1.0),
            ] {
                let visits = d
                    .tree
                    .animation_query_retirement
                    .continuation_ancestry_visits
                    .get();
                let out = d.step(us, vec![]);
                assert!(out.result.is_ok(), "{:?}", out.result);
                assert!(
                    out.inspection.queries.len() <= 9,
                    "{us}: {}",
                    out.inspection.queries.len()
                );
                assert!(
                    d.tree
                        .animation_query_retirement
                        .continuation_ancestry_visits
                        .get()
                        - visits
                        <= 5_000
                );
                assert_eq!(out.inspection.after.model_copies, 1);
                for node in [2].into_iter().chain(20..83) {
                    close(
                        capture_axis(&d.tree, &id(node), axis).unwrap().visible,
                        40.0 + (parent / 65.0 - 40.0) * p,
                    );
                }
            }
            assert!(d.runtime.groups.is_empty());
        }
    }
}

fn coupled_tree(axis: Axis, upward: bool) -> ElementTree {
    if upward {
        let mut tree = fixture(Length::Px(40.0), Length::Fill, axis);
        tree.get_mut(&id(1)).unwrap().spec.declared.animate =
            Some(spec(Length::Px(200.0), Length::Content, 1000.0, axis));
        let child = tree
            .get_mut(&id(2))
            .unwrap()
            .spec
            .declared
            .animate
            .as_mut()
            .unwrap();
        child.repeat = AnimationRepeat::Loop;
        child.duration_ms = 2000.0;
        tree
    } else {
        mixed_peer_pool(axis)
    }
}

#[test]
fn coupled_mixed_deadlines_use_frozen_loop_presentations_and_native_release() {
    for axis in [Axis::Width, Axis::Height] {
        for upward in [false, true] {
            for mode in [Mode::Full, Mode::Active, Mode::Dirty] {
                for scale in [0.5, 1.0, 2.0] {
                    for curve in [
                        AnimationCurve::Linear,
                        AnimationCurve::EaseIn,
                        AnimationCurve::EaseOut,
                        AnimationCurve::EaseInOut,
                    ] {
                        for deadline in [1_000_000, 1_100_000, 2_000_000, 2_100_000, 2_001_100_000]
                        {
                            let mut tree = coupled_tree(axis, upward);
                            let owner = id(if upward { 1 } else { 2 });
                            let driver = id(if upward { 2 } else { 3 });
                            for node in [owner, driver] {
                                tree.get_mut(&node)
                                    .unwrap()
                                    .spec
                                    .declared
                                    .animate
                                    .as_mut()
                                    .unwrap()
                                    .curve = curve.clone();
                            }
                            if deadline >= 2_000_000 {
                                tree.get_mut(&owner)
                                    .unwrap()
                                    .spec
                                    .declared
                                    .animate
                                    .as_mut()
                                    .unwrap()
                                    .duration_ms = 2000.0;
                            }
                            let mut d = Driver::new(tree, Instant::now(), mode);
                            assert!(
                                d.step(
                                    0,
                                    vec![Event::Viewport {
                                        width: 600.0 * scale,
                                        height: 600.0 * scale,
                                        scale
                                    }]
                                )
                                .result
                                .is_ok()
                            );
                            assert!(d.step(500_000, vec![]).result.is_ok());
                            let clock = d.tree.length_runtime.as_ref().unwrap().tracks
                                [&(driver, axis)]
                                .key
                                .started;
                            if deadline >= 2_000_000 {
                                assert!(d.step(1_750_000, vec![]).result.is_ok());
                            }
                            let out = d.step(deadline, vec![]);
                            assert!(
                                out.result.is_ok(),
                                "{axis:?} {mode:?} up={upward} scale={scale} {curve:?} {deadline}: {:?}",
                                out.result
                            );
                            assert!(d.runtime.groups.is_empty());
                            assert_eq!(
                                d.tree.length_runtime.as_ref().unwrap().tracks[&(driver, axis)]
                                    .key
                                    .started,
                                clock
                            );
                            let target = out
                                .inspection
                                .queries
                                .iter()
                                .find(|q| q.kind == QueryKind::Release)
                                .unwrap()
                                .result
                                .as_ref()
                                .unwrap()[&(owner, axis)];
                            assert!(
                                target
                                    .release_matches(capture_axis(&d.tree, &owner, axis).unwrap())
                            );
                            assert!(
                                out.inspection.queries.len() <= 12,
                                "{}",
                                out.inspection.queries.len()
                            );
                            assert_eq!(out.inspection.after.model_copies, 1);
                            assert!(
                                d.tree
                                    .length_runtime
                                    .as_ref()
                                    .unwrap()
                                    .tracks
                                    .values()
                                    .all(|track| track.forecast.is_none())
                            );
                            if deadline == 1_000_000 && matches!(curve, AnimationCurve::Linear) {
                                close(target.visible, if upward { 140.0 / 3.0 } else { 280.0 });
                                close(
                                    capture_axis(&d.tree, &driver, axis).unwrap().visible,
                                    if upward { 140.0 / 3.0 } else { 320.0 },
                                );
                            }
                        }
                    }
                }
            }
        }
    }
}

#[test]
fn coupled_release_failures_and_corrupt_forecasts_do_not_publish_or_restart() {
    for axis in [Axis::Width, Axis::Height] {
        for upward in [false, true] {
            for mode in [Mode::Full, Mode::Active, Mode::Dirty] {
                for boundary in [false, true] {
                    for fault in 0..8 {
                        if fault == 2 && !upward || fault == 6 && !boundary {
                            continue;
                        }
                        let owner = id(if upward { 1 } else { 2 });
                        let driver = id(if upward { 2 } else { 3 });
                        let mut tree = coupled_tree(axis, upward);
                        if boundary {
                            tree.get_mut(&owner)
                                .unwrap()
                                .spec
                                .declared
                                .animate
                                .as_mut()
                                .unwrap()
                                .duration_ms = 2000.0;
                        }
                        let mut d = Driver::new(tree.clone(), Instant::now(), mode);
                        let mut oracle = Driver::new(tree, Instant::now(), mode);
                        assert!(d.step(0, vec![]).result.is_ok());
                        assert!(oracle.step(0, vec![]).result.is_ok());
                        let before = d.step(500_000, vec![]).published_after.unwrap();
                        assert!(oracle.step(500_000, vec![]).result.is_ok());
                        let saved = d.tree.length_runtime.as_ref().unwrap().tracks.clone();
                        if fault == 0 {
                            Arc::make_mut(
                                d.tree
                                    .length_runtime
                                    .as_mut()
                                    .unwrap()
                                    .tracks
                                    .get_mut(&(owner, axis))
                                    .unwrap(),
                            )
                            .to
                            .visible += 7.0;
                        }
                        if fault == 1 {
                            Arc::make_mut(
                                d.tree
                                    .length_runtime
                                    .as_mut()
                                    .unwrap()
                                    .tracks
                                    .get_mut(&(driver, axis))
                                    .unwrap(),
                            )
                            .to
                            .policy
                            .fill_request *= 0.5;
                        }
                        if fault == 2 {
                            let owner = Arc::make_mut(
                                d.tree
                                    .length_runtime
                                    .as_mut()
                                    .unwrap()
                                    .tracks
                                    .get_mut(&(owner, axis))
                                    .unwrap(),
                            );
                            let input = Arc::make_mut(
                                Arc::make_mut(owner.forecast.as_mut().unwrap())
                                    .get_mut(&(driver, axis))
                                    .unwrap(),
                            );
                            // Preserve the .5s forecast exactly while corrupting its
                            // native destination. A sample-only witness must not pass.
                            input.to.visible += 12.0;
                            input.from.visible -= 4.0;
                        }
                        let failure = match fault {
                            0..=2 => Injection::None,
                            3 => Injection::Query(QueryKind::PriorTarget),
                            4 => Injection::Query(QueryKind::ForeignTarget),
                            5 => Injection::Query(QueryKind::Release),
                            6 => Injection::Query(QueryKind::Source),
                            _ => Injection::BeforeLayout,
                        };
                        let deadline = if boundary { 2_000_000 } else { 1_000_000 };
                        let out = d.attempt(deadline, vec![], failure);
                        assert!(
                            out.result.is_err(),
                            "{axis:?} up={upward} boundary={boundary} fault={fault}"
                        );
                        out.published_after.unwrap().assert_same(&before);
                        if fault <= 2 {
                            d.tree.length_runtime.as_mut().unwrap().tracks = saved;
                        }
                        let out = d.step(deadline + 100_000, vec![]);
                        assert!(out.result.is_ok(), "{fault}: {:?}", out.result);
                        assert!(oracle.step(deadline + 100_000, vec![]).result.is_ok());
                        for node in [owner, driver] {
                            assert!(
                                capture_axis(&d.tree, &node, axis).unwrap().release_matches(
                                    capture_axis(&oracle.tree, &node, axis).unwrap()
                                )
                            );
                        }
                        assert!(d.runtime.groups.is_empty());
                        assert!(d.step(deadline + 250_000, vec![]).result.is_ok());
                    }
                }
            }
        }
    }
}

#[test]
fn multiple_coupled_loop_boundaries_use_one_joint_release_and_stable_clocks() {
    for axis in [Axis::Width, Axis::Height] {
        for mode in [Mode::Full, Mode::Active, Mode::Dirty] {
            for reverse in [false, true] {
                for deadline in [1_000_000, 2_001_100_000] {
                    let mut tree = mixed_peer_pool(axis);
                    let mut other = tree.get(&id(3)).unwrap().spec.declared.clone();
                    let mut clock = spec(Length::Px(80.0), Length::FillWeighted(2.0), 1000.0, axis);
                    clock.repeat = AnimationRepeat::Loop;
                    other.animate = Some(clock);
                    tree.insert(Element::with_attrs(id(4), ElementKind::El, vec![], other));
                    tree.set_children(
                        &id(1),
                        if reverse {
                            vec![id(4), id(2), id(3)]
                        } else {
                            vec![id(2), id(3), id(4)]
                        },
                    )
                    .unwrap();
                    let mut d = Driver::new(tree, Instant::now(), mode);
                    assert!(d.step(0, vec![]).result.is_ok());
                    assert!(d.step(500_000, vec![]).result.is_ok());
                    let out = d.step(deadline, vec![]);
                    assert!(
                        out.result.is_ok(),
                        "{axis:?} {mode:?} {deadline}: {:?}",
                        out.result
                    );
                    assert!(d.runtime.groups.is_empty());
                    let releases = out
                        .inspection
                        .queries
                        .iter()
                        .filter(|q| q.kind == QueryKind::Release)
                        .collect::<Vec<_>>();
                    assert_eq!(releases.len(), 1);
                    let target = releases[0].result.as_ref().unwrap()[&(id(2), axis)];
                    assert!(target.release_matches(capture_axis(&d.tree, &id(2), axis).unwrap()));
                    assert!(
                        out.inspection.queries.len() <= 20,
                        "{}",
                        out.inspection.queries.len()
                    );
                    assert_eq!(out.inspection.after.model_copies, 1);
                }
            }
        }
    }
}

#[test]
fn upward_enter_and_change_owners_release_against_native_loop_forecasts() {
    for axis in [Axis::Width, Axis::Height] {
        for mode in [Mode::Full, Mode::Active, Mode::Dirty] {
            for change in [false, true] {
                let mut tree = coupled_tree(axis, true);
                let root = tree.get_mut(&id(1)).unwrap();
                let animation = root.spec.declared.animate.take().unwrap();
                if change {
                    if axis == Axis::Width {
                        root.spec.declared.width = Some(Length::Px(200.0));
                    } else {
                        root.spec.declared.height = Some(Length::Px(200.0));
                    }
                    root.spec.declared.animate_change = Some(Arc::new(vec![ChangePolicy {
                        field: if axis == Axis::Width {
                            Field::Width
                        } else {
                            Field::Height
                        },
                        duration_ms: 1000.0,
                        curve: AnimationCurve::Linear,
                    }]));
                } else {
                    root.lifecycle.mounted_at_revision = 1;
                    root.spec.declared.animate_enter = Some(animation);
                }
                let mut d = Driver::new(tree, Instant::now(), mode);
                assert!(d.step(0, vec![]).result.is_ok());
                if change {
                    let mut attrs = d.attrs(id(1));
                    if axis == Axis::Width {
                        attrs.width = Some(Length::Content);
                    } else {
                        attrs.height = Some(Length::Content);
                    }
                    assert!(
                        d.step(0, vec![Event::Attrs(id(1), Box::new(attrs))])
                            .result
                            .is_ok()
                    );
                }
                assert!(d.step(500_000, vec![]).result.is_ok());
                let out = d.step(1_000_000, vec![]);
                assert!(
                    out.result.is_ok(),
                    "{axis:?} change={change}: {:?}",
                    out.result
                );
                assert!(d.runtime.groups.is_empty());
                let expected = out
                    .inspection
                    .queries
                    .iter()
                    .find(|q| q.kind == QueryKind::Release)
                    .unwrap()
                    .result
                    .as_ref()
                    .unwrap()[&(id(1), axis)];
                assert!(expected.release_matches(capture_axis(&d.tree, &id(1), axis).unwrap()));
            }
        }
    }
}

#[test]
fn coupled_release_certifies_both_axes_together() {
    for mode in [Mode::Full, Mode::Active, Mode::Dirty] {
        let mut tree = coupled_tree(Axis::Width, true);
        for node in [1, 2] {
            let spec = tree
                .get_mut(&id(node))
                .unwrap()
                .spec
                .declared
                .animate
                .as_mut()
                .unwrap();
            for attrs in &mut spec.keyframes {
                attrs.height = attrs.width.clone();
            }
        }
        let mut d = Driver::new(tree, Instant::now(), mode);
        assert!(d.step(0, vec![]).result.is_ok());
        assert!(d.step(500_000, vec![]).result.is_ok());
        let out = d.step(1_000_000, vec![]);
        assert!(out.result.is_ok(), "{:?}", out.result);
        for axis in [Axis::Width, Axis::Height] {
            let target = out
                .inspection
                .queries
                .iter()
                .find(|q| q.kind == QueryKind::Release)
                .unwrap()
                .result
                .as_ref()
                .unwrap()[&(id(1), axis)];
            assert!(target.release_matches(capture_axis(&d.tree, &id(1), axis).unwrap()));
        }
        assert!(d.runtime.groups.is_empty());
    }
}

#[test]
fn ongoing_mixed_feedback_preserves_the_finite_curve_and_frozen_driver() {
    for axis in [Axis::Width, Axis::Height] {
        for upward in [false, true] {
            let mut d = Driver::new(coupled_tree(axis, upward), Instant::now(), Mode::Full);
            assert!(d.step(0, vec![]).result.is_ok());
            let owner = id(if upward { 1 } else { 2 });
            let driver = id(if upward { 2 } else { 3 });
            let initial = d.tree.length_runtime.as_ref().unwrap().tracks[&(owner, axis)].from;
            let out = d.step(500_000, vec![]);
            assert!(out.result.is_ok(), "{:?}", out.result);
            let track = &d.tree.length_runtime.as_ref().unwrap().tracks[&(owner, axis)];
            assert!(track.from.release_matches(initial));
            assert_eq!(track.anchor, 0.0);
            let expected = initial.interpolate(track.to, 0.5).unwrap();
            assert!(expected.release_matches(capture_axis(&d.tree, &owner, axis).unwrap()));
            close(expected.visible, if upward { 127.5 } else { 175.0 });
            let frozen = out.inspection.frozen.as_ref().unwrap().nodes[&driver].clone();
            let fp = if axis == Axis::Width {
                frozen.width
            } else {
                frozen.height
            };
            assert!(
                fp.unwrap()
                    .release_matches(capture_axis(&d.tree, &driver, axis).unwrap())
            );
        }
    }
}

#[test]
fn continuous_feedback_follows_native_targets_on_original_curves_across_schedules() {
    for axis in [Axis::Width, Axis::Height] {
        for upward in [false, true] {
            for mode in [Mode::Full, Mode::Active, Mode::Dirty] {
                for scale in [0.5, 1.0, 2.0] {
                    for curve in [
                        AnimationCurve::Linear,
                        AnimationCurve::EaseIn,
                        AnimationCurve::EaseOut,
                        AnimationCurve::EaseInOut,
                    ] {
                        for hz in [30, 60, 120] {
                            let mut tree = coupled_tree(axis, upward);
                            let owner = id(if upward { 1 } else { 2 });
                            let driver = id(if upward { 2 } else { 3 });
                            for node in [owner, driver] {
                                tree.get_mut(&node)
                                    .unwrap()
                                    .spec
                                    .declared
                                    .animate
                                    .as_mut()
                                    .unwrap()
                                    .curve = curve.clone();
                            }
                            let mut d = Driver::new(tree, Instant::now(), mode);
                            assert!(
                                d.step(
                                    0,
                                    vec![Event::Viewport {
                                        width: 600.0 * scale,
                                        height: 600.0 * scale,
                                        scale
                                    }]
                                )
                                .result
                                .is_ok()
                            );
                            let initial =
                                d.tree.length_runtime.as_ref().unwrap().tracks[&(owner, axis)].from;
                            let key =
                                d.tree.length_runtime.as_ref().unwrap().tracks[&(owner, axis)].key;
                            for frame in 1..hz {
                                let us = frame * 1_000_000 / hz;
                                let out = d.step(us, vec![]);
                                assert!(
                                    out.result.is_ok(),
                                    "{axis:?} up={upward} {curve:?} hz={hz} frame={frame}: {:?}",
                                    out.result
                                );
                                let state = d.tree.length_runtime.as_ref().unwrap();
                                let track = &state.tracks[&(owner, axis)];
                                assert_eq!(track.key, key);
                                assert_eq!(track.anchor, 0.0);
                                assert!(track.from.release_matches(initial));
                                let expected = initial
                                    .interpolate(
                                        track.to,
                                        apply_curve(&curve, us as f64 / 1_000_000.0) as f32,
                                    )
                                    .unwrap();
                                assert!(
                                    expected.release_matches(
                                        capture_axis(&d.tree, &owner, axis).unwrap()
                                    )
                                );
                                let driver_node =
                                    &out.inspection.frozen.as_ref().unwrap().nodes[&driver];
                                let frozen = if axis == Axis::Width {
                                    driver_node.width
                                } else {
                                    driver_node.height
                                };
                                assert!(frozen.unwrap().release_matches(
                                    capture_axis(&d.tree, &driver, axis).unwrap()
                                ));
                                assert!(out.inspection.queries.len() <= 12);
                                assert_eq!(out.inspection.after.model_copies, 1);
                                assert!(state.tracks[&(driver, axis)].forecast.is_none());
                            }
                            assert!(d.step(1_000_000, vec![]).result.is_ok());
                            assert!(d.runtime.groups.is_empty());
                        }
                    }
                }
            }
        }
    }
}

#[test]
fn coupled_hold_keeps_its_admitted_interval_when_the_other_finite_owner_is_cancelled() {
    for axis in [Axis::Width, Axis::Height] {
        for mode in [Mode::Full, Mode::Active, Mode::Dirty] {
            let mut tree = mixed_peer_pool(axis);
            let mut attrs = tree.get(&id(2)).unwrap().spec.declared.clone();
            attrs.animate.as_mut().unwrap().duration_ms = 2000.0;
            tree.insert(Element::with_attrs(id(4), ElementKind::El, vec![], attrs));
            tree.set_children(&id(1), vec![id(2), id(3), id(4)])
                .unwrap();
            let mut d = Driver::new(tree, Instant::now(), mode);
            for us in [0, 500_000, 1_000_000, 1_250_000, 1_500_000] {
                assert!(d.step(us, vec![]).result.is_ok());
            }
            let interval = d.tree.length_runtime.as_ref().unwrap().tracks[&(id(2), axis)]
                .hold_interval
                .unwrap();
            let mut attrs = d.attrs(id(4));
            attrs.animate = None;
            if axis == Axis::Width {
                attrs.width = Some(Length::Fill);
            } else {
                attrs.height = Some(Length::Fill);
            }
            let out = d.step(1_750_000, vec![Event::Attrs(id(4), Box::new(attrs))]);
            assert!(out.result.is_ok(), "{:?}", out.result);
            assert_eq!(
                d.tree.length_runtime.as_ref().unwrap().tracks[&(id(2), axis)]
                    .hold_interval
                    .unwrap()
                    .1,
                interval.1
            );
            assert!(!d.runtime.groups.is_empty());
            let out = d.step(2_000_000, vec![]);
            assert!(out.result.is_ok(), "{:?} {:#?}", out.result, out.inspection);
            assert!(d.runtime.groups.is_empty());
        }
    }
}

#[test]
fn ongoing_feedback_retries_preserve_published_sources_and_the_original_clock() {
    for axis in [Axis::Width, Axis::Height] {
        for upward in [false, true] {
            for mode in [Mode::Full, Mode::Active, Mode::Dirty] {
                for fault in 0..6 {
                    let owner = id(if upward { 1 } else { 2 });
                    let driver = id(if upward { 2 } else { 3 });
                    let mut d = Driver::new(coupled_tree(axis, upward), Instant::now(), mode);
                    let mut control = Driver::new(coupled_tree(axis, upward), Instant::now(), mode);
                    assert!(d.step(0, vec![]).result.is_ok());
                    assert!(control.step(0, vec![]).result.is_ok());
                    let before = d.step(250_000, vec![]).published_after.unwrap();
                    assert!(control.step(250_000, vec![]).result.is_ok());
                    let saved = d.tree.length_runtime.as_ref().unwrap().tracks.clone();
                    if fault == 0 {
                        Arc::make_mut(
                            d.tree
                                .length_runtime
                                .as_mut()
                                .unwrap()
                                .tracks
                                .get_mut(&(owner, axis))
                                .unwrap(),
                        )
                        .to
                        .visible += 7.0;
                    }
                    if fault == 1 {
                        let track = Arc::make_mut(
                            d.tree
                                .length_runtime
                                .as_mut()
                                .unwrap()
                                .tracks
                                .get_mut(&(owner, axis))
                                .unwrap(),
                        );
                        let input = Arc::make_mut(
                            Arc::make_mut(track.forecast.as_mut().unwrap())
                                .get_mut(&(driver, axis))
                                .unwrap(),
                        );
                        input.to.visible += 14.0;
                        input.from.visible -= 2.0; // same .125 forecast, wrong native goal
                    }
                    let injection = match fault {
                        0..=1 => Injection::None,
                        2 => Injection::Query(QueryKind::PriorTarget),
                        3 => Injection::Query(QueryKind::ForeignTarget),
                        4 => Injection::Query(QueryKind::Target),
                        _ => Injection::BeforeLayout,
                    };
                    let out = d.attempt(500_000, vec![], injection);
                    assert!(out.result.is_err(), "up={upward} fault={fault}");
                    out.published_after.unwrap().assert_same(&before);
                    if fault <= 1 {
                        d.tree.length_runtime.as_mut().unwrap().tracks = saved.clone();
                    }
                    let out = d.step(750_000, vec![]);
                    assert!(out.result.is_ok(), "{:?}", out.result);
                    assert!(control.step(750_000, vec![]).result.is_ok());
                    for node in [owner, driver] {
                        assert!(
                            capture_axis(&d.tree, &node, axis)
                                .unwrap()
                                .release_matches(capture_axis(&control.tree, &node, axis).unwrap())
                        );
                    }
                    let track = &d.tree.length_runtime.as_ref().unwrap().tracks[&(owner, axis)];
                    assert_eq!(track.key, saved[&(owner, axis)].key);
                    assert_eq!(track.anchor, 0.0);
                    assert!(d.step(1_000_000, vec![]).result.is_ok());
                    assert!(d.runtime.groups.is_empty());
                }
            }
        }
    }
}

#[test]
fn ongoing_feedback_resets_only_the_anchor_at_foreign_interval_boundaries() {
    for axis in [Axis::Width, Axis::Height] {
        for upward in [false, true] {
            for mode in [Mode::Full, Mode::Active, Mode::Dirty] {
                for boundary in [2_000_000, 2_100_000] {
                    let owner = id(if upward { 1 } else { 2 });
                    let mut tree = coupled_tree(axis, upward);
                    tree.get_mut(&owner)
                        .unwrap()
                        .spec
                        .declared
                        .animate
                        .as_mut()
                        .unwrap()
                        .duration_ms = 4000.0;
                    let mut d = Driver::new(tree, Instant::now(), mode);
                    for us in [0, 500_000, 1_750_000] {
                        assert!(d.step(us, vec![]).result.is_ok());
                    }
                    let key = d.tree.length_runtime.as_ref().unwrap().tracks[&(owner, axis)].key;
                    let out = d.step(boundary, vec![]);
                    assert!(out.result.is_ok(), "{:?}", out.result);
                    let track = &d.tree.length_runtime.as_ref().unwrap().tracks[&(owner, axis)];
                    assert_eq!(track.key, key);
                    assert!(track.anchor > 0.0);
                    let projected = &out.inspection.frozen.as_ref().unwrap().nodes[&owner];
                    let before = if axis == Axis::Width {
                        projected.width
                    } else {
                        projected.height
                    };
                    assert!(
                        before
                            .unwrap()
                            .release_matches(capture_axis(&d.tree, &owner, axis).unwrap())
                    );
                    for us in [2_500_000, 3_000_000, 4_000_000] {
                        let out = d.step(us, vec![]);
                        assert!(
                            out.result.is_ok(),
                            "{axis:?} up={upward} {boundary}/{us}: {:?}",
                            out.result
                        );
                    }
                    assert!(d.runtime.groups.is_empty());
                }
            }
        }
    }
}

fn self_axis_wrapping() -> ElementTree {
    use crate::tree::animation::change::{ChangePolicy, Field};
    let mut tree = fixture(Length::Px(40.0), Length::Fill, Axis::Width);
    let node = tree.get_mut(&id(2)).unwrap();
    node.spec.kind = ElementKind::WrappedRow;
    let mut width = spec(Length::Px(80.0), Length::Fill, 2000.0, Axis::Width);
    width.repeat = AnimationRepeat::Loop;
    node.spec.declared.animate = Some(width);
    node.spec.declared.height = Some(Length::Px(200.0));
    node.spec.declared.animate_change = Some(Arc::new(vec![ChangePolicy {
        field: Field::Height,
        duration_ms: 1000.0,
        curve: AnimationCurve::Linear,
    }]));
    for n in 10..18 {
        tree.insert(Element::with_attrs(
            id(n),
            ElementKind::El,
            vec![],
            Attrs {
                width: Some(Length::Px(40.0)),
                height: Some(Length::Px(20.0)),
                ..Default::default()
            },
        ));
    }
    tree.set_children(&id(2), (10..18).map(id).collect())
        .unwrap();
    tree.set_children(&id(1), vec![id(2)]).unwrap();
    tree
}

#[test]
fn self_axis_wrapping_feedback_preserves_the_height_curve_and_width_clock() {
    for mode in [Mode::Full, Mode::Active, Mode::Dirty] {
        for scale in [0.5, 1.0, 2.0] {
            let mut d = Driver::new(self_axis_wrapping(), Instant::now(), mode);
            assert!(
                d.step(
                    0,
                    vec![Event::Viewport {
                        width: 600.0 * scale,
                        height: 600.0 * scale,
                        scale
                    }]
                )
                .result
                .is_ok()
            );
            let width_key =
                d.tree.length_runtime.as_ref().unwrap().tracks[&(id(2), Axis::Width)].key;
            let mut attrs = d.attrs(id(2));
            attrs.height = Some(Length::Content);
            assert!(
                d.step(0, vec![Event::Attrs(id(2), Box::new(attrs))])
                    .result
                    .is_ok()
            );
            let initial =
                d.tree.length_runtime.as_ref().unwrap().tracks[&(id(2), Axis::Height)].from;
            for us in [250_000, 500_000, 750_000] {
                let out = d.step(us, vec![]);
                assert!(
                    out.result.is_ok(),
                    "{mode:?} {scale} {us}: {:?}",
                    out.result
                );
                let tracks = &d.tree.length_runtime.as_ref().unwrap().tracks;
                assert_eq!(tracks[&(id(2), Axis::Width)].key, width_key);
                let height = &tracks[&(id(2), Axis::Height)];
                assert!(height.from.release_matches(initial));
                assert_eq!(height.anchor, 0.0);
                assert!(
                    initial
                        .interpolate(height.to, us as f32 / 1_000_000.0)
                        .unwrap()
                        .release_matches(capture_axis(&d.tree, &id(2), Axis::Height).unwrap())
                );
                if us == 500_000 {
                    close(
                        capture_axis(&d.tree, &id(2), Axis::Height).unwrap().visible,
                        120.0,
                    );
                }
            }
            let out = d.step(1_000_000, vec![]);
            assert!(out.result.is_ok(), "{:?}", out.result);
            assert!(d.runtime.groups.is_empty());
        }
    }
}

#[test]
fn numeric_and_resolved_inputs_compose_under_native_rotation_and_scale() {
    for axis in [Axis::Width, Axis::Height] {
        for mode in [Mode::Full, Mode::Active, Mode::Dirty] {
            for transform in [false, true] {
                let mut tree = mixed_peer_pool(axis);
                let mut parent = spec(Length::Px(600.0), Length::Px(800.0), 2000.0, axis);
                parent.repeat = AnimationRepeat::Loop;
                tree.get_mut(&id(1)).unwrap().spec.declared.animate = Some(parent);
                if transform {
                    let attrs = &mut tree.get_mut(&id(2)).unwrap().spec.declared;
                    attrs.layout_rotate = Some(15.0);
                    attrs.layout_scale = Some(1.25);
                }
                let mut d = Driver::new(tree, Instant::now(), mode);
                assert!(d.step(0, vec![]).result.is_ok());
                let initial = d.tree.length_runtime.as_ref().unwrap().tracks[&(id(2), axis)].from;
                for us in [250_000, 500_000, 750_000] {
                    let out = d.step(us, vec![]);
                    assert!(
                        out.result.is_ok(),
                        "{axis:?} transform={transform} {us}: {:?}",
                        out.result
                    );
                    let track = &d.tree.length_runtime.as_ref().unwrap().tracks[&(id(2), axis)];
                    assert!(track.from.release_matches(initial));
                    assert_eq!(track.anchor, 0.0);
                    assert!(
                        initial
                            .interpolate(track.to, us as f32 / 1_000_000.0)
                            .unwrap()
                            .release_matches(capture_axis(&d.tree, &id(2), axis).unwrap())
                    );
                }
                let out = d.step(1_000_000, vec![]);
                assert!(
                    out.result.is_ok(),
                    "{axis:?} transform={transform}: {:?}",
                    out.result
                );
                assert!(d.runtime.groups.is_empty());
            }
        }
    }
}

#[test]
fn historical_clock_receipts_reject_corruption_and_retry_the_original_admission() {
    for mode in [Mode::Full, Mode::Active, Mode::Dirty] {
        for fault in 0..5 {
            let mut d = Driver::new(self_axis_wrapping(), Instant::now(), mode);
            assert!(d.step(0, vec![]).result.is_ok());
            let mut attrs = d.attrs(id(2));
            attrs.height = Some(Length::Content);
            let before = d
                .step(0, vec![Event::Attrs(id(2), Box::new(attrs))])
                .published_after
                .unwrap();
            let saved = d.tree.length_runtime.as_ref().unwrap().tracks.clone();
            if fault < 4 {
                let owner = Arc::make_mut(
                    d.tree
                        .length_runtime
                        .as_mut()
                        .unwrap()
                        .tracks
                        .get_mut(&(id(2), Axis::Height))
                        .unwrap(),
                );
                let input = Arc::make_mut(
                    Arc::make_mut(owner.forecast.as_mut().unwrap())
                        .get_mut(&(id(2), Axis::Width))
                        .unwrap(),
                );
                match fault {
                    0 => input.to.visible += 7.0,
                    1 => input.projection = Arc::new((*input.projection).clone()),
                    2 => input.context = Arc::new((*input.context).clone()),
                    _ => input.model += 1,
                }
            }
            let out = d.attempt(
                500_000,
                vec![],
                if fault == 4 {
                    Injection::Query(QueryKind::HistoricalTarget)
                } else {
                    Injection::None
                },
            );
            assert!(out.result.is_err(), "{mode:?} fault={fault}");
            out.published_after.unwrap().assert_same(&before);
            d.tree.length_runtime.as_mut().unwrap().tracks = saved;
            let out = d.step(750_000, vec![]);
            assert!(
                out.result.is_ok(),
                "{mode:?} fault={fault}: {:?}",
                out.result
            );
            assert_eq!(
                d.tree.length_runtime.as_ref().unwrap().tracks[&(id(2), Axis::Height)].anchor,
                0.0
            );
            assert!(d.step(1_000_000, vec![]).result.is_ok());
            assert!(d.runtime.groups.is_empty());
        }
    }
}

#[test]
fn model_viewport_and_mixed_boundary_release_replay_original_joint_inputs() {
    for axis in [Axis::Width, Axis::Height] {
        for mode in [Mode::Full, Mode::Active, Mode::Dirty] {
            for scale in [0.5, 1.0, 2.0] {
                let mut tree = mixed_peer_pool(axis);
                tree.get_mut(&id(2))
                    .unwrap()
                    .spec
                    .declared
                    .animate
                    .as_mut()
                    .unwrap()
                    .duration_ms = 2000.0;
                let mut d = Driver::new(tree, Instant::now(), mode);
                for us in [0, 500_000, 1_750_000] {
                    assert!(d.step(us, vec![]).result.is_ok());
                }
                let mut attrs = d.attrs(id(1));
                attrs.padding = Some(crate::tree::attrs::Padding::Sides {
                    top: 5.0,
                    right: 10.0,
                    bottom: 5.0,
                    left: 10.0,
                });
                let out = d.step(
                    2_100_000,
                    vec![
                        Event::Attrs(id(1), Box::new(attrs)),
                        Event::Viewport {
                            width: 800.0 * scale,
                            height: 800.0 * scale,
                            scale,
                        },
                    ],
                );
                assert!(
                    out.result.is_ok(),
                    "{axis:?} {mode:?} {scale}: {:?}",
                    out.result
                );
                assert!(d.runtime.groups.is_empty());
                assert!(
                    out.inspection
                        .queries
                        .iter()
                        .any(|q| q.kind == QueryKind::PreviousModel)
                );
            }
        }
    }
}

#[test]
fn seed_membership_changes_at_coupled_release_use_native_receipts() {
    for mode in [Mode::Full, Mode::Active, Mode::Dirty] {
        for remove in [false, true] {
            let mut tree = mixed_peer_pool(Axis::Width);
            tree.insert(Element::with_attrs(
                id(4),
                ElementKind::El,
                vec![],
                Attrs {
                    width: Some(Length::Px(10.0)),
                    height: Some(Length::Px(10.0)),
                    scrollbar_y: remove.then_some(true),
                    ..Default::default()
                },
            ));
            tree.set_children(&id(1), vec![id(2), id(3), id(4)])
                .unwrap();
            let mut d = Driver::new(tree, Instant::now(), mode);
            assert!(d.step(0, vec![]).result.is_ok());
            let before = d.step(500_000, vec![]).published_after.unwrap();
            let mut attrs = d.attrs(id(4));
            attrs.scrollbar_y = (!remove).then_some(true);
            let out = d.attempt(
                1_000_000,
                vec![Event::Attrs(id(4), Box::new(attrs))],
                Injection::Query(QueryKind::HistoricalTarget),
            );
            assert!(out.result.is_err(), "{mode:?} remove={remove}");
            out.published_after.unwrap().assert_same(&before);
            let out = d.step(1_100_000, vec![]);
            assert!(
                out.result.is_ok(),
                "{mode:?} remove={remove}: {:?}",
                out.result
            );
            assert!(
                out.inspection
                    .queries
                    .iter()
                    .any(|q| q.kind == QueryKind::HistoricalTarget)
            );
            assert!(d.runtime.groups.is_empty());
        }
    }
}

#[test]
fn composed_feedback_respects_native_parent_imposed_sizes() {
    for axis in [Axis::Width, Axis::Height] {
        for mode in [Mode::Full, Mode::Active, Mode::Dirty] {
            let mut tree = mixed_peer_pool(axis);
            tree.get_mut(&id(1)).unwrap().spec.kind = ElementKind::Slider;
            let mut parent = spec(Length::Px(600.0), Length::Px(800.0), 2000.0, axis);
            parent.repeat = AnimationRepeat::Loop;
            tree.get_mut(&id(1)).unwrap().spec.declared.animate = Some(parent);
            let mut d = Driver::new(tree, Instant::now(), mode);
            for us in [0, 250_000, 500_000, 750_000, 1_000_000] {
                let out = d.step(us, vec![]);
                assert!(
                    out.result.is_ok(),
                    "{axis:?} {mode:?} {us}: {:?}",
                    out.result
                );
                if us == 1_000_000 {
                    let expected = out
                        .inspection
                        .queries
                        .iter()
                        .find(|q| q.kind == QueryKind::Release)
                        .unwrap()
                        .result
                        .as_ref()
                        .unwrap()[&(id(2), axis)];
                    assert!(expected.release_matches(capture_axis(&d.tree, &id(2), axis).unwrap()));
                    assert!(d.runtime.groups.is_empty());
                }
            }
        }
    }
}

#[test]
fn self_axis_independence_does_not_reanchor_the_loop() {
    for mode in [Mode::Full, Mode::Active, Mode::Dirty] {
        let mut tree = self_axis_wrapping();
        tree.get_mut(&id(2)).unwrap().spec.kind = ElementKind::Column;
        let mut d = Driver::new(tree, Instant::now(), mode);
        assert!(d.step(0, vec![]).result.is_ok());
        let mut attrs = d.attrs(id(2));
        attrs.height = Some(Length::Content);
        assert!(
            d.step(0, vec![Event::Attrs(id(2), Box::new(attrs))])
                .result
                .is_ok()
        );
        let source = d.tree.length_runtime.as_ref().unwrap().tracks[&(id(2), Axis::Width)].from;
        for us in [250_000, 500_000, 750_000, 1_000_000] {
            let out = d.step(us, vec![]);
            assert!(out.result.is_ok(), "{:?}", out.result);
            let track = &d.tree.length_runtime.as_ref().unwrap().tracks[&(id(2), Axis::Width)];
            assert!(track.from.release_matches(source));
            assert_eq!(track.anchor, 0.0);
            close(
                capture_axis(&d.tree, &id(2), Axis::Width).unwrap().visible,
                80.0 + 520.0 * us as f32 / 2_000_000.0,
            );
        }
        assert!(d.runtime.groups.is_empty());
    }
}

#[test]
fn a_rotated_mixed_driver_can_also_have_a_numeric_axis() {
    for mode in [Mode::Full, Mode::Active, Mode::Dirty] {
        let mut tree = mixed_peer_pool(Axis::Width);
        let peer = &mut tree.get_mut(&id(3)).unwrap().spec.declared;
        peer.layout_rotate = Some(15.0);
        for (index, attrs) in peer
            .animate
            .as_mut()
            .unwrap()
            .keyframes
            .iter_mut()
            .enumerate()
        {
            attrs.height = Some(Length::Px(20.0 + index as f64 * 40.0));
        }
        let mut d = Driver::new(tree, Instant::now(), mode);
        assert!(d.step(0, vec![]).result.is_ok());
        let source = d.tree.length_runtime.as_ref().unwrap().tracks[&(id(2), Axis::Width)].from;
        for us in [250_000, 500_000, 750_000] {
            let out = d.step(us, vec![]);
            assert!(out.result.is_ok(), "{:?}", out.result);
            let track = &d.tree.length_runtime.as_ref().unwrap().tracks[&(id(2), Axis::Width)];
            assert_eq!(track.anchor, 0.0);
            assert!(track.from.release_matches(source));
        }
        let out = d.step(1_000_000, vec![]);
        assert!(out.result.is_ok(), "{:?}", out.result);
        assert!(d.runtime.groups.is_empty());
    }
}

#[test]
fn noncanonical_self_and_cross_axis_repeats_interruptions_and_failures_preserve_admission_clocks() {
    use crate::tree::animation::change::{ChangePolicy, Field};
    for vertical in [false, true] {
        let driver_axis = if vertical { Axis::Height } else { Axis::Width };
        let consumer_axis = if vertical { Axis::Width } else { Axis::Height };
        let field = if vertical {
            Field::Width
        } else {
            Field::Height
        };
        let set_axis = |attrs: &mut Attrs, axis: Axis, value: Length| match axis {
            Axis::Width => attrs.width = Some(value),
            Axis::Height => attrs.height = Some(value),
        };
        for cross in [false, true] {
            for looping in [false, true] {
                for kind in [
                    ElementKind::WrappedRow,
                    ElementKind::Paragraph,
                    ElementKind::TextColumn,
                    ElementKind::Slider,
                ] {
                    for mode in [Mode::Full, Mode::Active, Mode::Dirty] {
                        for curve in [
                            AnimationCurve::Linear,
                            AnimationCurve::EaseIn,
                            AnimationCurve::EaseOut,
                            AnimationCurve::EaseInOut,
                        ] {
                            for scale in [0.5, 2.0] {
                                for fault in
                                    [Injection::Query(QueryKind::Target), Injection::BeforeLayout]
                                {
                                    let mut tree = self_axis_wrapping();
                                    let node = tree.get_mut(&id(2)).unwrap();
                                    node.spec.kind = kind;
                                    let width = node.spec.declared.animate.as_mut().unwrap();
                                    width.curve = curve.clone();
                                    width.repeat = if looping {
                                        AnimationRepeat::Loop
                                    } else {
                                        AnimationRepeat::Times(3)
                                    };
                                    width.keyframes = vec![
                                        Attrs {
                                            width: Some(Length::Px(80.0)),
                                            ..Default::default()
                                        },
                                        Attrs {
                                            width: Some(Length::Fill),
                                            ..Default::default()
                                        },
                                        Attrs {
                                            width: Some(Length::Px(40.0)),
                                            ..Default::default()
                                        },
                                        Attrs {
                                            width: Some(Length::Fill),
                                            ..Default::default()
                                        },
                                    ];
                                    node.spec.declared.animate_change =
                                        Some(Arc::new(vec![ChangePolicy {
                                            field: Field::Height,
                                            duration_ms: 4500.0,
                                            curve: curve.clone(),
                                        }]));
                                    let owner = if cross {
                                        let policy = node.spec.declared.animate_change.take();
                                        node.spec.declared.height = Some(Length::Content);
                                        let parent = tree.get_mut(&id(1)).unwrap();
                                        parent.spec.declared.height = Some(Length::Px(200.0));
                                        parent.spec.declared.animate_change = policy;
                                        id(1)
                                    } else {
                                        id(2)
                                    };
                                    if vertical {
                                        for node in tree.iter_nodes_mut() {
                                            let attrs = &mut node.spec.declared;
                                            std::mem::swap(&mut attrs.width, &mut attrs.height);
                                            if let Some(spec) = attrs.animate.as_mut() {
                                                for attrs in &mut spec.keyframes {
                                                    std::mem::swap(
                                                        &mut attrs.width,
                                                        &mut attrs.height,
                                                    );
                                                }
                                            }
                                            if let Some(policies) = attrs.animate_change.as_mut() {
                                                for policy in Arc::make_mut(policies) {
                                                    policy.field = match policy.field {
                                                        Field::Width => Field::Height,
                                                        Field::Height => Field::Width,
                                                        _ => unreachable!(),
                                                    };
                                                }
                                            }
                                        }
                                    }
                                    let mut d = Driver::new(tree, Instant::now(), mode);
                                    assert!(
                                        d.step(
                                            0,
                                            vec![Event::Viewport {
                                                width: 600.0 * scale,
                                                height: 600.0 * scale,
                                                scale
                                            }]
                                        )
                                        .result
                                        .is_ok()
                                    );
                                    let width_key = d.tree.length_runtime.as_ref().unwrap().tracks
                                        [&(id(2), driver_axis)]
                                        .key;
                                    let mut attrs = d.attrs(owner);
                                    set_axis(&mut attrs, consumer_axis, Length::Content);
                                    let first =
                                        d.step(0, vec![Event::Attrs(owner, Box::new(attrs))]);
                                    assert!(first.result.is_ok(), "{kind:?}: {:?}", first.result);
                                    let height_key = d.tree.length_runtime.as_ref().unwrap().tracks
                                        [&(owner, consumer_axis)]
                                        .key;
                                    for us in [
                                        250_000, 500_000, 666_667, 900_000, 1_333_334, 1_500_000,
                                        2_000_000, 2_100_000,
                                    ] {
                                        let out = d.step(us, vec![]);
                                        assert!(
                                            out.result.is_ok(),
                                            "vertical={vertical} cross={cross} looping={looping} {kind:?} {mode:?} {curve:?} {scale} {us}: {:?}",
                                            out.result
                                        );
                                        let tracks =
                                            &d.tree.length_runtime.as_ref().unwrap().tracks;
                                        let w = tracks[&(id(2), driver_axis)].key;
                                        let h = tracks[&(owner, consumer_axis)].key;
                                        assert_eq!(
                                            (w.started, w.generation, w.mount),
                                            (
                                                width_key.started,
                                                width_key.generation,
                                                width_key.mount
                                            )
                                        );
                                        assert_eq!(h, height_key);
                                    }
                                    let before = d.step(2_150_000, vec![]).published_after.unwrap();
                                    let mut attrs = d.attrs(owner);
                                    set_axis(&mut attrs, consumer_axis, Length::Px(240.0));
                                    attrs.animate_change = Some(Arc::new(vec![ChangePolicy {
                                        field,
                                        duration_ms: 800.0,
                                        curve: curve.clone(),
                                    }]));
                                    let failed = d.attempt(
                                        2_250_000,
                                        vec![Event::Attrs(owner, Box::new(attrs))],
                                        fault,
                                    );
                                    assert!(failed.result.is_err());
                                    failed.published_after.unwrap().assert_same(&before);
                                    let out = d.step(2_400_000, vec![]);
                                    assert!(
                                        out.result.is_ok(),
                                        "vertical={vertical} cross={cross} looping={looping} {kind:?} {mode:?} {curve:?} {scale} retry: {:?}",
                                        out.result
                                    );
                                    let key = d.tree.length_runtime.as_ref().unwrap().tracks
                                        [&(owner, consumer_axis)]
                                        .key;
                                    assert_ne!(key.generation, height_key.generation);
                                    assert_eq!(
                                        key.started,
                                        height_key.started + Duration::from_micros(2_250_000)
                                    );
                                    for us in [2_666_667, 2_950_000, 3_050_000] {
                                        let out = d.step(us, vec![]);
                                        assert!(
                                            out.result.is_ok(),
                                            "vertical={vertical} cross={cross} looping={looping} {kind:?} {mode:?} {curve:?} {scale} interrupted {us}: {:?}",
                                            out.result
                                        );
                                    }
                                    assert_eq!(
                                        d.runtime.groups.is_empty(),
                                        looping,
                                        "only the three-repeat driver remains a finite waiter until six seconds"
                                    );
                                    let mut attrs = d.attrs(id(2));
                                    attrs.animate = None;
                                    set_axis(&mut attrs, driver_axis, Length::Fill);
                                    assert!(
                                        d.step(
                                            3_100_000,
                                            vec![Event::Attrs(id(2), Box::new(attrs))]
                                        )
                                        .result
                                        .is_ok()
                                    );
                                    assert!(d.step(3_200_000, vec![]).result.is_ok());
                                    assert!(d.runtime.groups.is_empty());
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}
