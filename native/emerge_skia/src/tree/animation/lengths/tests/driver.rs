use super::super::inspection::QueryKind;
use super::support::*;
use super::*;

fn drawable(a: Length, b: Length) -> ElementTree {
    let mut tree = fixture(a, b, Axis::Width);
    tree.get_mut(&id(1)).unwrap().spec.declared.width = Some(Length::Fill);
    let node = tree.get_mut(&id(2)).unwrap();
    node.spec.declared.background = Some(Background::Color(Color::Rgba {
        r: 255,
        g: 0,
        b: 0,
        a: 255,
    }));
    node.spec.declared.on_click = Some(true);
    tree
}
fn fp(poses: &Poses, node: u64) -> AxisFootprint {
    poses[&id(node)].footprints[0].unwrap()
}
fn same_geometry(a: &Poses, b: &Poses) {
    assert_eq!(a.len(), b.len());
    for (id, pose) in a {
        assert_eq!(pose.frame, b[id].frame);
        assert_eq!(pose.scroll, b[id].scroll);
        for (a, b) in pose.footprints.into_iter().zip(b[id].footprints) {
            assert!(
                match (a, b) {
                    (Some(a), Some(b)) => a.release_matches(b),
                    (None, None) => true,
                    _ => false,
                },
                "{id:?}: {a:?} != {b:?}"
            );
        }
    }
}
#[test]
fn driver_full_active_dirty_replay_observes_the_same_real_release() {
    let epoch = Instant::now();
    let run = |mode| {
        let mut d = Driver::new(drawable(Length::Px(40.0), Length::Fill), epoch, mode);
        let cold = d.step(0, vec![]);
        assert!(cold.result.is_ok());
        assert_eq!(cold.inspection.queries.len(), 2);
        assert!(
            cold.inspection
                .queries
                .iter()
                .any(|q| q.kind == QueryKind::Source)
        );
        let warm = d.step(500_000, vec![]);
        assert!(warm.result.is_ok());
        assert_eq!(warm.mode, mode);
        assert_eq!(warm.inspection.before, warm.inspection.after);
        assert!(warm.inspection.queries.is_empty());
        close(fp(&warm.after_layout.clone().unwrap(), 2).visible, 170.0);
        let terminal = d.step(1_000_000, vec![]);
        assert_eq!(terminal.number, 3);
        assert_eq!(terminal.at, Duration::from_secs(1));
        assert_eq!(terminal.revision, 1);
        assert!(terminal.events.is_empty());
        assert_eq!(terminal.owners_after_sync.groups, 1);
        assert_eq!(terminal.inspection.releasing.len(), 1);
        assert_eq!(
            terminal.inspection.cached_targets[&(id(2), Axis::Width)].visible,
            300.0
        );
        assert_eq!(terminal.inspection.queries.len(), 1);
        assert_eq!(
            terminal.inspection.after.layout_queries,
            terminal.inspection.before.layout_queries + 1
        );
        assert_eq!(
            terminal.inspection.queries[0].result.as_ref().unwrap()[&(id(2), Axis::Width)].visible,
            300.0
        );
        assert!(terminal.inspection.context.is_some());
        same_geometry(
            terminal.with_samples.as_ref().unwrap(),
            terminal.without_samples.as_ref().unwrap(),
        );
        assert!(terminal.sources_after_prepare.is_empty());
        assert!(terminal.result.is_ok());
        let published = terminal.published_after.unwrap();
        assert_eq!(published.owners.groups, 0);
        assert!(!published.output.animations_active);
        assert!(!published.output.scene.nodes.is_empty());
        assert!(
            published
                .output
                .event_rebuild
                .base_registry
                .view()
                .iter_precedence()
                .next()
                .is_some()
        );
        published
    };
    let full = run(Mode::Full);
    full.assert_same(&run(Mode::Active));
    full.assert_same(&run(Mode::Dirty));
}

