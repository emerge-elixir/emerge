use super::*;
use std::cell::Cell;

#[derive(Default)]
struct Metrics {
    text_calls: Cell<usize>,
}
impl TextMeasurer for Metrics {
    fn measure_with_font(&self, text: &str, size: f32, _: &str, _: u16, _: bool) -> (f32, f32) {
        self.text_calls.set(self.text_calls.get() + 1);
        (text.len() as f32 * 8.0, size)
    }
    fn font_metrics(&self, size: f32, _: &str, _: u16, _: bool) -> (f32, f32) {
        (size * 0.75, size * 0.25)
    }
    fn image_dimensions(&self, _: &ImageSource, _: bool) -> Option<(u32, u32)> {
        panic!("projection reached a live asset provider");
    }
}

fn id(n: u64) -> NodeId {
    NodeId::from_wire_u64(n)
}
fn node(n: u64, kind: ElementKind, width: Length, height: Length) -> Element {
    Element::with_attrs(
        id(n),
        kind,
        vec![1, 2, 3],
        Attrs {
            width: Some(width),
            height: Some(height),
            ..Attrs::default()
        },
    )
}
fn tree() -> ElementTree {
    let mut root = node(1, ElementKind::Row, Length::Px(600.0), Length::Px(100.0));
    root.children = vec![id(2), id(3)];
    let mut tree = ElementTree::new();
    tree.set_root_id(root.id);
    [
        root,
        node(2, ElementKind::El, Length::Px(40.0), Length::Px(20.0)),
        node(3, ElementKind::El, Length::Fill, Length::Px(20.0)),
    ]
    .into_iter()
    .for_each(|node| tree.insert(node));
    tree
}
fn context() -> Arc<QueryContext> {
    Arc::new(QueryContext {
        constraint: Constraint::new(600.0, 100.0),
        scale: 1.0,
        inherited: FontContext::default(),
        metrics_epoch: 0,
        fonts: None,
        images: Arc::new(HashMap::new()),
        seeds: HashMap::new(),
    })
}
fn width(n: u64, length: Length) -> Arc<Projection> {
    Arc::new(Projection {
        nodes: HashMap::from([(
            id(n),
            NodeProjection {
                attrs: Attrs {
                    width: Some(length),
                    ..Attrs::default()
                },
                ..NodeProjection::default()
            },
        )]),
    })
}
fn cap() -> Length {
    Length::Min(Box::new(Length::Px(50.0)), Box::new(Length::Fill))
}
fn requests() -> [Endpoint; 3] {
    [
        (id(1), Axis::Width),
        (id(2), Axis::Width),
        (id(3), Axis::Width),
    ]
}
fn resolve(
    resolver: &mut EndpointResolver,
    tree: &ElementTree,
    projection: Arc<Projection>,
    context: Arc<QueryContext>,
) -> HashMap<Endpoint, AxisFootprint> {
    resolver
        .resolve(
            tree,
            0,
            projection,
            context,
            &requests(),
            &Metrics::default(),
        )
        .unwrap()
}
fn visible(output: &HashMap<Endpoint, AxisFootprint>, n: u64) -> f32 {
    output[&(id(n), Axis::Width)].visible
}

#[test]
fn isolated_batched_endpoints_retain_charge_and_never_play_declared_animations() {
    use crate::tree::animation::{AnimationCurve, AnimationRepeat, AnimationSpec};
    let mut tree = tree();
    tree.get_mut(&id(2)).unwrap().spec.declared.animate = Some(AnimationSpec {
        keyframes: vec![
            Attrs {
                width: Some(Length::Px(999.0)),
                ..Attrs::default()
            },
            Attrs {
                width: Some(Length::Px(800.0)),
                ..Attrs::default()
            },
        ],
        duration_ms: 1000.0,
        curve: AnimationCurve::Linear,
        repeat: AnimationRepeat::Loop,
    });
    let mut resolver = EndpointResolver::default();
    let baseline = resolve(
        &mut resolver,
        &tree,
        Arc::new(Projection::default()),
        context(),
    );
    assert_eq!(
        (visible(&baseline, 2), visible(&baseline, 3)),
        (40.0, 560.0)
    );
    let projected = resolve(&mut resolver, &tree, width(2, cap()), context());
    assert_eq!(
        (visible(&projected, 2), visible(&projected, 3)),
        (50.0, 300.0)
    );
    assert_eq!(projected[&(id(2), Axis::Width)].charge, 300.0);
    assert!(tree.iter_nodes().all(|node| node.layout.frame.is_none()));
    assert_eq!(tree.get(&id(2)).unwrap().spec.attrs_raw, [1, 2, 3]);
    assert_eq!(resolver.stats().model_copies, 1);
}

