//! Application authority and rejection preservation, including frames with no release.
use super::support::{AttemptError, Driver, Event, Injection, Mode, poses};
use super::*;
use crate::tree::layout::{layout_and_refresh_prepared_default, prepare_frame_attrs_for_update};

fn moving() -> (Driver, Instant) {
    let start = Instant::now();
    let mut d = Driver::new(
        fixture(Length::Px(40.0), Length::Fill, Axis::Width),
        start,
        Mode::Full,
    );
    assert!(d.step(0, vec![]).result.is_ok());
    assert!(d.step(500_000, vec![]).result.is_ok());
    (d, start)
}

#[test]
fn rejected_moving_and_releasing_frames_preserve_pose_sources_and_committed_tracks_in_all_modes() {
    for mode in [Mode::Full, Mode::Active, Mode::Dirty] {
        for at in [750_000, 1_000_000] {
            let (mut d, _) = moving();
            d.mode = mode;
            let before = poses(&d.tree);
            let tracks = d.tree.length_runtime.as_ref().unwrap().tracks.clone();
            let mut attrs = d.attrs(id(2));
            attrs.alpha = Some(0.25);
            let rejected = d.attempt(
                at,
                vec![Event::Attrs(id(2), Box::new(attrs))],
                Injection::BeforeLayout,
            );
            assert_eq!(rejected.result, Err(AttemptError::BeforeLayout));
            assert_eq!(before, rejected.after_prepare);
            assert_eq!(rejected.sources_after_prepare.len(), 1);
            assert!(tracks.iter().all(|(key, old)| Arc::ptr_eq(
                old,
                &d.tree.length_runtime.as_ref().unwrap().tracks[key]
            )));
            rejected
                .published_before
                .as_ref()
                .unwrap()
                .assert_same(rejected.published_after.as_ref().unwrap());
            let recovery = d.step(at, vec![]);
            assert!(recovery.result.is_ok());
            assert!(d.tree.pending_patch_effects.sources.is_empty());
        }
    }
}

#[test]
fn changed_frame_inputs_reject_before_application_including_nonrelease_frames() {
    for mutation in 0..8 {
        let (mut d, start) = moving();
        let now = start + Duration::from_millis(750);
        d.runtime.sync_with_tree(&d.tree, now);
        let prep =
            prepare_frame_attrs_for_update(&mut d.tree, 1.0, Some(&mut d.runtime), Some(now));
        match mutation {
            0 => d.tree.set_revision(d.tree.revision() + 1),
            1 => d.tree.layout_model_epoch += 1,
            2 => d.tree.animation_constraint = Some(Constraint::new(800.0, 600.0)),
            3 => d.tree.set_current_scale(2.0),
            4 => {
                d.runtime
                    .sync_with_tree(&d.tree, now + Duration::from_millis(1));
            }
            5 => {
                d.tree
                    .get_mut(&id(1))
                    .unwrap()
                    .lifecycle
                    .mounted_at_revision += 1;
            }
            6 => {
                d.runtime.last_generation += 1;
            }
            _ => {
                d.runtime.synced_sample_time = Some(now + Duration::from_millis(1));
            }
        }
        let before = poses(&d.tree);
        assert!(matches!(
            prep.apply(&mut d.tree, Some(&d.runtime)),
            Err(ProjectionError::StalePreparation)
        ));
        assert_eq!(before, poses(&d.tree));
    }
}

#[test]
fn preparations_cannot_cross_tree_runtime_or_workspace_identity() {
    for other in 0..3 {
        let (mut d, start) = moving();
        let now = start + Duration::from_millis(750);
        d.runtime.sync_with_tree(&d.tree, now);
        let prep =
            prepare_frame_attrs_for_update(&mut d.tree, 1.0, Some(&mut d.runtime), Some(now));
        match other {
            0 => d.tree = d.tree.clone(),
            1 => d.runtime = d.runtime.clone(),
            _ => d.tree.length_runtime = d.tree.length_runtime.clone(),
        }
        let before = poses(&d.tree);
        assert!(matches!(
            prep.apply(&mut d.tree, Some(&d.runtime)),
            Err(ProjectionError::StalePreparation)
        ));
        assert_eq!(before, poses(&d.tree));
    }
}

