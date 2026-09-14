//! Commit-identity tests, not late-context continuity or full entrypoint qualification.
use super::*;
use crate::tree::layout::{
    layout_and_refresh_prepared_default_reusing_clean_registry, prepare_frame_attrs_for_update,
};

fn stage_release(tree: &mut ElementTree, rt: &mut AnimationRuntime, now: Instant) {
    let sync = rt.sync_with_tree(tree, now);
    assert!(sync.completed.preparation_error.is_none());
    let prep = prepare_frame_attrs_for_update(tree, 1.0, Some(rt), Some(now));
    assert!(prep.animation_result.preparation_error.is_none());
    let prep = prep.apply(tree, Some(rt)).unwrap();
    layout_and_refresh_prepared_default_reusing_clean_registry(
        tree,
        Constraint::new(600.0, 600.0),
        prep,
        None,
    );
    assert!(
        tree.length_runtime
            .as_ref()
            .unwrap()
            .pending_release
            .is_some()
    );
    assert_eq!(rt.groups.len(), 1);
}

fn ready() -> (ElementTree, AnimationRuntime, Instant) {
    let mut tree = fixture(Length::Px(40.0), Length::Fill, Axis::Width);
    let mut rt = AnimationRuntime::default();
    let start = Instant::now();
    tick(&mut tree, &mut rt, start, 0, 1.0);
    let now = start + Duration::from_secs(1);
    stage_release(&mut tree, &mut rt, now);
    (tree, rt, now)
}

#[test]
fn same_time_change_replacements_get_new_generations_and_keep_the_presented_source() {
    let mut tree = fixture(Length::Px(40.0), Length::Fill, Axis::Width);
    tree.get_mut(&id(2)).unwrap().spec.declared.animate = None;
    tree.get_mut(&id(2)).unwrap().spec.declared.width = Some(Length::Px(40.0));
    let mut rt = AnimationRuntime::default();
    let start = Instant::now();
    tick(&mut tree, &mut rt, start, 0, 1.0);
    change_target(&mut tree, Length::Fill, 1000.0);
    tick(&mut tree, &mut rt, start, 0, 1.0);
    tick(&mut tree, &mut rt, start, 500_000, 1.0);
    close(extent(&tree, 2, Axis::Width), 170.0);
    change_target(&mut tree, Length::FillWeighted(3.0), 1000.0);
    tick(&mut tree, &mut rt, start, 500_000, 1.0);
    let first = tree.length_runtime.as_ref().unwrap().tracks[&(id(2), Axis::Width)].key;
    let target = Length::Fill;
    change_target(&mut tree, target.clone(), 1000.0);
    tick(&mut tree, &mut rt, start, 500_000, 1.0);
    let second = tree.length_runtime.as_ref().unwrap().tracks[&(id(2), Axis::Width)].key;
    assert_eq!(first.started, second.started);
    assert!(second.generation.0 > first.generation.0);
    close(extent(&tree, 2, Axis::Width), 170.0);
    let generation = rt.last_generation;
    change_target(&mut tree, target, 2000.0);
    tick(&mut tree, &mut rt, start, 500_000, 1.0);
    assert_eq!(
        rt.last_generation, generation,
        "timing-only rerender is not a new run"
    );
    tick(&mut tree, &mut rt, start, 1_000_000, 1.0);
    close(extent(&tree, 2, Axis::Width), 235.0);
    close(
        capture_axis(&tree, &id(2), Axis::Width).unwrap().charge,
        235.0,
    );
}