#[test]
fn driver_deadline_viewport_resize_releases_at_the_new_native_pose() {
    for mode in [Mode::Full, Mode::Active, Mode::Dirty] {
        for axis in [Axis::Width, Axis::Height] {
            for scale in [0.5, 1.0, 2.0] {
                let mut tree = fixture(Length::FillWeighted(1.0), Length::FillWeighted(3.0), axis);
                let root = &mut tree.get_mut(&id(1)).unwrap().spec.declared;
                root.width = Some(Length::Fill);
                root.height = Some(Length::Fill);
                let mut d = Driver::new(tree, Instant::now(), mode);
                d.step(0, vec![]);
                let before = d.step(999_999, vec![]).published_after.unwrap();
                let done = d.step(
                    1_000_000,
                    vec![Event::Viewport {
                        width: 800.0,
                        height: 800.0,
                        scale,
                    }],
                );
                assert!(done.result.is_ok(), "{:?}", done.result);
                assert_eq!(done.before_queries, done.after_prepare);
                let pose = done.after_layout.as_ref().unwrap();
                let index = if axis == Axis::Width { 0 } else { 1 };
                close(
                    pose[&id(2)].footprints[index].unwrap().visible * scale,
                    600.0,
                );
                close(
                    pose[&id(3)].footprints[index].unwrap().visible * scale,
                    200.0,
                );
                same_geometry(
                    done.with_samples.as_ref().unwrap(),
                    done.without_samples.as_ref().unwrap(),
                );
                assert_eq!(done.inspection.queries.len(), 2);
                assert_eq!(done.inspection.queries[1].kind, QueryKind::PreviousViewport);
                let published = done.published_after.unwrap();
                assert_eq!(published.sequence, before.sequence + 1);
                assert!(!published.output.animations_active);
                assert!(d.tree.length_runtime.is_none());
                assert_eq!(
                    d.tree.animation_constraint,
                    Some(Constraint::new(800.0, 800.0))
                );
            }
        }
    }
}

#[test]
fn viewport_release_rejects_corruption_and_failed_witness_retries_in_the_new_viewport() {
    for corruption in 0..3 {
        let corrupt = corruption != 0;
        let mut d = Driver::new(
            drawable(Length::FillWeighted(1.0), Length::FillWeighted(3.0)),
            Instant::now(),
            Mode::Dirty,
        );
        d.step(0, vec![]);
        let before = d.step(999_999, vec![]).published_after.unwrap();
        let original = d.tree.length_runtime.as_ref().unwrap().tracks[&(id(2), Axis::Width)].to;
        if corrupt {
            Arc::make_mut(
                d.tree
                    .length_runtime
                    .as_mut()
                    .unwrap()
                    .tracks
                    .get_mut(&(id(2), Axis::Width))
                    .unwrap(),
            )
            .to
            .charge -= if corruption == 1 { 20.0 } else { 0.0 };
            if corruption == 2 {
                Arc::make_mut(
                    d.tree
                        .length_runtime
                        .as_mut()
                        .unwrap()
                        .tracks
                        .get_mut(&(id(2), Axis::Width))
                        .unwrap(),
                )
                .to
                .policy
                .fill_request *= 0.5;
            }
        }
        let injection = if corrupt {
            Injection::None
        } else {
            Injection::Query(QueryKind::PreviousViewport)
        };
        let failed = d.attempt(
            1_000_000,
            vec![Event::Viewport {
                width: 800.0,
                height: 600.0,
                scale: 1.0,
            }],
            injection,
        );
        let expected = if corrupt {
            ProjectionError::ReleaseMismatch(id(2), Axis::Width)
        } else {
            ProjectionError::InjectedQuery
        };
        assert_eq!(failed.result, Err(AttemptError::Native(expected)));
        assert_eq!(failed.before_queries, failed.after_prepare);
        failed
            .published_after
            .as_ref()
            .unwrap()
            .assert_same(&before);
        if corrupt {
            Arc::make_mut(
                d.tree
                    .length_runtime
                    .as_mut()
                    .unwrap()
                    .tracks
                    .get_mut(&(id(2), Axis::Width))
                    .unwrap(),
            )
            .to = original;
        }
        let recovered = d.step(1_000_000, vec![]);
        assert!(recovered.result.is_ok(), "{:?}", recovered.result);
        close(
            fp(recovered.after_layout.as_ref().unwrap(), 2).visible,
            600.0,
        );
        assert_eq!(
            d.tree.animation_constraint,
            Some(Constraint::new(800.0, 600.0))
        );
    }
}

