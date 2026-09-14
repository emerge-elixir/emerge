use super::super::inspection::QueryKind;
use super::support::*;
use super::*;
use crate::tree::animation::change::{ChangePolicy, Field};

fn content_tree(text: &str) -> ElementTree {
    let mut tree = fixture(Length::Px(40.0), Length::Content, Axis::Width);
    let owner = &mut tree.get_mut(&id(2)).unwrap().spec.declared;
    owner.animate = None;
    owner.width = Some(Length::Content);
    owner.height = Some(Length::Content);
    owner.animate_change = Some(Arc::new(vec![ChangePolicy {
        field: Field::Width,
        duration_ms: 1000.0,
        curve: AnimationCurve::Linear,
    }]));
    tree.insert(Element::with_attrs(
        id(4),
        ElementKind::Text,
        vec![],
        Attrs {
            content: Some(text.into()),
            width: Some(Length::Content),
            ..Default::default()
        },
    ));
    tree.set_children(&id(2), vec![id(4)]).unwrap();
    tree
}
fn native_width(text: &str) -> f32 {
    let mut tree = content_tree(text);
    crate::tree::layout::layout_tree(
        &mut tree,
        Constraint::new(600.0, 600.0),
        1.0,
        &SkiaTextMeasurer,
    );
    extent(&tree, 2, Axis::Width)
}
fn width(d: &Driver) -> f32 {
    extent(&d.tree, 2, Axis::Width)
}
fn text_event(d: &Driver, text: &str) -> Event {
    let mut attrs = d.attrs(id(4));
    attrs.content = Some(text.into());
    Event::Attrs(id(4), Box::new(attrs))
}

#[test]
fn unchanged_content_declaration_animates_descendant_text_growth_and_shrink() {
    for mode in [Mode::Full, Mode::Active, Mode::Dirty] {
        let mut d = Driver::new(content_tree("S"), Instant::now(), mode);
        assert!(d.step(0, vec![]).result.is_ok());
        let a = width(&d);
        let b = native_width("Something");
        assert!(b > a);
        assert!(d.runtime.is_empty());
        let event = text_event(&d, "Something");
        let update = d.step(100_000, vec![event]);
        assert!(update.result.is_ok(), "{:?}", update.result);
        close(width(&d), a);
        assert_eq!(d.runtime.changes.len(), 1);
        for us in [250_000, 500_000, 750_000, 1_000_000] {
            let frame = d.step(100_000 + us, vec![]);
            assert!(frame.result.is_ok(), "{:?}", frame.result);
            close(width(&d), a + (b - a) * us as f32 / 1_000_000.0);
            assert!(
                frame
                    .inspection
                    .queries
                    .iter()
                    .all(|query| query.kind != QueryKind::ContentTarget)
            );
        }
        assert!(d.runtime.is_empty());
        let event = text_event(&d, "S");
        assert!(d.step(1_200_000, vec![event]).result.is_ok());
        close(width(&d), b);
        assert!(d.step(1_700_000, vec![]).result.is_ok());
        close(width(&d), (a + b) / 2.0);
        assert!(d.step(2_200_000, vec![]).result.is_ok());
        close(width(&d), a);
        assert!(d.runtime.is_empty());
    }
}

#[test]
fn content_interruption_uses_the_published_pose_and_incoming_timing() {
    for mode in [Mode::Full, Mode::Active, Mode::Dirty] {
        let mut d = Driver::new(content_tree("S"), Instant::now(), mode);
        d.step(0, vec![]);
        let event = text_event(&d, "Something");
        assert!(d.step(100_000, vec![event]).result.is_ok());
        assert!(d.step(600_000, vec![]).result.is_ok());
        let source = width(&d);
        let old = d
            .runtime
            .changes
            .get(&(id(2), Field::Width))
            .unwrap()
            .generation;
        let mut attrs = d.attrs(id(2));
        Arc::make_mut(attrs.animate_change.as_mut().unwrap())[0].duration_ms = 2000.0;
        let text = text_event(&d, "Something else");
        assert!(
            d.step(600_000, vec![Event::Attrs(id(2), Box::new(attrs)), text])
                .result
                .is_ok()
        );
        close(width(&d), source);
        assert_ne!(
            old,
            d.runtime
                .changes
                .get(&(id(2), Field::Width))
                .unwrap()
                .generation
        );
        assert!(d.step(1_600_000, vec![]).result.is_ok());
        close(width(&d), (source + native_width("Something else")) / 2.0);
        assert!(d.step(2_600_000, vec![]).result.is_ok());
        close(width(&d), native_width("Something else"));
        assert!(d.runtime.is_empty());
    }
}