#[test]
fn repeated_requests_are_cached_and_permuted_queries_match_fresh_evaluation() {
    let tree = tree();
    let mut resolver = EndpointResolver::default();
    let context = context();
    let a = width(2, cap());
    let b = width(2, Length::Px(150.0));
    let c = width(3, Length::Px(70.0));
    for projection in [&a, &a, &b, &c, &a, &b, &a] {
        let actual = resolve(
            &mut resolver,
            &tree,
            Arc::clone(projection),
            Arc::clone(&context),
        );
        let fresh = resolve(
            &mut EndpointResolver::default(),
            &tree,
            Arc::clone(projection),
            Arc::clone(&context),
        );
        assert_eq!(actual, fresh);
    }
    assert_eq!(resolver.stats().model_copies, 1);
    assert_eq!(resolver.stats().layout_queries, 6);
    assert_eq!(resolver.stats().cache_hits, 1);
    // A cached layout can answer a new endpoint set without another evaluation.
    resolver
        .resolve(
            &tree,
            0,
            a,
            context,
            &[(id(2), Axis::Height)],
            &Metrics::default(),
        )
        .unwrap();
    assert_eq!(resolver.stats().cache_hits, 2);
}

#[test]
fn simultaneous_projection_and_foreign_full_samples_share_one_evaluation() {
    let tree = tree();
    let context = context();
    let mut resolver = EndpointResolver::default();
    let joint = Arc::new(Projection {
        nodes: [2, 3]
            .into_iter()
            .map(|n| {
                (
                    id(n),
                    NodeProjection {
                        attrs: Attrs {
                            width: Some(Length::Fill),
                            ..Attrs::default()
                        },
                        ..NodeProjection::default()
                    },
                )
            })
            .collect(),
    });
    let output = resolve(&mut resolver, &tree, joint, Arc::clone(&context));
    assert_eq!((visible(&output, 2), visible(&output, 3)), (300.0, 300.0));
    let bounded = resolve(&mut resolver, &tree, width(2, cap()), Arc::clone(&context));
    let frozen = bounded[&(id(2), Axis::Width)];
    let foreign = Arc::new(Projection {
        nodes: HashMap::from([
            (
                id(2),
                NodeProjection {
                    width: Some(frozen),
                    ..NodeProjection::default()
                },
            ),
            (
                id(3),
                NodeProjection {
                    attrs: Attrs {
                        width: Some(Length::Fill),
                        ..Attrs::default()
                    },
                    ..NodeProjection::default()
                },
            ),
        ]),
    });
    let replay = resolve(&mut resolver, &tree, foreign, Arc::clone(&context));
    assert_eq!((visible(&replay, 2), visible(&replay, 3)), (50.0, 300.0));
    let cleared = resolve(&mut resolver, &tree, width(2, Length::Px(50.0)), context);
    assert_eq!(visible(&cleared, 3), 550.0);
}

#[test]
fn scale_viewport_and_model_changes_do_not_alias_cached_queries() {
    let mut tree = tree();
    let mut resolver = EndpointResolver::default();
    let projection = width(2, cap());
    let base_context = context();
    resolve(
        &mut resolver,
        &tree,
        Arc::clone(&projection),
        Arc::clone(&base_context),
    );
    let bigger = Arc::new(QueryContext {
        scale: 2.0,
        constraint: Constraint::new(1200.0, 200.0),
        ..(*base_context).clone()
    });
    let scaled = resolve(
        &mut resolver,
        &tree,
        Arc::clone(&projection),
        Arc::clone(&bigger),
    );
    let fresh = resolve(
        &mut EndpointResolver::default(),
        &tree,
        Arc::clone(&projection),
        bigger,
    );
    assert_eq!(scaled, fresh);
    assert_eq!(visible(&scaled, 3), 300.0);
    assert_eq!(resolver.stats().model_copies, 1);
    tree.get_mut(&id(1)).unwrap().spec.declared.width = Some(Length::Px(800.0));
    let changed = resolver
        .resolve(
            &tree,
            1,
            projection,
            base_context,
            &requests(),
            &Metrics::default(),
        )
        .unwrap();
    assert_eq!(visible(&changed, 3), 400.0);
    assert_eq!(resolver.stats().model_copies, 2);
    resolver.release();
    assert!(resolver.workspace.is_none());
}