#[test]
fn driver_corrupt_charge_cannot_hide_behind_equal_visible_size_or_paint_patch() {
    let target = Length::FillWeighted(3.0);
    let mut d = Driver::new(
        drawable(Length::Px(40.0), target),
        Instant::now(),
        Mode::Dirty,
    );
    d.step(0, vec![]);
    d.step(999_999, vec![]);
    Arc::make_mut(
        d.tree
            .length_runtime
            .as_mut()
            .unwrap()
            .tracks
            .get_mut(&(id(2), Axis::Width))
            .unwrap(),
    )
    .to
    .charge -= 20.0;
    let mut attrs = d.attrs(id(2));
    attrs.alpha = Some(0.5);
    let failure = d.step(1_000_000, vec![Event::Attrs(id(2), Box::new(attrs))]);
    assert_eq!(
        failure.result,
        Err(AttemptError::Native(ProjectionError::ReleaseMismatch(
            id(2),
            Axis::Width
        )))
    );
    assert_eq!(failure.before_queries, failure.after_prepare);
    assert_eq!(failure.sources_after_prepare.len(), 1);
    let cached = fp(failure.with_samples.as_ref().unwrap(), 2);
    let native = fp(failure.without_samples.as_ref().unwrap(), 2);
    assert_eq!(cached.visible, native.visible);
    assert_ne!(cached.charge, native.charge);
    failure
        .published_before
        .as_ref()
        .unwrap()
        .assert_same(failure.published_after.as_ref().unwrap());
}

#[test]
fn driver_distinguishes_successful_preparation_from_publication() {
    let mut d = Driver::new(
        drawable(Length::Px(40.0), Length::Fill),
        Instant::now(),
        Mode::Full,
    );
    d.step(0, vec![]);
    d.step(500_000, vec![]);
    let rejected = d.attempt(1_000_000, vec![], Injection::BeforeLayout);
    assert_eq!(rejected.result, Err(AttemptError::BeforeLayout));
    assert!(rejected.after_layout.is_none());
    assert!(rejected.before_queries[&id(2)].sampled[0]);
    assert_eq!(rejected.before_queries, rejected.after_prepare);
    assert_eq!(
        rejected.published_before.as_ref().unwrap().pose,
        rejected.after_prepare
    );
    rejected
        .published_before
        .as_ref()
        .unwrap()
        .assert_same(rejected.published_after.as_ref().unwrap());
    let recovered = d.step(1_000_000, vec![]);
    assert!(recovered.result.is_ok());
    assert_eq!(
        recovered.published_after.unwrap().sequence,
        rejected.published_before.unwrap().sequence + 1
    );
}

#[test]
fn driver_injects_source_and_target_failures_without_publishing_the_candidate_owner() {
    for kind in [QueryKind::Source, QueryKind::Target] {
        let mut d = Driver::new(
            drawable(Length::Px(40.0), Length::Fill),
            Instant::now(),
            Mode::Full,
        );
        d.step(0, vec![]);
        d.step(250_000, vec![]);
        let original = d
            .runtime
            .animate_entries
            .committed(&id(2))
            .unwrap()
            .generation;
        let mut attrs = d.attrs(id(2));
        attrs.animate = Some(spec(
            Length::Px(40.0),
            Length::FillWeighted(3.0),
            1000.0,
            Axis::Width,
        ));
        let rejected = d.attempt(
            250_000,
            vec![Event::Attrs(id(2), Box::new(attrs))],
            Injection::Query(kind),
        );
        assert_eq!(
            rejected.result,
            Err(AttemptError::Native(ProjectionError::InjectedQuery))
        );
        assert_eq!(rejected.before_queries, rejected.after_prepare);
        assert!(rejected.after_layout.is_none());
        assert_eq!(rejected.inspection.queries.last().unwrap().kind, kind);
        assert!(
            rejected.inspection.after.layout_queries > rejected.inspection.before.layout_queries
        );
        assert_eq!(
            d.runtime
                .animate_entries
                .committed(&id(2))
                .unwrap()
                .generation,
            original
        );
        rejected
            .published_before
            .unwrap()
            .assert_same(&rejected.published_after.unwrap());
        assert_eq!(rejected.sources_after_prepare.len(), 1);
        assert!(d.step(250_000, vec![]).result.is_ok());
    }
}

