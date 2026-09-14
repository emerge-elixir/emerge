use super::support::{AttemptError, Driver, Event, Injection, Mode};
use super::*;
use crate::tree::animation::{change::Field, fields::FieldMask};

fn entering(axis: Axis) -> ElementTree {
    let mut tree = fixture(Length::Px(40.0), Length::Fill, axis);
    let node = tree.get_mut(&id(2)).unwrap();
    node.lifecycle.mounted_at_revision = 1;
    let mut enter = node.spec.declared.animate.take().unwrap();
    enter.keyframes[0].alpha = Some(0.0);
    enter.keyframes[1].alpha = Some(1.0);
    node.spec.declared.animate_enter = Some(enter);
    let mut regular = spec(Length::Fill, Length::Px(40.0), 1000.0, axis);
    regular.keyframes[0].alpha = Some(1.0);
    regular.keyframes[1].alpha = Some(0.2);
    node.spec.declared.animate = Some(regular);
    let other = tree.get_mut(&id(3)).unwrap();
    other.spec.declared.animate = Some(spec(Length::Px(40.0), Length::Fill, 2000.0, axis));
    tree
}
fn near(a: f32, b: f32) {
    assert!((a - b).abs() < 0.002, "{a} != {b}");
}
#[test]
fn regular_paint_hands_off_while_geometry_waits_then_uses_its_own_admitted_clock() {
    for axis in [Axis::Width, Axis::Height] {
        for mode in [Mode::Full, Mode::Active, Mode::Dirty] {
            let start = Instant::now();
            let mut d = Driver::new(entering(axis), start, mode);
            for (us, width, alpha) in [
                (0, 40.0, 0.0),
                (1_000_000, 300.0, 1.0),
                (1_500_000, 300.0, 0.6),
                (2_000_000, 300.0, 0.2),
                (2_500_000, 170.0, 0.2),
                (3_000_000, 40.0, 0.2),
            ] {
                assert!(d.step(us, vec![]).result.is_ok());
                near(capture_axis(&d.tree, &id(2), axis).unwrap().visible, width);
                near(
                    d.tree.get(&id(2)).unwrap().layout.effective.alpha.unwrap() as f32,
                    alpha,
                );
            }
            let entry = d.runtime.animate_entries.get(&id(2)).unwrap();
            assert_eq!(entry.clock.started_at, start + Duration::from_secs(1));
            assert_eq!(entry.handoffs.len(), 1);
            assert_eq!(
                entry.handoffs[0].clock.started_at,
                start + Duration::from_secs(2)
            );
            assert!(entry.pending.is_empty());
            assert!(d.runtime.groups.is_empty());
            assert!(d.tree.length_runtime.is_none());
        }
    }
}
#[test]
fn rejected_regular_handoff_retries_the_same_spec_source_and_clock() {
    let start = Instant::now();
    let mut d = Driver::new(entering(Axis::Width), start, Mode::Full);
    assert!(d.step(0, vec![]).result.is_ok());
    let result = d.attempt(1_000_000, vec![], Injection::BeforeLayout);
    assert_eq!(result.result, Err(AttemptError::BeforeLayout));
    assert!(d.runtime.animate_entries.committed(&id(2)).is_none());
    let admitted = d.runtime.animate_entries.get(&id(2)).unwrap().clone();
    assert_eq!(admitted.fields, FieldMask::field(Field::Alpha));
    assert_eq!(admitted.pending, FieldMask::field(Field::Width));
    assert!(d.step(1_250_000, vec![]).result.is_ok());
    let retry = d.runtime.animate_entries.committed(&id(2)).unwrap();
    assert_eq!(retry.generation, admitted.generation);
    assert_eq!(retry.clock.started_at, admitted.clock.started_at);
    assert!(Arc::ptr_eq(&retry.spec, &admitted.spec));
    assert!(Arc::ptr_eq(
        retry.source.as_ref().unwrap(),
        admitted.source.as_ref().unwrap()
    ));
    near(
        d.tree.get(&id(2)).unwrap().layout.effective.alpha.unwrap() as f32,
        0.8,
    );
}
#[test]
fn reversing_an_unpublished_regular_replacement_restores_all_selected_run_clocks() {
    let start = Instant::now();
    let mut d = Driver::new(entering(Axis::Width), start, Mode::Full);
    for us in [0, 1_000_000, 2_000_000, 2_250_000] {
        assert!(d.step(us, vec![]).result.is_ok());
    }
    let old = d.runtime.animate_entries.committed(&id(2)).unwrap().clone();
    let attrs = d.attrs(id(2));
    let mut changed = attrs.clone();
    changed.animate.as_mut().unwrap().keyframes[1].alpha = Some(0.4);
    assert!(
        d.attempt(
            2_250_000,
            vec![Event::Attrs(id(2), Box::new(changed))],
            Injection::BeforeLayout
        )
        .result
        .is_err()
    );
    assert!(
        d.step(2_500_000, vec![Event::Attrs(id(2), Box::new(attrs))])
            .result
            .is_ok()
    );
    let restored = d.runtime.animate_entries.committed(&id(2)).unwrap();
    assert!(Arc::ptr_eq(&old.spec, &restored.spec));
    assert_eq!(old.clock.started_at, restored.clock.started_at);
    assert!(Arc::ptr_eq(&old.handoffs[0], &restored.handoffs[0]));
    near(
        capture_axis(&d.tree, &id(2), Axis::Width).unwrap().visible,
        170.0,
    );
}