#[test]
fn separately_presented_regular_restarts_at_the_same_time_do_not_alias() {
    let mut tree = fixture(Length::Px(40.0), Length::Fill, Axis::Width);
    let original = tree.get(&id(2)).unwrap().spec.declared.animate.clone();
    let mut rt = AnimationRuntime::default();
    let start = Instant::now();
    tick(&mut tree, &mut rt, start, 0, 1.0);
    let first = tree.length_runtime.as_ref().unwrap().tracks[&(id(2), Axis::Width)].key;
    tree.get_mut(&id(2)).unwrap().spec.declared.animate = Some(spec(
        Length::Px(40.0),
        Length::FillWeighted(3.0),
        1000.0,
        Axis::Width,
    ));
    tree.set_revision(tree.revision() + 1);
    tick(&mut tree, &mut rt, start, 0, 1.0);
    let second = tree.length_runtime.as_ref().unwrap().tracks[&(id(2), Axis::Width)].key;
    tree.get_mut(&id(2)).unwrap().spec.declared.animate = original;
    tree.set_revision(tree.revision() + 1);
    tick(&mut tree, &mut rt, start, 0, 1.0);
    let third = tree.length_runtime.as_ref().unwrap().tracks[&(id(2), Axis::Width)].key;
    assert_eq!(first.started, third.started);
    assert_eq!(first.revision, third.revision);
    assert!(first.generation.0 < second.generation.0 && second.generation.0 < third.generation.0);
    assert_ne!(first, third);
}

#[test]
fn ready_receipt_rejects_each_changed_frame_input_without_retiring_tracks() {
    for mutation in 0..5 {
        let (mut tree, mut rt, _) = ready();
        let key = tree.length_runtime.as_ref().unwrap().tracks[&(id(2), Axis::Width)].key;
        match mutation {
            0 => tree.set_revision(tree.revision() + 1),
            1 => tree.layout_model_epoch += 1,
            2 => tree.animation_constraint = Some(Constraint::new(800.0, 600.0)),
            3 => tree.set_current_scale(2.0),
            _ => tree.get_mut(&id(1)).unwrap().lifecycle.mounted_at_revision += 1,
        }
        assert_eq!(
            rt.finish_prepared_frame(&mut tree),
            Err(ProjectionError::StalePreparation)
        );
        assert_eq!(rt.groups.len(), 1);
        assert_eq!(
            tree.length_runtime.as_ref().unwrap().tracks[&(id(2), Axis::Width)].key,
            key
        );
    }
}

#[test]
fn ready_receipt_rejects_an_advanced_clock_but_allows_identical_sync() {
    let (mut tree, mut rt, now) = ready();
    rt.sync_with_tree(&tree, now + Duration::from_millis(1));
    assert_eq!(
        rt.finish_prepared_frame(&mut tree),
        Err(ProjectionError::StalePreparation)
    );
    assert_eq!(rt.groups.len(), 1);
    // A new preparation is required for the advanced frame, not a recovery resize.
    stage_release(&mut tree, &mut rt, now + Duration::from_millis(1));
    rt.sync_with_tree(&tree, now + Duration::from_millis(1));
    assert_eq!(rt.finish_prepared_frame(&mut tree), Ok(false));
    assert!(tree.length_runtime.is_none());
    assert_eq!(rt.finish_prepared_frame(&mut tree), Ok(false));
}

#[test]
fn a_ready_receipt_cannot_commit_in_a_cloned_runtime_or_geometry_workspace() {
    let (mut tree, mut rt, _) = ready();
    let mut other = rt.clone();
    assert_eq!(
        other.finish_prepared_frame(&mut tree),
        Err(ProjectionError::StalePreparation)
    );
    assert_eq!(rt.groups.len(), 1);
    assert_eq!(other.groups.len(), 1);
    assert_eq!(rt.finish_prepared_frame(&mut tree), Ok(false));

    let (mut tree, mut rt, _) = ready();
    let receipt = tree
        .length_runtime
        .as_mut()
        .unwrap()
        .take_release()
        .unwrap();
    let mut cloned = tree.clone();
    assert!(
        cloned
            .length_runtime
            .as_ref()
            .unwrap()
            .pending_release
            .is_none()
    );
    cloned.length_runtime.as_mut().unwrap().pending_release = Some(receipt);
    assert_eq!(
        rt.finish_prepared_frame(&mut cloned),
        Err(ProjectionError::StalePreparation)
    );
    assert_eq!(rt.groups.len(), 1);
}