#[test]
fn injected_release_failure_retains_the_same_publication_and_recovers_once() {
    let mut d = Driver::new(
        drawable(Length::Px(40.0), Length::Fill),
        Instant::now(),
        Mode::Dirty,
    );
    d.step(0, vec![]);
    d.step(500_000, vec![]);
    let rejected = d.attempt(1_000_000, vec![], Injection::Query(QueryKind::Release));
    assert_eq!(
        rejected.result,
        Err(AttemptError::Native(ProjectionError::InjectedQuery))
    );
    assert_eq!(rejected.before_queries, rejected.after_prepare);
    rejected
        .published_before
        .unwrap()
        .assert_same(&rejected.published_after.unwrap());
    assert_eq!(d.runtime.groups.len(), 1);
    assert!(d.step(1_000_000, vec![]).result.is_ok());
    assert_eq!(d.runtime.groups.len(), 0);
}

#[test]
fn declared_container_completion_edits_retry_without_restoring_the_old_model() {
    for mode in [Mode::Full, Mode::Active, Mode::Dirty] {
        for missing in [false, true] {
            for scale in [0.5, 1.0, 2.0] {
                let mut tree = drawable(Length::FillWeighted(1.0), Length::FillWeighted(3.0));
                tree.get_mut(&id(1)).unwrap().spec.declared.width = if missing {
                    None
                } else {
                    Some(Length::Px(600.0))
                };
                let mut d = Driver::new(tree, Instant::now(), mode);
                d.step(
                    0,
                    vec![Event::Viewport {
                        width: 600.0,
                        height: 600.0,
                        scale,
                    }],
                );
                let before = d.step(999_999, vec![]).published_after.unwrap();
                let mut attrs = d.attrs(id(1));
                attrs.width = Some(Length::Px(800.0));
                let failed = d.attempt(
                    1_000_000,
                    vec![Event::Attrs(id(1), Box::new(attrs))],
                    Injection::Query(QueryKind::PreviousModel),
                );
                assert_eq!(
                    failed.result,
                    Err(AttemptError::Native(ProjectionError::InjectedQuery))
                );
                failed.published_after.unwrap().assert_same(&before);
                let done = d.step(1_000_000, vec![]);
                assert!(done.result.is_ok(), "{:?}", done.result);
                same_geometry(
                    done.with_samples.as_ref().unwrap(),
                    done.without_samples.as_ref().unwrap(),
                );
                let native = done.after_layout.unwrap();
                close(fp(&native, 2).visible, 600.0);
                close(fp(&native, 3).visible, 200.0);
                close(native[&id(2)].frame.unwrap().width, 600.0 * scale);
                close(native[&id(3)].frame.unwrap().width, 200.0 * scale);
                assert_eq!(done.inspection.queries.len(), 2);
                assert!(d.tree.pending_patch_effects.sources.is_empty());
                assert!(d.tree.length_runtime.is_none());
            }
        }
    }
}

#[test]
fn model_preimages_cannot_hide_corrupted_terminal_reservations() {
    for policy in [false, true] {
        let mut d = Driver::new(
            drawable(Length::FillWeighted(1.0), Length::FillWeighted(3.0)),
            Instant::now(),
            Mode::Dirty,
        );
        d.step(0, vec![]);
        let before = d.step(999_999, vec![]).published_after.unwrap();
        let track = Arc::make_mut(
            d.tree
                .length_runtime
                .as_mut()
                .unwrap()
                .tracks
                .get_mut(&(id(2), Axis::Width))
                .unwrap(),
        );
        if policy {
            track.to.policy.fill_request *= 0.5;
        } else {
            track.to.charge -= 20.0;
        }
        let mut attrs = d.attrs(id(1));
        attrs.width = Some(Length::Px(800.0));
        let result = d.step(1_000_000, vec![Event::Attrs(id(1), Box::new(attrs))]);
        assert_eq!(
            result.result,
            Err(AttemptError::Native(ProjectionError::ReleaseMismatch(
                id(2),
                Axis::Width
            )))
        );
        result.published_after.unwrap().assert_same(&before);
    }
}