#[test]
fn removing_ancestor_layout_scale_restores_descendants() {
    let tree = tree();
    let context = context();
    let mut resolver = EndpointResolver::default();
    let scaled = Arc::new(Projection {
        nodes: HashMap::from([(
            id(1),
            NodeProjection {
                attrs: Attrs {
                    layout_scale: Some(2.0),
                    ..Attrs::default()
                },
                ..NodeProjection::default()
            },
        )]),
    });
    for projection in [scaled, width(2, cap()), Arc::new(Projection::default())] {
        assert_eq!(
            resolve(
                &mut resolver,
                &tree,
                Arc::clone(&projection),
                Arc::clone(&context)
            ),
            resolve(
                &mut EndpointResolver::default(),
                &tree,
                projection,
                Arc::clone(&context)
            )
        );
    }
}

#[test]
fn model_copy_omits_live_frames_encoded_attrs_and_retained_caches() {
    use crate::tree::render::render_tree;
    let mut tree = tree();
    layout_tree(
        &mut tree,
        Constraint::new(600.0, 100.0),
        1.0,
        &Metrics::default(),
    );
    render_tree(&tree);
    let snapshot = tree.layout_query_snapshot();
    for copy in snapshot.iter_nodes() {
        assert!(copy.spec.attrs_raw.is_empty());
        assert!(copy.layout.frame.is_none());
        assert!(copy.layout.measured_frame.is_none());
        assert!(copy.layout.resolve_cache.is_none());
        assert!(copy.layout.subtree_measure_cache.is_none());
        assert!(copy.layout.dimension_samples.is_none());
        assert!(copy.refresh.registry_cache.is_none());
        assert!(copy.refresh.render_fragment_cache.borrow().is_none());
    }
    assert_eq!(snapshot.child_ids(&id(1)), tree.child_ids(&id(1)));
    assert!(tree.get(&id(1)).unwrap().layout.frame.is_some());
}

#[test]
fn image_queries_use_only_frozen_metrics_and_metric_updates_invalidate_ancestors() {
    let mut tree = tree();
    let image = tree.get_mut(&id(2)).unwrap();
    image.spec.kind = ElementKind::Image;
    image.spec.declared.width = Some(Length::Content);
    image.spec.declared.height = Some(Length::Content);
    let source = ImageSource::Id("not-loaded-by-query".into());
    image.spec.declared.image_src = Some(source.clone());
    let base = context();
    let projection = Arc::new(Projection::default());
    let mut resolver = EndpointResolver::default();
    let unknown = resolve(
        &mut resolver,
        &tree,
        Arc::clone(&projection),
        Arc::clone(&base),
    );
    assert_eq!(visible(&unknown, 2), 64.0);
    for width in [200, 320, 200] {
        let ctx = Arc::new(QueryContext {
            images: Arc::new(HashMap::from([(source.clone(), (width, 100))])),
            ..(*base).clone()
        });
        let output = resolve(&mut resolver, &tree, Arc::clone(&projection), ctx);
        assert_eq!(visible(&output, 2), width as f32);
        assert_eq!(visible(&output, 3), 600.0 - width as f32);
    }
    assert_eq!(resolver.stats().metric_invalidations, 3);
    assert_eq!(resolver.stats().model_copies, 1);
}

#[test]
fn failed_requests_do_not_poison_the_previous_or_next_projection() {
    let tree = tree();
    let mut resolver = EndpointResolver::default();
    let context = context();
    let valid = width(2, cap());
    let expected = resolve(
        &mut resolver,
        &tree,
        Arc::clone(&valid),
        Arc::clone(&context),
    );
    assert_eq!(
        resolver.resolve(
            &tree,
            0,
            width(999, Length::Fill),
            Arc::clone(&context),
            &requests(),
            &Metrics::default()
        ),
        Err(ProjectionError::UnknownNode(id(999)))
    );
    let bad = Arc::new(Projection {
        nodes: HashMap::from([(
            id(3),
            NodeProjection {
                width: Some(expected[&(id(1), Axis::Width)]),
                ..NodeProjection::default()
            },
        )]),
    });
    assert!(
        resolver
            .resolve(
                &tree,
                0,
                bad,
                Arc::clone(&context),
                &requests(),
                &Metrics::default()
            )
            .is_err()
    );
    assert_eq!(resolve(&mut resolver, &tree, valid, context), expected);
}