#[test]
fn equal_content_targets_and_timing_only_edits_do_not_restart() {
    let mut tree = content_tree("S");
    tree.get_mut(&id(4)).unwrap().spec.declared.width = Some(Length::Px(40.0));
    let mut d = Driver::new(tree, Instant::now(), Mode::Full);
    d.step(0, vec![]);
    let mut child = d.attrs(id(4));
    child.width = Some(Length::Px(120.0));
    assert!(
        d.step(100_000, vec![Event::Attrs(id(4), Box::new(child))])
            .result
            .is_ok()
    );
    let generation = d
        .runtime
        .changes
        .get(&(id(2), Field::Width))
        .unwrap()
        .generation;
    let mut attrs = d.attrs(id(2));
    Arc::make_mut(attrs.animate_change.as_mut().unwrap())[0].duration_ms = 2000.0;
    let text = text_event(&d, "T");
    assert!(
        d.step(600_000, vec![Event::Attrs(id(2), Box::new(attrs)), text])
            .result
            .is_ok()
    );
    close(width(&d), 80.0);
    assert_eq!(
        generation,
        d.runtime
            .changes
            .get(&(id(2), Field::Width))
            .unwrap()
            .generation
    );
    assert!(d.step(1_100_000, vec![]).result.is_ok());
    close(width(&d), 120.0);
    assert!(d.runtime.is_empty());
    let text = text_event(&d, "S");
    assert!(d.step(1_200_000, vec![text]).result.is_ok());
    assert!(d.runtime.is_empty());
}

#[test]
fn content_query_and_output_failures_preserve_publication_and_retry_clocks() {
    for mode in [Mode::Full, Mode::Active, Mode::Dirty] {
        for injection in [
            Injection::Query(QueryKind::ContentTarget),
            Injection::Query(QueryKind::Target),
            Injection::BeforeLayout,
        ] {
            let mut d = Driver::new(content_tree("S"), Instant::now(), mode);
            d.step(0, vec![]);
            let a = width(&d);
            let event = text_event(&d, "Something");
            let failed = d.attempt(100_000, vec![event], injection);
            assert!(failed.result.is_err());
            failed
                .published_before
                .unwrap()
                .assert_same(&failed.published_after.unwrap());
            let admitted = d
                .runtime
                .changes
                .get(&(id(2), Field::Width))
                .map(|entry| (entry.generation, entry.started_at));
            let retry = d.step(200_000, vec![]);
            assert!(retry.result.is_ok(), "{:?}", retry.result);
            let entry = d.runtime.changes.get(&(id(2), Field::Width)).unwrap();
            if let Some(admitted) = admitted {
                assert_eq!(admitted, (entry.generation, entry.started_at));
                close(width(&d), a + 0.1 * (native_width("Something") - a));
            } else {
                close(width(&d), a + 0.1 * (native_width("Something") - a));
            }
            assert!(d.step(1_200_000, vec![]).result.is_ok());
            close(width(&d), native_width("Something"));
        }
    }
}

