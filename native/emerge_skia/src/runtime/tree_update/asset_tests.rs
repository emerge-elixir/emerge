use super::*;
use crate::{
    render_scene::RenderScene,
    renderer::{RenderFrame, RenderState, SceneRenderer},
    tree::{
        attrs::{Attrs, Background, Color, ImageFit, ImageSource, Length},
        element::{Element, ElementKind, NearbyMount, NearbySlot},
    },
};

fn options() -> TreeUpdateOptions<'static> {
    TreeUpdateOptions::new(None, TreeUpdateDecodePolicy::ReturnErr)
}

fn output(effect: TreeUpdateEffect) -> Box<LayoutOutput> {
    match effect {
        TreeUpdateEffect::Layout { output, .. } => output,
        _ => panic!("expected a published scene"),
    }
}

fn background_tree(source: ImageSource, nearby: bool) -> ElementTree {
    let mut tree = ElementTree::new();
    for (id, background) in [
        (NodeId(1), None),
        (
            NodeId(2),
            Some(Background::Color(Color::Rgb { r: 0, g: 0, b: 255 })),
        ),
        (
            NodeId(3),
            Some(Background::Image {
                source,
                fit: ImageFit::Contain,
            }),
        ),
    ] {
        tree.insert(Element::with_attrs(
            id,
            ElementKind::El,
            vec![],
            Attrs {
                width: Some(Length::Px(16.0)),
                height: Some(Length::Px(16.0)),
                background,
                on_mouse_move: Some(true),
                ..Default::default()
            },
        ));
    }
    tree.set_root_id(NodeId(1));
    if nearby {
        tree.set_nearby_mounts(
            &NodeId(1),
            vec![NearbyMount {
                slot: NearbySlot::InFront,
                id: NodeId(2),
            }],
        )
        .unwrap();
    } else {
        tree.set_children(&NodeId(1), vec![NodeId(2)]).unwrap();
    }
    tree.set_children(&NodeId(2), vec![NodeId(3)]).unwrap();
    tree
}

fn pixels(scene: RenderScene) -> Vec<u8> {
    let info = skia_safe::ImageInfo::new(
        (16, 16),
        skia_safe::ColorType::RGBA8888,
        skia_safe::AlphaType::Premul,
        None,
    );
    let mut surface = skia_safe::surfaces::raster(&info, None, None).unwrap();
    let mut renderer = SceneRenderer::new();
    renderer.render(
        &mut RenderFrame::new(&mut surface, None),
        &RenderState::new(scene, skia_safe::Color::TRANSPARENT, 1, false),
    );
    let mut pixels = vec![0; 16 * 16 * 4];
    assert!(surface.read_pixels(&info, &mut pixels[..], 16 * 4, (0, 0)));
    pixels
}

#[test]
fn idle_background_completion_refreshes_retained_paint_without_layout_changes() {
    background_completion(false);
}

#[test]
fn idle_svg_background_completion_refreshes_retained_paint_without_layout_changes() {
    background_completion(true);
}

fn register(id: &str, vector: bool, green: bool) {
    if vector {
        let color = if green { "lime" } else { "red" };
        let svg = format!(
            "<svg xmlns='http://www.w3.org/2000/svg' width='1' height='1'><rect width='1' height='1' fill='{color}'/></svg>"
        );
        crate::renderer::insert_vector_asset(
            id,
            resvg::usvg::Tree::from_str(&svg, &resvg::usvg::Options::default()).unwrap(),
        )
        .unwrap();
    } else {
        let rgba = if green {
            [0, 255, 0, 255]
        } else {
            [255, 0, 0, 255]
        };
        crate::renderer::insert_test_raster_asset_rgba(id, 1, 1, &rgba).unwrap();
    }
}

