use super::*;
use crate::events::{ScrollPayload, scroll::ScrollPosition};

#[derive(Default)]
struct ScrollEvents(Mutex<Vec<ScrollPayload>>);
impl HostEventSink for ScrollEvents {
    fn send_raw_input(&self, _: &InputEvent) {}
    fn send_element_event(
        &self,
        _: &NodeId,
        kind: ElementEventKind,
        payload: Option<&registry_builder::ElixirEventPayload>,
    ) {
        if kind == ElementEventKind::Scroll
            && let Some(ElixirEventPayload::Scroll(value)) = payload
        {
            self.0.lock().unwrap().push(*value);
        }
    }
}
fn dimensions(width: f64, height: f64) -> impl Iterator<Item = u8> {
    [1, 2]
        .into_iter()
        .chain(width.to_be_bytes())
        .chain([2, 2])
        .chain(height.to_be_bytes())
}
fn root_attrs(position: Option<ScrollPosition>, emit: bool) -> Vec<u8> {
    let value: Vec<_> = match position {
        None => vec![],
        Some(ScrollPosition::Fraction(bits)) => {
            [86, 0].into_iter().chain(bits.to_be_bytes()).collect()
        }
        Some(ScrollPosition::Target { declaration, node }) => [86, 1]
            .into_iter()
            .chain(declaration.to_be_bytes())
            .chain(node.to_be_bytes())
            .collect(),
    };
    (5u16 + u16::from(position.is_some()))
        .to_be_bytes()
        .into_iter()
        .chain(dimensions(200.0, 100.0))
        .chain([7, 1, 8, 1, 87, u8::from(emit)])
        .chain(value)
        .collect()
}
fn tree(position: Option<ScrollPosition>, emit: bool) -> ElementTree {
    let mut tree = ElementTree::new();
    let raw = root_attrs(position, emit);
    tree.insert(Element::with_attrs(
        NodeId(1),
        ElementKind::Column,
        raw.clone(),
        crate::tree::attrs::decode_attrs(&raw).unwrap(),
    ));
    for id in 2..=5 {
        let raw: Vec<_> = 2u16
            .to_be_bytes()
            .into_iter()
            .chain(dimensions(400.0, 100.0))
            .collect();
        tree.insert(Element::with_attrs(
            NodeId(id),
            ElementKind::El,
            raw.clone(),
            crate::tree::attrs::decode_attrs(&raw).unwrap(),
        ));
    }
    tree.set_children(&NodeId(1), (2..=5).map(NodeId).collect())
        .unwrap();
    tree.set_root_id(NodeId(1));
    tree
}
fn start(
    position: Option<ScrollPosition>,
    emit: bool,
) -> (TreeUpdateEngine, HostEventRuntime, Arc<ScrollEvents>) {
    let mut engine = TreeUpdateEngine::new(ElementTree::new(), 200, 100);
    let events = Arc::new(ScrollEvents::default());
    let mut host = HostEventRuntime::new(false, 40.0, false, events.clone(), None);
    host.install_rebuild(response(
        &mut engine,
        vec![TreeMsg::UploadTree {
            bytes: crate::tree::serialize::encode_tree(&tree(position, emit)),
            submitted_at: None,
        }],
        TreeUpdateDecodePolicy::ReturnErr,
    ));
    settle(&mut engine, &mut host);
    (engine, host, events)
}
fn settle(engine: &mut TreeUpdateEngine, host: &mut HostEventRuntime) {
    for _ in 0..16 {
        let messages = host.drain_tree_messages();
        if messages.is_empty() {
            assert!(!host.driver.runtime.listener_lane.is_stale());
            return;
        }
        assert!(
            messages
                .iter()
                .flat_map(TreeMsg::commands)
                .all(|m| !matches!(
                    m,
                    TreeMsg::ScrollRequest { .. }
                        | TreeMsg::ScrollbarThumbDragX { .. }
                        | TreeMsg::ScrollbarThumbDragY { .. }
                ))
        );
        host.install_rebuild(response(
            engine,
            messages,
            TreeUpdateDecodePolicy::ReturnErr,
        ));
    }
    panic!("scroll feedback loop");
}
fn wheel(engine: &mut TreeUpdateEngine, host: &mut HostEventRuntime, dx: f32, dy: f32) {
    host.handle_input(InputEvent::CursorScroll {
        dx,
        dy,
        x: 50.0,
        y: 50.0,
    });
    settle(engine, host);
}
fn patch(position: Option<ScrollPosition>) -> TreeMsg {
    let raw = root_attrs(position, true);
    TreeMsg::PatchTree {
        bytes: [1]
            .into_iter()
            .chain(NodeId(1).to_be_bytes())
            .chain((raw.len() as u32).to_be_bytes())
            .chain(raw)
            .collect(),
        submitted_at: None,
    }
}
fn app(
    engine: &mut TreeUpdateEngine,
    host: &mut HostEventRuntime,
    position: Option<ScrollPosition>,
) -> RegistryRebuildPayload {
    let rebuild = response(
        engine,
        vec![patch(position)],
        TreeUpdateDecodePolicy::ReturnErr,
    );
    host.install_rebuild(rebuild.clone());
    settle(engine, host);
    rebuild
}
fn fraction(value: f64) -> ScrollPosition {
    ScrollPosition::Fraction(value.to_bits())
}
fn assert_y(engine: &TreeUpdateEngine, host: &HostEventRuntime, value: f64) {
    let session = &host.driver.runtime.scroll_sessions[&NodeId(1)];
    assert!(
        (session.position[1] - value).abs() < 1e-10,
        "live {:?}",
        session.position
    );
    let node = engine.tree().get(&NodeId(1)).unwrap();
    assert!(
        (node.layout.scroll_y as f64 - value * node.layout.scroll_y_max as f64).abs() < 1e-4,
        "mirror {} range {}",
        node.layout.scroll_y,
        node.layout.scroll_y_max
    );
}