#[test]
fn model_release_requires_preimages_and_unchanged_published_topology() {
    for topology in [false, true] {
        let mut d = Driver::new(
            drawable(Length::FillWeighted(1.0), Length::FillWeighted(3.0)),
            Instant::now(),
            Mode::Full,
        );
        d.step(0, vec![]);
        let before = d.step(999_999, vec![]).published_after.unwrap();
        if topology {
            // Returning to the same child order is not a proof of unchanged
            // topology. A model-only witness must not transport allocation scopes.
            d.tree.set_children(&id(1), vec![id(3), id(2)]).unwrap();
            d.tree.set_children(&id(1), vec![id(2), id(3)]).unwrap();
        } else {
            // A caller that edits raw declarations without a first-write source
            // cannot reconstruct the previous native model at completion.
            d.tree.get_mut(&id(1)).unwrap().spec.declared.width = Some(Length::Px(800.0));
            d.tree.layout_model_epoch += 1;
        }
        let mut attrs = d.attrs(id(1));
        attrs.width = Some(Length::Px(800.0));
        let events = if topology {
            vec![Event::Attrs(id(1), Box::new(attrs))]
        } else {
            vec![]
        };
        let result = d.step(1_000_000, events);
        assert!(result.result.is_err());
        result.published_after.unwrap().assert_same(&before);
    }
}

#[test]
fn continuous_pixel_contexts_validate_cached_targets_and_retry_failed_witnesses() {
    for mode in [Mode::Full, Mode::Active, Mode::Dirty] {
        for corrupt in [false, true] {
            let mut tree = drawable(Length::Px(40.0), Length::Fill);
            tree.get_mut(&id(1)).unwrap().spec.declared.animate = Some(spec(
                Length::Px(600.0),
                Length::Px(800.0),
                2000.0,
                Axis::Width,
            ));
            let mut d = Driver::new(tree, Instant::now(), mode);
            d.step(0, vec![]);
            let before = d.step(500_000, vec![]).published_after.unwrap();
            if corrupt {
                Arc::make_mut(
                    d.tree
                        .length_runtime
                        .as_mut()
                        .unwrap()
                        .tracks
                        .get_mut(&(id(2), Axis::Width))
                        .unwrap(),
                )
                .to
                .charge -= 20.0;
            }
            let failed = d.attempt(
                1_000_000,
                vec![],
                if corrupt {
                    Injection::None
                } else {
                    Injection::Query(QueryKind::PriorTarget)
                },
            );
            assert_eq!(
                failed.result,
                Err(AttemptError::Native(if corrupt {
                    ProjectionError::ReleaseMismatch(id(2), Axis::Width)
                } else {
                    ProjectionError::InjectedQuery
                }))
            );
            failed.published_after.unwrap().assert_same(&before);
            if !corrupt {
                let recovered = d.step(1_000_000, vec![]);
                assert!(recovered.result.is_ok(), "{:?}", recovered.result);
                close(
                    fp(recovered.after_layout.as_ref().unwrap(), 2).visible,
                    350.0,
                );
                same_geometry(
                    recovered.with_samples.as_ref().unwrap(),
                    recovered.without_samples.as_ref().unwrap(),
                );
                assert_eq!(recovered.inspection.queries.len(), 2);
                assert!(d.tree.length_runtime.is_none());
            }
        }
    }
}