#[test]
fn content_both_axes_use_joint_native_destinations_at_all_scales() {
    for mode in [Mode::Full, Mode::Active, Mode::Dirty] {
        for scale in [0.5, 1.0, 2.0] {
            let mut tree = content_tree("S");
            let child = tree.get_mut(&id(4)).unwrap();
            child.spec.kind = ElementKind::El;
            child.spec.declared = Attrs {
                width: Some(Length::Px(40.0)),
                height: Some(Length::Px(20.0)),
                ..Default::default()
            };
            Arc::make_mut(
                tree.get_mut(&id(2))
                    .unwrap()
                    .spec
                    .declared
                    .animate_change
                    .as_mut()
                    .unwrap(),
            )
            .push(ChangePolicy {
                field: Field::Height,
                duration_ms: 1000.0,
                curve: AnimationCurve::Linear,
            });
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
            let mut attrs = d.attrs(id(4));
            attrs.width = Some(Length::Px(120.0));
            attrs.height = Some(Length::Px(60.0));
            assert!(
                d.step(100_000, vec![Event::Attrs(id(4), Box::new(attrs))])
                    .result
                    .is_ok()
            );
            for (us, w, h) in [
                (100_000, 40.0, 20.0),
                (600_000, 80.0, 40.0),
                (1_100_000, 120.0, 60.0),
            ] {
                let out = d.step(us, vec![]);
                assert!(out.result.is_ok(), "{:?}", out.result);
                close(width(&d), w * scale);
                close(extent(&d.tree, 2, Axis::Height), h * scale);
            }
            assert!(d.runtime.is_empty());
        }
    }
}

#[test]
fn removing_content_policy_cancels_motion_and_releases_the_idle_workspace() {
    let mut d = Driver::new(content_tree("S"), Instant::now(), Mode::Dirty);
    d.step(0, vec![]);
    let event = text_event(&d, "Something");
    d.step(100_000, vec![event]);
    d.step(600_000, vec![]);
    let mut attrs = d.attrs(id(2));
    attrs.animate_change = None;
    assert!(
        d.step(600_000, vec![Event::Attrs(id(2), Box::new(attrs))])
            .result
            .is_ok()
    );
    close(width(&d), native_width("Something"));
    assert!(d.runtime.is_empty());
    assert!(d.tree.length_runtime.is_none());
}

#[test]
fn content_reversal_after_failed_retarget_restores_committed_identity() {
    for mode in [Mode::Full, Mode::Active, Mode::Dirty] {
        let mut d = Driver::new(content_tree("S"), Instant::now(), mode);
        d.step(0, vec![]);
        let a = width(&d);
        let b = native_width("Something");
        let event = text_event(&d, "Something");
        d.step(100_000, vec![event]);
        d.step(350_000, vec![]);
        let entry = d.runtime.changes.get(&(id(2), Field::Width)).unwrap();
        let identity = (entry.generation, entry.started_at);
        let event = text_event(&d, "Something else");
        let failed = d.attempt(400_000, vec![event], Injection::BeforeLayout);
        assert!(failed.result.is_err());
        failed
            .published_before
            .unwrap()
            .assert_same(&failed.published_after.unwrap());
        let event = text_event(&d, "Something");
        assert!(d.step(600_000, vec![event]).result.is_ok());
        let entry = d.runtime.changes.get(&(id(2), Field::Width)).unwrap();
        assert_eq!(identity, (entry.generation, entry.started_at));
        close(width(&d), (a + b) / 2.0);
        assert!(d.step(1_100_000, vec![]).result.is_ok());
        close(width(&d), b);
        assert!(d.runtime.is_empty());
    }
}

#[test]
fn content_ancestors_detect_child_replacement_without_an_owner_attribute_patch() {
    let mut tree = content_tree("S");
    let mut rt = AnimationRuntime::default();
    let start = Instant::now();
    tick(&mut tree, &mut rt, start, 0, 1.0);
    let a = extent(&tree, 2, Axis::Width);
    tree.insert(Element::with_attrs(
        id(5),
        ElementKind::Text,
        vec![],
        Attrs {
            content: Some("Something".into()),
            width: Some(Length::Content),
            ..Default::default()
        },
    ));
    tree.set_children(&id(2), vec![id(5)]).unwrap();
    tree.remove_node(&id(4));
    tree.bump_revision();
    tick(&mut tree, &mut rt, start, 100_000, 1.0);
    close(extent(&tree, 2, Axis::Width), a);
    tick(&mut tree, &mut rt, start, 600_000, 1.0);
    close(
        extent(&tree, 2, Axis::Width),
        (a + native_width("Something")) / 2.0,
    );
    tick(&mut tree, &mut rt, start, 1_100_000, 1.0);
    close(extent(&tree, 2, Axis::Width), native_width("Something"));
}

