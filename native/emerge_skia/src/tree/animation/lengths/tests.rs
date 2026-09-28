use super::*;
use crate::tree::layout::dimensions::{capture_axis, set_axis_sample};
use crate::tree::{
    element::{Element, ElementKind},
    layout::{Constraint, SkiaTextMeasurer, try_layout_tree_with_animation},
};
use std::time::Duration;
fn id(n: u64) -> NodeId {
    NodeId::from_wire_u64(n)
}
fn spec(a: Length, b: Length, ms: f64, axis: Axis) -> AnimationSpec {
    let attrs = |value| match axis {
        Axis::Width => Attrs {
            width: Some(value),
            ..Attrs::default()
        },
        Axis::Height => Attrs {
            height: Some(value),
            ..Attrs::default()
        },
    };
    AnimationSpec {
        keyframes: vec![attrs(a), attrs(b)],
        duration_ms: ms,
        curve: AnimationCurve::Linear,
        repeat: AnimationRepeat::Once,
    }
}
fn fixture(a: Length, b: Length, axis: Axis) -> ElementTree {
    let mut tree = ElementTree::new();
    tree.set_root_id(id(1));
    tree.insert(Element::with_attrs(
        id(1),
        if axis == Axis::Width {
            ElementKind::Row
        } else {
            ElementKind::Column
        },
        vec![],
        Attrs {
            width: Some(Length::Px(600.0)),
            height: Some(Length::Px(600.0)),
            ..Attrs::default()
        },
    ));
    tree.insert(Element::with_attrs(
        id(2),
        ElementKind::El,
        vec![],
        Attrs {
            animate: Some(spec(a, b, 1000.0, axis)),
            ..Attrs::default()
        },
    ));
    tree.insert(Element::with_attrs(
        id(3),
        ElementKind::El,
        vec![],
        Attrs {
            width: Some(Length::Fill),
            height: Some(Length::Fill),
            ..Attrs::default()
        },
    ));
    tree.set_children(&id(1), vec![id(2), id(3)]).unwrap();
    tree.set_revision(1);
    tree
}
fn tick(
    tree: &mut ElementTree,
    runtime: &mut AnimationRuntime,
    start: Instant,
    ms: u64,
    scale: f32,
) {
    let now = start + Duration::from_micros(ms);
    let sync = runtime.sync_with_tree(tree, now);
    for effect in sync.completed.effects {
        tree.mark_layout_dirty_for_invalidation(&effect.id, effect.invalidation);
    }
    try_layout_tree_with_animation(
        tree,
        Constraint::new(600.0 * scale, 600.0 * scale),
        scale,
        runtime,
        now,
        &SkiaTextMeasurer,
        &FontContext::default(),
    )
    .unwrap();
    assert!(tree.animation_error.is_none(), "{:?}", tree.animation_error);
}
fn extent(tree: &ElementTree, n: u64, axis: Axis) -> f32 {
    let f = tree.get(&id(n)).unwrap().layout.frame.unwrap();
    if axis == Axis::Width {
        f.width
    } else {
        f.height
    }
}
#[test]
fn live_fill_keeps_box_and_charge_continuous_and_releases_at_completion() {
    for axis in [Axis::Width, Axis::Height] {
        for scale in [0.5, 1.0, 2.0] {
            let mut tree = fixture(Length::Px(40.0), Length::Fill, axis);
            let mut rt = AnimationRuntime::default();
            let start = Instant::now();
            for (us, t) in [
                (0, 0.0),
                (250000, 0.25),
                (500000, 0.5),
                (750000, 0.75),
                (999999, 0.999999),
                (1000000, 1.0),
            ] {
                tick(&mut tree, &mut rt, start, us, scale);
                assert!((extent(&tree, 2, axis) / scale - (40.0 + 260.0 * t)).abs() < 0.002);
                assert!((extent(&tree, 3, axis) / scale - (560.0 - 260.0 * t)).abs() < 0.002);
            }
            assert!(tree.length_runtime.is_none());
            assert!(tree.get(&id(2)).unwrap().layout.dimension_samples.is_none());
        }
    }
}
#[test]
fn synchronous_fields_share_endpoints_and_warm_queries() {
    let mut tree = fixture(Length::Px(40.0), Length::Fill, Axis::Width);
    tree.get_mut(&id(3)).unwrap().spec.declared = tree.get(&id(2)).unwrap().spec.declared.clone();
    let mut rt = AnimationRuntime::default();
    let start = Instant::now();
    tick(&mut tree, &mut rt, start, 0, 1.0);
    assert_eq!(
        tree.length_runtime
            .as_ref()
            .unwrap()
            .resolver
            .stats()
            .layout_queries,
        2
    );
    tick(&mut tree, &mut rt, start, 500000, 1.0);
    assert_eq!(extent(&tree, 2, Axis::Width), 170.0);
    assert_eq!(extent(&tree, 3, Axis::Width), 170.0);
    assert_eq!(
        tree.length_runtime
            .as_ref()
            .unwrap()
            .resolver
            .stats()
            .layout_queries,
        2
    );
}
#[test]
fn weighted_interpolation_uses_resolved_endpoints_from_the_start() {
    let mut tree = fixture(
        Length::FillWeighted(1.0),
        Length::FillWeighted(3.0),
        Axis::Width,
    );
    let mut rt = AnimationRuntime::default();
    let start = Instant::now();
    tick(&mut tree, &mut rt, start, 0, 1.0);
    tick(&mut tree, &mut rt, start, 500000, 1.0);
    assert_eq!(extent(&tree, 2, Axis::Width), 375.0);
    assert_eq!(
        tree.length_runtime
            .as_ref()
            .unwrap()
            .resolver
            .stats()
            .layout_queries,
        2
    );
}

fn change_target(tree: &mut ElementTree, width: Length, duration: f64) {
    use crate::tree::animation::change::{ChangePolicy, Field};
    tree.capture_animation_source(&id(2));
    let node = tree.get_mut(&id(2)).unwrap();
    node.spec.declared.width = Some(width);
    node.spec.declared.animate_change = Some(Arc::new(vec![ChangePolicy {
        field: Field::Width,
        duration_ms: duration,
        curve: AnimationCurve::Linear,
    }]));
    tree.layout_model_epoch = tree.layout_model_epoch.wrapping_add(1);
    tree.set_revision(tree.revision() + 1);
    tree.mark_measure_dirty(&id(2));
}
#[test]
fn change_admission_consumes_presentation_and_interruption_keeps_full_samples() {
    let mut tree = fixture(Length::Px(40.0), Length::Fill, Axis::Width);
    tree.get_mut(&id(2)).unwrap().spec.declared.animate = None;
    tree.get_mut(&id(2)).unwrap().spec.declared.width = Some(Length::Px(40.0));
    let mut rt = AnimationRuntime::default();
    let start = Instant::now();
    tick(&mut tree, &mut rt, start, 0, 1.0);
    change_target(&mut tree, Length::Fill, 1000.0);
    tick(&mut tree, &mut rt, start, 250000, 1.0);
    assert_eq!(extent(&tree, 2, Axis::Width), 40.0);
    assert!(tree.pending_patch_effects.sources.is_empty());
    tick(&mut tree, &mut rt, start, 500000, 1.0);
    assert_eq!(extent(&tree, 2, Axis::Width), 105.0);
    change_target(&mut tree, Length::Px(50.0), 1000.0);
    tick(&mut tree, &mut rt, start, 500000, 1.0);
    assert_eq!(extent(&tree, 2, Axis::Width), 105.0);
    tick(&mut tree, &mut rt, start, 1000000, 1.0);
    assert_eq!(extent(&tree, 2, Axis::Width), 77.5);
    assert_eq!(extent(&tree, 3, Axis::Width), 522.5);
    tick(&mut tree, &mut rt, start, 1500000, 1.0);
    assert_eq!(extent(&tree, 2, Axis::Width), 50.0);
    assert!(rt.changes.is_empty());
    assert!(tree.length_runtime.is_none());
}
#[test]
fn identical_change_targets_do_not_restart_and_removing_policy_completes_damage() {
    let mut tree = fixture(Length::Px(40.0), Length::Fill, Axis::Width);
    tree.get_mut(&id(2)).unwrap().spec.declared.animate = None;
    tree.get_mut(&id(2)).unwrap().spec.declared.width = Some(Length::Px(40.0));
    let mut rt = AnimationRuntime::default();
    let start = Instant::now();
    tick(&mut tree, &mut rt, start, 0, 1.0);
    change_target(&mut tree, Length::Fill, 1000.0);
    tick(&mut tree, &mut rt, start, 0, 1.0);
    tick(&mut tree, &mut rt, start, 250000, 1.0);
    change_target(&mut tree, Length::Fill, 5000.0);
    tick(&mut tree, &mut rt, start, 500000, 1.0);
    assert_eq!(extent(&tree, 2, Axis::Width), 170.0);
    tree.get_mut(&id(2)).unwrap().spec.declared.animate_change = None;
    let sync = rt.sync_with_tree(&tree, start + Duration::from_millis(500));
    assert!(sync.completed.invalidation == TreeInvalidation::Measure);
    tick(&mut tree, &mut rt, start, 500000, 1.0);
    assert_eq!(extent(&tree, 2, Axis::Width), 300.0);
    assert!(tree.length_runtime.is_none());
}
#[test]
fn finite_sibling_targets_do_not_follow_each_others_samples() {
    let run = |hz: u64| {
        let mut tree = fixture(Length::Px(40.0), Length::Fill, Axis::Width);
        tree.get_mut(&id(3)).unwrap().spec.declared.animate =
            Some(spec(Length::Px(40.0), Length::Fill, 2000.0, Axis::Width));
        let mut rt = AnimationRuntime::default();
        let start = Instant::now();
        for step in 0..=hz / 2 {
            tick(&mut tree, &mut rt, start, step * 1_000_000 / hz, 1.0);
        }
        extent(&tree, 2, Axis::Width)
    };
    // These are now members of one finite group, not foreign trajectories.
    // Joint endpoints give 40 + (300 - 40) / 2 at either tested schedule.
    // This does not require schedule independence for foreign-group retargeting.
    for (hz, expected) in [(30, 170.0), (120, 170.0)] {
        let actual = run(hz);
        assert!(
            (actual - expected).abs() < 0.002,
            "{hz}Hz: {actual} != {expected}"
        );
        assert_eq!(
            run(hz),
            actual,
            "same schedule must replay deterministically"
        );
    }
}