#[test]
fn scroll_end_follow_is_reseeded_per_query_without_mutating_live_state() {
    let mut tree = tree();
    tree.get_mut(&id(1)).unwrap().spec.declared.scrollbar_x = Some(true);
    tree.get_mut(&id(2)).unwrap().spec.declared.width = Some(Length::Px(900.0));
    layout_tree(
        &mut tree,
        Constraint::new(600.0, 100.0),
        1.0,
        &Metrics::default(),
    );
    let root = tree.get_mut(&id(1)).unwrap();
    root.layout.scroll_x = root.layout.scroll_x_max;
    let live = QuerySeed::capture(root);
    assert_eq!(live.scroll[0], 300.0);
    let mut resolver = EndpointResolver::default();
    let context = context();
    for size in [1200.0, 700.0, 1200.0, 500.0, 1200.0] {
        let projection = width(2, Length::Px(size));
        let actual = resolve(
            &mut resolver,
            &tree,
            Arc::clone(&projection),
            Arc::clone(&context),
        );
        let mut fresh = EndpointResolver::default();
        let expected = resolve(&mut fresh, &tree, projection, Arc::clone(&context));
        assert_eq!(actual, expected);
        assert_eq!(
            QuerySeed::capture(
                resolver
                    .workspace
                    .as_ref()
                    .unwrap()
                    .tree
                    .get(&id(1))
                    .unwrap()
            ),
            QuerySeed::capture(fresh.workspace.as_ref().unwrap().tree.get(&id(1)).unwrap())
        );
        assert_eq!(QuerySeed::capture(tree.get(&id(1)).unwrap()), live);
    }
    assert_eq!(resolver.stats().model_copies, 1);
}

#[test]
fn rotation_and_font_projection_order_cannot_reuse_old_render_frames_as_intrinsics() {
    let mut tree = tree();
    let text = tree.get_mut(&id(2)).unwrap();
    text.spec.kind = ElementKind::Text;
    text.spec.declared.width = Some(Length::Content);
    text.spec.declared.height = Some(Length::Content);
    text.spec.declared.content = Some("text".into());
    let context = context();
    let mut resolver = EndpointResolver::default();
    let projection = |angle, size| {
        Arc::new(Projection {
            nodes: HashMap::from([(
                id(2),
                NodeProjection {
                    attrs: Attrs {
                        layout_rotate: Some(angle),
                        font_size: Some(size),
                        ..Attrs::default()
                    },
                    ..NodeProjection::default()
                },
            )]),
        })
    };
    for request in [
        projection(33.0, 12.0),
        projection(0.0, 24.0),
        projection(33.0, 18.0),
        projection(0.0, 12.0),
    ] {
        let endpoints = [(id(2), Axis::Width), (id(2), Axis::Height)];
        let actual = resolver
            .resolve(
                &tree,
                0,
                Arc::clone(&request),
                Arc::clone(&context),
                &endpoints,
                &Metrics::default(),
            )
            .unwrap();
        let fresh = EndpointResolver::default()
            .resolve(
                &tree,
                0,
                request,
                Arc::clone(&context),
                &endpoints,
                &Metrics::default(),
            )
            .unwrap();
        assert_eq!(actual, fresh);
    }
}