#[test]
fn skipped_regular_handoffs_are_bounded_and_completed_sources_are_released() {
    for repeat in [
        AnimationRepeat::Once,
        AnimationRepeat::Times(500),
        AnimationRepeat::Loop,
    ] {
        let start = Instant::now();
        let mut tree = entering(Axis::Width);
        tree.get_mut(&id(2))
            .unwrap()
            .spec
            .declared
            .animate
            .as_mut()
            .unwrap()
            .repeat = repeat.clone();
        let mut d = Driver::new(tree, start, Mode::Full);
        for us in [0, 1_000_000, 2_000_000] {
            assert!(d.step(us, vec![]).result.is_ok());
        }
        let source = Arc::downgrade(
            d.runtime.animate_entries.get(&id(2)).unwrap().handoffs[0]
                .source
                .as_ref()
                .unwrap(),
        );
        let output = d.step(1_002_500_000, vec![]);
        assert!(output.result.is_ok(), "{:?}", output.result);
        assert!(
            output.inspection.queries.len() <= 3,
            "skips cannot replay every cycle"
        );
        let entry = d.runtime.animate_entries.get(&id(2)).unwrap();
        assert_eq!(entry.handoffs.len(), 1);
        assert!(entry.source.is_none());
        assert!(entry.handoffs[0].source.is_none());
        assert!(source.upgrade().is_none());
        if !matches!(repeat, AnimationRepeat::Loop) {
            assert!(d.tree.length_runtime.is_none());
        }
    }
}

#[test]
fn cancelling_pending_regular_geometry_drops_the_old_spec_without_waiting_for_release() {
    let start = Instant::now();
    let mut d = Driver::new(entering(Axis::Width), start, Mode::Full);
    for us in [0, 1_000_000] {
        assert!(d.step(us, vec![]).result.is_ok());
    }
    let old = Arc::downgrade(&d.runtime.animate_entries.get(&id(2)).unwrap().spec);
    let mut attrs = d.attrs(id(2));
    attrs.animate = None;
    assert!(
        d.step(1_250_000, vec![Event::Attrs(id(2), Box::new(attrs))])
            .result
            .is_ok()
    );
    assert!(old.upgrade().is_none());
    assert!(d.runtime.animate_entries.get(&id(2)).is_none());
    near(
        capture_axis(&d.tree, &id(2), Axis::Width).unwrap().visible,
        300.0,
    );
}

#[test]
fn regular_field_handoff_supports_cold_multisegment_sampling_without_replaying_boundaries() {
    for repeat in [
        AnimationRepeat::Once,
        AnimationRepeat::Times(500),
        AnimationRepeat::Loop,
    ] {
        let start = Instant::now();
        let mut tree = entering(Axis::Width);
        let regular = tree
            .get_mut(&id(2))
            .unwrap()
            .spec
            .declared
            .animate
            .as_mut()
            .unwrap();
        regular.keyframes.push(regular.keyframes[0].clone());
        regular.repeat = repeat;
        let mut d = Driver::new(tree, start, Mode::Full);
        for us in [0, 1_000_000, 2_000_000] {
            assert!(d.step(us, vec![]).result.is_ok());
        }
        let skipped = d.step(2_750_000, vec![]);
        assert!(skipped.result.is_ok(), "{:?}", skipped.result);
        near(
            capture_axis(&d.tree, &id(2), Axis::Width).unwrap().visible,
            170.0,
        );
        assert!(skipped.inspection.queries.len() <= 3);
    }
}