#[test]
fn content_changes_follow_supplied_metric_epochs_without_a_model_edit() {
    use std::cell::Cell;
    struct Metrics(Cell<f32>);
    impl TextMeasurer for Metrics {
        fn measure_with_font(&self, text: &str, _: f32, _: &str, _: u16, _: bool) -> (f32, f32) {
            (text.chars().count() as f32 * self.0.get(), 20.0)
        }
        fn font_metrics(&self, _: f32, _: &str, _: u16, _: bool) -> (f32, f32) {
            (15.0, 5.0)
        }
        fn metrics_epoch(&self) -> u64 {
            self.0.get().to_bits() as u64
        }
    }
    let metrics = Metrics(Cell::new(10.0));
    let mut tree = content_tree("S");
    let mut rt = AnimationRuntime::default();
    let start = Instant::now();
    let tick = |tree: &mut ElementTree, rt: &mut AnimationRuntime, ms| {
        try_layout_tree_with_animation(
            tree,
            Constraint::new(600.0, 600.0),
            1.0,
            rt,
            start + Duration::from_millis(ms),
            &metrics,
            &FontContext::default(),
        )
        .unwrap()
    };
    tick(&mut tree, &mut rt, 0);
    let a = extent(&tree, 2, Axis::Width);
    let model = tree.layout_model_epoch;
    metrics.0.set(30.0);
    tick(&mut tree, &mut rt, 100);
    close(extent(&tree, 2, Axis::Width), a);
    assert_eq!(model, tree.layout_model_epoch);
    assert_eq!(rt.changes.len(), 1);
    tick(&mut tree, &mut rt, 600);
    close(extent(&tree, 2, Axis::Width), 20.0);
    tick(&mut tree, &mut rt, 1100);
    close(extent(&tree, 2, Axis::Width), 30.0);
    assert!(rt.is_empty());
}

#[test]
fn content_changes_use_frozen_image_facts_without_hydration() {
    use std::cell::Cell;
    struct Images(Cell<(u32, u32)>, Cell<usize>);
    impl TextMeasurer for Images {
        fn font_metrics(&self, _: f32, _: &str, _: u16, _: bool) -> (f32, f32) {
            (15.0, 5.0)
        }
        fn measure_with_font(&self, _: &str, _: f32, _: &str, _: u16, _: bool) -> (f32, f32) {
            (0.0, 0.0)
        }
        fn image_dimensions(
            &self,
            _: &crate::tree::attrs::ImageSource,
            load: bool,
        ) -> Option<(u32, u32)> {
            if load {
                self.1.set(self.1.get() + 1);
            }
            Some(self.0.get())
        }
    }
    let images = Images(Cell::new((100, 40)), Cell::new(0));
    let mut tree = content_tree("S");
    let node = tree.get_mut(&id(4)).unwrap();
    node.spec.kind = ElementKind::Image;
    node.spec.declared = Attrs {
        image_src: Some(crate::tree::attrs::ImageSource::RuntimePath(
            "content-query-image.png".into(),
        )),
        width: Some(Length::Content),
        ..Default::default()
    };
    let mut rt = AnimationRuntime::default();
    let start = Instant::now();
    let tick = |tree: &mut ElementTree, rt: &mut AnimationRuntime, ms| {
        try_layout_tree_with_animation(
            tree,
            Constraint::new(600.0, 600.0),
            1.0,
            rt,
            start + Duration::from_millis(ms),
            &images,
            &FontContext::default(),
        )
        .unwrap()
    };
    tick(&mut tree, &mut rt, 0);
    close(extent(&tree, 2, Axis::Width), 100.0);
    let initial_loads = images.1.get();
    images.0.set((200, 40));
    tick(&mut tree, &mut rt, 100);
    close(extent(&tree, 2, Axis::Width), 100.0);
    tick(&mut tree, &mut rt, 600);
    close(extent(&tree, 2, Axis::Width), 150.0);
    tick(&mut tree, &mut rt, 1100);
    close(extent(&tree, 2, Axis::Width), 200.0);
    assert!(rt.is_empty());
    assert_eq!(images.1.get(), initial_loads);
}