#[test]
fn metric_epochs_invalidate_text_leaf_caches_without_copying_the_model() {
    struct VariableMetrics(Cell<f32>);
    impl TextMeasurer for VariableMetrics {
        fn measure_with_font(&self, _: &str, _: f32, _: &str, _: u16, _: bool) -> (f32, f32) {
            (self.0.get(), 16.0)
        }
        fn font_metrics(&self, _: f32, _: &str, _: u16, _: bool) -> (f32, f32) {
            (12.0, 4.0)
        }
    }
    let mut tree = tree();
    let text = tree.get_mut(&id(2)).unwrap();
    text.spec.kind = ElementKind::Text;
    text.spec.declared.content = Some("text".into());
    text.spec.declared.width = Some(Length::Content);
    let metrics = VariableMetrics(Cell::new(20.0));
    let mut resolver = EndpointResolver::default();
    let projection = Arc::new(Projection::default());
    let base = context();
    for (epoch, width) in [(0, 20.0), (1, 80.0), (2, 30.0)] {
        metrics.0.set(width);
        let context = Arc::new(QueryContext {
            metrics_epoch: epoch,
            fonts: None,
            ..(*base).clone()
        });
        let output = resolver
            .resolve(
                &tree,
                0,
                Arc::clone(&projection),
                context,
                &requests(),
                &metrics,
            )
            .unwrap();
        assert_eq!(visible(&output, 2), width);
    }
    assert_eq!(resolver.stats().model_copies, 1);
    assert_eq!(resolver.stats().metric_invalidations, 2);
}

#[test]
fn sparse_context_scroll_and_hover_inputs_do_not_leak_between_queries() {
    let mut tree = tree();
    let text = tree.get_mut(&id(2)).unwrap();
    text.spec.kind = ElementKind::Text;
    text.spec.declared.height = Some(Length::Content);
    text.spec.declared.content = Some("text".into());
    text.spec.declared.mouse_over = Some(MouseOverAttrs {
        font_size: Some(64.0),
        ..MouseOverAttrs::default()
    });
    let mut seed = QuerySeed::capture(text);
    seed.runtime.mouse_over_active = true;
    let base = context();
    let hovered = Arc::new(QueryContext {
        seeds: HashMap::from([(id(2), seed)]),
        ..(*base).clone()
    });
    let projection = Arc::new(Projection {
        nodes: HashMap::from([(
            id(2),
            NodeProjection {
                attrs: Attrs {
                    font_size: Some(32.0),
                    ..Attrs::default()
                },
                ..NodeProjection::default()
            },
        )]),
    });
    let mut resolver = EndpointResolver::default();
    for (ctx, expected) in [(&hovered, 64.0), (&base, 32.0), (&hovered, 64.0)] {
        let result = resolver
            .resolve(
                &tree,
                0,
                Arc::clone(&projection),
                Arc::clone(ctx),
                &[(id(2), Axis::Height)],
                &Metrics::default(),
            )
            .unwrap();
        assert_eq!(result[&(id(2), Axis::Height)].visible, expected);
    }
    assert!(!tree.get(&id(2)).unwrap().runtime.mouse_over_active);
    assert_eq!(resolver.stats().model_copies, 1);
}

#[test]
fn frozen_scroll_overrides_declared_scroll_consistently_for_full_and_sparse_preparation() {
    let mut tree = tree();
    tree.get_mut(&id(1)).unwrap().spec.declared.scrollbar_x = Some(true);
    tree.get_mut(&id(1)).unwrap().spec.declared.scroll_x = Some(0.0);
    tree.get_mut(&id(2)).unwrap().spec.declared.width = Some(Length::Px(900.0));
    layout_tree(
        &mut tree,
        Constraint::new(600.0, 100.0),
        1.0,
        &Metrics::default(),
    );
    tree.get_mut(&id(1)).unwrap().layout.scroll_x = 300.0;
    let mut seed = QuerySeed::capture(tree.get(&id(1)).unwrap());
    seed.scroll[0] = 125.0;
    let base = context();
    let scrolled = Arc::new(QueryContext {
        seeds: HashMap::from([(id(1), seed)]),
        ..(*base).clone()
    });
    let projection = width(2, Length::Px(1200.0));
    let mut resolver = EndpointResolver::default();
    for (ctx, expected) in [(&scrolled, 125.0), (&base, 600.0), (&scrolled, 125.0)] {
        resolve(
            &mut resolver,
            &tree,
            Arc::clone(&projection),
            Arc::clone(ctx),
        );
        assert_eq!(
            resolver
                .workspace
                .as_ref()
                .unwrap()
                .tree
                .get(&id(1))
                .unwrap()
                .layout
                .scroll_x,
            expected
        );
    }
    assert_eq!(tree.get(&id(1)).unwrap().layout.scroll_x, 300.0);
}