#[test]
fn enter_handoff_admits_only_the_latest_pending_target() {
    for mixed in [false, true] {
        let end = if mixed {
            Length::Fill
        } else {
            Length::Px(40.0)
        };
        let mut tree = fixture(Length::Px(20.0), end.clone(), Axis::Width);
        let node = tree.get_mut(&id(2)).unwrap();
        node.lifecycle.mounted_at_revision = 1;
        node.spec.declared.animate_enter = node.spec.declared.animate.take();
        node.spec.declared.width = Some(end);
        let mut rt = AnimationRuntime::default();
        let start = Instant::now();
        tick(&mut tree, &mut rt, start, 0, 1.0);
        tick(&mut tree, &mut rt, start, 250000, 1.0);
        change_target(&mut tree, Length::Px(100.0), 1000.0);
        tick(&mut tree, &mut rt, start, 250000, 1.0);
        change_target(&mut tree, Length::Px(50.0), 1000.0);
        tick(&mut tree, &mut rt, start, 500000, 1.0);
        assert!(rt.changes.values().all(|entry| entry.pending));
        tick(&mut tree, &mut rt, start, 1000000, 1.0);
        let from = if mixed { 300.0 } else { 40.0 };
        assert_eq!(extent(&tree, 2, Axis::Width), from);
        tick(&mut tree, &mut rt, start, 1500000, 1.0);
        assert_eq!(extent(&tree, 2, Axis::Width), (from + 50.0) * 0.5);
        tick(&mut tree, &mut rt, start, 2000000, 1.0);
        assert_eq!(extent(&tree, 2, Axis::Width), 50.0);
        assert!(rt.changes.is_empty());
        assert!(tree.length_runtime.is_none());
    }
}

fn sibling_fixture(axis: Axis) -> ElementTree {
    let mut tree = fixture(Length::Px(40.0), Length::Fill, axis);
    tree.get_mut(&id(3)).unwrap().spec.declared.animate =
        Some(spec(Length::Px(40.0), Length::Fill, 2000.0, axis));
    tree
}
fn close(actual: f32, expected: f32) {
    assert!((actual - expected).abs() < 0.002, "{actual} != {expected}");
}
#[test]
fn finite_sibling_runtime_holds_and_atomically_releases_both_axes_at_all_schedules() {
    for axis in [Axis::Width, Axis::Height] {
        for scale in [0.5, 1.0, 2.0] {
            for hz in [30, 60, 120] {
                let mut tree = sibling_fixture(axis);
                let mut rt = AnimationRuntime::default();
                let start = Instant::now();
                for step in 0..2 * hz {
                    let us = step * 1_000_000 / hz;
                    tick(&mut tree, &mut rt, start, us, scale);
                    close(
                        extent(&tree, 2, axis) / scale,
                        40.0 + 260.0 * (us as f32 / 1_000_000.0).min(1.0),
                    );
                    close(
                        extent(&tree, 3, axis) / scale,
                        40.0 + 260.0 * us as f32 / 2_000_000.0,
                    );
                    assert!(tree.get(&id(2)).unwrap().layout.dimension_samples.is_some());
                    assert_eq!(rt.groups.len(), 1);
                }
                tick(&mut tree, &mut rt, start, 1_999_999, scale);
                let before = [extent(&tree, 2, axis), extent(&tree, 3, axis)];
                assert_eq!(
                    tree.length_runtime
                        .as_ref()
                        .unwrap()
                        .resolver
                        .stats()
                        .layout_queries,
                    2,
                    "warm holds must not query again"
                );
                tick(&mut tree, &mut rt, start, 2_000_000, scale);
                for (n, value) in [(2, before[0]), (3, before[1])] {
                    close(extent(&tree, n, axis), value);
                    close(extent(&tree, n, axis) / scale, 300.0);
                    assert!(tree.get(&id(n)).unwrap().layout.dimension_samples.is_none());
                }
                assert_eq!(rt.groups.len(), 0);
                assert!(tree.length_runtime.is_none());
                let mut fresh = fixture(Length::Px(40.0), Length::Fill, axis);
                let attrs = &mut fresh.get_mut(&id(2)).unwrap().spec.declared;
                attrs.animate = None;
                match axis {
                    Axis::Width => attrs.width = Some(Length::Fill),
                    Axis::Height => attrs.height = Some(Length::Fill),
                };
                tick(
                    &mut fresh,
                    &mut AnimationRuntime::default(),
                    start,
                    0,
                    scale,
                );
                for n in 1..=3 {
                    assert_eq!(
                        tree.get(&id(n)).unwrap().layout.frame,
                        fresh.get(&id(n)).unwrap().layout.frame
                    );
                }
            }
        }
    }
}
#[test]
fn grouped_weighted_finisher_keeps_full_reservation_for_static_peers() {
    let mut tree = sibling_fixture(Axis::Width);
    tree.get_mut(&id(2)).unwrap().spec.declared.animate = Some(spec(
        Length::Px(40.0),
        Length::FillWeighted(3.0),
        1000.0,
        Axis::Width,
    ));
    tree.get_mut(&id(3)).unwrap().spec.declared.animate =
        Some(spec(Length::Px(40.0), Length::Fill, 2000.0, Axis::Width));
    tree.insert(Element::with_attrs(
        id(4),
        ElementKind::El,
        vec![],
        Attrs {
            width: Some(Length::Fill),
            ..Attrs::default()
        },
    ));
    tree.set_children(&id(1), vec![id(2), id(3), id(4)])
        .unwrap();
    let mut rt = AnimationRuntime::default();
    let start = Instant::now();
    tick(&mut tree, &mut rt, start, 0, 1.0);
    tick(&mut tree, &mut rt, start, 1_000_000, 1.0);
    close(extent(&tree, 2, Axis::Width), 360.0);
    close(
        capture_axis(&tree, &id(2), Axis::Width).unwrap().charge,
        360.0,
    );
    close(extent(&tree, 3, Axis::Width), 80.0);
    close(extent(&tree, 4, Axis::Width), 160.0);
    tick(&mut tree, &mut rt, start, 1_999_999, 1.0);
    let before = extent(&tree, 4, Axis::Width);
    tick(&mut tree, &mut rt, start, 2_000_000, 1.0);
    close(extent(&tree, 4, Axis::Width), before);
    close(extent(&tree, 4, Axis::Width), 120.0);
}
#[test]
fn completed_enter_retains_geometry_but_not_paint_until_sibling_finishes() {
    let mut tree = sibling_fixture(Axis::Width);
    let a = tree.get_mut(&id(2)).unwrap();
    a.lifecycle.mounted_at_revision = 1;
    let mut enter = a.spec.declared.animate.take().unwrap();
    enter.keyframes[0].alpha = Some(0.1);
    enter.keyframes[1].alpha = Some(0.9);
    a.spec.declared.animate_enter = Some(enter);
    a.spec.declared.width = Some(Length::Fill);
    a.spec.declared.alpha = Some(0.25);
    let mut rt = AnimationRuntime::default();
    let start = Instant::now();
    tick(&mut tree, &mut rt, start, 0, 1.0);
    tick(&mut tree, &mut rt, start, 1_000_000, 1.0);
    assert!(rt.enter_entry(&id(2)).is_some());
    close(extent(&tree, 2, Axis::Width), 300.0);
    assert_eq!(tree.get(&id(2)).unwrap().layout.effective.alpha, Some(0.25));
    tick(&mut tree, &mut rt, start, 1_500_000, 1.0);
    close(extent(&tree, 2, Axis::Width), 300.0);
    close(extent(&tree, 3, Axis::Width), 235.0);
    tick(&mut tree, &mut rt, start, 2_000_000, 1.0);
    assert!(rt.enter_entry(&id(2)).is_none());
    assert!(tree.length_runtime.is_none());
    close(extent(&tree, 2, Axis::Width), 300.0);
}
#[test]
fn completed_change_keeps_its_source_only_until_joint_release() {
    use crate::tree::animation::change::{ChangePolicy, Field};
    let mut tree = sibling_fixture(Axis::Width);
    for n in [2, 3] {
        let a = &mut tree.get_mut(&id(n)).unwrap().spec.declared;
        a.animate = None;
        a.width = Some(Length::Px(40.0));
    }
    let mut rt = AnimationRuntime::default();
    let start = Instant::now();
    tick(&mut tree, &mut rt, start, 0, 1.0);
    for (n, duration) in [(2, 1000.0), (3, 2000.0)] {
        tree.capture_animation_source(&id(n));
        let a = &mut tree.get_mut(&id(n)).unwrap().spec.declared;
        a.width = Some(Length::Fill);
        a.animate_change = Some(Arc::new(vec![ChangePolicy {
            field: Field::Width,
            duration_ms: duration,
            curve: AnimationCurve::Linear,
        }]));
        tree.mark_measure_dirty(&id(n));
    }
    tree.layout_model_epoch += 1;
    tree.set_revision(2);
    tick(&mut tree, &mut rt, start, 0, 1.0);
    tick(&mut tree, &mut rt, start, 1_000_000, 1.0);
    close(extent(&tree, 2, Axis::Width), 300.0);
    close(extent(&tree, 3, Axis::Width), 170.0);
    assert_eq!(rt.changes.len(), 2);
    tick(&mut tree, &mut rt, start, 2_000_000, 1.0);
    assert!(rt.changes.is_empty());
    assert!(rt.is_empty());
    assert_eq!(rt.groups.len(), 0);
    assert!(tree.length_runtime.is_none());
    assert!(tree.pending_patch_effects.sources.is_empty());
    close(extent(&tree, 2, Axis::Width), 300.0);
    close(extent(&tree, 3, Axis::Width), 300.0);
}