fn background_completion(vector: bool) {
    for nearby in [false, true] {
        for slow in [false, true] {
            let runtime = assets::AssetRuntime::new();
            let _guard = runtime.enter();
            let source = ImageSource::Id("idle-background".into());
            let mut engine = TreeUpdateEngine::new(background_tree(source.clone(), nearby), 16, 16);
            let blank = output(
                engine
                    .process_messages(vec![TreeMsg::RebuildRegistry], options())
                    .unwrap(),
            );
            assert_eq!(pixels(blank.scene.clone()), [0, 0, 255, 255].repeat(256));
            let deadline = assets::next_loading_indicator_deadline().unwrap();
            if slow {
                let indicator = output(
                    engine
                        .process_messages_at(vec![], options(), deadline)
                        .unwrap(),
                );
                assert_eq!(
                    indicator.scene.summary().image_loading,
                    1,
                    "nearby={nearby}"
                );
            }
            register("idle-background", vector, false);
            assets::ensure_source(&source);
            let ready_at = if slow {
                deadline + Duration::from_millis(1)
            } else {
                deadline - Duration::from_millis(1)
            };
            let ready = output(
                engine
                    .process_messages_at(vec![TreeMsg::AssetStateChanged], options(), ready_at)
                    .unwrap(),
            );
            assert_eq!(
                ready.scene.summary().images,
                1,
                "nearby={nearby}, slow={slow}"
            );
            assert_eq!(pixels(ready.scene.clone()), [255, 0, 0, 255].repeat(256));
            assert!(!ready.event_rebuild_changed);
            assert!(!ready.animations_active);
            register("idle-background", vector, true);
            assets::ensure_source(&source);
            let replaced = output(
                engine
                    .process_messages(vec![TreeMsg::AssetStateChanged], options())
                    .unwrap(),
            );
            assert_eq!(pixels(replaced.scene), [0, 255, 0, 255].repeat(256));
            // Replacing same-sized content cannot rebind historical scenes.
            assert_eq!(pixels(ready.scene), [255, 0, 0, 255].repeat(256));
            assert_eq!(pixels(blank.scene), [0, 0, 255, 255].repeat(256));
            assert_eq!(assets::next_loading_indicator_deadline(), None);
            assert!(matches!(
                engine.process_messages(vec![], options()).unwrap(),
                TreeUpdateEffect::Skip
            ));
        }
    }
}

#[test]
fn idle_background_failure_replaces_blank_or_loading_paint_in_cached_ancestors() {
    for slow in [false, true] {
        let runtime = assets::AssetRuntime::new();
        let _guard = runtime.enter();
        let source = ImageSource::Id("failed-background".into());
        let mut engine = TreeUpdateEngine::new(background_tree(source.clone(), true), 16, 16);
        let blank = output(
            engine
                .process_messages(vec![TreeMsg::RebuildRegistry], options())
                .unwrap(),
        );
        let deadline = assets::next_loading_indicator_deadline().unwrap();
        if slow {
            let indicator = output(
                engine
                    .process_messages_at(vec![], options(), deadline)
                    .unwrap(),
            );
            assert_eq!(indicator.scene.summary().image_loading, 1);
        }
        assets::resolve_tree_sources_sync(engine.tree(), None).unwrap();
        assert_eq!(
            assets::source_status(&source),
            Some(assets::AssetStatus::Failed)
        );
        let failed = output(
            engine
                .process_messages(vec![TreeMsg::AssetStateChanged], options())
                .unwrap(),
        );
        assert_eq!(failed.scene.summary().image_failed, 1);
        assert_eq!(failed.scene.summary().image_loading, 0);
        assert_ne!(pixels(failed.scene), pixels(blank.scene));
        assert!(!failed.event_rebuild_changed);
        assert!(!failed.animations_active);
        assert_eq!(assets::next_loading_indicator_deadline(), None);
        assert!(matches!(
            engine.process_messages(vec![], options()).unwrap(),
            TreeUpdateEffect::Skip
        ));
    }
}

struct Wake(crossbeam_channel::Sender<()>);
impl crate::backend::wake::BackendWake for Wake {
    fn request_stop(&self) {}
    fn request_redraw(&self) {
        let _ = self.0.send(());
    }
    fn notify_video_frame(&self) {}
}

struct Actor {
    tx: crossbeam_channel::Sender<TreeMsg>,
    handle: Option<std::thread::JoinHandle<()>>,
}
impl Drop for Actor {
    fn drop(&mut self) {
        let _ = self.tx.send(TreeMsg::Stop);
        self.handle.take().unwrap().join().unwrap();
    }
}

