use super::*;

fn config() -> AssetConfig {
    AssetConfig {
        sources: vec![format!("{}/../../priv", env!("CARGO_MANIFEST_DIR"))],
        ..AssetConfig::default()
    }
}

fn source(file: &str) -> ImageSource {
    ImageSource::Logical(format!("test_assets/{file}.svg"))
}

fn request(source: ImageSource) -> LoadRequest {
    let context = current_context();
    let mut state = context.state.lock().unwrap();
    state.set_source_status(source.clone(), AssetStatus::Pending);
    state.pending_count += 1;
    new_load_request(&mut state, source, true)
}

fn worker() -> (Worker, crossbeam_channel::Receiver<TreeMsg>) {
    let (tree_tx, rx) = bounded(16);
    (
        Worker {
            tree_tx,
            state: current_context().state,
            log_render: false,
        },
        rx,
    )
}

#[test]
fn parsed_completion_after_scene_removal_is_cached_without_resurrecting_source() {
    let runtime = AssetRuntime::new();
    let _guard = runtime.enter();
    runtime.configure(config());
    let src = source("cache_complex");
    let req = request(src.clone());
    let result = load_source_in_epoch(&src, &req.config, req.epoch, true).unwrap();
    ensure_tree_sources(&ElementTree::new()); // leave before parse result is published
    let (mut worker, rx) = worker();
    worker.complete_load(req, Ok(result.clone()));
    assert!(rx.is_empty());
    assert!(source_status(&src).is_none());
    assert!(asset_record(&result.id).is_some()); // parsed cache, not active source
    assert!(
        !current_context()
            .state
            .lock()
            .unwrap()
            .records
            .contains_key(&result.id)
    );
    ensure_source(&src);
    assert_eq!(source_status(&src), Some(AssetStatus::Ready(result)));
    assert_eq!(svg_cache_stats().parses, 1);
    assert_eq!(svg_cache_stats().rasterizations, 0);
}

#[test]
fn stale_result_cannot_complete_a_new_request_for_the_same_source() {
    let runtime = AssetRuntime::new();
    let _guard = runtime.enter();
    runtime.configure(config());
    let src = source("cache_complex");
    let old = request(src.clone());
    let result = load_source_in_epoch(&src, &old.config, old.epoch, true).unwrap();
    ensure_tree_sources(&ElementTree::new());
    let new = request(src.clone());
    let (mut worker, rx) = worker();
    worker.complete_load(old, Ok(result.clone()));
    assert!(rx.is_empty());
    assert_eq!(source_status(&src), Some(AssetStatus::Pending));
    worker.handle_load(new);
    assert!(matches!(rx.try_recv(), Ok(TreeMsg::AssetStateChanged)));
    assert_eq!(source_status(&src), Some(AssetStatus::Ready(result)));
    assert_eq!(svg_cache_stats().parses, 1);
}

#[test]
fn configuration_epoch_rejects_late_registration_and_completion() {
    let runtime = AssetRuntime::new();
    let _guard = runtime.enter();
    runtime.configure(config());
    let src = source("cache_complex");
    let old = request(src.clone());
    let result = load_source_in_epoch(&src, &old.config, old.epoch, true).unwrap();
    let old_epoch = old.epoch;
    runtime.configure(AssetConfig {
        sources: vec!["/not-authorized-anymore".into()],
        ..config()
    });
    let (mut worker, rx) = worker();
    worker.complete_load(old, Ok(result.clone()));
    assert!(rx.is_empty());
    assert!(asset_record(&result.id).is_none());
    assert!(source_status(&src).is_none());
    let tree = usvg::Tree::from_str(
        "<svg xmlns='http://www.w3.org/2000/svg' width='1' height='1'/>",
        &usvg::Options::default(),
    )
    .unwrap();
    assert!(register_vector_asset_in_epoch("late", "late", tree, 1, old_epoch, 1).is_err());
    assert_eq!(svg_cache_stats().entries, 0);
    ensure_source(&src);
    assert_eq!(source_status(&src), Some(AssetStatus::Pending));
    assert!(asset_record(&result.id).is_none());
}