#[test]
fn a_delayed_receipt_cannot_retire_a_new_same_node_change_or_its_source() {
    let mut tree = fixture(Length::Px(40.0), Length::Fill, Axis::Width);
    tree.get_mut(&id(2)).unwrap().spec.declared.animate = None;
    tree.get_mut(&id(2)).unwrap().spec.declared.width = Some(Length::Px(40.0));
    let mut rt = AnimationRuntime::default();
    let start = Instant::now();
    tick(&mut tree, &mut rt, start, 0, 1.0);
    change_target(&mut tree, Length::Fill, 1000.0);
    tick(&mut tree, &mut rt, start, 0, 1.0);
    let now = start + Duration::from_secs(1);
    stage_release(&mut tree, &mut rt, now);
    let old = rt.changes[&(id(2), change::Field::Width)].generation;
    change_target(&mut tree, Length::FillWeighted(3.0), 1000.0);
    rt.sync_with_tree(&tree, now);
    let entry = &rt.changes[&(id(2), change::Field::Width)];
    let generation = entry.generation;
    let source = Arc::clone(&entry.source);
    assert!(generation.0 > old.0);
    assert_eq!(
        rt.finish_prepared_frame(&mut tree),
        Err(ProjectionError::StalePreparation)
    );
    assert_eq!(
        rt.changes[&(id(2), change::Field::Width)].generation,
        generation
    );
    assert!(Arc::ptr_eq(
        &source,
        &rt.changes[&(id(2), change::Field::Width)].source
    ));
    assert_eq!(rt.groups.len(), 1);
}

#[test]
fn a_changed_track_terminal_or_generation_cannot_be_committed_by_an_old_receipt() {
    for generation in [false, true] {
        let (mut tree, mut rt, _) = ready();
        let track = Arc::make_mut(
            tree.length_runtime
                .as_mut()
                .unwrap()
                .tracks
                .get_mut(&(id(2), Axis::Width))
                .unwrap(),
        );
        if generation {
            track.key.generation.0 += 1;
        } else {
            track.to.charge += 1.0;
        }
        assert_eq!(
            rt.finish_prepared_frame(&mut tree),
            Err(ProjectionError::StalePreparation)
        );
        assert_eq!(rt.groups.len(), 1);
        assert_eq!(tree.length_runtime.as_ref().unwrap().tracks.len(), 1);
    }
}

#[test]
fn generation_exhaustion_is_typed_and_does_not_wrap_or_prepare_a_frame() {
    let mut tree = fixture(Length::Px(40.0), Length::Fill, Axis::Width);
    let mut rt = AnimationRuntime {
        last_generation: u64::MAX - 1,
        ..Default::default()
    };
    let now = Instant::now();
    tick(&mut tree, &mut rt, now, 0, 1.0);
    assert_eq!(
        rt.animate_entries[&id(2)].generation,
        RunGeneration(u64::MAX)
    );
    assert!(
        rt.sync_with_tree(&tree, now)
            .completed
            .preparation_error
            .is_none()
    );
    let before = tree.get(&id(2)).unwrap().layout.dimension_samples.clone();
    tree.get_mut(&id(2)).unwrap().spec.declared.animate = Some(spec(
        Length::Px(40.0),
        Length::FillWeighted(3.0),
        1000.0,
        Axis::Width,
    ));
    tree.set_revision(tree.revision() + 1);
    assert_eq!(
        rt.sync_with_tree(&tree, now).completed.preparation_error,
        Some(ProjectionError::GenerationExhausted)
    );
    assert_eq!(rt.last_generation, u64::MAX);
    assert_eq!(
        try_layout_tree_with_animation(
            &mut tree,
            Constraint::new(600.0, 600.0),
            1.0,
            &mut rt,
            now,
            &SkiaTextMeasurer,
            &FontContext::default()
        ),
        Err(ProjectionError::GenerationExhausted)
    );
    assert_eq!(tree.get(&id(2)).unwrap().layout.dimension_samples, before);
}

