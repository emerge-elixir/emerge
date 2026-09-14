use super::*;
use crate::{
    actors::EventMsg,
    events::{
        ElementEventKind, HostEventRuntime, HostEventSink,
        registry_builder::{ElixirEventPayload, assert_registry_rebuild_payloads_equivalent},
    },
    input::InputEvent,
    runtime::{
        tree_actor::spawn_tree_actor_with_initial_tree,
        tree_update::{
            TreeUpdateDecodePolicy, TreeUpdateEffect, TreeUpdateEngine, TreeUpdateOptions,
        },
    },
    tree::{
        animation::{AnimationCurve, AnimationRepeat, AnimationSpec},
        attrs::{Attrs, Background, Color as TreeColor, Length},
        element::{Element, ElementKind, ElementTree, NearbyMount, NearbySlot, NodeId},
    },
};
use std::sync::{Arc, Mutex};

#[derive(Default)]
struct Sink(Mutex<Vec<(NodeId, ElementEventKind)>>);
impl HostEventSink for Sink {
    fn send_raw_input(&self, _: &InputEvent) {}
    fn send_element_event(
        &self,
        id: &NodeId,
        kind: ElementEventKind,
        _: Option<&ElixirEventPayload>,
    ) {
        self.0.lock().unwrap().push((*id, kind));
    }
}
fn tree() -> ElementTree {
    let mut tree = ElementTree::new();
    tree.insert(Element::with_attrs(
        NodeId(1),
        ElementKind::Column,
        vec![],
        Attrs {
            width: Some(Length::Fill),
            height: Some(Length::Fill),
            ..Default::default()
        },
    ));
    tree.insert(Element::with_attrs(
        NodeId(2),
        ElementKind::Column,
        vec![],
        Attrs {
            height: Some(Length::Fill),
            scrollbar_y: Some(true),
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
        NodeId(3),
        ElementKind::TextInput,
        vec![],
        Attrs {
            width: Some(Length::Px(200.0)),
            height: Some(Length::Px(30.0)),
            content: Some("abcdef".into()),
            focus_on_mount: Some(true),
            on_focus: Some(true),
            on_change: Some(true),
            ..Default::default()
        },
    ));
    tree.insert(Element::with_attrs(
        NodeId(4),
        ElementKind::El,
        vec![],
        Attrs {
            width: Some(Length::Px(200.0)),
            height: Some(Length::Px(140.0)),
            background: Some(Background::Color(TreeColor::Rgb { r: 0, g: 0, b: 255 })),
            ..Default::default()
        },
    ));
    tree.insert(Element::with_attrs(
        NodeId(5),
        ElementKind::El,
        vec![],
        Attrs {
            width: Some(Length::Px(40.0)),
            height: Some(Length::Px(20.0)),
            move_x: Some(400.0),
            on_mouse_enter: Some(true),
            on_mouse_leave: Some(true),
            background: Some(Background::Color(TreeColor::Rgb { r: 255, g: 0, b: 0 })),
            ..Default::default()
        },
    ));
    tree.set_children(&NodeId(1), vec![NodeId(2)]).unwrap();
    tree.set_children(&NodeId(2), vec![NodeId(3), NodeId(4)])
        .unwrap();
    tree.set_nearby_mounts(
        &NodeId(1),
        vec![NearbyMount {
            id: NodeId(5),
            slot: NearbySlot::InFront,
        }],
    )
    .unwrap();
    tree.set_root_id(NodeId(1));
    tree.set_revision(1);
    tree.stamp_all_mounted_at_revision(1);
    tree
}
struct Harness {
    engine: TreeUpdateEngine,
    tx: Sender<TreeMsg>,
    events: Receiver<EventMsg>,
    renders: Receiver<RenderMsg>,
    handle: Option<std::thread::JoinHandle<()>>,
    direct_events: HostEventRuntime,
    actor_events: HostEventRuntime,
    sinks: [Arc<Sink>; 2],
    direct_raster: RasterHeadlessRenderer,
    actor_raster: RasterHeadlessRenderer,
    retained: Option<(crate::render_scene::RenderScene, Vec<u8>)>,
    frames: usize,
}
fn raster() -> RasterHeadlessRenderer {
    RasterHeadlessRenderer::new(
        600,
        60,
        RendererCacheConfig::default(),
        RasterPixelFormat::Rgba8888Premul,
    )
    .unwrap()
}
impl Harness {
    fn new(context: assets::AssetContext) -> Self {
        let (tx, rx) = bounded(64);
        let (event_tx, events) = bounded(64);
        let (render_tx, renders) = bounded(8);
        let tree = tree();
        let handle = spawn_tree_actor_with_initial_tree(
            rx,
            TreeActorConfig {
                render_sender: RenderSender {
                    tx: render_tx,
                    drop_rx: renders.clone(),
                    log_render: false,
                },
                event_tx,
                render_counter: Arc::new(std::sync::atomic::AtomicU64::new(0)),
                stats: None,
                log_input: false,
                window_wake: BackendWakeHandle::noop(),
                initial_width: 600,
                initial_height: 60,
                asset_context: context,
            },
            tree.clone(),
        );
        let sinks = [Arc::new(Sink::default()), Arc::new(Sink::default())];
        Self {
            engine: TreeUpdateEngine::new(tree, 600, 60),
            tx,
            events,
            renders,
            handle: Some(handle),
            direct_events: HostEventRuntime::new(false, 40.0, false, sinks[0].clone(), None),
            actor_events: HostEventRuntime::new(false, 40.0, false, sinks[1].clone(), None),
            sinks,
            direct_raster: raster(),
            actor_raster: raster(),
            retained: None,
            frames: 0,
        }
    }
    fn registry(&mut self, rebuild: crate::events::RegistryRebuildPayload) {
        let EventMsg::RegistryUpdate { rebuild: other } =
            self.events.recv_timeout(Duration::from_secs(5)).unwrap()
        else {
            panic!("registry");
        };
        assert_registry_rebuild_payloads_equivalent(&rebuild, &other);
        self.direct_events.install_rebuild(rebuild);
        self.actor_events.install_rebuild(other);
    }
    fn round_trip(&mut self, mut messages: Vec<TreeMsg>) {
        for _ in 0..16 {
            let effect = self
                .engine
                .process_messages(
                    messages.clone(),
                    TreeUpdateOptions::new(None, TreeUpdateDecodePolicy::ReturnErr),
                )
                .unwrap();
            self.tx.send(TreeMsg::Batch(messages)).unwrap();
            match effect {
                TreeUpdateEffect::RegistryUpdate { rebuild } => self.registry(rebuild),
                TreeUpdateEffect::Layout { output, .. } => {
                    if output.event_rebuild_changed {
                        self.registry(output.event_rebuild.clone());
                    }
                    let RenderMsg::Scene {
                        scene,
                        version,
                        animate,
                        ime_enabled,
                        ime_cursor_area,
                        ime_text_state,
                        ..
                    } = self.renders.recv_timeout(Duration::from_secs(5)).unwrap()
                    else {
                        panic!("scene");
                    };
                    assert_eq!(animate, output.animations_active);
                    assert_eq!(ime_enabled, output.ime_enabled);
                    assert_eq!(ime_cursor_area, output.ime_cursor_area);
                    assert_eq!(*ime_text_state, output.ime_text_state);
                    let expected = RenderState::new(
                        output.scene.clone(),
                        Color::TRANSPARENT,
                        version,
                        animate,
                    );
                    let actual = RenderState::new(*scene, Color::TRANSPARENT, version, animate);
                    let a = self.direct_raster.render(&expected);
                    let b = self.actor_raster.render(&actual);
                    assert_eq!(a.row_bytes, b.row_bytes);
                    assert_eq!(a.data, b.data);
                    assert_eq!(
                        a.data,
                        raster().render(&expected).data,
                        "retained damage must match a fresh raster surface"
                    );
                    if self.retained.is_none() {
                        self.retained = Some((output.scene, a.data));
                    }
                    self.frames += 1;
                }
                TreeUpdateEffect::Skip => {}
                TreeUpdateEffect::Stop => panic!("unexpected stop"),
            }
            messages = self.direct_events.drain_tree_messages();
            let other = self.actor_events.drain_tree_messages();
            assert_eq!(format!("{messages:?}"), format!("{other:?}"));
            if messages.is_empty() {
                return;
            }
        }
        panic!("input/registry feedback did not settle");
    }
    fn input(&mut self, event: InputEvent) {
        self.direct_events.handle_input(event.clone());
        self.actor_events.handle_input(event);
        let messages = self.direct_events.drain_tree_messages();
        let other = self.actor_events.drain_tree_messages();
        assert_eq!(format!("{messages:?}"), format!("{other:?}"));
        if !messages.is_empty() {
            self.round_trip(messages);
        }
        assert_eq!(
            *self.sinks[0].0.lock().unwrap(),
            *self.sinks[1].0.lock().unwrap()
        );
    }
}
impl Drop for Harness {
    fn drop(&mut self) {
        let _ = self.tx.send(TreeMsg::Stop);
        if let Some(handle) = self.handle.take() {
            handle.join().unwrap();
        }
    }
}

#[test]
fn actor_direct_and_raster_headless_match_focus_ime_drag_wheel_nearby_and_final_damage() {
    let assets = assets::AssetRuntime::new();
    let _assets = assets.enter();
    let mut h = Harness::new(assets.context());
    let start = Instant::now();
    let pulse = |us| TreeMsg::AnimationPulse {
        presented_at: start + Duration::from_micros(us),
        predicted_next_present_at: start + Duration::from_micros(us),
        trace: None,
    };
    h.round_trip(vec![pulse(0)]);
    assert!(h.direct_events.focused_text_state().is_some());
    h.input(InputEvent::CursorEntered { entered: true });
    h.input(InputEvent::CursorPos { x: 10.0, y: 10.0 });
    h.round_trip(vec![pulse(250_000)]);
    h.input(InputEvent::TextPreedit {
        text: "xy".into(),
        cursor: Some((1, 1)),
    });
    assert_eq!(
        h.direct_events.focused_text_state().unwrap().preedit,
        Some("xy".into())
    );
    h.input(InputEvent::TextCommit {
        text: "Z".into(),
        mods: 0,
    });
    assert!(
        h.direct_events
            .focused_text_state()
            .unwrap()
            .content
            .contains('Z')
    );
    h.round_trip(vec![pulse(500_000)]);
    h.input(InputEvent::CursorButton {
        button: "left".into(),
        action: 1,
        mods: 0,
        x: 10.0,
        y: 10.0,
    });
    h.input(InputEvent::CursorPos { x: 70.0, y: 10.0 });
    assert!(
        h.direct_events
            .focused_text_state()
            .unwrap()
            .selection_anchor
            .is_some()
    );
    h.input(InputEvent::CursorButton {
        button: "left".into(),
        action: 0,
        mods: 0,
        x: 70.0,
        y: 10.0,
    });
    h.input(InputEvent::Key {
        key: crate::keys::CanonicalKey::ArrowLeft,
        action: 1,
        mods: 0,
    });
    h.input(InputEvent::CursorScroll {
        dx: 0.0,
        dy: -40.0,
        x: 10.0,
        y: 40.0,
    });
    assert!(h.engine.tree().get(&NodeId(2)).unwrap().layout.scroll_y > 0.0);
    h.round_trip(vec![pulse(750_000)]);
    h.input(InputEvent::CursorPos { x: 410.0, y: 10.0 });
    assert!(
        h.sinks[0]
            .0
            .lock()
            .unwrap()
            .iter()
            .any(|(id, kind)| *id == NodeId(5) && *kind == ElementEventKind::MouseEnter)
    );
    h.round_trip(vec![pulse(1_000_000)]);
    h.round_trip(vec![pulse(1_100_000)]);
    assert!(
        h.engine
            .tree()
            .get(&NodeId(2))
            .unwrap()
            .layout
            .dimension_samples
            .is_none()
    );
    assert!(h.frames >= 8);
    let (scene, original) = h.retained.take().unwrap();
    assert_eq!(
        original,
        h.actor_raster
            .render(&RenderState::new(scene, Color::TRANSPARENT, 10_000, false))
            .data
    );
}