struct TestImage(std::path::PathBuf);
impl Drop for TestImage {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

#[test]
fn idle_runtime_path_completion_queues_static_scene_and_backend_wake() {
    use crate::{
        RenderSender,
        actors::RenderMsg,
        backend::wake::BackendWakeHandle,
        runtime::tree_actor::{TreeActorConfig, spawn_tree_actor_with_initial_tree},
    };
    use crossbeam_channel::{bounded, unbounded};
    use std::sync::atomic::AtomicU64;
    let file = TestImage(std::env::temp_dir().join(format!(
        "emerge-issue-107-{}-{}.jpg",
        std::process::id(),
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()
    )));
    std::fs::copy(
        concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../priv/sample_assets/static.jpg"
        ),
        &file.0,
    )
    .unwrap();
    let runtime = assets::AssetRuntime::new();
    let _guard = runtime.enter();
    runtime.configure(assets::AssetConfig {
        runtime_enabled: true,
        runtime_allowlist: vec![std::env::temp_dir().to_string_lossy().into_owned()],
        ..Default::default()
    });
    let source = ImageSource::RuntimePath(file.0.to_string_lossy().into_owned());
    let (tx, rx) = bounded(64);
    let (event_tx, _events) = unbounded();
    let (render_tx, scenes) = bounded(8);
    let (wake_tx, wakes) = unbounded();
    let _actor = Actor {
        tx: tx.clone(),
        handle: Some(spawn_tree_actor_with_initial_tree(
            rx,
            TreeActorConfig {
                render_sender: RenderSender {
                    tx: render_tx,
                    drop_rx: scenes.clone(),
                    log_render: false,
                },
                event_tx,
                render_counter: Arc::new(AtomicU64::new(0)),
                stats: None,
                log_input: false,
                window_wake: BackendWakeHandle::new(Wake(wake_tx)),
                initial_width: 16,
                initial_height: 16,
                asset_context: runtime.context(),
            },
            background_tree(source.clone(), true),
        )),
    };
    tx.send(TreeMsg::RebuildRegistry).unwrap();
    let RenderMsg::Scene {
        scene: blank,
        version: first,
        animate: false,
        ..
    } = scenes.recv_timeout(Duration::from_secs(5)).unwrap()
    else {
        panic!("expected initial static scene")
    };
    wakes.recv_timeout(Duration::from_secs(5)).unwrap();
    assert_eq!(blank.summary().images, 0);
    // Start real file I/O only after the pending scene was published. Completion
    // must be the sole cause of the ready scene, not an animation or tree patch.
    runtime.start(tx, false);
    assets::ensure_source(&source);
    let until = Instant::now() + Duration::from_secs(5);
    loop {
        let RenderMsg::Scene {
            scene,
            version,
            animate,
            ..
        } = scenes
            .recv_timeout(until.saturating_duration_since(Instant::now()))
            .unwrap()
        else {
            panic!("expected completion scene")
        };
        wakes.recv_timeout(Duration::from_secs(5)).unwrap();
        assert!(!animate);
        if scene.summary().images == 0 {
            assert_eq!(scene.summary().image_loading, 1);
            continue;
        }
        assert!(version > first);
        assert_eq!(scene.summary().images, 1);
        assert_ne!(pixels(*scene), pixels((*blank).clone()));
        break;
    }
    assert!(matches!(
        scenes.recv_timeout(Duration::from_millis(150)),
        Err(crossbeam_channel::RecvTimeoutError::Timeout)
    ));
    assert!(wakes.is_empty());
}

#[test]
fn successive_same_sized_background_sources_draw_on_completion_without_animation() {
    let runtime = assets::AssetRuntime::new();
    let _guard = runtime.enter();
    let mut engine = TreeUpdateEngine::new(
        background_tree(ImageSource::Id("initial".into()), true),
        16,
        16,
    );
    output(
        engine
            .process_messages(vec![TreeMsg::RebuildRegistry], options())
            .unwrap(),
    );
    for name in ["snapshot-a", "snapshot-b", "snapshot-c", "snapshot-d"] {
        // Fixed 16px width/height and an ID image background, encoded exactly
        // like a path-changing application patch, without an enter animation.
        let attrs = [
            vec![0, 3, 1, 2],
            16.0_f64.to_be_bytes().to_vec(),
            vec![2, 2],
            16.0_f64.to_be_bytes().to_vec(),
            vec![12, 2, 0],
            (name.len() as u16).to_be_bytes().to_vec(),
            name.as_bytes().to_vec(),
            vec![0],
        ]
        .concat();
        let bytes = [
            vec![1],
            3_u64.to_be_bytes().to_vec(),
            (attrs.len() as u32).to_be_bytes().to_vec(),
            attrs,
        ]
        .concat();
        let pending = output(
            engine
                .process_messages(
                    vec![TreeMsg::PatchTree {
                        bytes,
                        submitted_at: None,
                    }],
                    options(),
                )
                .unwrap(),
        );
        assert_eq!(pending.scene.summary().images, 0);
        register(name, false, true);
        assets::ensure_source(&ImageSource::Id(name.into()));
        let ready = output(
            engine
                .process_messages(vec![TreeMsg::AssetStateChanged], options())
                .unwrap(),
        );
        assert_eq!(ready.scene.summary().images, 1);
        assert_eq!(pixels(ready.scene), [0, 255, 0, 255].repeat(256));
        assert!(!ready.animations_active);
        assert!(matches!(
            engine.process_messages(vec![], options()).unwrap(),
            TreeUpdateEffect::Skip
        ));
    }
}