#[test]
fn exhausted_handoff_capacity_preserves_the_entire_ready_group() {
    let mut tree = fixture(Length::Px(40.0), Length::Fill, Axis::Width);
    let node = tree.get_mut(&id(2)).unwrap();
    node.lifecycle.mounted_at_revision = 1;
    node.spec.declared.animate_enter = node.spec.declared.animate.take();
    node.spec.declared.animate = Some(spec(Length::Fill, Length::Px(40.0), 1000.0, Axis::Width));
    let mut rt = AnimationRuntime {
        last_generation: u64::MAX - 1,
        ..Default::default()
    };
    let start = Instant::now();
    tick(&mut tree, &mut rt, start, 0, 1.0);
    let generation = rt.enter_entries[&id(2)].generation;
    let before = tree.get(&id(2)).unwrap().layout.dimension_samples.clone();
    let now = start + Duration::from_secs(1);
    rt.sync_with_tree(&tree, now);
    let prep = prepare_frame_attrs_for_update(&mut tree, 1.0, Some(&mut rt), Some(now));
    assert_eq!(
        prep.animation_result.preparation_error,
        Some(ProjectionError::GenerationExhausted)
    );
    assert_eq!(tree.get(&id(2)).unwrap().layout.dimension_samples, before);
    assert_eq!(rt.finish_prepared_frame(&mut tree), Ok(false));
    assert_eq!(rt.groups.len(), 1);
    assert_eq!(rt.enter_entries[&id(2)].generation, generation);
    assert!(rt.animate_entries.is_empty());
    assert!(
        tree.length_runtime
            .as_ref()
            .unwrap()
            .pending_release
            .is_none()
    );
}

#[test]
fn generation_exhaustion_preserves_a_completed_enter_during_model_sync() {
    let mut tree = fixture(Length::Px(40.0), Length::Px(80.0), Axis::Width);
    let node = tree.get_mut(&id(2)).unwrap();
    node.lifecycle.mounted_at_revision = 1;
    node.spec.declared.animate_enter = node.spec.declared.animate.take();
    node.spec.declared.animate = Some(spec(
        Length::Px(80.0),
        Length::Px(40.0),
        1000.0,
        Axis::Width,
    ));
    let mut rt = AnimationRuntime {
        last_generation: u64::MAX - 1,
        ..Default::default()
    };
    let start = Instant::now();
    tick(&mut tree, &mut rt, start, 0, 1.0);
    tree.set_revision(tree.revision() + 1);
    let result = rt.sync_with_tree(&tree, start + Duration::from_secs(1));
    assert_eq!(
        result.completed.preparation_error,
        Some(ProjectionError::GenerationExhausted)
    );
    assert_eq!(rt.enter_entries[&id(2)].generation, RunGeneration(u64::MAX));
    assert!(rt.animate_entries.is_empty());
}

#[test]
fn presentation_anchoring_preserves_run_generation_but_invalidates_a_prepared_clock() {
    let mut tree = fixture(Length::Px(40.0), Length::Fill, Axis::Width);
    let node = tree.get_mut(&id(2)).unwrap();
    node.lifecycle.mounted_at_revision = 1;
    node.spec.declared.animate_enter = node.spec.declared.animate.take();
    let mut rt = AnimationRuntime::default();
    let start = Instant::now();
    tick(&mut tree, &mut rt, start, 0, 1.0);
    let generation = rt.enter_entries[&id(2)].generation;
    let now = start + Duration::from_secs(1);
    stage_release(&mut tree, &mut rt, now);
    rt.anchor_pending_transient_entries_to_present(now);
    assert_eq!(rt.enter_entries[&id(2)].generation, generation);
    assert_eq!(
        rt.finish_prepared_frame(&mut tree),
        Err(ProjectionError::StalePreparation)
    );
    assert_eq!(rt.groups.len(), 1);
}