#[test]
fn held_geometry_retargets_from_current_footprint_over_remaining_group_time() {
    let mut tree = sibling_fixture(Axis::Width);
    let mut rt = AnimationRuntime::default();
    let start = Instant::now();
    tick(&mut tree, &mut rt, start, 0, 1.0);
    tick(&mut tree, &mut rt, start, 1_250_000, 1.0);
    close(extent(&tree, 2, Axis::Width), 300.0);
    close(extent(&tree, 3, Axis::Width), 202.5);
    tree.get_mut(&id(1)).unwrap().spec.declared.width = Some(Length::Px(800.0));
    tree.layout_model_epoch += 1;
    tree.set_revision(2);
    tree.mark_measure_dirty(&id(1));
    tick(&mut tree, &mut rt, start, 1_250_000, 1.0);
    close(extent(&tree, 2, Axis::Width), 300.0);
    close(extent(&tree, 3, Axis::Width), 202.5);
    tick(&mut tree, &mut rt, start, 1_625_000, 1.0);
    close(extent(&tree, 2, Axis::Width), 350.0);
    close(extent(&tree, 3, Axis::Width), 301.25);
    tick(&mut tree, &mut rt, start, 1_999_999, 1.0);
    let before = [extent(&tree, 2, Axis::Width), extent(&tree, 3, Axis::Width)];
    tick(&mut tree, &mut rt, start, 2_000_000, 1.0);
    for (n, value) in [(2, before[0]), (3, before[1])] {
        close(extent(&tree, n, Axis::Width), value);
        close(extent(&tree, n, Axis::Width), 400.0);
    }
    assert!(tree.length_runtime.is_none());
}
#[test]
fn adjacent_segments_use_previous_joint_endpoint_and_reanchor_siblings() {
    let mut tree = sibling_fixture(Axis::Width);
    let a = tree
        .get_mut(&id(2))
        .unwrap()
        .spec
        .declared
        .animate
        .as_mut()
        .unwrap();
    a.keyframes.push(a.keyframes[0].clone());
    a.duration_ms = 2000.0;
    let mut rt = AnimationRuntime::default();
    let start = Instant::now();
    for (us, a, b) in [
        (0, 40.0, 40.0),
        (500_000, 170.0, 105.0),
        (1_000_000, 300.0, 170.0),
        (1_500_000, 170.0, 365.0),
        (2_000_000, 40.0, 560.0),
    ] {
        tick(&mut tree, &mut rt, start, us, 1.0);
        close(extent(&tree, 2, Axis::Width), a);
        close(extent(&tree, 3, Axis::Width), b);
    }
    assert!(tree.length_runtime.is_none());
    assert_eq!(rt.groups.len(), 0);
}
#[test]
fn finite_repeat_resets_source_at_cycle_boundary_without_releasing_the_group() {
    let mut tree = sibling_fixture(Axis::Width);
    tree.get_mut(&id(2))
        .unwrap()
        .spec
        .declared
        .animate
        .as_mut()
        .unwrap()
        .repeat = AnimationRepeat::Times(2);
    tree.get_mut(&id(3))
        .unwrap()
        .spec
        .declared
        .animate
        .as_mut()
        .unwrap()
        .duration_ms = 3000.0;
    let mut rt = AnimationRuntime::default();
    let start = Instant::now();
    tick(&mut tree, &mut rt, start, 0, 1.0);
    tick(&mut tree, &mut rt, start, 500_000, 1.0);
    close(extent(&tree, 2, Axis::Width), 170.0);
    tick(&mut tree, &mut rt, start, 1_000_000, 1.0);
    close(extent(&tree, 2, Axis::Width), 40.0);
    tick(&mut tree, &mut rt, start, 1_500_000, 1.0);
    close(extent(&tree, 2, Axis::Width), 170.0);
    tick(&mut tree, &mut rt, start, 2_000_000, 1.0);
    close(extent(&tree, 2, Axis::Width), 300.0);
    assert!(tree.get(&id(2)).unwrap().layout.dimension_samples.is_some());
    tick(&mut tree, &mut rt, start, 3_000_000, 1.0);
    close(extent(&tree, 2, Axis::Width), 300.0);
    close(extent(&tree, 3, Axis::Width), 300.0);
    assert!(tree.length_runtime.is_none());
}
#[test]
fn looping_foreign_trajectories_are_characterized_per_sampling_schedule() {
    let run = |hz: u64| {
        let mut tree = sibling_fixture(Axis::Width);
        tree.get_mut(&id(3))
            .unwrap()
            .spec
            .declared
            .animate
            .as_mut()
            .unwrap()
            .repeat = AnimationRepeat::Loop;
        let mut rt = AnimationRuntime::default();
        let start = Instant::now();
        // Independent scalar oracle for this fixed 600px pool: predict both
        // presentations once, move finite A along its original curve to the
        // new goal, and retarget loop B from its predicted presentation.
        let (_, _, _, _, expected) = (0..=hz / 2).fold(
            (560.0, 40.0, 560.0, 0.0, 40.0),
            |(a_to, b_from, b_to, b_anchor, _), step| {
                let us = step * 1_000_000 / hz;
                let t = us as f64 / 1_000_000.0;
                let p = t / 2.0;
                let a_forecast = 40.0 + (a_to - 40.0) * t;
                let b = b_from + (b_to - b_from) * (p - b_anchor) / (1.0 - b_anchor);
                let a_to = 600.0 - b;
                let a = 40.0 + (a_to - 40.0) * t;
                tick(&mut tree, &mut rt, start, us, 1.0);
                close(extent(&tree, 2, Axis::Width), a as f32);
                close(extent(&tree, 3, Axis::Width), b as f32);
                (a_to, b, 600.0 - a_forecast, p, a)
            },
        );
        assert_eq!(
            rt.groups.len(),
            1,
            "the loop is not a finite barrier member"
        );
        let actual = extent(&tree, 2, Axis::Width);
        close(actual, expected as f32);
        actual
    };
    // Same schedule must replay, not converge to a schedule-independent curve.
    for hz in [30, 60, 120] {
        assert_eq!(run(hz), run(hz));
    }
}

