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
struct Sink(
    Mutex<Vec<(NodeId, ElementEventKind)>>,
    Mutex<Vec<InputEvent>>,
);
impl HostEventSink for Sink {
    fn send_raw_input(&self, event: &InputEvent) {
        self.1.lock().unwrap().push(event.clone());
    }
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
    render_tap: Option<Sender<RenderMsg>>,
    last_pixels: Vec<u8>,
    defer_install: bool,
    deferred: std::collections::VecDeque<(
        crate::events::RegistryRebuildPayload,
        crate::events::RegistryRebuildPayload,
    )>,
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
            render_tap: None,
            last_pixels: Vec::new(),
            defer_install: false,
            deferred: Default::default(),
        }
    }
    fn registry(&mut self, rebuild: crate::events::RegistryRebuildPayload) {
        let EventMsg::RegistryUpdate { rebuild: other } =
            self.events.recv_timeout(Duration::from_secs(5)).unwrap()
        else {
            panic!("registry");
        };
        assert_registry_rebuild_payloads_equivalent(&rebuild, &other);
        if self.defer_install {
            self.deferred.push_back((rebuild, other));
        } else {
            self.direct_events.install_rebuild(rebuild);
            self.actor_events.install_rebuild(other);
        }
    }
    fn flush_deferred(&mut self) {
        for _ in 0..128 {
            let Some((a, b)) = self.deferred.pop_front() else {
                return;
            };
            self.direct_events.install_rebuild(a);
            self.actor_events.install_rebuild(b);
            let messages = self.direct_events.drain_tree_messages();
            let other = self.actor_events.drain_tree_messages();
            assert_eq!(format!("{messages:?}"), format!("{other:?}"));
            if !messages.is_empty() {
                self.round_trip_pair(messages, other);
            }
        }
        panic!("deferred registry feedback did not settle");
    }
    fn round_trip(&mut self, messages: Vec<TreeMsg>) {
        self.round_trip_pair(messages.clone(), messages);
    }
    fn round_trip_pair(&mut self, mut messages: Vec<TreeMsg>, mut other: Vec<TreeMsg>) {
        for _ in 0..16 {
            let effect = self
                .engine
                .process_messages(
                    messages.clone(),
                    TreeUpdateOptions::new(None, TreeUpdateDecodePolicy::ReturnErr),
                )
                .unwrap();
            self.tx.send(TreeMsg::Batch(other)).unwrap();
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
                    if let Some(tap) = &self.render_tap {
                        tap.send(RenderMsg::Scene {
                            scene: scene.clone(),
                            version,
                            animate,
                            ime_enabled,
                            ime_cursor_area,
                            ime_text_state: ime_text_state.clone(),
                            pipeline_submitted_at: None,
                            pipeline_render_queued_at: None,
                            animation_trace: None,
                        })
                        .unwrap();
                    }
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
                    if self.render_tap.is_some() {
                        self.last_pixels = a.data.clone();
                    }
                    if self.retained.is_none() {
                        self.retained = Some((output.scene, a.data));
                    }
                    self.frames += 1;
                }
                TreeUpdateEffect::Skip => {}
                TreeUpdateEffect::Stop => panic!("unexpected stop"),
            }
            messages = self.direct_events.drain_tree_messages();
            other = self.actor_events.drain_tree_messages();
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
            self.round_trip_pair(messages, other);
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

#[test]
fn delayed_registry_install_replays_keyboard_ime_and_wheel_during_animation_without_losing_input() {
    let assets = assets::AssetRuntime::new();
    let _assets = assets.enter();
    for queued in [1, 8, 32] {
        let mut h = Harness::new(assets.context());
        let start = Instant::now();
        let pulse = |us| TreeMsg::AnimationPulse {
            presented_at: start + Duration::from_micros(us),
            predicted_next_present_at: start + Duration::from_micros(us),
            trace: None,
        };
        h.round_trip(vec![pulse(0)]);
        let initial = h.direct_events.focused_text_state().unwrap().content;
        h.defer_install = true;
        for index in 1..=queued {
            h.round_trip(vec![pulse(index * 10000)]);
        }
        h.input(InputEvent::TextCommit {
            text: "X".into(),
            mods: 0,
        });
        h.input(InputEvent::TextPreedit {
            text: "ime".into(),
            cursor: Some((1, 1)),
        });
        h.input(InputEvent::TextCommit {
            text: "Y".into(),
            mods: 0,
        });
        h.input(InputEvent::CursorScroll {
            dx: 0.0,
            dy: -40.0,
            x: 10.0,
            y: 40.0,
        });
        h.round_trip(vec![pulse(500000)]);
        h.flush_deferred();
        let final_state = h.direct_events.focused_text_state().unwrap();
        assert!(final_state.content.contains("XY"), "{final_state:?}");
        assert_eq!(final_state.content.replace("XY", ""), initial);
        assert!(final_state.preedit.is_none());
        assert_eq!(h.actor_events.focused_text_state(), Some(final_state));
        assert!(h.engine.tree().get(&NodeId(2)).unwrap().layout.scroll_y > 0.0);
        h.defer_install = false;
        h.round_trip(vec![pulse(1000000)]);
        h.round_trip(vec![pulse(1100000)]);
        assert!(
            h.engine
                .tree()
                .get(&NodeId(2))
                .unwrap()
                .layout
                .dimension_samples
                .is_none()
        );
        assert_eq!(*h.sinks[0].0.lock().unwrap(), *h.sinks[1].0.lock().unwrap());
        assert!(h.events.is_empty());
        assert!(h.deferred.is_empty());
        let (scene, old_pixels) = h.retained.take().unwrap();
        let retained = RenderState::new(scene, Color::TRANSPARENT, 0, false);
        assert_eq!(raster().render(&retained).data, old_pixels);
    }
}