#[test]
fn compact_snapshot_preserves_nearby_parents_and_remounts_invalidate_cached_results() {
    let mut tree = tree();
    tree.insert(node(99, ElementKind::El, Length::Px(1.0), Length::Px(1.0)));
    tree.insert(node(4, ElementKind::El, Length::Fill, Length::Fill));
    tree.get_mut(&id(2))
        .unwrap()
        .nearby
        .push(NearbySlot::InFront, id(4));
    tree.remove_node(&id(99));
    let snapshot = tree.layout_query_snapshot();
    assert_eq!(snapshot.nodes.len(), 4);
    assert!(snapshot.free_list.is_empty());
    let nearby = snapshot.ix_of(&id(4)).unwrap();
    assert_eq!(
        snapshot.parent_link_of(nearby),
        Some(super::super::super::element::ParentLink::Nearby {
            host: snapshot.ix_of(&id(2)).unwrap(),
            slot: NearbySlot::InFront
        })
    );
    let mut resolver = EndpointResolver::default();
    let projection = width(2, cap());
    let context = context();
    resolve(
        &mut resolver,
        &tree,
        Arc::clone(&projection),
        Arc::clone(&context),
    );
    tree.get_mut(&id(1)).unwrap().lifecycle.mounted_at_revision += 1;
    resolve(&mut resolver, &tree, projection, context);
    assert_eq!(resolver.stats().model_copies, 2);
}

#[test]
fn warm_queries_reuse_untouched_text_measurements_and_do_not_recopy_the_model() {
    let mut tree = tree();
    let mut text = node(4, ElementKind::Text, Length::Content, Length::Content);
    text.spec.declared.content = Some("untouched".into());
    tree.insert(text);
    tree.get_mut(&id(1)).unwrap().children.push(id(4));
    let metrics = Metrics::default();
    let mut resolver = EndpointResolver::default();
    let context = context();
    resolver
        .resolve(
            &tree,
            0,
            width(2, cap()),
            Arc::clone(&context),
            &requests(),
            &metrics,
        )
        .unwrap();
    let cold_calls = metrics.text_calls.get();
    assert!(cold_calls > 0);
    let next = width(2, Length::Px(150.0));
    for _ in 0..2 {
        resolver
            .resolve(
                &tree,
                0,
                Arc::clone(&next),
                Arc::clone(&context),
                &requests(),
                &metrics,
            )
            .unwrap();
        assert_eq!(metrics.text_calls.get(), cold_calls);
    }
    assert_eq!(resolver.stats().model_copies, 1);
    assert_eq!(resolver.stats().layout_queries, 2);
    assert_eq!(resolver.stats().cache_hits, 1);
    assert_eq!(resolver.stats().seed_reset_visits, 1);
}

#[test]
fn query_measurer_preserves_specialized_text_measurement() {
    struct Specialized;
    impl TextMeasurer for Specialized {
        fn measure_with_font(&self, _: &str, _: f32, _: &str, _: u16, _: bool) -> (f32, f32) {
            (5.0, 5.0)
        }
        fn measure_visual_width_with_font(&self, _: &str, _: f32, _: &str, _: u16, _: bool) -> f32 {
            37.0
        }
        fn measure_text_layout_with_font(
            &self,
            _: &str,
            _: f32,
            _: &str,
            _: u16,
            _: bool,
        ) -> (f32, f32) {
            (77.0, 19.0)
        }
        fn font_metrics(&self, _: f32, _: &str, _: u16, _: bool) -> (f32, f32) {
            (15.0, 4.0)
        }
    }
    let mut tree = tree();
    let text = tree.get_mut(&id(2)).unwrap();
    text.spec.kind = ElementKind::Text;
    text.spec.declared.width = Some(Length::Content);
    text.spec.declared.height = Some(Length::Content);
    text.spec.declared.content = Some("text".into());
    let endpoints = [(id(2), Axis::Width), (id(2), Axis::Height)];
    let output = EndpointResolver::default()
        .resolve(
            &tree,
            0,
            Arc::new(Projection::default()),
            context(),
            &endpoints,
            &Specialized,
        )
        .unwrap();
    assert_eq!(output[&(id(2), Axis::Width)].visible, 77.0);
    assert_eq!(output[&(id(2), Axis::Height)].visible, 19.0);
}