#[test]
fn a_new_preparation_invalidates_an_old_one_without_mutating_presentation() {
    let (mut d, start) = moving();
    let now = start + Duration::from_millis(750);
    d.runtime.sync_with_tree(&d.tree, now);
    let before = poses(&d.tree);
    let first = prepare_frame_attrs_for_update(&mut d.tree, 1.0, Some(&mut d.runtime), Some(now));
    let second = prepare_frame_attrs_for_update(&mut d.tree, 1.0, Some(&mut d.runtime), Some(now));
    assert!(matches!(
        first.apply(&mut d.tree, Some(&d.runtime)),
        Err(ProjectionError::StalePreparation)
    ));
    assert_eq!(before, poses(&d.tree));
    let applied = second.apply(&mut d.tree, Some(&d.runtime)).unwrap();
    layout_and_refresh_prepared_default(&mut d.tree, Constraint::new(600.0, 600.0), applied);
    d.runtime.finish_prepared_frame(&mut d.tree).unwrap();
}

#[test]
fn nonrelease_sources_and_tracks_commit_only_after_native_output_and_identical_sync_is_valid() {
    let (mut d, start) = moving();
    let now = start + Duration::from_millis(750);
    d.tree.capture_animation_source(&id(2));
    d.runtime.sync_with_tree(&d.tree, now);
    let tracks = d.tree.length_runtime.as_ref().unwrap().tracks.clone();
    let prep = prepare_frame_attrs_for_update(&mut d.tree, 1.0, Some(&mut d.runtime), Some(now));
    d.runtime.sync_with_tree(&d.tree, now);
    let applied = prep.apply(&mut d.tree, Some(&d.runtime)).unwrap();
    assert!(!d.tree.pending_patch_effects.sources.is_empty());
    assert!(
        tracks.iter().all(|(key, old)| Arc::ptr_eq(
            old,
            &d.tree.length_runtime.as_ref().unwrap().tracks[key]
        ))
    );
    layout_and_refresh_prepared_default(&mut d.tree, Constraint::new(600.0, 600.0), applied);
    assert!(!d.tree.pending_patch_effects.sources.is_empty());
    d.runtime.finish_prepared_frame(&mut d.tree).unwrap();
    assert!(d.tree.pending_patch_effects.sources.is_empty());
}

#[test]
fn a_changed_committed_track_anchor_rejects_before_application() {
    let (mut d, start) = moving();
    let now = start + Duration::from_millis(750);
    d.runtime.sync_with_tree(&d.tree, now);
    let prep = prepare_frame_attrs_for_update(&mut d.tree, 1.0, Some(&mut d.runtime), Some(now));
    let old = d
        .tree
        .length_runtime
        .as_mut()
        .unwrap()
        .tracks
        .get_mut(&(id(2), Axis::Width))
        .unwrap();
    Arc::make_mut(old).anchor = 0.4;
    let before = poses(&d.tree);
    assert!(matches!(
        prep.apply(&mut d.tree, Some(&d.runtime)),
        Err(ProjectionError::StalePreparation)
    ));
    assert_eq!(before, poses(&d.tree));
}

#[test]
fn native_patch_preserves_published_effective_attrs_and_first_write_until_commit() {
    use crate::tree::patch::{Patch, apply_patches};
    let (mut d, _) = moving();
    let before = poses(&d.tree);
    let raw = [vec![0, 1, 1, 2], 90.0f64.to_be_bytes().to_vec()].concat();
    apply_patches(
        &mut d.tree,
        vec![Patch::SetAttrs {
            id: id(2),
            attrs_raw: raw,
        }],
    )
    .unwrap();
    assert_eq!(before, poses(&d.tree));
    let failed = d.attempt(750_000, vec![], Injection::BeforeLayout);
    assert_eq!(failed.result, Err(AttemptError::BeforeLayout));
    assert_eq!(before, failed.after_prepare);
    assert!(!d.tree.pending_patch_effects.sources.is_empty());
    assert!(d.step(750_000, vec![]).result.is_ok());
    assert!(d.tree.pending_patch_effects.sources.is_empty());
    close(extent(&d.tree, 2, Axis::Width), 90.0);
}