fn retargeted_hold(mode: Mode) -> Driver {
    let mut tree = sibling_fixture(Axis::Width);
    tree.get_mut(&id(1)).unwrap().spec.declared.width = Some(Length::Fill);
    tree.insert(Element::with_attrs(
        id(4),
        ElementKind::El,
        vec![],
        Attrs {
            width: Some(Length::Px(0.0)),
            height: Some(Length::Fill),
            ..Default::default()
        },
    ));
    tree.set_children(&id(1), vec![id(2), id(3), id(4)])
        .unwrap();
    let mut d = Driver::new(tree, Instant::now(), mode);
    for at in [0, 1_000_000] {
        assert!(d.step(at, vec![]).result.is_ok());
    }
    assert!(
        d.step(
            1_250_000,
            vec![Event::Viewport {
                width: 800.0,
                height: 600.0,
                scale: 1.0
            }]
        )
        .result
        .is_ok()
    );
    d
}

#[test]
fn cancelling_a_sibling_preserves_in_flight_hold_work_through_query_failure() {
    for mode in [Mode::Full, Mode::Active, Mode::Dirty] {
        let mut d = retargeted_hold(mode);
        let mut attrs = d.attrs(id(3));
        attrs.animate = None;
        attrs.width = Some(Length::Fill);
        let failed = d.attempt(
            1_500_000,
            vec![Event::Attrs(id(3), Box::new(attrs))],
            Injection::Query(QueryKind::Target),
        );
        assert_eq!(
            failed.result,
            Err(AttemptError::Native(ProjectionError::InjectedQuery))
        );
        failed
            .published_before
            .unwrap()
            .assert_same(&failed.published_after.unwrap());
        let retry = d.step(1_500_000, vec![]);
        assert!(retry.result.is_ok(), "{:?}", retry.result);
        close(
            fp(retry.after_layout.as_ref().unwrap(), 2).visible,
            300.0 + 100.0 / 3.0,
        );
        let middle = d.step(1_750_000, vec![]);
        assert!(middle.result.is_ok());
        close(
            fp(middle.after_layout.as_ref().unwrap(), 2).visible,
            300.0 + 200.0 / 3.0,
        );
        let done = d.step(2_000_000, vec![]);
        assert!(done.result.is_ok(), "{:?}", done.result);
        close(fp(done.after_layout.as_ref().unwrap(), 2).visible, 400.0);
        assert!(d.tree.length_runtime.is_none());
        assert!(d.runtime.groups.is_empty());
    }
}

#[test]
fn arriving_sibling_does_not_extend_an_unchanged_hold_trajectory() {
    for mode in [Mode::Full, Mode::Active, Mode::Dirty] {
        let mut d = retargeted_hold(mode);
        let mut attrs = d.attrs(id(4));
        attrs.animate = Some(spec(Length::Px(0.0), Length::Px(0.0), 1500.0, Axis::Width));
        let joined = d.step(1_500_000, vec![Event::Attrs(id(4), Box::new(attrs))]);
        assert!(joined.result.is_ok());
        close(
            fp(joined.after_layout.as_ref().unwrap(), 2).visible,
            300.0 + 100.0 / 3.0,
        );
        let middle = d.step(1_750_000, vec![]);
        assert!(middle.result.is_ok());
        close(
            fp(middle.after_layout.as_ref().unwrap(), 2).visible,
            300.0 + 200.0 / 3.0,
        );
        let own_end = d.step(2_000_000, vec![]);
        assert!(own_end.result.is_ok());
        close(fp(own_end.after_layout.as_ref().unwrap(), 2).visible, 400.0);
        let done = d.step(3_000_000, vec![]);
        assert!(done.result.is_ok());
        close(fp(done.after_layout.as_ref().unwrap(), 2).visible, 400.0);
        assert!(d.tree.length_runtime.is_none());
        assert!(d.runtime.groups.is_empty());
    }
}