#[test]
fn concurrent_loaders_share_font_discovery_and_parsing() {
    let runtime = AssetRuntime::new();
    let _guard = runtime.enter();
    runtime.configure(config());
    let threads: Vec<_> = (0..8)
        .map(|_| {
            let context = runtime.context();
            thread::spawn(move || {
                let _guard = context.enter();
                load_source_in_epoch(&source("cache_complex"), &config(), current_epoch(), true)
                    .unwrap()
            })
        })
        .collect();
    let ids: HashSet<_> = threads
        .into_iter()
        .map(|handle| handle.join().unwrap().id)
        .collect();
    assert_eq!(ids.len(), 1);
    assert_eq!(svg_cache_stats().parses, 1);
    assert_eq!(svg_cache_stats().font_discoveries, 1);
}

#[test]
fn parsed_lru_and_estimated_budget_evict_without_duplicate_charges() {
    let runtime = AssetRuntime::new();
    let _guard = runtime.enter();
    runtime.configure(AssetConfig {
        svg_tree_max_entries: 2,
        ..config()
    });
    let (mut worker, _) = worker();
    let load = |worker: &mut Worker, src: ImageSource| {
        worker.handle_load(request(src.clone()));
        let Some(AssetStatus::Ready(asset)) = source_status(&src) else {
            panic!("not ready");
        };
        ensure_tree_sources(&ElementTree::new());
        asset
    };
    let a = load(&mut worker, source("cache_complex"));
    let b = load(&mut worker, source("single_axis_square"));
    let a_record = asset_record(&a.id).unwrap(); // a is now the newer tree
    let a_charge = estimated_tree_bytes(&a_record);
    let c = load(&mut worker, source("single_axis_wide"));
    assert!(asset_record(&b.id).is_none());
    assert!(asset_record(&a.id).is_some());
    assert!(asset_record(&c.id).is_some());
    assert_eq!(svg_cache_stats().entries, 2);
    let before = svg_cache_stats().estimated_bytes;
    load(&mut worker, source("cache_complex"));
    assert_eq!(svg_cache_stats().estimated_bytes, before);
    {
        let context = current_context();
        let mut state = context.state.lock().unwrap();
        state.svg_cache.configure(2, a_charge - 1);
        assert!(state.svg_cache.stats.estimated_bytes < a_charge);
    }
    assert!(asset_record(&a.id).is_none());
    assert_eq!(svg_cache_stats().font_discoveries, 1);
}

#[test]
fn replacing_source_status_releases_unreferenced_active_records() {
    let runtime = AssetRuntime::new();
    let _guard = runtime.enter();
    runtime.configure(config());
    let src = source("cache_complex");
    let (mut worker, _) = worker();
    worker.handle_load(request(src.clone()));
    let Some(AssetStatus::Ready(asset)) = source_status(&src) else {
        panic!("not ready");
    };
    let context = current_context();
    let mut state = context.state.lock().unwrap();
    state.set_source_status(src, AssetStatus::Failed);
    assert!(!state.records.contains_key(&asset.id));
    assert!(state.svg_cache.get(&asset.id).is_some()); // separate bounded ownership
}