#[test]
fn delayed_pointer_release_and_nearby_hover_match_actor_and_headless_damage() {
    let assets = assets::AssetRuntime::new();
    let _assets = assets.enter();
    for queued in [1, 8, 32] {
        let mut h = Harness::new(assets.context());
        h.direct_events.set_input_mask(u32::MAX);
        h.actor_events.set_input_mask(u32::MAX);
        let start = Instant::now();
        let pulse = |us| TreeMsg::AnimationPulse {
            presented_at: start + Duration::from_micros(us),
            predicted_next_present_at: start + Duration::from_micros(us),
            trace: None,
        };
        h.round_trip(vec![pulse(0)]);
        h.input(InputEvent::CursorEntered { entered: true });
        h.input(InputEvent::CursorPos { x: 410.0, y: 10.0 });
        h.input(InputEvent::CursorButton {
            button: "left".into(),
            action: 1,
            mods: 0,
            x: 10.0,
            y: 10.0,
        });
        h.defer_install = true;
        for index in 1..=queued {
            h.round_trip(vec![pulse(index * 10000)]);
        }
        h.input(InputEvent::TextCommit {
            text: "X".into(),
            mods: 0,
        });
        h.input(InputEvent::CursorPos { x: 150.0, y: 10.0 });
        h.input(InputEvent::CursorPos { x: 70.0, y: 10.0 });
        h.input(InputEvent::CursorButton {
            button: "left".into(),
            action: 0,
            mods: 0,
            x: 70.0,
            y: 10.0,
        });
        h.input(InputEvent::CursorPos { x: 410.0, y: 10.0 });
        h.round_trip(vec![pulse(500000)]);
        h.flush_deferred();
        h.defer_install = false;
        let selected = h.direct_events.focused_text_state().unwrap();
        assert!(selected.selection_anchor.is_some());
        assert!(selected.cursor > 0);
        h.input(InputEvent::CursorPos { x: 0.0, y: 10.0 });
        assert_eq!(
            h.direct_events.focused_text_state(),
            Some(selected),
            "motion after release must not keep changing selection"
        );
        assert!(
            !h.engine
                .tree()
                .get(&NodeId(5))
                .unwrap()
                .runtime
                .mouse_over_active
        );
        let events = h.sinks[0].0.lock().unwrap();
        assert!(events.contains(&(NodeId(5), ElementEventKind::MouseEnter)));
        assert!(events.contains(&(NodeId(5), ElementEventKind::MouseLeave)));
        drop(events);
        assert_eq!(*h.sinks[0].1.lock().unwrap(), *h.sinks[1].1.lock().unwrap());
        assert_eq!(
            h.sinks[0].1.lock().unwrap().len(),
            9,
            "buffered replay and synthetic cursor checks do not duplicate observer input"
        );
        h.round_trip(vec![pulse(1000000)]);
        h.round_trip(vec![pulse(1100000)]);
        assert!(
            h.engine
                .tree()
                .get(&NodeId(2))
                .unwrap()
                .layout
                .dimension_samples
                .is_none()
        );
        let (scene, pixels) = h.retained.take().unwrap();
        assert_eq!(
            raster()
                .render(&RenderState::new(scene, Color::TRANSPARENT, 0, false))
                .data,
            pixels
        );
    }
}

#[test]
fn queued_old_mount_edit_matches_actor_and_headless_after_replacement() {
    let assets = assets::AssetRuntime::new();
    let _assets = assets.enter();
    let mut h = Harness::new(assets.context());
    let now = Instant::now();
    h.round_trip(vec![TreeMsg::AnimationPulse {
        presented_at: now,
        predicted_next_present_at: now,
        trace: None,
    }]);
    assert!(h.direct_events.focused_text_state().is_some());
    let input = InputEvent::TextCommit {
        text: "X".into(),
        mods: 0,
    };
    h.direct_events.handle_input(input.clone());
    h.actor_events.handle_input(input);
    let queued = h.direct_events.drain_tree_messages();
    let other = h.actor_events.drain_tree_messages();
    assert!(!queued.is_empty());
    let raw: Vec<_> = 3u16
        .to_be_bytes()
        .into_iter()
        .chain([1, 2])
        .chain(200f64.to_be_bytes())
        .chain([2, 2])
        .chain(30f64.to_be_bytes())
        .chain([56, 1])
        .collect();
    let attrs = crate::tree::attrs::decode_attrs(&raw).unwrap();
    let mut replacement = ElementTree::new();
    replacement.insert(Element::with_attrs(
        NodeId(3),
        ElementKind::TextInput,
        raw,
        attrs,
    ));
    replacement.set_root_id(NodeId(3));
    h.round_trip(vec![TreeMsg::UploadTree {
        bytes: crate::tree::serialize::encode_tree(&replacement),
        submitted_at: None,
    }]);
    h.round_trip_pair(queued, other);
    assert_eq!(
        h.engine
            .tree()
            .get(&NodeId(3))
            .unwrap()
            .spec
            .declared
            .content
            .as_deref()
            .unwrap_or_default(),
        ""
    );
    assert!(h.direct_events.focused_text_state().is_none());
    assert!(h.actor_events.focused_text_state().is_none());
    assert_eq!(*h.sinks[0].0.lock().unwrap(), *h.sinks[1].0.lock().unwrap());
    let (scene, pixels) = h.retained.take().unwrap();
    assert_eq!(
        raster()
            .render(&RenderState::new(scene, Color::TRANSPARENT, 0, false))
            .data,
        pixels
    );
}