#[test]
fn admission_without_an_applied_frame_cannot_acknowledge_sources_or_commit_owners() {
    let (mut d, start) = moving();
    d.tree.capture_animation_source(&id(2));
    d.tree.get_mut(&id(2)).unwrap().spec.declared.animate = None;
    d.tree.set_revision(d.tree.revision() + 1);
    d.runtime
        .sync_with_tree(&d.tree, start + Duration::from_millis(750));
    assert!(d.runtime.animate_entries.committed(&id(2)).is_some());
    assert!(d.runtime.animate_entries.get(&id(2)).is_none());
    assert_eq!(d.runtime.finish_prepared_frame(&mut d.tree), Ok(false));
    assert!(d.runtime.animate_entries.committed(&id(2)).is_some());
    assert!(!d.tree.pending_patch_effects.sources.is_empty());
}

#[test]
fn mutable_public_entrypoints_advance_and_commit_without_external_sync() {
    use crate::tree::layout::{
        layout_and_refresh_default_with_animation, layout_or_refresh_default_with_animation,
        layout_tree_default_with_animation,
    };
    for entrypoint in 0..3 {
        let mut tree = fixture(Length::Px(40.0), Length::Fill, Axis::Width);
        let mut runtime = AnimationRuntime::default();
        let start = Instant::now();
        for (ms, width) in [(0, 40.0), (500, 170.0), (1000, 300.0)] {
            let now = start + Duration::from_millis(ms);
            let constraint = Constraint::new(600.0, 600.0);
            match entrypoint {
                0 => {
                    layout_and_refresh_default_with_animation(
                        &mut tree,
                        constraint,
                        1.0,
                        &mut runtime,
                        now,
                    )
                    .unwrap();
                }
                1 => {
                    layout_or_refresh_default_with_animation(
                        &mut tree,
                        constraint,
                        1.0,
                        &mut runtime,
                        now,
                    )
                    .unwrap();
                }
                _ => {
                    layout_tree_default_with_animation(
                        &mut tree,
                        constraint,
                        1.0,
                        &mut runtime,
                        now,
                    )
                    .unwrap();
                }
            }
            close(extent(&tree, 2, Axis::Width), width);
            assert!(!runtime.has_pending_admissions());
            assert!(tree.pending_patch_effects.sources.is_empty());
        }
        assert!(runtime.groups.is_empty());
        assert!(tree.length_runtime.is_none());
    }
}

#[test]
fn a_live_foreign_runtime_cannot_apply_another_owners_preparation() {
    let (mut d, start) = moving();
    let now = start + Duration::from_millis(750);
    d.runtime.sync_with_tree(&d.tree, now);
    let prep = prepare_frame_attrs_for_update(&mut d.tree, 1.0, Some(&mut d.runtime), Some(now));
    let other = d.runtime.clone();
    let before = poses(&d.tree);
    assert!(matches!(
        prep.apply(&mut d.tree, Some(&other)),
        Err(ProjectionError::StalePreparation)
    ));
    assert_eq!(before, poses(&d.tree));
}

