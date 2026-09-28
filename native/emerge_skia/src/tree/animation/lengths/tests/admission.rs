use super::support::*;
use super::*;

fn change_event(d: &Driver, target: Length, ms: f64) -> Event {
    let mut attrs = d.attrs(id(2));
    attrs.width = Some(target);
    attrs.animate_change = Some(Arc::new(vec![change::ChangePolicy {
        field: change::Field::Width,
        duration_ms: ms,
        curve: AnimationCurve::Linear,
    }]));
    Event::Attrs(id(2), Box::new(attrs))
}
fn viewport(width: f32) -> Event {
    Event::Viewport {
        width,
        height: 600.0,
        scale: 1.0,
    }
}
fn changing(mode: Mode) -> Driver {
    let mut tree = fixture(Length::Px(40.0), Length::Fill, Axis::Width);
    tree.get_mut(&id(1)).unwrap().spec.declared.width = Some(Length::Fill);
    let attrs = &mut tree.get_mut(&id(2)).unwrap().spec.declared;
    attrs.animate = None;
    attrs.width = Some(Length::Px(40.0));
    let mut d = Driver::new(tree, Instant::now(), mode);
    assert!(d.step(0, vec![]).result.is_ok());
    let start = change_event(&d, Length::Fill, 2000.0);
    assert!(d.step(0, vec![start]).result.is_ok());
    assert!(d.step(500_000, vec![]).result.is_ok());
    close(extent(&d.tree, 2, Axis::Width), 105.0);
    d
}
fn fail_b(d: &mut Driver) -> RunGeneration {
    let before = d
        .runtime
        .changes
        .committed(&(id(2), change::Field::Width))
        .unwrap()
        .generation;
    let b = change_event(d, Length::FillWeighted(3.0), 1000.0);
    let failed = d.step(500_000, vec![b, viewport(f32::NAN)]);
    assert_eq!(
        failed.result,
        Err(AttemptError::Native(ProjectionError::InvalidContext))
    );
    assert_eq!(failed.before_queries, failed.after_prepare);
    failed
        .published_before
        .unwrap()
        .assert_same(&failed.published_after.unwrap());
    assert_eq!(
        d.runtime
            .changes
            .committed(&(id(2), change::Field::Width))
            .unwrap()
            .generation,
        before
    );
    assert_eq!(d.runtime.changes.record_counts(), (1, 1));
    let candidate = d.runtime.changes[&(id(2), change::Field::Width)].generation;
    assert_ne!(before, candidate);
    candidate
}
#[test]
fn failed_change_b_then_a_restores_committed_generation_clock_and_source() {
    for mode in [Mode::Full, Mode::Active, Mode::Dirty] {
        let mut d = changing(mode);
        let original = d.runtime.changes[&(id(2), change::Field::Width)].clone();
        fail_b(&mut d);
        let a = change_event(&d, Length::Fill, 9000.0);
        let recovered = d.step(750_000, vec![a, viewport(600.0)]);
        assert!(recovered.result.is_ok());
        let now = &d.runtime.changes[&(id(2), change::Field::Width)];
        assert_eq!(now.generation, original.generation);
        assert_eq!(now.started_at, original.started_at);
        assert!(Arc::ptr_eq(&now.source, &original.source));
        assert!(Arc::ptr_eq(&now.spec, &original.spec));
        assert_eq!(d.runtime.changes.record_counts(), (1, 0));
        close(extent(&d.tree, 2, Axis::Width), 137.5);
    }
}
#[test]
fn retrying_failed_b_keeps_its_clock_and_source_instead_of_restarting() {
    let mut d = changing(Mode::Full);
    let generation = fail_b(&mut d);
    let candidate = d.runtime.changes[&(id(2), change::Field::Width)].clone();
    let result = d.step(750_000, vec![viewport(600.0)]);
    assert!(result.result.is_ok());
    let current = &d.runtime.changes[&(id(2), change::Field::Width)];
    assert_eq!(current.generation, generation);
    assert_eq!(current.started_at, candidate.started_at);
    assert!(Arc::ptr_eq(&current.source, &candidate.source));
    assert_eq!(d.runtime.changes.record_counts(), (1, 0));
    close(extent(&d.tree, 2, Axis::Width), 191.25);
}
#[test]
fn failed_b_then_c_uses_the_first_published_source_and_discards_b() {
    let mut d = changing(Mode::Dirty);
    let b_generation = fail_b(&mut d);
    let rejected = Arc::downgrade(&d.runtime.changes[&(id(2), change::Field::Width)].spec);
    let c = change_event(&d, Length::FillWeighted(2.0), 1000.0);
    assert!(d.step(750_000, vec![c, viewport(600.0)]).result.is_ok());
    let entry = &d.runtime.changes[&(id(2), change::Field::Width)];
    assert!(entry.generation.0 > b_generation.0);
    close(entry.source.dimensions[0].unwrap().visible, 105.0);
    close(extent(&d.tree, 2, Axis::Width), 105.0);
    assert!(rejected.upgrade().is_none());
    assert_eq!(d.runtime.changes.record_counts(), (1, 0));
    assert!(d.step(1_250_000, vec![]).result.is_ok());
    close(extent(&d.tree, 2, Axis::Width), 252.5);
}
#[test]
fn coalesced_a_b_a_does_not_allocate_an_unnecessary_generation() {
    let mut d = changing(Mode::Full);
    let generation = d.runtime.last_generation;
    let b = change_event(&d, Length::FillWeighted(3.0), 1000.0);
    let a = change_event(&d, Length::Fill, 2000.0);
    assert!(d.step(750_000, vec![b, a]).result.is_ok());
    assert_eq!(d.runtime.last_generation, generation);
    assert_eq!(d.runtime.changes.record_counts(), (1, 0));
    close(extent(&d.tree, 2, Axis::Width), 137.5);
}
#[test]
fn published_b_then_a_is_a_new_interruption_not_a_restore() {
    let mut d = changing(Mode::Full);
    let original = d.runtime.changes[&(id(2), change::Field::Width)].generation;
    let b = change_event(&d, Length::FillWeighted(3.0), 1000.0);
    assert!(d.step(500_000, vec![b]).result.is_ok());
    assert!(d.step(750_000, vec![]).result.is_ok());
    let published_b = d.runtime.changes[&(id(2), change::Field::Width)].generation;
    let a = change_event(&d, Length::Fill, 1000.0);
    assert!(d.step(750_000, vec![a]).result.is_ok());
    let current = &d.runtime.changes[&(id(2), change::Field::Width)];
    assert!(current.generation.0 > published_b.0 && published_b.0 > original.0);
    close(current.source.dimensions[0].unwrap().visible, 191.25);
    close(extent(&d.tree, 2, Axis::Width), 191.25);
}
#[test]
fn a_cancelled_unpublished_first_run_leaves_no_idle_history() {
    let mut tree = fixture(Length::Px(40.0), Length::Fill, Axis::Width);
    let attrs = &mut tree.get_mut(&id(2)).unwrap().spec.declared;
    attrs.animate = None;
    attrs.width = Some(Length::Px(40.0));
    let mut d = Driver::new(tree, Instant::now(), Mode::Full);
    d.step(0, vec![]);
    let b = change_event(&d, Length::Fill, 1000.0);
    assert!(d.step(0, vec![b, viewport(f32::NAN)]).result.is_err());
    assert_eq!(d.runtime.changes.record_counts(), (0, 1));
    let a = change_event(&d, Length::Px(40.0), 1000.0);
    assert!(d.step(0, vec![a, viewport(600.0)]).result.is_ok());
    assert_eq!(d.runtime.changes.record_counts(), (0, 0));
    assert!(d.runtime.groups.is_empty());
    assert!(d.tree.length_runtime.is_none());
}
#[test]
fn failed_regular_replacement_can_restore_the_original_run() {
    let tree = fixture(Length::Px(40.0), Length::Fill, Axis::Width);
    let mut d = Driver::new(tree, Instant::now(), Mode::Full);
    d.step(0, vec![]);
    d.step(500_000, vec![]);
    let original = d.runtime.animate_entries[&id(2)].clone();
    let a = d.attrs(id(2));
    let mut b = a.clone();
    b.animate = Some(spec(
        Length::Px(40.0),
        Length::FillWeighted(3.0),
        1000.0,
        Axis::Width,
    ));
    assert!(
        d.step(
            500_000,
            vec![Event::Attrs(id(2), Box::new(b)), viewport(f32::NAN)]
        )
        .result
        .is_err()
    );
    assert_eq!(
        d.runtime
            .animate_entries
            .committed(&id(2))
            .unwrap()
            .generation,
        original.generation
    );
    assert!(
        d.step(
            750_000,
            vec![Event::Attrs(id(2), Box::new(a)), viewport(600.0)]
        )
        .result
        .is_ok()
    );
    assert_eq!(
        d.runtime.animate_entries[&id(2)].generation,
        original.generation
    );
    assert_eq!(
        d.runtime.animate_entries[&id(2)].clock.started_at,
        original.clock.started_at
    );
    close(extent(&d.tree, 2, Axis::Width), 235.0);
}
#[test]
fn cancellation_does_not_retire_committed_owner_until_successful_frame() {
    let mut d = changing(Mode::Full);
    let original = d.runtime.changes[&(id(2), change::Field::Width)].clone();
    let mut attrs = d.attrs(id(2));
    attrs.animate_change = None;
    let failed = d.attempt(
        750_000,
        vec![Event::Attrs(id(2), Box::new(attrs))],
        Injection::BeforeLayout,
    );
    assert!(failed.result.is_err());
    assert!(d.runtime.changes.is_empty());
    assert_eq!(
        d.runtime
            .changes
            .committed(&(id(2), change::Field::Width))
            .unwrap()
            .generation,
        original.generation
    );
    assert_eq!(d.runtime.changes.record_counts(), (1, 1));
    assert!(d.step(750_000, vec![]).result.is_ok());
    assert_eq!(d.runtime.changes.record_counts(), (0, 0));
}