#[test]
fn scroll_fractions_initialize_and_publish_diagonal_callbacks_once() {
    let assets = crate::assets::AssetRuntime::new();
    let _guard = assets.enter();
    let (mut engine, mut host, events) = start(Some(fraction(0.5)), true);
    assert_y(&engine, &host, 0.5);
    assert!(events.0.lock().unwrap().is_empty());
    wheel(&mut engine, &mut host, -20.0, -30.0);
    assert_y(&engine, &host, 0.6);
    assert_eq!(
        *events.0.lock().unwrap(),
        [ScrollPayload { x: 0.1, y: 0.6 }]
    );
}
#[test]
fn scroll_delayed_fifo_echoes_do_not_rewind_or_resend() {
    let assets = crate::assets::AssetRuntime::new();
    let _guard = assets.enter();
    let (mut engine, mut host, events) = start(None, true);
    wheel(&mut engine, &mut host, 0.0, -30.0);
    wheel(&mut engine, &mut host, 0.0, -30.0);
    let old = app(&mut engine, &mut host, Some(fraction(0.1)));
    assert_y(&engine, &host, 0.2);
    app(&mut engine, &mut host, Some(fraction(0.2)));
    wheel(&mut engine, &mut host, 0.0, -30.0);
    host.install_rebuild(old);
    assert!(host.drain_tree_messages().is_empty());
    assert_y(&engine, &host, 0.3);
    assert_eq!(events.0.lock().unwrap().len(), 3);
}
#[test]
fn scroll_key_jump_aligns_visible_child_and_clamps_last_child() {
    let assets = crate::assets::AssetRuntime::new();
    let _guard = assets.enter();
    let (mut engine, mut host, events) = start(
        Some(ScrollPosition::Target {
            declaration: 1,
            node: NodeId(3),
        }),
        true,
    );
    assert_y(&engine, &host, 1.0 / 3.0);
    app(
        &mut engine,
        &mut host,
        Some(ScrollPosition::Target {
            declaration: 2,
            node: NodeId(5),
        }),
    );
    assert_y(&engine, &host, 1.0);
    wheel(&mut engine, &mut host, 0.0, 30.0);
    let old = app(
        &mut engine,
        &mut host,
        Some(ScrollPosition::Target {
            declaration: 2,
            node: NodeId(5),
        }),
    );
    assert_y(&engine, &host, 0.9);
    host.install_rebuild(old);
    assert!(host.drain_tree_messages().is_empty());
    assert_eq!(events.0.lock().unwrap().len(), 1);
    app(
        &mut engine,
        &mut host,
        Some(ScrollPosition::Target {
            declaration: 3,
            node: NodeId(999),
        }),
    );
    assert_y(&engine, &host, 0.9);
}
#[test]
fn scroll_resize_preserves_fraction_and_no_callback_history_without_handler() {
    let assets = crate::assets::AssetRuntime::new();
    let _guard = assets.enter();
    let (mut engine, mut host, events) = start(Some(fraction(0.5)), false);
    wheel(&mut engine, &mut host, 0.0, -30.0);
    let raw: Vec<_> = 2u16
        .to_be_bytes()
        .into_iter()
        .chain(dimensions(400.0, 300.0))
        .collect();
    let patch = TreeMsg::PatchTree {
        bytes: [1]
            .into_iter()
            .chain(NodeId(5).to_be_bytes())
            .chain((raw.len() as u32).to_be_bytes())
            .chain(raw)
            .collect(),
        submitted_at: None,
    };
    host.install_rebuild(response(
        &mut engine,
        vec![patch],
        TreeUpdateDecodePolicy::ReturnErr,
    ));
    assert_y(&engine, &host, 0.6);
    assert!(host.drain_tree_messages().is_empty());
    assert!(events.0.lock().unwrap().is_empty());
}
#[test]
fn scroll_app_replacement_survives_older_echo_and_remount_resets_session() {
    let assets = crate::assets::AssetRuntime::new();
    let _guard = assets.enter();
    let (mut engine, mut host, events) = start(None, true);
    wheel(&mut engine, &mut host, 0.0, -30.0);
    app(&mut engine, &mut host, Some(fraction(0.8)));
    app(&mut engine, &mut host, Some(fraction(0.1)));
    assert_y(&engine, &host, 0.8);
    assert_eq!(events.0.lock().unwrap().len(), 1);
    host.install_rebuild(response(
        &mut engine,
        vec![TreeMsg::UploadTree {
            bytes: crate::tree::serialize::encode_tree(&tree(Some(fraction(0.25)), true)),
            submitted_at: None,
        }],
        TreeUpdateDecodePolicy::ReturnErr,
    ));
    settle(&mut engine, &mut host);
    assert_y(&engine, &host, 0.25);
}