#[test]
fn revalidation_reuses_unchanged_parses_and_publishes_changed_or_deleted_sources() {
    let runtime = AssetRuntime::new();
    let _guard = runtime.enter();
    let root = std::env::temp_dir().join(format!(
        "emerge-svg-revalidate-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir(&root).unwrap();
    struct Cleanup(PathBuf);
    impl Drop for Cleanup {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
    let _cleanup = Cleanup(root.clone());
    let path = root.join("changing.svg");
    fs::write(
        &path,
        "<svg xmlns='http://www.w3.org/2000/svg' width='200' height='200'/>",
    )
    .unwrap();
    runtime.configure(AssetConfig {
        sources: vec![root.display().to_string()],
        ..config()
    });
    let src = ImageSource::Logical("changing.svg".into());
    let (mut worker, _) = worker();
    worker.handle_load(request(src.clone()));
    let Some(AssetStatus::Ready(original)) = source_status(&src) else {
        panic!("not ready");
    };
    let revalidate = || {
        let context = current_context();
        let mut state = context.state.lock().unwrap();
        new_load_request(&mut state, src.clone(), false)
    };
    ensure_tree_sources(&ElementTree::new());
    ensure_source(&src);
    assert_eq!(
        source_status(&src),
        Some(AssetStatus::Ready(original.clone()))
    );
    worker.handle_load(revalidate());
    assert_eq!(svg_cache_stats().parses, 1);
    fs::write(
        &path,
        "<svg xmlns='http://www.w3.org/2000/svg' width='100' height='50'/>",
    )
    .unwrap();
    worker.handle_load(revalidate());
    let Some(AssetStatus::Ready(changed)) = source_status(&src) else {
        panic!("not ready");
    };
    assert_ne!(changed.id, original.id);
    assert_eq!((changed.width, changed.height), (100, 50));
    assert_eq!(svg_cache_stats().parses, 2);
    assert_eq!(svg_cache_stats().font_discoveries, 1);
    assert!(
        !current_context()
            .state
            .lock()
            .unwrap()
            .records
            .contains_key(&original.id)
    );
    fs::remove_file(&path).unwrap();
    worker.handle_load(revalidate());
    assert_eq!(source_status(&src), Some(AssetStatus::Failed));
    assert!(
        !current_context()
            .state
            .lock()
            .unwrap()
            .records
            .contains_key(&changed.id)
    );
}

#[test]
fn hydration_is_deduplicated_and_invalidates_pre_eviction_layer_fingerprints() {
    let runtime = AssetRuntime::new();
    let _guard = runtime.enter();
    runtime.configure(config());
    let src = source("cache_complex");
    let (mut worker, _) = worker();
    worker.handle_load(request(src.clone()));
    let Some(AssetStatus::Ready(asset)) = source_status(&src) else {
        panic!("not ready");
    };
    let original_revision = asset_record(&asset.id)
        .unwrap()
        .render_revision
        .load(Ordering::Relaxed);
    let context = current_context();
    {
        let mut state = context.state.lock().unwrap();
        state.records.clear();
        state.svg_cache.configure(0, 0);
    }
    let (tx, rx) = bounded(16);
    *context.tx.lock().unwrap() = Some(tx);
    for _ in 0..8 {
        request_asset_hydration(&asset.id);
    }
    assert_eq!(rx.len(), 1);
    let AssetMsg::Load(request) = rx.recv().unwrap() else {
        panic!("not a load");
    };
    worker.handle_load(request);
    assert_ne!(
        asset_record(&asset.id)
            .unwrap()
            .render_revision
            .load(Ordering::Relaxed),
        original_revision
    );
    assert!(asset_record(&asset.id).is_some());
    ensure_tree_sources(&ElementTree::new());
    assert!(asset_record(&asset.id).is_none());
}

#[test]
fn stale_completion_after_replacement_publication_preserves_animation_and_retained_images() {
    use crate::renderer::{RenderFrame, RenderState, SceneRenderer};
    use crate::runtime::tree_update::{
        TreeUpdateDecodePolicy, TreeUpdateEffect, TreeUpdateEngine, TreeUpdateOptions,
    };
    use crate::tree::{
        animation::{AnimationCurve, AnimationRepeat, AnimationSpec},
        attrs::{Attrs, Length},
        element::{Element, ElementKind, NodeId},
    };
    use std::time::{Duration, Instant};
    let paint = |renderer: &mut SceneRenderer, scene: crate::render_scene::RenderScene, version| {
        let info = skia_safe::ImageInfo::new(
            (200, 60),
            skia_safe::ColorType::RGBA8888,
            skia_safe::AlphaType::Premul,
            None,
        );
        let mut surface = skia_safe::surfaces::raster(&info, None, None).unwrap();
        renderer.render(
            &mut RenderFrame::new(&mut surface, None),
            &RenderState::new(scene, skia_safe::Color::TRANSPARENT, version, false),
        );
        let mut pixels = vec![0; 200 * 60 * 4];
        assert!(surface.read_pixels(&info, &mut pixels[..], 200 * 4, (0, 0)));
        pixels
    };
    for policy in [
        TreeUpdateDecodePolicy::ReturnErr,
        TreeUpdateDecodePolicy::LogAndContinue,
    ] {
        let runtime = AssetRuntime::new();
        let _assets = runtime.enter();
        runtime.configure(config());
        let src = source("cache_complex");
        let first = request(src.clone());
        let original = load_source_in_epoch(&src, &first.config, first.epoch, true).unwrap();
        let (mut loader, rx) = worker();
        loader.complete_load(first, Ok(original.clone()));
        let _ = rx.try_recv();
        let mut tree = ElementTree::new();
        let id = |n| NodeId::from_wire_u64(n);
        tree.insert(Element::with_attrs(
            id(1),
            ElementKind::Row,
            vec![],
            Attrs {
                width: Some(Length::Fill),
                height: Some(Length::Fill),
                ..Default::default()
            },
        ));
        tree.insert(Element::with_attrs(
            id(2),
            ElementKind::Image,
            vec![],
            Attrs {
                image_src: Some(src.clone()),
                height: Some(Length::Px(20.0)),
                animate: Some(AnimationSpec {
                    keyframes: vec![
                        Attrs {
                            width: Some(Length::Px(40.0)),
                            ..Default::default()
                        },
                        Attrs {
                            width: Some(Length::Fill),
                            ..Default::default()
                        },
                    ],
                    duration_ms: 1000.0,
                    curve: AnimationCurve::Linear,
                    repeat: AnimationRepeat::Once,
                }),
                ..Default::default()
            },
        ));
        tree.insert(Element::with_attrs(
            id(3),
            ElementKind::El,
            vec![],
            Attrs {
                width: Some(Length::Fill),
                ..Default::default()
            },
        ));
        tree.set_children(&id(1), vec![id(2), id(3)]).unwrap();
        tree.set_root_id(id(1));
        tree.set_revision(1);
        let mut engine = TreeUpdateEngine::new(tree, 200, 60);
        let start = Instant::now();
        let pulse = |us| TreeMsg::AnimationPulse {
            presented_at: start + Duration::from_micros(us),
            predicted_next_present_at: start + Duration::from_micros(us),
            trace: None,
        };
        let layout = |engine: &mut TreeUpdateEngine, messages| match engine
            .process_messages(messages, TreeUpdateOptions::new(None, policy))
            .unwrap()
        {
            TreeUpdateEffect::Layout { output, .. } => output,
            _ => panic!("expected native publication"),
        };
        layout(&mut engine, vec![pulse(0)]);
        let before = layout(&mut engine, vec![pulse(500_000)]);
        let mut renderer = SceneRenderer::new();
        let old_pixels = paint(&mut renderer, before.scene.clone(), 1);
        let obsolete = request(src.clone());
        let replacement = request(src.clone());
        let svg=usvg::Tree::from_str("<svg xmlns='http://www.w3.org/2000/svg' width='160' height='20'><rect width='160' height='20' fill='lime'/></svg>",&usvg::Options::default()).unwrap();
        let record = register_vector_asset_in_epoch(
            &original.id,
            &original.id,
            svg,
            120,
            current_epoch(),
            next_render_revision(),
        )
        .unwrap();
        let next = ResolvedAsset {
            id: original.id.clone(),
            width: 160,
            height: 20,
        };
        loader.complete_load(replacement, Ok(next.clone()));
        let message = rx.try_recv().unwrap();
        let changed = layout(&mut engine, vec![message, pulse(600_000)]);
        assert_ne!(old_pixels, paint(&mut renderer, changed.scene, 2));
        loader.complete_load(obsolete, Ok(original));
        assert!(rx.is_empty());
        assert_eq!(source_status(&src), Some(AssetStatus::Ready(next)));
        assert!(Arc::ptr_eq(&record, &asset_record(&record.id).unwrap()));
        assert_eq!(old_pixels, paint(&mut renderer, before.scene.clone(), 3));
        let final_frame = layout(&mut engine, vec![pulse(1_000_000)]);
        assert!(
            !final_frame.animations_active,
            "asset replacement must not restart the original run"
        );
        assert!(
            engine
                .tree()
                .get(&id(2))
                .unwrap()
                .layout
                .dimension_samples
                .is_none()
        );
        assert_eq!(old_pixels, paint(&mut renderer, before.scene, 4));
    }
}

#[test]
fn replacement_and_reconfiguration_between_prepare_and_publish_keep_one_asset_input() {
    use crate::renderer::{RenderFrame, RenderState, SceneRenderer};
    use crate::tree::{
        animation::{AnimationCurve, AnimationRepeat, AnimationRuntime, AnimationSpec},
        attrs::{Attrs, Length},
        element::{Element, ElementKind, NodeId},
        layout::{self, Constraint},
    };
    let id = |n| NodeId::from_wire_u64(n);
    for logical in [false, true] {
        for reset in [false, true] {
            for failed in [false, true] {
                for micros in [0, 500_000, 1_000_000] {
                    let assets = AssetRuntime::new();
                    let _assets = assets.enter();
                    assets.configure(config());
                    let source = if logical {
                        source("cache_complex")
                    } else {
                        ImageSource::Id("atomic-image".into())
                    };
                    let register = |width, color| {
                        let svg=usvg::Tree::from_str(&format!("<svg xmlns='http://www.w3.org/2000/svg' width='{width}' height='20'><rect width='{width}' height='20' fill='{color}'/></svg>"),&usvg::Options::default()).unwrap();
                        register_vector_asset("atomic-image", svg).unwrap();
                        let resolved = ResolvedAsset {
                            id: "atomic-image".into(),
                            width,
                            height: 20,
                        };
                        current_context().state.lock().unwrap().set_source_status(
                            source.clone(),
                            AssetStatus::Ready(resolved.clone()),
                        );
                        resolved
                    };
                    let original = register(40, "red");
                    let mut tree = ElementTree::new();
                    tree.insert(Element::with_attrs(
                        id(1),
                        ElementKind::Row,
                        vec![],
                        Attrs {
                            width: Some(Length::Fill),
                            height: Some(Length::Fill),
                            ..Default::default()
                        },
                    ));
                    tree.insert(Element::with_attrs(
                        id(4),
                        ElementKind::Image,
                        vec![],
                        Attrs {
                            image_src: Some(source.clone()),
                            width: Some(Length::Content),
                            height: Some(Length::Px(20.0)),
                            ..Default::default()
                        },
                    ));
                    tree.insert(Element::with_attrs(
                        id(2),
                        ElementKind::El,
                        vec![],
                        Attrs {
                            height: Some(Length::Px(20.0)),
                            animate: Some(AnimationSpec {
                                keyframes: vec![
                                    Attrs {
                                        width: Some(Length::Px(40.0)),
                                        ..Default::default()
                                    },
                                    Attrs {
                                        width: Some(Length::Fill),
                                        ..Default::default()
                                    },
                                ],
                                duration_ms: 1000.0,
                                curve: AnimationCurve::Linear,
                                repeat: AnimationRepeat::Once,
                            }),
                            ..Default::default()
                        },
                    ));
                    tree.insert(Element::with_attrs(
                        id(3),
                        ElementKind::El,
                        vec![],
                        Attrs {
                            width: Some(Length::Fill),
                            ..Default::default()
                        },
                    ));
                    tree.set_children(&id(1), vec![id(4), id(2), id(3)])
                        .unwrap();
                    tree.set_root_id(id(1));
                    tree.set_revision(1);
                    let mut animation = AnimationRuntime::default();
                    let start = Instant::now();
                    let constraint = Constraint::new(200.0, 40.0);
                    layout::layout_and_refresh_default_with_animation(
                        &mut tree,
                        constraint,
                        1.0,
                        &mut animation,
                        start,
                    )
                    .unwrap();
                    let old = tree.frame_assets.clone().unwrap();
                    let at = start + Duration::from_micros(micros);
                    animation.sync_with_tree(&tree, at);
                    let preparation = layout::prepare_frame_attrs_for_update(
                        &mut tree,
                        1.0,
                        Some(&mut animation),
                        Some(at),
                    );
                    let obsolete = request(source.clone());
                    let (mut loader, rx) = worker();
                    // A separate writer must finish while the preparation remains alive:
                    // neither asset lock is retained across the native query/publication gap.
                    std::thread::scope(|scope| {
                        scope
                            .spawn(|| {
                                let _assets = assets.enter();
                                if reset {
                                    assets.configure(config());
                                }
                                let replacement = request(source.clone());
                                let resolved = register(80, "lime");
                                let (mut worker, _) = worker();
                                worker.complete_load(replacement, Ok(resolved));
                            })
                            .join()
                            .unwrap();
                    });
                    loader.complete_load(obsolete, Ok(original));
                    assert!(rx.is_empty());
                    let expected = source_status(&source).unwrap();
                    if failed {
                        tree.get_mut(&id(2)).unwrap(); // invalidate exclusive preparation authority
                        assert!(
                            preparation
                                .publish(&mut tree, &mut animation, constraint, true, None)
                                .is_err()
                        );
                        assert!(Arc::ptr_eq(tree.frame_assets.as_ref().unwrap(), &old));
                    } else {
                        let (output, _) = preparation
                            .publish(&mut tree, &mut animation, constraint, true, None)
                            .unwrap_or_else(|err|panic!("logical={logical} reset={reset} failed={failed} micros={micros}: {err:?}"));
                        assert_eq!(tree.get(&id(4)).unwrap().layout.frame.unwrap().width, 40.0);
                        assert!(
                            (tree.get(&id(2)).unwrap().layout.frame.unwrap().width
                                - (40.0 + 40.0 * micros as f32 / 1_000_000.0))
                                .abs()
                                < 0.01
                        );
                        let info = skia_safe::ImageInfo::new(
                            (200, 40),
                            skia_safe::ColorType::RGBA8888,
                            skia_safe::AlphaType::Premul,
                            None,
                        );
                        let mut surface = skia_safe::surfaces::raster(&info, None, None).unwrap();
                        SceneRenderer::new().render(
                            &mut RenderFrame::new(&mut surface, None),
                            &RenderState::new(
                                output.output.scene,
                                skia_safe::Color::TRANSPARENT,
                                1,
                                false,
                            ),
                        );
                        let mut pixels = vec![0; 200 * 40 * 4];
                        assert!(surface.read_pixels(&info, &mut pixels[..], 800, (0, 0)));
                        assert_eq!(
                            &pixels[(10 * 200 + 10) * 4..(10 * 200 + 10) * 4 + 4],
                            &[255, 0, 0, 255]
                        );
                    }
                    assert_eq!(source_status(&source), Some(expected)); // frame scope restored
                    let result = layout::layout_and_refresh_default_with_animation(
                        &mut tree,
                        constraint,
                        1.0,
                        &mut animation,
                        start + Duration::from_micros(1_100_000),
                    )
                    .unwrap();
                    assert!(!result.animations_active);
                    assert_eq!(tree.get(&id(4)).unwrap().layout.frame.unwrap().width, 80.0);
                    assert_eq!(tree.get(&id(2)).unwrap().layout.frame.unwrap().width, 60.0);
                }
            }
        }
    }
}

#[test]
fn pending_frame_facts_and_status_generation_do_not_observe_late_readiness() {
    let assets = AssetRuntime::new();
    let _assets = assets.enter();
    assets.configure(config());
    let src = ImageSource::Id("late-frame".into());
    ensure_source(&src);
    let frame = Arc::new(FrameAssets::capture(&FrameSourceList {
        all: vec![src.clone()],
        measured: vec![],
    }));
    let generation = source_status_generation();
    let before = svg_cache_stats();
    {
        let _frame = frame.enter();
        std::thread::scope(|scope| {
            scope
                .spawn(|| {
                    let _assets = assets.enter();
                    assets.configure(config());
                    let svg = usvg::Tree::from_str(
                        "<svg xmlns='http://www.w3.org/2000/svg' width='80' height='20'/>",
                        &usvg::Options::default(),
                    )
                    .unwrap();
                    register_vector_asset("late-frame", svg).unwrap();
                    ensure_source(&src);
                })
                .join()
                .unwrap();
        });
        assert_eq!(source_status(&src), Some(AssetStatus::Pending));
        assert_eq!(source_status_generation(), generation);
        assert_eq!(source_dimensions(&src), None);
        assert_eq!(crate::renderer::asset_kind("late-frame"), None);
    }
    assert_eq!(source_dimensions(&src), Some((80, 20)));
    assert_eq!(svg_cache_stats().rasterizations, before.rasterizations);
    let _again = frame.enter();
    assert_eq!(source_dimensions(&src), None);
}

#[test]
fn paint_only_background_edits_and_interaction_sources_refresh_the_frame_source_list() {
    use crate::tree::{
        attrs::{Attrs, Background, ImageFit, Length, MouseOverAttrs},
        element::{Element, ElementKind, NodeId},
        layout::{Constraint, layout_and_refresh_default},
        patch::{Patch, apply_patches},
    };
    let assets = AssetRuntime::new();
    let _assets = assets.enter();
    for name in ["a", "b", "focus", "down"] {
        let svg = usvg::Tree::from_str(
            "<svg xmlns='http://www.w3.org/2000/svg' width='40' height='20'/>",
            &usvg::Options::default(),
        )
        .unwrap();
        register_vector_asset(name, svg).unwrap();
    }
    let background = |name: &str| Background::Image {
        source: ImageSource::Id(name.into()),
        fit: ImageFit::Contain,
    };
    let mut tree = ElementTree::new();
    tree.insert(Element::with_attrs(
        NodeId(1),
        ElementKind::El,
        vec![],
        Attrs {
            width: Some(Length::Px(40.0)),
            height: Some(Length::Px(20.0)),
            background: Some(background("a")),
            focused: Some(MouseOverAttrs {
                background: Some(background("focus")),
                ..Default::default()
            }),
            mouse_down: Some(MouseOverAttrs {
                background: Some(background("down")),
                ..Default::default()
            }),
            ..Default::default()
        },
    ));
    tree.set_root_id(NodeId(1));
    let output = layout_and_refresh_default(&mut tree, Constraint::new(100.0, 100.0), 1.0);
    let frame = tree.frame_assets.as_ref().unwrap();
    assert!(frame.sources.contains_key(&ImageSource::Id("focus".into())));
    assert!(frame.sources.contains_key(&ImageSource::Id("down".into())));
    assert_eq!(output.scene.images.unwrap().retention().bindings, 1);
    let model = tree.layout_model_epoch;
    let raw = [
        vec![0, 3, 1, 2],
        40.0_f64.to_be_bytes().to_vec(),
        vec![2, 2],
        20.0_f64.to_be_bytes().to_vec(),
        vec![12, 2, 0, 0, 1, b'b', 0],
    ]
    .concat();
    // First remove unused interaction declarations, then test a pure background
    // replacement between otherwise identical declarations.
    apply_patches(
        &mut tree,
        vec![Patch::SetAttrs {
            id: NodeId(1),
            attrs_raw: raw.clone(),
        }],
    )
    .unwrap();
    let output = layout_and_refresh_default(&mut tree, Constraint::new(100.0, 100.0), 1.0);
    assert_eq!(output.scene.images.unwrap().dimensions("b"), Some((40, 20)));
    let model = tree.layout_model_epoch.max(model);
    let mut raw = raw;
    let index = raw.len() - 2;
    raw[index] = b'a';
    apply_patches(
        &mut tree,
        vec![Patch::SetAttrs {
            id: NodeId(1),
            attrs_raw: raw,
        }],
    )
    .unwrap();
    assert_eq!(tree.layout_model_epoch, model);
    let now = std::time::Instant::now();
    let mut runtime = crate::tree::animation::AnimationRuntime::default();
    runtime.sync_with_tree(&tree, now);
    let preparation = crate::tree::layout::prepare_frame_attrs_for_update(
        &mut tree,
        1.0,
        Some(&mut runtime),
        Some(now),
    );
    assert!(
        matches!(
            preparation.animation_result.invalidation,
            crate::tree::invalidation::TreeInvalidation::None
                | crate::tree::invalidation::TreeInvalidation::Registry
                | crate::tree::invalidation::TreeInvalidation::Paint
        ),
        "decorative image bindings do not dirty intrinsic measurement"
    );
    drop(preparation);
    let output = layout_and_refresh_default(&mut tree, Constraint::new(100.0, 100.0), 1.0);
    assert_eq!(output.scene.images.unwrap().dimensions("a"), Some((40, 20)));
    tree.get_mut(&NodeId(1)).unwrap().spec.declared.background = Some(background("focus"));
    let output = layout_and_refresh_default(&mut tree, Constraint::new(100.0, 100.0), 1.0);
    assert_eq!(
        output.scene.images.unwrap().dimensions("focus"),
        Some((40, 20))
    );
}

#[test]
fn frozen_frame_lookups_do_not_resurrect_sources_after_epoch_reset() {
    use crate::tree::{
        attrs::Attrs,
        element::{Element, ElementKind, NodeId},
    };
    let assets = AssetRuntime::new();
    let _assets = assets.enter();
    assets.configure(config());
    let svg = || {
        usvg::Tree::from_str(
            "<svg xmlns='http://www.w3.org/2000/svg' width='40' height='20'/>",
            &usvg::Options::default(),
        )
        .unwrap()
    };
    let source = ImageSource::Id("outer-frame".into());
    register_vector_asset("outer-frame", svg()).unwrap();
    ensure_source(&source);
    let outer = Arc::new(FrameAssets::capture(&FrameSourceList {
        all: vec![source.clone()],
        measured: vec![],
    }));
    assets.configure(config());
    let generation = source_status_generation();
    {
        let _outer = outer.enter();
        ensure_source(&source);
        assert_eq!(source_dimensions(&source), Some((40, 20)));
        {
            let state = current_context().state;
            let state = state.lock().unwrap();
            assert!(
                !state.sources.contains_key(&source),
                "paint/query lookup must not write a new-epoch source"
            );
            assert_eq!(state.status_generation, generation);
        }
        // A genuinely new preparation is distinct from a lookup in the frozen
        // frame and must still register its references, even when nested.
        let inner_source = ImageSource::Id("inner-frame".into());
        register_vector_asset("inner-frame", svg()).unwrap();
        let mut tree = ElementTree::new();
        tree.insert(Element::with_attrs(
            NodeId(1),
            ElementKind::Image,
            vec![],
            Attrs {
                image_src: Some(inner_source.clone()),
                ..Default::default()
            },
        ));
        tree.set_root_id(NodeId(1));
        let inner = tree.capture_frame_assets();
        {
            let _inner = inner.enter();
            assert_eq!(source_dimensions(&inner_source), Some((40, 20)));
            assert_eq!(source_status(&source), None);
        }
        assert_eq!(source_dimensions(&source), Some((40, 20)));
    }
    assert_eq!(source_status(&source), None);
    ensure_source(&source);
    assert_eq!(source_status(&source), Some(AssetStatus::Pending));
}