#[test]
fn weighted_member_reaches_and_holds_joint_target_without_a_completion_switch() {
    let mut tree = sibling_fixture(Axis::Width);
    tree.get_mut(&id(2)).unwrap().spec.declared.animate = Some(spec(
        Length::FillWeighted(1.0),
        Length::FillWeighted(3.0),
        1000.0,
        Axis::Width,
    ));
    let mut rt = AnimationRuntime::default();
    let start = Instant::now();
    tick(&mut tree, &mut rt, start, 0, 1.0);
    tick(&mut tree, &mut rt, start, 1_000_000, 1.0);
    close(extent(&tree, 2, Axis::Width), 450.0);
    close(extent(&tree, 3, Axis::Width), 95.0);
    assert!(tree.get(&id(2)).unwrap().layout.dimension_samples.is_some());
    tick(&mut tree, &mut rt, start, 2_000_000, 1.0);
    close(extent(&tree, 2, Axis::Width), 450.0);
    close(extent(&tree, 3, Axis::Width), 150.0);
}

#[test]
fn corrupt_structural_source_does_not_partially_replace_other_tracks_or_samples() {
    use crate::tree::animation::change::{ChangePolicy, Field};
    // Different HashMap seeds exercise both successful-query/failing-source orders.
    for _ in 0..12 {
        let mut tree = sibling_fixture(Axis::Width);
        let mut rt = AnimationRuntime::default();
        let start = Instant::now();
        tick(&mut tree, &mut rt, start, 0, 1.0);
        tick(&mut tree, &mut rt, start, 250_000, 1.0);
        let samples: Vec<_> = [2, 3]
            .into_iter()
            .map(|n| tree.get(&id(n)).unwrap().layout.dimension_samples.clone())
            .collect();
        tree.capture_animation_source(&id(3));
        let b = &mut tree.get_mut(&id(3)).unwrap().spec.declared;
        b.animate = None;
        b.width = Some(Length::Px(100.0));
        b.animate_change = Some(Arc::new(vec![ChangePolicy {
            field: Field::Width,
            duration_ms: 1000.0,
            curve: AnimationCurve::Linear,
        }]));
        tree.insert(Element::with_attrs(
            id(4),
            ElementKind::Row,
            vec![],
            Attrs {
                width: Some(Length::Px(200.0)),
                ..Attrs::default()
            },
        ));
        tree.set_children(&id(1), vec![id(2), id(4)]).unwrap();
        tree.set_children(&id(4), vec![id(3)]).unwrap();
        tree.set_revision(2);
        let valid_source = tree.pending_patch_effects.sources[&id(3)].clone();
        // Reparenting itself is now valid. Corrupt the sealed published source
        // instead; it must fail before any lane is installed.
        tree.pending_patch_effects
            .sources
            .get_mut(&id(3))
            .unwrap()
            .dimensions[0]
            .as_mut()
            .unwrap()
            .charge += 1.0;
        let now = start + Duration::from_millis(500);
        rt.sync_with_tree(&tree, now);
        let mut engine = tree.length_runtime.take().unwrap();
        let before = engine.tracks.clone();
        let mut frame = sample_animation_overlays(&tree, Some(&rt), Some(now));
        let result = engine.resolve(
            &mut tree,
            Some(&rt),
            Some(now),
            1.0,
            &FontContext::default(),
            &crate::tree::layout::SkiaTextMeasurer,
            &mut frame,
        );
        assert_eq!(result, Err(ProjectionError::StalePreparation));
        for (key, old) in &before {
            let new = &engine.tracks[key];
            assert_eq!(old.from, new.from);
            assert_eq!(old.to, new.to);
            assert_eq!(old.anchor, new.anchor);
            assert!(Arc::ptr_eq(&old.projection, &new.projection));
        }
        for (n, sample) in [2, 3].into_iter().zip(samples) {
            assert_eq!(tree.get(&id(n)).unwrap().layout.dimension_samples, sample);
        }
        assert!(!tree.pending_patch_effects.sources.is_empty());
        // Restore the genuine source and original attachment; retry must recover.
        tree.pending_patch_effects
            .sources
            .insert(id(3), valid_source);
        tree.set_children(&id(4), vec![]).unwrap();
        tree.set_children(&id(1), vec![id(2), id(3)]).unwrap();
        tree.set_revision(3);
        tree.length_runtime = Some(engine);
        tick(&mut tree, &mut rt, start, 500_000, 1.0);
        assert!(tree.pending_patch_effects.sources.is_empty());
    }
}

#[test]
fn cancelling_last_mover_releases_completed_change_sources_in_the_same_sync() {
    use crate::tree::animation::change::{ChangePolicy, Field};
    let mut tree = sibling_fixture(Axis::Width);
    for n in [2, 3] {
        let a = &mut tree.get_mut(&id(n)).unwrap().spec.declared;
        a.animate = None;
        a.width = Some(Length::Px(40.0));
    }
    let mut rt = AnimationRuntime::default();
    let start = Instant::now();
    tick(&mut tree, &mut rt, start, 0, 1.0);
    for (n, duration) in [(2, 1000.0), (3, 2000.0)] {
        tree.capture_animation_source(&id(n));
        let a = &mut tree.get_mut(&id(n)).unwrap().spec.declared;
        a.width = Some(Length::Fill);
        a.animate_change = Some(Arc::new(vec![ChangePolicy {
            field: Field::Width,
            duration_ms: duration,
            curve: AnimationCurve::Linear,
        }]));
        tree.mark_measure_dirty(&id(n));
    }
    tree.layout_model_epoch += 1;
    tree.set_revision(2);
    tick(&mut tree, &mut rt, start, 0, 1.0);
    tick(&mut tree, &mut rt, start, 1_500_000, 1.0);
    assert_eq!(rt.changes.len(), 2);
    tree.get_mut(&id(3)).unwrap().spec.declared.animate_change = None;
    tree.set_revision(3);
    tick(&mut tree, &mut rt, start, 1_500_000, 1.0);
    assert!(rt.changes.is_empty());
    assert_eq!(rt.groups.len(), 0);
    assert!(tree.length_runtime.is_none());
    close(extent(&tree, 2, Axis::Width), 300.0);
    close(extent(&tree, 3, Axis::Width), 300.0);
}

#[test]
fn change_arrival_at_old_barrier_joins_before_completion_and_retargets_the_hold() {
    use crate::tree::animation::change::{ChangePolicy, Field};
    let mut tree = sibling_fixture(Axis::Width);
    for n in [2, 3] {
        let a = &mut tree.get_mut(&id(n)).unwrap().spec.declared;
        a.animate = None;
        a.width = Some(Length::Px(40.0));
    }
    let mut rt = AnimationRuntime::default();
    let start = Instant::now();
    tick(&mut tree, &mut rt, start, 0, 1.0);
    change_target(&mut tree, Length::Fill, 1000.0);
    tick(&mut tree, &mut rt, start, 0, 1.0);
    tick(&mut tree, &mut rt, start, 999_999, 1.0);
    close(extent(&tree, 2, Axis::Width), 560.0);
    tree.capture_animation_source(&id(3));
    let b = &mut tree.get_mut(&id(3)).unwrap().spec.declared;
    b.width = Some(Length::Fill);
    b.animate_change = Some(Arc::new(vec![ChangePolicy {
        field: Field::Width,
        duration_ms: 1000.0,
        curve: AnimationCurve::Linear,
    }]));
    tree.layout_model_epoch += 1;
    tree.set_revision(tree.revision() + 1);
    tree.mark_measure_dirty(&id(3));
    tick(&mut tree, &mut rt, start, 1_000_000, 1.0);
    close(extent(&tree, 2, Axis::Width), 560.0);
    close(extent(&tree, 3, Axis::Width), 40.0);
    assert_eq!(rt.changes.len(), 2);
    tick(&mut tree, &mut rt, start, 1_500_000, 1.0);
    close(extent(&tree, 2, Axis::Width), 430.0);
    close(extent(&tree, 3, Axis::Width), 170.0);
    tick(&mut tree, &mut rt, start, 2_000_000, 1.0);
    close(extent(&tree, 2, Axis::Width), 300.0);
    close(extent(&tree, 3, Axis::Width), 300.0);
    assert!(rt.changes.is_empty());
    assert!(tree.length_runtime.is_none());
}