#[test]
fn interaction_metrics_at_completion_replay_old_seeds_without_rewinding_live_input() {
    for fail in [false, true] {
        let mut tree = drawable(Length::Content, Length::Fill);
        let node = tree.get_mut(&id(2)).unwrap();
        node.spec.kind = ElementKind::Text;
        node.spec.declared.content = Some("Interaction metrics".into());
        node.spec.declared.font_size = Some(10.0);
        node.spec.declared.mouse_over = Some(crate::tree::attrs::MouseOverAttrs {
            font_size: Some(20.0),
            ..Default::default()
        });
        let mut d = Driver::new(tree, Instant::now(), Mode::Active);
        d.step(0, vec![]);
        let before = d.step(999_999, vec![]).published_after.unwrap();
        d.tree.get_mut(&id(2)).unwrap().runtime.mouse_over_active = true;
        if fail {
            let rejected = d.attempt(
                1_000_000,
                vec![],
                Injection::Query(QueryKind::PreviousInputs),
            );
            assert_eq!(
                rejected.result,
                Err(AttemptError::Native(ProjectionError::InjectedQuery))
            );
            rejected.published_after.unwrap().assert_same(&before);
        }
        let done = d.step(1_000_000, vec![]);
        assert!(done.result.is_ok(), "{:?}", done.result);
        assert_eq!(done.inspection.queries[1].kind, QueryKind::PreviousInputs);
        let after = done.after_layout.unwrap();
        assert_ne!(fp(&before.pose, 2).intrinsic, fp(&after, 2).intrinsic);
        assert_eq!(after[&id(2)].effective.font_size, Some(20.0));
        assert!(d.tree.get(&id(2)).unwrap().runtime.mouse_over_active);
        assert!(d.tree.length_runtime.is_none());
    }
}

#[test]
fn initially_empty_scroll_ranges_still_capture_later_runtime_scroll_input() {
    let mut tree = drawable(Length::FillWeighted(1.0), Length::FillWeighted(3.0));
    tree.get_mut(&id(1)).unwrap().spec.declared.scrollbar_x = Some(true);
    tree.get_mut(&id(3)).unwrap().spec.declared.width = Some(Length::Px(900.0));
    let mut d = Driver::new(tree, Instant::now(), Mode::Full);
    assert_eq!(d.tree.get(&id(1)).unwrap().layout.scroll_x_max, 0.0);
    assert!(d.step(0, vec![]).result.is_ok());
    assert!(d.tree.get(&id(1)).unwrap().layout.scroll_x_max > 0.0);
    d.tree.get_mut(&id(1)).unwrap().layout.scroll_x = 150.0;
    let output = d.step(500_000, vec![]);
    assert!(output.result.is_ok());
    assert_eq!(
        output.inspection.context.as_ref().unwrap().seeds[&id(1)].scroll[0],
        150.0
    );
    assert_eq!(d.tree.get(&id(1)).unwrap().layout.scroll_x, 150.0);
}

#[test]
fn dependent_scopes_validate_joint_release_before_publishing_or_retiring() {
    for mode in [Mode::Full, Mode::Active, Mode::Dirty] {
        let mut tree = fixture(Length::Px(200.0), Length::Content, Axis::Width);
        tree.insert(Element::with_attrs(
            id(4),
            ElementKind::El,
            vec![],
            Attrs {
                animate: Some(spec(Length::Px(40.0), Length::Fill, 2000.0, Axis::Width)),
                ..Default::default()
            },
        ));
        tree.set_children(&id(2), vec![id(4)]).unwrap();
        let mut d = Driver::new(tree, Instant::now(), mode);
        assert!(d.step(0, vec![]).result.is_ok());
        let held = d.step(1_000_000, vec![]);
        assert!(held.result.is_ok());
        close(fp(held.after_layout.as_ref().unwrap(), 2).visible, 0.0);
        close(fp(held.after_layout.as_ref().unwrap(), 4).visible, 20.0);
        let failed = d.attempt(2_000_000, vec![], Injection::Query(QueryKind::Release));
        assert_eq!(
            failed.result,
            Err(AttemptError::Native(ProjectionError::InjectedQuery))
        );
        failed
            .published_before
            .unwrap()
            .assert_same(&failed.published_after.unwrap());
        assert!(!d.runtime.groups.is_empty());
        let done = d.step(2_000_000, vec![]);
        assert!(done.result.is_ok());
        same_geometry(
            done.with_samples.as_ref().unwrap(),
            done.without_samples.as_ref().unwrap(),
        );
        assert_eq!(done.inspection.queries.len(), 1);
        assert!(d.tree.length_runtime.is_none());
        assert!(d.runtime.groups.is_empty());
    }
}