#[test]
fn paint_only_preparation_has_the_same_authority_without_allocating_geometry_workspace() {
    let mut tree = fixture(Length::Px(40.0), Length::Fill, Axis::Width);
    let start = Instant::now();
    tree.get_mut(&id(2)).unwrap().spec.declared.animate = Some(AnimationSpec {
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
    let mut d = Driver::new(tree, start, Mode::Active);
    d.step(0, vec![]);
    let before = poses(&d.tree);
    let failed = d.attempt(500_000, vec![], Injection::BeforeLayout);
    assert_eq!(failed.result, Err(AttemptError::BeforeLayout));
    assert_eq!(before, failed.after_prepare);
    assert!(d.tree.length_runtime.is_none());
    assert!(d.step(500_000, vec![]).result.is_ok());
    close(
        d.tree.get(&id(2)).unwrap().layout.effective.alpha.unwrap() as f32,
        0.5,
    );
    assert!(d.tree.length_runtime.is_none());
}

#[test]
fn a_changed_release_ticket_rejects_before_samples_are_removed() {
    let (mut d, start) = moving();
    let now = start + Duration::from_millis(1000);
    d.runtime.sync_with_tree(&d.tree, now);
    let prep = prepare_frame_attrs_for_update(&mut d.tree, 1.0, Some(&mut d.runtime), Some(now));
    assert!(prep.animation_result.preparation_error.is_none());
    let before = poses(&d.tree);
    d.runtime.groups.commit(&d.runtime.groups.ready(now));
    assert!(matches!(
        prep.apply(&mut d.tree, Some(&d.runtime)),
        Err(ProjectionError::StalePreparation)
    ));
    assert_eq!(before, poses(&d.tree));
    assert!(
        d.tree
            .get(&id(2))
            .unwrap()
            .layout
            .dimension_samples
            .is_some()
    );
}

#[test]
fn unpublished_preparations_are_invalidated_by_runtime_and_scroll_access() {
    for mutation in 0..3 {
        let (mut d, start) = moving();
        let now = start + Duration::from_millis(750);
        d.runtime.sync_with_tree(&d.tree, now);
        let prep =
            prepare_frame_attrs_for_update(&mut d.tree, 1.0, Some(&mut d.runtime), Some(now));
        match mutation {
            0 => d.tree.get_mut(&id(2)).unwrap().layout.scroll_x = 12.0,
            1 => {
                d.tree.set_mouse_over_active(&id(2), true);
            }
            _ => {
                d.tree.iter_nodes_mut().next().unwrap().layout.scroll_y = 8.0;
            }
        }
        let before = poses(&d.tree);
        assert!(matches!(
            prep.publish(
                &mut d.tree,
                &mut d.runtime,
                Constraint::new(600.0, 600.0),
                true,
                None
            ),
            Err(ProjectionError::StalePreparation)
        ));
        assert_eq!(before, poses(&d.tree));
    }
}

#[test]
fn font_preparations_cannot_cross_renderer_asset_contexts() {
    let first = crate::assets::AssetRuntime::new();
    let second = crate::assets::AssetRuntime::new();
    let _first = first.enter();
    let (mut d, start) = moving();
    let now = start + Duration::from_millis(750);
    d.runtime.sync_with_tree(&d.tree, now);
    let prep = prepare_frame_attrs_for_update(&mut d.tree, 1.0, Some(&mut d.runtime), Some(now));
    let before = poses(&d.tree);
    {
        let _second = second.enter();
        assert!(matches!(
            prep.publish(
                &mut d.tree,
                &mut d.runtime,
                Constraint::new(600.0, 600.0),
                true,
                None
            ),
            Err(ProjectionError::StalePreparation)
        ));
    }
    assert_eq!(before, poses(&d.tree));
    assert!(d.step(750_000, vec![]).result.is_ok());
}

#[test]
fn event_owned_text_and_slider_values_keep_published_attrs_until_native_commit() {
    for slider in [false, true] {
        let start = Instant::now();
        let mut tree = fixture(Length::Px(40.0), Length::Fill, Axis::Width);
        let node = tree.get_mut(&id(2)).unwrap();
        node.spec.kind = if slider {
            crate::tree::element::ElementKind::Slider
        } else {
            crate::tree::element::ElementKind::TextInput
        };
        node.spec.declared.content = Some("before".into());
        node.spec.declared.slider_value = Some(0.0);
        let mut d = Driver::new(tree, start, Mode::Full);
        assert!(d.step(0, vec![]).result.is_ok());
        assert!(d.step(250_000, vec![]).result.is_ok());
        let before = poses(&d.tree);
        if slider {
            d.tree.set_slider_value(&id(2), 0.5);
        } else {
            d.tree.set_text_input_content(&id(2), "after".into());
        }
        assert_eq!(before, poses(&d.tree));
        assert_eq!(d.tree.pending_patch_effects.sources.len(), 1);
        let rejected = d.attempt(500_000, vec![], Injection::BeforeLayout);
        assert_eq!(rejected.result, Err(AttemptError::BeforeLayout));
        assert_eq!(before, rejected.after_prepare);
        assert!(d.step(500_000, vec![]).result.is_ok());
        let attrs = &d.tree.get(&id(2)).unwrap().layout.effective;
        if slider {
            assert_eq!(attrs.slider_value, Some(0.5));
        } else {
            assert_eq!(attrs.content.as_deref(), Some("after"));
        }
        assert!(d.tree.pending_patch_effects.sources.is_empty());
    }
}

#[test]
fn publication_rejects_an_unqueried_constraint_before_installing_samples() {
    let (mut d, start) = moving();
    let now = start + Duration::from_millis(750);
    d.runtime.sync_with_tree(&d.tree, now);
    let prep = prepare_frame_attrs_for_update(&mut d.tree, 1.0, Some(&mut d.runtime), Some(now));
    let before = poses(&d.tree);
    assert!(matches!(
        prep.publish(
            &mut d.tree,
            &mut d.runtime,
            Constraint::new(800.0, 600.0),
            true,
            None
        ),
        Err(ProjectionError::StalePreparation)
    ));
    assert_eq!(before, poses(&d.tree));
}

#[test]
fn refresh_requests_cannot_commit_geometry_without_native_layout() {
    let (mut d, start) = moving();
    let now = start + Duration::from_millis(750);
    d.runtime.sync_with_tree(&d.tree, now);
    let prep = prepare_frame_attrs_for_update(&mut d.tree, 1.0, Some(&mut d.runtime), Some(now));
    let constraint = d.tree.animation_constraint.unwrap();
    let (update, _) = prep
        .publish(&mut d.tree, &mut d.runtime, constraint, false, None)
        .unwrap();
    assert!(update.layout_performed);
    let published = capture_axis(&d.tree, &id(2), Axis::Width).unwrap();
    assert!((published.visible - 235.0).abs() < 0.002);
    crate::tree::layout::refresh_default_with_frame_attrs(
        &mut d.tree,
        1.0,
        Some(&mut d.runtime),
        Some(start + Duration::from_millis(1000)),
    )
    .unwrap();
    let terminal = capture_axis(&d.tree, &id(2), Axis::Width).unwrap();
    assert!((terminal.visible - 300.0).abs() < 0.002);
    assert!(d.tree.length_runtime.is_none());
}

#[test]
fn same_generation_candidate_mutations_invalidate_prepared_owner_authority() {
    let (mut d, start) = moving();
    let now = start + Duration::from_millis(750);
    d.runtime.sync_with_tree(&d.tree, now);
    let prep = prepare_frame_attrs_for_update(&mut d.tree, 1.0, Some(&mut d.runtime), Some(now));
    let before = poses(&d.tree);
    let generation = d.runtime.last_generation;
    d.runtime.animate_entries.get_mut(&id(2)).unwrap().fields =
        crate::tree::animation::fields::FieldMask::default();
    assert_eq!(d.runtime.last_generation, generation);
    let constraint = d.tree.animation_constraint.unwrap();
    assert!(matches!(
        prep.publish(&mut d.tree, &mut d.runtime, constraint, true, None),
        Err(ProjectionError::StalePreparation)
    ));
    assert_eq!(before, poses(&d.tree));
}

#[test]
fn a_cached_frame_on_an_unpublished_remount_is_not_an_implicit_change_source() {
    let (mut d, _) = moving();
    let revision = d.tree.revision() + 1;
    d.tree.set_revision(revision);
    let node = d.tree.get_mut(&id(2)).unwrap();
    node.lifecycle.mounted_at_revision = revision;
    node.spec.declared.animate = None;
    node.spec.declared.animate_change = Some(Arc::new(vec![
        crate::tree::animation::change::ChangePolicy {
            field: crate::tree::animation::change::Field::Width,
            duration_ms: 1000.0,
            curve: AnimationCurve::Linear,
        },
    ]));
    assert!(source::PresentationSource::capture(&d.tree, &id(2)).is_none());
    let mut attrs = d.attrs(id(2));
    attrs.width = Some(Length::Px(200.0));
    assert!(
        d.step(750_000, vec![Event::Attrs(id(2), Box::new(attrs))])
            .result
            .is_ok()
    );
    assert!(d.runtime.changes.is_empty());
    assert!((capture_axis(&d.tree, &id(2), Axis::Width).unwrap().visible - 200.0).abs() < 0.002);
    assert!(source::PresentationSource::capture(&d.tree, &id(2)).is_some());
}

#[test]
fn invalid_direct_ownership_keeps_successfully_admitted_prefix_clocks_on_retry() {
    let mut tree = fixture(Length::Px(40.0), Length::Fill, Axis::Width);
    tree.stamp_all_mounted_at_revision(1);
    let bad = tree.get_mut(&id(3)).unwrap();
    bad.spec.declared.animate = Some(spec(Length::Px(40.0), Length::Fill, 1000.0, Axis::Width));
    bad.spec.declared.animate_change = Some(Arc::new(vec![
        crate::tree::animation::change::ChangePolicy {
            field: crate::tree::animation::change::Field::Width,
            duration_ms: 1000.0,
            curve: AnimationCurve::Linear,
        },
    ]));
    let mut rt = AnimationRuntime::default();
    let now = Instant::now();
    assert_eq!(
        rt.sync_with_tree(&tree, now).completed.preparation_error,
        Some(ProjectionError::InvalidOwnership(id(3)))
    );
    let first = rt.animate_entries.get(&id(2)).unwrap().clone();
    assert!(rt.animate_entries.committed(&id(2)).is_none());
    tree.get_mut(&id(3)).unwrap().spec.declared.animate_change = None;
    let next = now + Duration::from_millis(250);
    assert!(
        rt.sync_with_tree(&tree, next)
            .completed
            .preparation_error
            .is_none()
    );
    let retained = rt.animate_entries.get(&id(2)).unwrap();
    assert_eq!(retained.generation, first.generation);
    assert_eq!(retained.clock.started_at, now);
    assert!(Arc::ptr_eq(&retained.spec, &first.spec));
}

#[test]
fn invalid_finite_and_loop_timing_is_rejected_before_native_application() {
    for duration in [0.0, -1.0, f64::NAN, f64::INFINITY, f64::MAX] {
        for repeat in [
            AnimationRepeat::Once,
            AnimationRepeat::Times(2),
            AnimationRepeat::Loop,
        ] {
            let mut tree = fixture(Length::Px(40.0), Length::Fill, Axis::Width);
            let spec = tree
                .get_mut(&id(2))
                .unwrap()
                .spec
                .declared
                .animate
                .as_mut()
                .unwrap();
            spec.duration_ms = duration;
            spec.repeat = repeat;
            let mut runtime = AnimationRuntime::default();
            assert_eq!(
                runtime
                    .sync_with_tree(&tree, Instant::now())
                    .completed
                    .preparation_error,
                Some(ProjectionError::InvalidTiming(id(2)))
            );
            assert!(tree.length_runtime.is_none());
            assert!(runtime.animate_entries.committed(&id(2)).is_none());
        }
    }
}