#[test]
fn weighted_footprints_follow_easing_on_both_axes() {
    for axis in [Axis::Width, Axis::Height] {
        for scale in [0.5, 1.0, 2.0] {
            for curve in [
                AnimationCurve::Linear,
                AnimationCurve::EaseIn,
                AnimationCurve::EaseOut,
                AnimationCurve::EaseInOut,
            ] {
                {
                    let value = Length::FillWeighted;
                    let mut tree = fixture(value(1.0), value(3.0), axis);
                    tree.get_mut(&id(2))
                        .unwrap()
                        .spec
                        .declared
                        .animate
                        .as_mut()
                        .unwrap()
                        .curve = curve.clone();
                    let mut rt = AnimationRuntime::default();
                    let start = Instant::now();
                    tick(&mut tree, &mut rt, start, 0, scale);
                    for us in [250_000, 500_000, 750_000, 999_999] {
                        tick(&mut tree, &mut rt, start, us, scale);
                        let track = &tree.length_runtime.as_ref().unwrap().tracks[&(id(2), axis)];
                        let p = timing::at_elapsed(
                            tree.get(&id(2))
                                .unwrap()
                                .spec
                                .declared
                                .animate
                                .as_ref()
                                .unwrap(),
                            us as f64 / 1000.0,
                        )
                        .unwrap();
                        let expected = 300.0 + 150.0 * p.eased as f32;
                        close(extent(&tree, 2, axis) / scale, expected);
                        close(extent(&tree, 3, axis) / scale, 600.0 - expected);
                        close(capture_axis(&tree, &id(2), axis).unwrap().charge, expected);
                        assert!(track.from.policy.fill_request > 0.0);
                    }
                    tick(&mut tree, &mut rt, start, 1_000_000, scale);
                    assert!(tree.length_runtime.is_none());
                    close(extent(&tree, 3, axis) / scale, 150.0);
                }
            }
        }
    }
}

#[test]
fn bounded_static_sources_cannot_enter_implicit_change_runs() {
    for axis in [Axis::Width, Axis::Height] {
        let mut tree = fixture(Length::Px(40.0), Length::Fill, axis);
        let attrs = &mut tree.get_mut(&id(2)).unwrap().spec.declared;
        attrs.animate = None;
        let bounded = Some(Length::Min(
            Box::new(Length::Px(50.0)),
            Box::new(Length::Fill),
        ));
        let field = if axis == Axis::Width {
            attrs.width = bounded;
            change::Field::Width
        } else {
            attrs.height = bounded;
            change::Field::Height
        };
        let mut rt = AnimationRuntime::default();
        let start = Instant::now();
        tick(&mut tree, &mut rt, start, 0, 1.0);
        let before = tree.get(&id(2)).unwrap().layout.effective.clone();
        tree.capture_animation_source(&id(2));
        let attrs = &mut tree.get_mut(&id(2)).unwrap().spec.declared;
        if axis == Axis::Width {
            attrs.width = Some(Length::Px(80.0));
        } else {
            attrs.height = Some(Length::Px(80.0));
        }
        attrs.animate_change = Some(Arc::new(vec![change::ChangePolicy {
            field,
            duration_ms: 1000.0,
            curve: AnimationCurve::Linear,
        }]));
        tree.bump_revision();
        tree.layout_model_epoch += 1;
        assert_eq!(
            rt.sync_with_tree(&tree, start).completed.preparation_error,
            Some(ProjectionError::UnsupportedLength(id(2)))
        );
        assert!(rt.changes.is_empty());
        assert!(tree.length_runtime.is_none());
        assert_eq!(tree.get(&id(2)).unwrap().layout.effective, before);
    }
}

#[test]
fn min_max_keyframes_are_rejected_for_each_owner_and_axis() {
    for axis in [Axis::Width, Axis::Height] {
        for bounded in [
            Length::Min(Box::new(Length::Px(50.0)), Box::new(Length::Fill)),
            Length::Max(Box::new(Length::Content), Box::new(Length::Px(50.0))),
        ] {
            for reverse in [false, true] {
                let (from, to) = if reverse {
                    (bounded.clone(), Length::Px(40.0))
                } else {
                    (Length::Px(40.0), bounded.clone())
                };
                for owner in 0..3 {
                    let mut tree = fixture(from.clone(), to.clone(), axis);
                    let attrs = &mut tree.get_mut(&id(2)).unwrap().spec.declared;
                    let spec = attrs.animate.take();
                    match owner {
                        0 => attrs.animate = spec,
                        1 => attrs.animate_enter = spec,
                        _ => attrs.animate_exit = spec,
                    }
                    let mut rt = AnimationRuntime::default();
                    assert_eq!(
                        rt.sync_with_tree(&tree, Instant::now())
                            .completed
                            .preparation_error,
                        Some(ProjectionError::UnsupportedLength(id(2)))
                    );
                    assert!(tree.length_runtime.is_none());
                }
                let mut before = Attrs::default();
                let mut after = Attrs::default();
                let field = if axis == Axis::Width {
                    before.width = Some(from);
                    after.width = Some(to);
                    change::Field::Width
                } else {
                    before.height = Some(from);
                    after.height = Some(to);
                    change::Field::Height
                };
                after.animate_change = Some(Arc::new(vec![change::ChangePolicy {
                    field,
                    duration_ms: 1000.0,
                    curve: AnimationCurve::Linear,
                }]));
                assert!(change::validate_transition(&before, &after).is_err());
            }
        }
    }
}

#[test]
fn direct_pixels_keep_the_zero_workspace_path() {
    let mut tree = fixture(Length::Px(40.0), Length::Px(80.0), Axis::Width);
    let mut rt = AnimationRuntime::default();
    let start = Instant::now();
    tick(&mut tree, &mut rt, start, 0, 1.0);
    tick(&mut tree, &mut rt, start, 500_000, 1.0);
    close(extent(&tree, 2, Axis::Width), 60.0);
    assert!(tree.length_runtime.is_none());
    assert_eq!(rt.groups.len(), 0);
}

#[test]
fn release_checks_charge_not_just_visible_size_and_preserves_sources_on_failure() {
    let mut tree = fixture(Length::Px(40.0), Length::Fill, Axis::Width);
    let a = &mut tree.get_mut(&id(2)).unwrap().spec.declared;
    a.animate = None;
    a.width = Some(Length::Px(40.0));
    let mut rt = AnimationRuntime::default();
    let start = Instant::now();
    tick(&mut tree, &mut rt, start, 0, 1.0);
    change_target(&mut tree, Length::FillWeighted(3.0), 1000.0);
    tick(&mut tree, &mut rt, start, 0, 1.0);
    tick(&mut tree, &mut rt, start, 999_999, 1.0);
    let original = tree.length_runtime.as_ref().unwrap().tracks[&(id(2), Axis::Width)].to;
    Arc::make_mut(
        tree.length_runtime
            .as_mut()
            .unwrap()
            .tracks
            .get_mut(&(id(2), Axis::Width))
            .unwrap(),
    )
    .to
    .charge -= 20.0;
    tree.capture_animation_source(&id(2));
    let before: Vec<_> = [2, 3]
        .into_iter()
        .map(|n| {
            let n = tree.get(&id(n)).unwrap();
            (
                n.layout.frame,
                n.layout.dimension_samples.clone(),
                n.layout.effective.clone(),
            )
        })
        .collect();
    let now = start + Duration::from_secs(1);
    rt.sync_with_tree(&tree, now);
    assert_eq!(
        rt.changes.len(),
        1,
        "nominal completion must not consume the held source"
    );
    let result = try_layout_tree_with_animation(
        &mut tree,
        Constraint::new(600.0, 600.0),
        1.0,
        &mut rt,
        now,
        &SkiaTextMeasurer,
        &FontContext::default(),
    );
    assert_eq!(
        result,
        Err(ProjectionError::ReleaseMismatch(id(2), Axis::Width))
    );
    assert_eq!(rt.changes.len(), 1);
    assert_eq!(rt.groups.len(), 1);
    assert!(!tree.pending_patch_effects.sources.is_empty());
    assert!(
        tree.length_runtime
            .as_ref()
            .unwrap()
            .pending_release
            .is_none()
    );
    for (n, (frame, sample, attrs)) in [2, 3].into_iter().zip(before) {
        let node = tree.get(&id(n)).unwrap();
        assert_eq!(node.layout.frame, frame);
        assert_eq!(node.layout.dimension_samples, sample);
        assert_eq!(node.layout.effective, attrs);
    }
    Arc::make_mut(
        tree.length_runtime
            .as_mut()
            .unwrap()
            .tracks
            .get_mut(&(id(2), Axis::Width))
            .unwrap(),
    )
    .to = original;
    tick(&mut tree, &mut rt, start, 1_000_000, 1.0);
    assert!(rt.changes.is_empty());
    assert!(tree.length_runtime.is_none());
    assert!(tree.pending_patch_effects.sources.is_empty());
    close(extent(&tree, 2, Axis::Width), 450.0);
    close(extent(&tree, 3, Axis::Width), 150.0);
}