#[test]
fn a_regular_remount_with_an_identical_spec_does_not_restore_the_old_clock() {
    let mut d = Driver::new(
        fixture(Length::Px(40.0), Length::Fill, Axis::Width),
        Instant::now(),
        Mode::Full,
    );
    d.step(0, vec![]);
    d.step(500_000, vec![]);
    let before = d.runtime.animate_entries[&id(2)].generation;
    // A new mount is not an unpublished target reversal on the old mount.
    let attrs = d.attrs(id(2));
    let mut node = Element::with_attrs(id(2), ElementKind::El, vec![], attrs);
    node.lifecycle.mounted_at_revision = 2;
    d.tree.insert(node);
    d.tree.set_revision(2);
    d.tree.layout_model_epoch += 1;
    d.tree.mark_all_measure_dirty();
    assert!(d.step(500_000, vec![]).result.is_ok());
    assert!(d.runtime.animate_entries[&id(2)].generation.0 > before.0);
    close(extent(&d.tree, 2, Axis::Width), 40.0);
}

#[test]
fn retry_of_same_target_keeps_a_staged_enter_handoff_on_its_existing_generation() {
    let mut tree = fixture(Length::Px(40.0), Length::Fill, Axis::Width);
    let width = spec(Length::Px(40.0), Length::Fill, 2000.0, Axis::Width);
    tree.get_mut(&id(3)).unwrap().spec.declared.animate = Some(width);
    let node = tree.get_mut(&id(2)).unwrap();
    node.lifecycle.mounted_at_revision = 1;
    node.spec.declared.animate = None;
    node.spec.declared.width = Some(Length::Px(40.0));
    node.spec.declared.alpha = Some(1.0);
    node.spec.declared.animate_enter = Some(AnimationSpec {
        keyframes: vec![
            Attrs {
                alpha: Some(0.0),
                ..Default::default()
            },
            Attrs {
                alpha: Some(1.0),
                ..Default::default()
            },
        ],
        duration_ms: 1000.0,
        curve: AnimationCurve::Linear,
        repeat: AnimationRepeat::Once,
    });
    let mut d = Driver::new(tree, Instant::now(), Mode::Full);
    d.step(0, vec![]);
    let mut target = d.attrs(id(2));
    target.alpha = Some(0.4);
    target.animate_change = Some(Arc::new(vec![change::ChangePolicy {
        field: change::Field::Alpha,
        duration_ms: 1000.0,
        curve: AnimationCurve::Linear,
    }]));
    assert!(
        d.step(500_000, vec![Event::Attrs(id(2), Box::new(target.clone()))])
            .result
            .is_ok()
    );
    let key = (id(2), change::Field::Alpha);
    assert!(d.runtime.changes.committed(&key).unwrap().pending);
    // Leave a same-target first-write source pending across the failed handoff.
    assert!(
        d.step(
            1_000_000,
            vec![Event::Attrs(id(2), Box::new(target)), viewport(f32::NAN)]
        )
        .result
        .is_err()
    );
    let handoff = d.runtime.changes[&key].clone();
    assert!(!handoff.pending);
    assert!(d.runtime.changes.committed(&key).unwrap().pending);
    assert!(d.step(1_000_000, vec![viewport(600.0)]).result.is_ok());
    let current = &d.runtime.changes[&key];
    assert!(!current.pending);
    assert_eq!(current.started_at, handoff.started_at);
    assert_eq!(current.generation, handoff.generation);
    assert!(Arc::ptr_eq(&current.spec, &handoff.spec));
    assert_eq!(
        d.tree.get(&id(2)).unwrap().layout.effective.alpha,
        Some(1.0)
    );
}