#[test]
fn scroll_thumb_drag_publishes_event_owned_progress_and_one_callback_per_motion() {
    let assets = crate::assets::AssetRuntime::new();
    let _guard = assets.enter();
    let (mut engine, mut host, events) = start(Some(fraction(0.5)), true);
    let thumb = host.driver.runtime.scrollbar_nodes[&(NodeId(1), ScrollbarAxis::Y)].thumb_rect;
    let (x, y) = (thumb.x + thumb.width / 2.0, thumb.y + thumb.height / 2.0);
    host.handle_input(super::pointer::button(ACTION_PRESS, x, y));
    settle(&mut engine, &mut host);
    host.handle_input(InputEvent::CursorPos { x, y: y + 10.0 });
    settle(&mut engine, &mut host);
    let position = host.driver.runtime.scroll_sessions[&NodeId(1)].position[1];
    assert!(position > 0.5);
    assert_y(&engine, &host, position);
    assert_eq!(events.0.lock().unwrap().len(), 1);
    host.handle_input(super::pointer::button(ACTION_RELEASE, x, y + 10.0));
    settle(&mut engine, &mut host);
    assert_eq!(events.0.lock().unwrap().len(), 1);
}

#[test]
fn scroll_control_removal_and_reintroduction_only_apply_new_proposals() {
    let assets = crate::assets::AssetRuntime::new();
    let _guard = assets.enter();
    let (mut engine, mut host, events) = start(Some(fraction(0.5)), true);
    wheel(&mut engine, &mut host, 0.0, -30.0);
    app(&mut engine, &mut host, None);
    assert_y(&engine, &host, 0.6);
    app(&mut engine, &mut host, Some(fraction(0.5)));
    assert_y(&engine, &host, 0.5);
    assert_eq!(events.0.lock().unwrap().len(), 1);
}

#[test]
fn scroll_echoes_preserve_momentum_and_app_jumps_cancel_only_the_affected_axis() {
    let assets = crate::assets::AssetRuntime::new();
    let _guard = assets.enter();
    let (mut engine, mut host, events) = start(None, true);
    wheel(&mut engine, &mut host, 0.0, -30.0);
    wheel(&mut engine, &mut host, 0.0, -30.0);
    let arm = |host: &mut HostEventRuntime, axis| {
        let now = Instant::now();
        host.driver.runtime.inertial_scroll = Some(InertialScrollState {
            element_id: NodeId(1),
            axis,
            simulation: AdaptiveScrollSimulation::new(0.0, -1000.0).unwrap(),
            started_at: now,
            last_sample_position: 0.0,
            watchdog_deadline: now + Duration::from_secs(1),
        });
    };
    arm(&mut host, ScrollbarAxis::Y);
    app(&mut engine, &mut host, Some(fraction(0.1)));
    assert_y(&engine, &host, 0.2);
    assert!(host.driver.runtime.inertial_scroll.is_some());
    app(&mut engine, &mut host, Some(fraction(0.8)));
    assert!(host.driver.runtime.inertial_scroll.is_none());
    arm(&mut host, ScrollbarAxis::X);
    app(&mut engine, &mut host, Some(fraction(0.9)));
    assert!(host.driver.runtime.inertial_scroll.is_some());
    assert_eq!(events.0.lock().unwrap().len(), 2);
}