#[test]
fn simultaneous_parent_groups_use_one_release_query_and_commit_after_layout() {
    let mut tree = ElementTree::new();
    tree.set_root_id(id(1));
    tree.insert(Element::with_attrs(
        id(1),
        ElementKind::Row,
        vec![],
        Attrs {
            width: Some(Length::Px(600.0)),
            height: Some(Length::Px(100.0)),
            ..Attrs::default()
        },
    ));
    for (parent, child, peer) in [(2, 4, 5), (3, 6, 7)] {
        tree.insert(Element::with_attrs(
            id(parent),
            ElementKind::Row,
            vec![],
            Attrs {
                width: Some(Length::Px(300.0)),
                height: Some(Length::Px(100.0)),
                ..Attrs::default()
            },
        ));
        tree.insert(Element::with_attrs(
            id(child),
            ElementKind::El,
            vec![],
            Attrs {
                animate: Some(spec(Length::Px(40.0), Length::Fill, 1000.0, Axis::Width)),
                ..Attrs::default()
            },
        ));
        tree.insert(Element::with_attrs(
            id(peer),
            ElementKind::El,
            vec![],
            Attrs {
                width: Some(Length::Fill),
                ..Attrs::default()
            },
        ));
        tree.set_children(&id(parent), vec![id(child), id(peer)])
            .unwrap();
    }
    tree.set_children(&id(1), vec![id(2), id(3)]).unwrap();
    tree.set_revision(1);
    let mut rt = AnimationRuntime::default();
    let start = Instant::now();
    tick(&mut tree, &mut rt, start, 0, 1.0);
    tick(&mut tree, &mut rt, start, 999_999, 1.0);
    let before = tree
        .length_runtime
        .as_ref()
        .unwrap()
        .resolver
        .stats()
        .layout_queries;
    let now = start + Duration::from_secs(1);
    rt.sync_with_tree(&tree, now);
    let target = tree.length_runtime.as_ref().unwrap().tracks[&(id(4), Axis::Width)].to;
    Arc::make_mut(
        tree.length_runtime
            .as_mut()
            .unwrap()
            .tracks
            .get_mut(&(id(4), Axis::Width))
            .unwrap(),
    )
    .to
    .charge -= 10.0;
    let failed = crate::tree::layout::prepare_frame_attrs_for_update(
        &mut tree,
        1.0,
        Some(&mut rt),
        Some(now),
    );
    assert_eq!(
        failed.animation_result.preparation_error,
        Some(ProjectionError::ReleaseMismatch(id(4), Axis::Width))
    );
    assert_eq!(rt.groups.len(), 2);
    for n in [4, 6] {
        assert!(tree.get(&id(n)).unwrap().layout.dimension_samples.is_some());
    }
    assert_eq!(
        tree.length_runtime
            .as_ref()
            .unwrap()
            .resolver
            .stats()
            .layout_queries,
        before + 1
    );
    Arc::make_mut(
        tree.length_runtime
            .as_mut()
            .unwrap()
            .tracks
            .get_mut(&(id(4), Axis::Width))
            .unwrap(),
    )
    .to = target;
    let prep = crate::tree::layout::prepare_frame_attrs_for_update(
        &mut tree,
        1.0,
        Some(&mut rt),
        Some(now),
    );
    assert!(prep.animation_result.preparation_error.is_none());
    assert_eq!(
        tree.length_runtime
            .as_ref()
            .unwrap()
            .resolver
            .stats()
            .layout_queries,
        before + 2
    );
    assert_eq!(rt.groups.len(), 2, "preparation does not finalize owners");
    for n in [4, 6] {
        assert!(tree.get(&id(n)).unwrap().layout.dimension_samples.is_some());
    }
    let prep = prep.apply(&mut tree, Some(&rt)).unwrap();
    crate::tree::layout::layout_and_refresh_prepared_default_reusing_clean_registry(
        &mut tree,
        Constraint::new(600.0, 600.0),
        prep,
        None,
    );
    rt.finish_prepared_frame(&mut tree).unwrap();
    assert_eq!(rt.groups.len(), 0);
    assert!(tree.length_runtime.is_none());
    for n in [4, 5, 6, 7] {
        close(extent(&tree, n, Axis::Width), 150.0);
    }
}

#[test]
fn enter_terminal_release_precedes_explicit_base_handoff_and_requests_final_frame() {
    let mut tree = fixture(Length::Px(40.0), Length::Fill, Axis::Width);
    let node = tree.get_mut(&id(2)).unwrap();
    node.lifecycle.mounted_at_revision = 1;
    node.spec.declared.animate_enter = node.spec.declared.animate.take();
    node.spec.declared.width = Some(Length::Px(100.0));
    let mut rt = AnimationRuntime::default();
    let start = Instant::now();
    tick(&mut tree, &mut rt, start, 0, 1.0);
    let now = start + Duration::from_secs(1);
    rt.sync_with_tree(&tree, now);
    let active = try_layout_tree_with_animation(
        &mut tree,
        Constraint::new(600.0, 600.0),
        1.0,
        &mut rt,
        now,
        &SkiaTextMeasurer,
        &FontContext::default(),
    )
    .unwrap();
    assert!(
        active,
        "the intentional handoff needs one final publication"
    );
    close(extent(&tree, 2, Axis::Width), 300.0);
    assert!(tree.length_runtime.is_none());
    assert!(rt.enter_entry(&id(2)).is_none());
    assert!(tree.pending_patch_effects.invalidation.requires_recompute());
    let active = try_layout_tree_with_animation(
        &mut tree,
        Constraint::new(600.0, 600.0),
        1.0,
        &mut rt,
        now + Duration::from_millis(16),
        &SkiaTextMeasurer,
        &FontContext::default(),
    )
    .unwrap();
    assert!(!active);
    close(extent(&tree, 2, Axis::Width), 100.0);
}

#[test]
fn release_uses_the_supplied_measurer_and_inherited_font_context() {
    struct Measurer;
    impl TextMeasurer for Measurer {
        fn measure_with_font(&self, _: &str, _: f32, family: &str, _: u16, _: bool) -> (f32, f32) {
            assert_eq!(family, "release-test");
            (120.0, 20.0)
        }
        fn font_metrics(&self, _: f32, family: &str, _: u16, _: bool) -> (f32, f32) {
            assert_eq!(family, "release-test");
            (15.0, 5.0)
        }
    }
    let mut tree = fixture(
        Length::FillWeighted(1.0),
        Length::FillWeighted(3.0),
        Axis::Width,
    );
    tree.insert(Element::with_attrs(
        id(4),
        ElementKind::Text,
        vec![],
        Attrs {
            content: Some("fixed".into()),
            width: Some(Length::Content),
            ..Attrs::default()
        },
    ));
    tree.set_children(&id(1), vec![id(2), id(3), id(4)])
        .unwrap();
    let font = FontContext {
        font_family: Some("release-test".into()),
        font_size: Some(20.0),
        ..FontContext::default()
    };
    let mut rt = AnimationRuntime::default();
    let start = Instant::now();
    for (ms, expected) in [(0, 240.0), (500, 300.0), (1000, 360.0)] {
        let now = start + Duration::from_millis(ms);
        rt.sync_with_tree(&tree, now);
        try_layout_tree_with_animation(
            &mut tree,
            Constraint::new(600.0, 600.0),
            1.0,
            &mut rt,
            now,
            &Measurer,
            &font,
        )
        .unwrap();
        close(extent(&tree, 2, Axis::Width), expected);
        close(extent(&tree, 4, Axis::Width), 120.0);
    }
    assert!(tree.length_runtime.is_none());
}

mod receipts;

mod driver;
mod support;