#[test]
fn restoring_a_completed_committed_owner_restores_its_hold_not_an_idle_declaration() {
    let mut d = changing(Mode::Full);
    let original = d.runtime.changes[&(id(2), change::Field::Width)].generation;
    fail_b(&mut d);
    let a = change_event(&d, Length::Fill, 2000.0);
    let result = d.step(2_000_000, vec![a, viewport(800.0)]);
    assert!(result.result.is_ok(), "{:?}", result.result);
    assert_eq!(
        result.owners_after_sync.changes[&(id(2), change::Field::Width)].generation,
        original
    );
    assert_eq!(
        result.owners_after_sync.groups, 1,
        "restored ownership is validated before release"
    );
    assert_eq!(d.runtime.changes.record_counts(), (0, 0));
    assert!(d.runtime.groups.is_empty());
    assert!(d.tree.length_runtime.is_none());
}

fn held_enter_with_pending_width_and_paint() -> Driver {
    let mut tree = fixture(Length::Px(40.0), Length::Fill, Axis::Width);
    tree.get_mut(&id(3)).unwrap().spec.declared.animate =
        Some(spec(Length::Px(40.0), Length::Fill, 2000.0, Axis::Width));
    let node = tree.get_mut(&id(2)).unwrap();
    node.lifecycle.mounted_at_revision = 1;
    node.spec.declared.animate = None;
    node.spec.declared.width = Some(Length::Px(40.0));
    node.spec.declared.alpha = Some(1.0);
    node.spec.declared.animate_enter = Some(AnimationSpec {
        keyframes: vec![
            Attrs {
                width: Some(Length::Px(40.0)),
                alpha: Some(0.0),
                ..Default::default()
            },
            Attrs {
                width: Some(Length::Fill),
                alpha: Some(1.0),
                ..Default::default()
            },
        ],
        duration_ms: 1000.0,
        curve: AnimationCurve::Linear,
        repeat: AnimationRepeat::Once,
    });
    let mut d = Driver::new(tree, Instant::now(), Mode::Full);
    assert!(d.step(0, vec![]).result.is_ok());
    let mut target = d.attrs(id(2));
    target.width = Some(Length::Px(200.0));
    target.alpha = Some(0.2);
    target.animate_change = Some(Arc::new(
        [change::Field::Width, change::Field::Alpha]
            .into_iter()
            .map(|field| change::ChangePolicy {
                field,
                duration_ms: 1000.0,
                curve: AnimationCurve::Linear,
            })
            .collect(),
    ));
    assert!(
        d.step(500_000, vec![Event::Attrs(id(2), Box::new(target))])
            .result
            .is_ok()
    );
    d
}