#[test]
fn scroll_zero_range_frame_preserves_latent_fraction_until_content_returns() {
    let assets = crate::assets::AssetRuntime::new();
    let _guard = assets.enter();
    let (mut engine, mut host, events) = start(Some(fraction(0.5)), true);
    wheel(&mut engine, &mut host, 0.0, -30.0);
    for height in [0.0, 100.0] {
        let bytes = (2..=5)
            .flat_map(|id| {
                let raw: Vec<_> = 2u16
                    .to_be_bytes()
                    .into_iter()
                    .chain(dimensions(400.0, height))
                    .collect();
                [1].into_iter()
                    .chain(NodeId(id).to_be_bytes())
                    .chain((raw.len() as u32).to_be_bytes())
                    .chain(raw)
            })
            .collect();
        host.install_rebuild(response(
            &mut engine,
            vec![TreeMsg::PatchTree {
                bytes,
                submitted_at: None,
            }],
            TreeUpdateDecodePolicy::ReturnErr,
        ));
        assert!(host.drain_tree_messages().is_empty());
        assert_y(&engine, &host, 0.6);
        assert_eq!(
            engine.tree().get(&NodeId(1)).unwrap().layout.scroll_y_max,
            if height == 0.0 { 0.0 } else { 300.0 }
        );
    }
    assert_eq!(events.0.lock().unwrap().len(), 1);
}

#[test]
fn scroll_disabling_all_axes_without_handlers_resets_the_native_mirror() {
    let assets = crate::assets::AssetRuntime::new();
    let _guard = assets.enter();
    let (mut engine, mut host, _) = start(Some(fraction(0.5)), false);
    let disabled: Vec<_> = 3u16
        .to_be_bytes()
        .into_iter()
        .chain(dimensions(200.0, 100.0))
        .chain([86, 0])
        .chain(0.5f64.to_be_bytes())
        .collect();
    for raw in [disabled, root_attrs(Some(fraction(0.5)), false)] {
        let bytes = [1]
            .into_iter()
            .chain(NodeId(1).to_be_bytes())
            .chain((raw.len() as u32).to_be_bytes())
            .chain(raw)
            .collect();
        host.install_rebuild(response(
            &mut engine,
            vec![TreeMsg::PatchTree {
                bytes,
                submitted_at: None,
            }],
            TreeUpdateDecodePolicy::ReturnErr,
        ));
        settle(&mut engine, &mut host);
        assert_y(&engine, &host, 0.0);
    }
}

#[test]
fn scroll_app_echo_before_native_receipt_preserves_inflight_progress() {
    let assets = crate::assets::AssetRuntime::new();
    let _guard = assets.enter();
    let (mut engine, mut host, events) = start(None, true);
    wheel(&mut engine, &mut host, 0.0, -30.0);
    host.handle_input(InputEvent::CursorScroll {
        dx: 0.0,
        dy: -30.0,
        x: 50.0,
        y: 50.0,
    });
    let held = host.drain_tree_messages();
    assert!(!held.is_empty());
    host.install_rebuild(response(
        &mut engine,
        vec![patch(Some(fraction(0.1)))],
        TreeUpdateDecodePolicy::ReturnErr,
    ));
    assert_eq!(
        host.driver.runtime.scroll_sessions[&NodeId(1)].position[1],
        0.2
    );
    assert_eq!(engine.tree().get(&NodeId(1)).unwrap().layout.scroll_y, 30.0);
    let pending = host.drain_tree_messages();
    assert!(
        pending
            .iter()
            .flat_map(TreeMsg::commands)
            .all(|msg| !matches!(msg, TreeMsg::SetScrollPosition { .. }))
    );
    host.install_rebuild(response(
        &mut engine,
        held,
        TreeUpdateDecodePolicy::ReturnErr,
    ));
    if !pending.is_empty() {
        host.install_rebuild(response(
            &mut engine,
            pending,
            TreeUpdateDecodePolicy::ReturnErr,
        ));
    }
    settle(&mut engine, &mut host);
    assert_y(&engine, &host, 0.2);
    assert_eq!(events.0.lock().unwrap().len(), 2);
}