mod admission;

mod application;

mod regular;

#[test]
fn declared_container_completion_edits_work_on_both_axes() {
    for axis in [Axis::Width, Axis::Height] {
        let mut tree = fixture(Length::FillWeighted(1.0), Length::FillWeighted(3.0), axis);
        let mut rt = AnimationRuntime::default();
        let start = Instant::now();
        tick(&mut tree, &mut rt, start, 0, 1.0);
        tick(&mut tree, &mut rt, start, 999_999, 1.0);
        tree.capture_animation_source(&id(1));
        let attrs = &mut tree.get_mut(&id(1)).unwrap().spec.declared;
        match axis {
            Axis::Width => attrs.width = Some(Length::Px(800.0)),
            Axis::Height => attrs.height = Some(Length::Px(800.0)),
        }
        tree.layout_model_epoch += 1;
        tree.set_revision(tree.revision() + 1);
        tree.mark_measure_dirty(&id(1));
        tick(&mut tree, &mut rt, start, 1_000_000, 1.0);
        close(extent(&tree, 2, axis), 600.0);
        close(extent(&tree, 3, axis), 200.0);
        assert!(tree.length_runtime.is_none());
    }
}

#[test]
fn continuously_resized_pixel_parent_preserves_the_original_curve_anchor() {
    for axis in [Axis::Width, Axis::Height] {
        for hz in [30_u64, 60, 120] {
            for curve in [
                AnimationCurve::Linear,
                AnimationCurve::EaseIn,
                AnimationCurve::EaseOut,
                AnimationCurve::EaseInOut,
            ] {
                let mut tree = fixture(Length::Px(40.0), Length::Fill, axis);
                tree.get_mut(&id(1)).unwrap().spec.declared.animate =
                    Some(spec(Length::Px(600.0), Length::Px(800.0), 2000.0, axis));
                tree.get_mut(&id(2))
                    .unwrap()
                    .spec
                    .declared
                    .animate
                    .as_mut()
                    .unwrap()
                    .curve = curve.clone();
                let mut rt = AnimationRuntime::default();
                let start = Instant::now();
                for step in 0..=hz {
                    let us = step * 1_000_000 / hz;
                    tick(&mut tree, &mut rt, start, us, 1.0);
                    let t = us as f64 / 1_000_000.0;
                    let eased = timing::apply_curve(&curve, t) as f32;
                    close(
                        extent(&tree, 2, axis),
                        40.0 + (300.0 + 50.0 * t as f32 - 40.0) * eased,
                    );
                }
                assert!(tree.length_runtime.is_none());
                close(extent(&tree, 2, axis), 350.0);
                close(extent(&tree, 3, axis), 350.0);
            }
        }
    }
}

#[test]
fn continuously_changing_held_targets_preserve_the_remaining_interval() {
    let mut tree = sibling_fixture(Axis::Width);
    tree.get_mut(&id(1)).unwrap().spec.declared.animate = Some(spec(
        Length::Px(600.0),
        Length::Px(800.0),
        2000.0,
        Axis::Width,
    ));
    let mut rt = AnimationRuntime::default();
    let start = Instant::now();
    for us in [0, 500_000, 1_000_000, 1_250_000] {
        tick(&mut tree, &mut rt, start, us, 1.0);
    }
    close(extent(&tree, 2, Axis::Width), 350.0);
    let interval =
        tree.length_runtime.as_ref().unwrap().tracks[&(id(2), Axis::Width)].hold_interval;
    tick(&mut tree, &mut rt, start, 1_500_000, 1.0);
    close(extent(&tree, 2, Axis::Width), 350.0 + 25.0 / 3.0);
    close(extent(&tree, 3, Axis::Width), 291.25);
    assert_eq!(
        tree.length_runtime.as_ref().unwrap().tracks[&(id(2), Axis::Width)].hold_interval,
        interval
    );
    tick(&mut tree, &mut rt, start, 2_000_000, 1.0);
    close(extent(&tree, 2, Axis::Width), 400.0);
    close(extent(&tree, 3, Axis::Width), 400.0);
    assert!(tree.length_runtime.is_none());
}

#[test]
fn font_load_at_completion_replays_old_typefaces_and_publishes_new_metrics() {
    let assets = crate::assets::AssetRuntime::new();
    let _assets = assets.enter();
    let mut tree = fixture(Length::Content, Length::Fill, Axis::Width);
    let node = tree.get_mut(&id(2)).unwrap();
    node.spec.kind = ElementKind::Text;
    node.spec.declared.content = Some("Deadline font 123".into());
    node.spec.declared.font = Some(crate::tree::attrs::Font::String("deadline-font".into()));
    node.spec.declared.font_size = Some(24.0);
    let mut rt = AnimationRuntime::default();
    let start = Instant::now();
    tick(&mut tree, &mut rt, start, 0, 1.0);
    tick(&mut tree, &mut rt, start, 999_999, 1.0);
    let old = capture_axis(&tree, &id(2), Axis::Width).unwrap();
    let bytes = std::fs::read(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../priv/test_assets/Lobster-Regular.ttf"),
    )
    .unwrap();
    crate::services::load_font_bytes(&assets, "deadline-font", 400, false, &bytes).unwrap();
    tick(&mut tree, &mut rt, start, 1_000_000, 1.0);
    let new = capture_axis(&tree, &id(2), Axis::Width).unwrap();
    close(new.visible, 300.0);
    assert_ne!(new.intrinsic, old.intrinsic);
    assert_eq!(
        tree.frame_metrics_epoch,
        Some(crate::renderer::live_font_cache_generation())
    );
    assert!(tree.length_runtime.is_none());
}

#[test]
fn image_dimensions_at_completion_use_old_facts_and_reflow_the_other_axis() {
    struct Images(std::cell::Cell<(u32, u32)>);
    impl TextMeasurer for Images {
        fn measure_with_font(&self, _: &str, _: f32, _: &str, _: u16, _: bool) -> (f32, f32) {
            (0.0, 0.0)
        }
        fn font_metrics(&self, _: f32, _: &str, _: u16, _: bool) -> (f32, f32) {
            (0.0, 0.0)
        }
        fn image_dimensions(
            &self,
            _: &crate::tree::attrs::ImageSource,
            load: bool,
        ) -> Option<(u32, u32)> {
            assert!(!load);
            Some(self.0.get())
        }
    }
    let images = Images(std::cell::Cell::new((200, 100)));
    let mut tree = fixture(Length::Px(40.0), Length::Fill, Axis::Width);
    let node = tree.get_mut(&id(2)).unwrap();
    node.spec.kind = ElementKind::Image;
    node.spec.declared.image_src =
        Some(crate::tree::attrs::ImageSource::Id("changing-size".into()));
    let mut rt = AnimationRuntime::default();
    let start = Instant::now();
    for us in [0, 999_999, 1_000_000] {
        if us == 1_000_000 {
            images.0.set((300, 100));
        }
        let now = start + Duration::from_micros(us);
        rt.sync_with_tree(&tree, now);
        try_layout_tree_with_animation(
            &mut tree,
            Constraint::new(600.0, 600.0),
            1.0,
            &mut rt,
            now,
            &images,
            &FontContext::default(),
        )
        .unwrap();
    }
    close(extent(&tree, 2, Axis::Width), 300.0);
    close(extent(&tree, 2, Axis::Height), 100.0);
    assert!(tree.length_runtime.is_none());
}

#[test]
fn dependent_content_fill_scopes_share_native_targets_and_release_together() {
    for axis in [Axis::Width, Axis::Height] {
        for child_duration in [1000.0, 2000.0] {
            let mut tree = fixture(Length::Px(200.0), Length::Content, axis);
            tree.insert(Element::with_attrs(
                id(4),
                ElementKind::El,
                vec![],
                Attrs {
                    animate: Some(spec(Length::Px(40.0), Length::Fill, child_duration, axis)),
                    ..Default::default()
                },
            ));
            tree.set_children(&id(2), vec![id(4)]).unwrap();
            let mut rt = AnimationRuntime::default();
            let start = Instant::now();
            for us in [0, 500_000, 999_000, 1_000_000] {
                tick(&mut tree, &mut rt, start, us, 1.0);
                close(
                    extent(&tree, 2, axis),
                    200.0 * (1.0 - us as f32 / 1_000_000.0),
                );
                close(
                    extent(&tree, 4, axis),
                    40.0 * (1.0 - us as f32 / (child_duration as f32 * 1000.0)),
                );
            }
            if child_duration > 1000.0 {
                assert!(!rt.groups.is_empty());
                tick(&mut tree, &mut rt, start, 1_500_000, 1.0);
                close(extent(&tree, 2, axis), 0.0);
                close(extent(&tree, 4, axis), 10.0);
                tick(&mut tree, &mut rt, start, 2_000_000, 1.0);
            }
            assert!(tree.length_runtime.is_none());
            assert!(rt.groups.is_empty());
        }
    }
}