#[test]
fn idle_content_watchers_do_not_force_layout_or_queries() {
    let start = Instant::now();
    let mut d = Driver::new(content_tree("S"), start, Mode::Dirty);
    d.step(0, vec![]);
    for (us, text) in [(100_000, "Something"), (1_200_000, "S")] {
        let event = text_event(&d, text);
        assert!(d.step(us, vec![event]).result.is_ok());
        assert!(d.step(us + 1_000_000, vec![]).result.is_ok());
    }
    let state = d.tree.length_runtime.as_ref().unwrap();
    assert!(state.tracks.is_empty());
    let before = state.resolver.stats();
    assert_eq!(before.model_copies, 2); // One rebuild per changed model, not per track/frame.
    let preparation = crate::tree::layout::prepare_frame_attrs_for_update(
        &mut d.tree,
        1.0,
        Some(&mut d.runtime),
        Some(start + Duration::from_micros(2_200_000)),
    );
    assert!(!preparation.requires_layout(&d.tree));
    assert_eq!(
        d.tree.length_runtime.as_ref().unwrap().resolver.stats(),
        before
    );
}

#[test]
fn nested_content_policies_share_destinations_without_sample_feedback() {
    for mode in [Mode::Full, Mode::Active, Mode::Dirty] {
        let mut tree = content_tree("S");
        tree.get_mut(&id(4)).unwrap().spec.declared.animate_change =
            Some(Arc::new(vec![ChangePolicy {
                field: Field::Width,
                duration_ms: 2000.0,
                curve: AnimationCurve::Linear,
            }]));
        let mut d = Driver::new(tree, Instant::now(), mode);
        d.step(0, vec![]);
        let a = width(&d);
        let b = native_width("Something");
        let event = text_event(&d, "Something");
        assert!(d.step(100_000, vec![event]).result.is_ok());
        close(width(&d), a);
        for us in [500_000, 1_000_000, 1_500_000, 2_000_000] {
            let result = d.step(100_000 + us, vec![]);
            assert!(result.result.is_ok(), "{:?}", result.result);
            close(width(&d), a + (b - a) * (us as f32 / 1_000_000.0).min(1.0));
            close(
                extent(&d.tree, 4, Axis::Width),
                a + (b - a) * us as f32 / 2_000_000.0,
            );
        }
        assert!(d.runtime.is_empty());
    }
}

#[test]
fn content_policy_first_mount_does_not_turn_child_animation_ticks_into_changes() {
    let mut tree = content_tree("S");
    let child = tree.get_mut(&id(4)).unwrap();
    child.spec.kind = ElementKind::El;
    child.spec.declared = Attrs {
        animate: Some(spec(
            Length::Px(40.0),
            Length::Px(80.0),
            1000.0,
            Axis::Width,
        )),
        ..Default::default()
    };
    let mut rt = AnimationRuntime::default();
    let start = Instant::now();
    for us in [0, 250_000, 500_000, 1_000_000] {
        tick(&mut tree, &mut rt, start, us, 1.0);
        close(
            extent(&tree, 2, Axis::Width),
            40.0 + 40.0 * us as f32 / 1_000_000.0,
        );
        assert!(rt.changes.is_empty());
        assert_eq!(
            tree.length_runtime
                .as_ref()
                .unwrap()
                .resolver
                .stats()
                .model_copies,
            0
        );
    }
}

#[test]
fn root_content_policy_animates_the_same_text_update() {
    let mut tree = content_tree("S");
    tree.set_children(&id(1), vec![]).unwrap();
    tree.set_root_id(id(2));
    tree.remove_node(&id(1));
    tree.remove_node(&id(3));
    let mut d = Driver::new(tree, Instant::now(), Mode::Full);
    assert!(d.step(0, vec![]).result.is_ok());
    let a = width(&d);
    close(a, native_width("S"));
    let event = text_event(&d, "Something");
    assert!(d.step(100_000, vec![event]).result.is_ok());
    close(width(&d), a);
    assert!(d.step(600_000, vec![]).result.is_ok());
    close(width(&d), (a + native_width("Something")) / 2.0);
    assert!(d.step(1_100_000, vec![]).result.is_ok());
    close(width(&d), native_width("Something"));
    assert!(d.runtime.is_empty());
}