#[test]
fn normal_layout_keeps_loading_and_read_only_image_lookup_stages() {
    #[derive(Default)]
    struct Images {
        loads: Cell<u32>,
        reads: Cell<u32>,
    }
    impl TextMeasurer for Images {
        fn measure_with_font(&self, _: &str, _: f32, _: &str, _: u16, _: bool) -> (f32, f32) {
            (0.0, 0.0)
        }
        fn font_metrics(&self, _: f32, _: &str, _: u16, _: bool) -> (f32, f32) {
            (0.0, 0.0)
        }
        fn image_dimensions(&self, _: &ImageSource, load: bool) -> Option<(u32, u32)> {
            let counter = if load { &self.loads } else { &self.reads };
            counter.set(counter.get() + 1);
            Some((222, 111))
        }
    }
    let mut tree = tree();
    let image = tree.get_mut(&id(2)).unwrap();
    image.spec.kind = ElementKind::Image;
    image.spec.declared.width = Some(Length::Fill);
    image.spec.declared.height = Some(Length::Content);
    image.spec.declared.image_src = Some(ImageSource::Id("metric-provider".into()));
    let metrics = Images::default();
    layout_tree(&mut tree, Constraint::new(600.0, 100.0), 1.0, &metrics);
    assert!(metrics.loads.get() > 0 && metrics.reads.get() > 0);
    let frame = tree.get(&id(2)).unwrap().layout.frame.unwrap();
    assert_eq!((frame.width, frame.height), (300.0, 150.0));
}

#[test]
fn declaration_preimages_restore_absent_attrs_and_do_not_leak_across_queries() {
    let mut source = tree();
    source.get_mut(&id(1)).unwrap().spec.declared.width = Some(Length::Px(800.0));
    let original = source.get(&id(1)).unwrap().spec.declared.clone();
    let mut before = original.clone();
    before.width = None;
    before.padding = Some(Padding::Uniform(20.0));
    let projection = Arc::new(Projection {
        nodes: HashMap::from([(
            id(1),
            NodeProjection {
                model: Some(Arc::new(before)),
                ..Default::default()
            },
        )]),
    });
    let normal = Arc::new(Projection::default());
    let context = context();
    let mut resolver = EndpointResolver::default();
    for request in [
        projection.clone(),
        normal.clone(),
        projection.clone(),
        normal,
    ] {
        let actual = resolve(&mut resolver, &source, request.clone(), context.clone());
        let fresh = resolve(
            &mut EndpointResolver::default(),
            &source,
            request,
            context.clone(),
        );
        assert_eq!(actual, fresh);
        assert_eq!(source.get(&id(1)).unwrap().spec.declared, original);
    }
    assert_eq!(resolver.stats().model_copies, 1);
}

#[test]
fn replay_requires_actual_old_metric_facts_and_complete_seed_membership() {
    let old = context();
    let mut next = (*old).clone();
    next.metrics_epoch += 1;
    assert!(
        !next.replayable_change_from(&old),
        "a version alone does not preserve old metrics"
    );
    next.metrics_epoch = old.metrics_epoch;
    next.constraint = Constraint::new(800.0, 100.0);
    assert!(next.replayable_change_from(&old));
    next.seeds
        .insert(id(1), QuerySeed::capture(tree().get(&id(1)).unwrap()));
    assert!(
        !next.replayable_change_from(&old),
        "new runtime keys have no captured preimage"
    );
}

#[test]
fn slider_imposed_widths_do_not_become_intrinsics_in_later_native_queries() {
    let mut tree = tree();
    tree.get_mut(&id(1)).unwrap().spec.kind = ElementKind::Slider;
    let context = context();
    let mut resolver = EndpointResolver::default();
    for length in [
        Length::Fill,
        Length::Px(80.0),
        Length::Fill,
        Length::Px(275.0),
        Length::Px(40.0),
        Length::Fill,
    ] {
        let projection = width(1, length);
        let actual = resolve(
            &mut resolver,
            &tree,
            Arc::clone(&projection),
            Arc::clone(&context),
        );
        let expected = resolve(
            &mut EndpointResolver::default(),
            &tree,
            projection,
            Arc::clone(&context),
        );
        assert_eq!(actual, expected);
        assert_eq!(
            resolver
                .workspace
                .as_ref()
                .unwrap()
                .tree
                .get(&id(2))
                .unwrap()
                .layout
                .effective
                .width,
            Some(Length::Px(40.0))
        );
    }
}