#[test]
fn exit_ghosts_remap_sampled_descendant_scopes_and_preserve_captured_pixel_units() {
    use crate::tree::patch::{Patch, apply_patches};
    for scale in [0.5, 1.0, 2.0] {
        let mut tree = fixture(Length::Px(40.0), Length::Fill, Axis::Width);
        let node = tree.get_mut(&id(2)).unwrap();
        node.spec.kind = ElementKind::Row;
        node.spec.declared.animate = None;
        node.spec.declared.width = Some(Length::Px(200.0));
        node.spec.declared.animate_exit = Some(AnimationSpec {
            keyframes: vec![
                Attrs {
                    alpha: Some(1.0),
                    ..Default::default()
                },
                Attrs {
                    alpha: Some(0.0),
                    ..Default::default()
                },
            ],
            duration_ms: 1000.0,
            curve: AnimationCurve::Linear,
            repeat: AnimationRepeat::Once,
        });
        tree.insert(Element::with_attrs(
            id(4),
            ElementKind::El,
            vec![],
            Attrs {
                animate: Some(spec(
                    Length::FillWeighted(1.0),
                    Length::FillWeighted(3.0),
                    1000.0,
                    Axis::Width,
                )),
                on_click: Some(true),
                ..Default::default()
            },
        ));
        tree.insert(Element::with_attrs(
            id(5),
            ElementKind::El,
            vec![],
            Attrs {
                width: Some(Length::Fill),
                ..Default::default()
            },
        ));
        tree.set_children(&id(2), vec![id(4), id(5)]).unwrap();
        let mut rt = AnimationRuntime::default();
        let start = Instant::now();
        tick(&mut tree, &mut rt, start, 0, scale);
        tick(&mut tree, &mut rt, start, 500_000, scale);
        let before = [id(2), id(4), id(5)].map(|id| tree.get(&id).unwrap().layout.frame.unwrap());
        apply_patches(&mut tree, vec![Patch::Remove { id: id(2) }]).unwrap();
        let root = tree
            .iter_nodes()
            .find(|node| node.is_ghost_root())
            .unwrap()
            .id;
        let children = tree.child_ids(&root);
        tick(&mut tree, &mut rt, start, 500_000, scale);
        let after =
            [root, children[0], children[1]].map(|id| tree.get(&id).unwrap().layout.frame.unwrap());
        assert_eq!(before, after, "scale {scale}");
        assert!(
            tree.get(&children[0])
                .unwrap()
                .layout
                .effective
                .on_click
                .is_none()
        );
        tick(&mut tree, &mut rt, start, 750_000, scale);
        assert_eq!(
            after,
            [root, children[0], children[1]].map(|id| tree.get(&id).unwrap().layout.frame.unwrap())
        );
    }
}

#[test]
fn exit_dimension_animation_starts_from_the_published_mixed_footprint() {
    use crate::tree::patch::{Patch, apply_patches};
    for axis in [Axis::Width, Axis::Height] {
        for scale in [0.5, 1.0, 2.0] {
            let mut tree = fixture(Length::FillWeighted(1.0), Length::FillWeighted(3.0), axis);
            tree.get_mut(&id(2)).unwrap().spec.declared.animate_exit =
                Some(spec(Length::Px(40.0), Length::Px(0.0), 1000.0, axis));
            let mut rt = AnimationRuntime::default();
            let start = Instant::now();
            tick(&mut tree, &mut rt, start, 0, scale);
            tick(&mut tree, &mut rt, start, 500_000, scale);
            close(extent(&tree, 2, axis), 375.0 * scale);
            apply_patches(&mut tree, vec![Patch::Remove { id: id(2) }]).unwrap();
            let ghost = tree
                .iter_nodes()
                .find(|node| node.is_ghost_root())
                .unwrap()
                .id;
            tick(&mut tree, &mut rt, start, 500_000, scale);
            let frame = tree.get(&ghost).unwrap().layout.frame.unwrap();
            close(
                if axis == Axis::Width {
                    frame.width
                } else {
                    frame.height
                },
                375.0 * scale,
            );
            tick(&mut tree, &mut rt, start, 1_000_000, scale);
            let frame = tree.get(&ghost).unwrap().layout.frame.unwrap();
            close(
                if axis == Axis::Width {
                    frame.width
                } else {
                    frame.height
                },
                187.5 * scale,
            );
            close(extent(&tree, 3, axis), 412.5 * scale);
            tick(&mut tree, &mut rt, start, 1_500_000, scale);
            assert!(tree.get(&ghost).is_none());
        }
    }
}

#[test]
fn removal_uses_incoming_exit_policy_not_deferred_visual_policy() {
    use crate::tree::patch::{Patch, apply_patches};
    for clear in [false, true] {
        let mut tree = fixture(Length::Px(40.0), Length::Fill, Axis::Width);
        tree.get_mut(&id(2)).unwrap().spec.declared.animate_exit =
            Some(spec(Length::Px(40.0), Length::Px(0.0), 1000.0, Axis::Width));
        let mut rt = AnimationRuntime::default();
        let start = Instant::now();
        tick(&mut tree, &mut rt, start, 0, 1.0);
        tick(&mut tree, &mut rt, start, 500_000, 1.0);
        tree.get_mut(&id(2)).unwrap().spec.declared.animate_exit =
            (!clear).then(|| spec(Length::Px(40.0), Length::Px(0.0), 2000.0, Axis::Width));
        apply_patches(&mut tree, vec![Patch::Remove { id: id(2) }]).unwrap();
        let ghost = tree.iter_nodes().find(|node| node.is_ghost_root());
        if clear {
            assert!(ghost.is_none());
        } else {
            assert_eq!(
                ghost
                    .unwrap()
                    .lifecycle
                    .ghost_exit_animation
                    .as_ref()
                    .unwrap()
                    .duration_ms,
                2000.0
            );
        }
    }
}

#[test]
fn exit_capture_does_not_hide_an_unsupported_declared_endpoint() {
    use crate::tree::patch::{Patch, apply_patches};
    let mut tree = fixture(Length::Px(40.0), Length::Fill, Axis::Width);
    let mut rt = AnimationRuntime::default();
    let start = Instant::now();
    tick(&mut tree, &mut rt, start, 0, 1.0);
    tree.get_mut(&id(2)).unwrap().spec.declared.animate_exit = Some(spec(
        Length::Min(Box::new(Length::Px(40.0)), Box::new(Length::Fill)),
        Length::Px(0.0),
        1000.0,
        Axis::Width,
    ));
    assert!(apply_patches(&mut tree, vec![Patch::Remove { id: id(2) }]).is_err());
    assert!(tree.get(&id(2)).is_some());
    assert!(!tree.iter_nodes().any(|node| node.is_ghost()));
}

#[test]
fn direct_pixel_and_paint_exits_do_not_retain_dimension_sources() {
    use crate::tree::patch::{Patch, apply_patches};
    for paint_only in [false, true] {
        let mut tree = fixture(Length::Px(40.0), Length::Fill, Axis::Width);
        let node = tree.get_mut(&id(2)).unwrap();
        node.spec.declared.animate = None;
        node.spec.declared.width = Some(Length::Px(40.0));
        let mut exit = spec(Length::Px(40.0), Length::Px(0.0), 1000.0, Axis::Width);
        if paint_only {
            exit.keyframes = vec![
                Attrs {
                    alpha: Some(1.0),
                    ..Default::default()
                },
                Attrs {
                    alpha: Some(0.0),
                    ..Default::default()
                },
            ];
        }
        node.spec.declared.animate_exit = Some(exit);
        let mut rt = AnimationRuntime::default();
        let start = Instant::now();
        tick(&mut tree, &mut rt, start, 0, 1.0);
        apply_patches(&mut tree, vec![Patch::Remove { id: id(2) }]).unwrap();
        let ghost = tree
            .iter_nodes()
            .find(|node| node.is_ghost_root())
            .unwrap()
            .id;
        tick(&mut tree, &mut rt, start, 0, 1.0);
        assert!(rt.exit_entry(&ghost).unwrap().source.is_none());
        assert!(tree.length_runtime.is_none());
    }
}

mod content;

mod continuation;

mod rendered;

mod transport;