#[test]
fn held_enter_hands_off_paint_while_width_remains_pending_on_the_same_shared_spec() {
    let mut d = held_enter_with_pending_width_and_paint();
    let enter_spec = Arc::clone(&d.runtime.enter_entries.get(&id(2)).unwrap().spec);
    let width = (id(2), change::Field::Width);
    let alpha = (id(2), change::Field::Alpha);
    assert!(d.runtime.changes[&width].pending && d.runtime.changes[&alpha].pending);
    assert!(d.step(1_000_000, vec![]).result.is_ok());
    assert!(d.runtime.changes[&width].pending);
    assert!(!d.runtime.changes[&alpha].pending);
    assert_eq!(d.runtime.changes[&alpha].spec.keyframes[0].alpha, Some(1.0));
    assert!(Arc::ptr_eq(
        &enter_spec,
        &d.runtime.enter_entries.get(&id(2)).unwrap().spec
    ));
    close(extent(&d.tree, 2, Axis::Width), 300.0);
    assert!(d.step(1_500_000, vec![]).result.is_ok());
    close(
        d.tree.get(&id(2)).unwrap().layout.effective.alpha.unwrap() as f32,
        0.6,
    );
    close(extent(&d.tree, 2, Axis::Width), 300.0);
    assert!(d.step(2_000_000, vec![]).result.is_ok());
    assert!(!d.runtime.changes[&width].pending);
    assert!(d.runtime.changes.get(&alpha).is_none());
    assert!(d.runtime.enter_entries.get(&id(2)).is_none());
    assert!(d.step(2_500_000, vec![]).result.is_ok());
    close(extent(&d.tree, 2, Axis::Width), 250.0);
    assert!(d.step(3_000_000, vec![]).result.is_ok());
    close(extent(&d.tree, 2, Axis::Width), 200.0);
    assert!(d.runtime.changes.is_empty());
}

#[test]
fn rejected_field_handoff_preserves_committed_pending_masks_and_retry_clock() {
    let mut d = held_enter_with_pending_width_and_paint();
    let alpha = (id(2), change::Field::Alpha);
    let width = (id(2), change::Field::Width);
    let failed = d.attempt(1_000_000, vec![], super::support::Injection::BeforeLayout);
    assert!(failed.result.is_err());
    assert_eq!(failed.before_queries, failed.after_prepare);
    assert!(d.runtime.changes.committed(&alpha).unwrap().pending);
    assert!(!d.runtime.changes[&alpha].pending);
    let clock = d.runtime.changes[&alpha].started_at;
    let generation = d.runtime.changes[&alpha].generation;
    assert!(d.step(1_250_000, vec![]).result.is_ok());
    assert_eq!(clock, d.runtime.changes[&alpha].started_at);
    assert_eq!(generation, d.runtime.changes[&alpha].generation);
    assert!(d.runtime.changes[&width].pending);
    close(
        d.tree.get(&id(2)).unwrap().layout.effective.alpha.unwrap() as f32,
        0.8,
    );
}
