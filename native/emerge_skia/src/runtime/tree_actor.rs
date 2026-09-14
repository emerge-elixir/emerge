use std::{
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    thread,
    time::Instant,
};

use crossbeam_channel::{Receiver, Sender};

use crate::{
    RenderSender,
    actors::{AnimationFrameTraceSeed, EventMsg, RenderMsg, TreeMsg},
    assets::AssetContext,
    backend::wake::BackendWakeHandle,
    stats::{RendererStatsCollector, record_pipeline_layout_queued},
    tree::{element::ElementTree, layout::LayoutOutput},
};

use super::tree_update::{
    TreeUpdateDecodePolicy, TreeUpdateEffect, TreeUpdateEngine, TreeUpdateOptions,
};

#[cfg(test)]
use super::tree_update::animation_pulse_sample_time;
pub(crate) use super::tree_update::push_tree_message_flat;

pub(crate) struct TreeActorConfig {
    pub(crate) render_sender: RenderSender,
    pub(crate) event_tx: Sender<EventMsg>,
    pub(crate) render_counter: Arc<AtomicU64>,
    pub(crate) stats: Option<Arc<RendererStatsCollector>>,
    pub(crate) log_input: bool,
    pub(crate) window_wake: BackendWakeHandle,
    pub(crate) initial_width: u32,
    pub(crate) initial_height: u32,
    pub(crate) asset_context: AssetContext,
}

#[cfg_attr(
    not(any(
        all(feature = "wayland-core", target_os = "linux"),
        all(feature = "drm-core", target_os = "linux")
    )),
    allow(dead_code)
)]
pub(crate) fn spawn_tree_actor(
    tree_rx: Receiver<TreeMsg>,
    config: TreeActorConfig,
) -> thread::JoinHandle<()> {
    spawn_tree_actor_with_initial_tree(tree_rx, config, ElementTree::new())
}

pub(crate) fn spawn_tree_actor_with_initial_tree(
    tree_rx: Receiver<TreeMsg>,
    config: TreeActorConfig,
    initial_tree: ElementTree,
) -> thread::JoinHandle<()> {
    thread::spawn(move || {
        let TreeActorConfig {
            render_sender,
            event_tx,
            render_counter,
            stats,
            log_input,
            window_wake,
            initial_width,
            initial_height,
            asset_context,
        } = config;

        let _asset_context_guard = asset_context.enter();
        let mut engine = TreeUpdateEngine::new(initial_tree, initial_width, initial_height);

        // At most one unsent full registry and one latest scene. Continue
        // processing input while the event consumer is blocked, especially Stop.
        let mut pending_registry = None;
        let mut pending_layout: Option<PendingLayout> = None;
        let targets = || LayoutOutputPublishTargets {
            render_sender: &render_sender,
            render_counter: &render_counter,
            window_wake: &window_wake,
            stats: stats.as_deref(),
        };
        loop {
            let message = if let Some(rebuild) = pending_registry.take() {
                crossbeam_channel::select! {
                    send(event_tx,EventMsg::RegistryUpdate {rebuild}) -> _ => {
                        if let Some(layout)=pending_layout.take() {layout.publish(targets());}
                        continue;
                    }
                    recv(tree_rx) -> message => {pending_registry=Some(rebuild);message}
                }
            } else {
                tree_rx.recv()
            };
            let Ok(message) = message else {
                return;
            };
            // Bound a batch even when producers continuously replenish the queue.
            let messages = std::iter::once(message)
                .chain(tree_rx.try_iter().take(63))
                .collect();
            match engine.process_messages(
                messages,
                TreeUpdateOptions::new(stats.as_ref(), TreeUpdateDecodePolicy::LogAndContinue),
            ) {
                Ok(TreeUpdateEffect::Stop) => return,
                Ok(TreeUpdateEffect::Skip) => {}
                Ok(TreeUpdateEffect::RegistryUpdate { rebuild }) => {
                    pending_registry = Some(coalesce_registry(
                        engine.tree(),
                        pending_registry.take(),
                        rebuild,
                    ));
                }
                Ok(TreeUpdateEffect::Layout {
                    mut output,
                    pipeline_submitted_at,
                    tree_batch_started_at,
                    animation_trace,
                }) => {
                    if let Some(stats) = stats.as_ref() {
                        stats.record_scene_constructed();
                    }
                    if output.event_rebuild_changed {
                        if log_input && pending_registry.is_some() {
                            eprintln!("event channel busy, coalescing latest registry and scene");
                        }
                        pending_registry = Some(coalesce_registry(
                            engine.tree(),
                            pending_registry.take(),
                            std::mem::take(&mut output.event_rebuild),
                        ));
                        output.event_rebuild_changed = false;
                    }
                    let layout = PendingLayout {
                        output,
                        pipeline_submitted_at,
                        tree_batch_started_at,
                        animation_trace,
                    };
                    if pending_registry.is_some() {
                        pending_layout = Some(layout);
                    } else {
                        layout.publish(targets());
                    }
                }
                Err(err) => eprintln!("tree update failed: {err}"),
            }
        }
    })
}

fn coalesce_registry(
    tree: &ElementTree,
    previous: Option<crate::events::RegistryRebuildPayload>,
    mut next: crate::events::RegistryRebuildPayload,
) -> crate::events::RegistryRebuildPayload {
    if next.focus_on_mount.is_none() {
        next.focus_on_mount = previous
            .filter(|old| next.focused_id.is_none() || old.focused_id == next.focused_id)
            .and_then(|old| old.focus_on_mount)
            .and_then(|pending| {
                crate::events::registry_builder::rebind_pending_mount_focus(
                    tree,
                    &next.base_registry,
                    pending,
                )
            });
    }
    next
}

struct PendingLayout {
    output: Box<LayoutOutput>,
    pipeline_submitted_at: Option<Instant>,
    tree_batch_started_at: Instant,
    animation_trace: Option<AnimationFrameTraceSeed>,
}
impl PendingLayout {
    fn publish(self, targets: LayoutOutputPublishTargets<'_>) {
        queue_layout_output(
            targets,
            *self.output,
            self.pipeline_submitted_at,
            self.pipeline_submitted_at
                .map(|_| self.tree_batch_started_at),
            self.animation_trace,
        );
    }
}

struct LayoutOutputPublishTargets<'a> {
    render_sender: &'a RenderSender,
    render_counter: &'a Arc<AtomicU64>,
    window_wake: &'a BackendWakeHandle,
    stats: Option<&'a RendererStatsCollector>,
}

fn queue_layout_output(
    targets: LayoutOutputPublishTargets<'_>,
    output: LayoutOutput,
    pipeline_submitted_at: Option<Instant>,
    pipeline_tree_started_at: Option<Instant>,
    animation_trace: Option<AnimationFrameTraceSeed>,
) {
    let version = targets.render_counter.fetch_add(1, Ordering::Relaxed) + 1;
    let pipeline = record_pipeline_layout_queued(
        targets.stats,
        None,
        None,
        pipeline_submitted_at,
        pipeline_tree_started_at,
        Instant::now(),
    );
    let queue_overwritten = targets.render_sender.send_latest(RenderMsg::Scene {
        scene: Box::new(output.scene),
        version,
        pipeline_submitted_at: pipeline.pipeline_submitted_at,
        pipeline_render_queued_at: pipeline.pipeline_render_queued_at,
        animation_trace: animation_trace
            .map(|trace| Box::new(trace.queued_at(pipeline.render_queued_at))),
        animate: output.animations_active,
        ime_enabled: output.ime_enabled,
        ime_cursor_area: output.ime_cursor_area,
        ime_text_state: Box::new(output.ime_text_state),
    });
    if queue_overwritten && let Some(stats) = targets.stats {
        stats.record_render_queue_overwrite();
    }

    targets.window_wake.request_redraw();
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        RenderSender,
        actors::AnimationPulseTrace,
        events::{RegistryRebuildPayload, test_support::AnimatedNearbyHitCase},
        input::InputEvent,
        render_scene::RenderScene,
        tree::{
            attrs::{Attrs, Background, Color, Length, MouseOverAttrs},
            element::{Element, ElementKind, NodeId},
            serialize::encode_tree,
        },
    };
    use crossbeam_channel::{Receiver, bounded};
    use std::sync::{Arc, atomic::AtomicU64};
    use std::time::Duration;

    mod backpressure;

    struct TreeActorHarness {
        tree_tx: Sender<TreeMsg>,
        event_rx: Receiver<EventMsg>,
        render_rx: Receiver<RenderMsg>,
        handle: thread::JoinHandle<()>,
    }

    impl TreeActorHarness {
        fn new(initial_tree: ElementTree, width: u32, height: u32) -> Self {
            let (tree_tx, tree_rx) = bounded(64);
            let (event_tx, event_rx) = bounded(64);
            let (render_tx, render_rx) = bounded(8);
            let render_sender = RenderSender {
                tx: render_tx,
                drop_rx: render_rx.clone(),
                log_render: false,
            };
            let handle = spawn_tree_actor_with_initial_tree(
                tree_rx,
                TreeActorConfig {
                    render_sender,
                    event_tx,
                    render_counter: Arc::new(AtomicU64::new(0)),
                    stats: None,
                    log_input: false,
                    window_wake: BackendWakeHandle::noop(),
                    initial_width: width,
                    initial_height: height,
                    asset_context: crate::assets::AssetRuntime::new().context(),
                },
                initial_tree,
            );

            Self {
                tree_tx,
                event_rx,
                render_rx,
                handle,
            }
        }

        fn send(&self, msg: TreeMsg) {
            self.tree_tx.send(msg).expect("tree actor should be alive");
        }

        fn recv_scene(&self) -> RenderMsg {
            self.render_rx
                .recv_timeout(Duration::from_millis(250))
                .expect("tree actor should publish a render scene")
        }

        fn recv_registry_update(&self) -> RegistryRebuildPayload {
            match self
                .event_rx
                .recv_timeout(Duration::from_millis(250))
                .expect("tree actor should publish a registry update")
            {
                EventMsg::RegistryUpdate { rebuild } => rebuild,
                _ => panic!("expected registry update"),
            }
        }

        fn stop(self) {
            let _ = self.tree_tx.send(TreeMsg::Stop);
            let _ = self.handle.join();
        }
    }

    fn single_node_tree(id: NodeId, attrs_raw: Vec<u8>, attrs: Attrs) -> ElementTree {
        let mut tree = ElementTree::new();
        tree.insert(Element::with_attrs(id, ElementKind::El, attrs_raw, attrs));
        tree.set_root_id(id);
        tree
    }

    fn fixed_listener_tree(id: NodeId) -> ElementTree {
        let attrs_raw = fixed_listener_attrs_raw();
        let attrs = Attrs {
            width: Some(Length::Px(64.0)),
            height: Some(Length::Px(32.0)),
            on_mouse_move: Some(true),
            ..Attrs::default()
        };
        single_node_tree(id, attrs_raw, attrs)
    }

    fn registry_matches_cursor_pos(
        rebuild: &RegistryRebuildPayload,
        element_id: NodeId,
        x: f32,
        y: f32,
    ) -> bool {
        let input = InputEvent::CursorPos { x, y };
        rebuild
            .base_registry
            .view()
            .find_precedence(|listener| {
                listener.element_id == Some(element_id) && listener.matcher.matches(&input)
            })
            .is_some()
    }

    fn rgba_background_attrs_raw(r: u8, g: u8, b: u8, a: u8) -> Vec<u8> {
        vec![0, 1, 12, 0, 1, r, g, b, a]
    }

    fn fixed_listener_attrs_raw() -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(&3u16.to_be_bytes());
        out.push(1);
        out.push(2);
        out.extend_from_slice(&64.0f64.to_be_bytes());
        out.push(2);
        out.push(2);
        out.extend_from_slice(&32.0f64.to_be_bytes());
        out.push(45);
        out.push(1);
        out
    }

    fn encode_set_attrs_patch(id: NodeId, attrs_raw: Vec<u8>) -> Vec<u8> {
        let mut out = Vec::new();
        out.push(1);
        out.extend_from_slice(&id.to_wire_u64().to_be_bytes());
        out.extend_from_slice(&(attrs_raw.len() as u32).to_be_bytes());
        out.extend_from_slice(&attrs_raw);
        out
    }

    fn assert_scene_trace(
        message: RenderMsg,
        expected_sequence: u64,
    ) -> crate::actors::AnimationFrameTrace {
        match message {
            RenderMsg::Scene {
                animation_trace, ..
            } => {
                let trace = animation_trace.expect("scene should carry animation trace");
                assert_eq!(trace.sequence, Some(expected_sequence));
                *trace
            }
            RenderMsg::Stop => panic!("expected scene render message"),
        }
    }

    #[test]
    fn animation_pulse_sample_time_never_regresses() {
        let base = Instant::now();
        let previous = base + std::time::Duration::from_millis(67);
        let presented_at = base + std::time::Duration::from_millis(20);
        let predicted_next_present_at = base + std::time::Duration::from_millis(36);

        assert_eq!(
            animation_pulse_sample_time(Some(previous), presented_at, predicted_next_present_at),
            previous
        );
    }

    #[test]
    fn animation_pulse_sample_time_uses_predicted_present_without_previous() {
        let base = Instant::now();
        let presented_at = base;
        let predicted_next_present_at = base + std::time::Duration::from_millis(16);

        assert_eq!(
            animation_pulse_sample_time(None, presented_at, predicted_next_present_at),
            predicted_next_present_at
        );
    }

    #[test]
    fn mixed_pulse_and_paint_patch_publish_sampled_scene() {
        let root_id = NodeId::from_u64(1);
        let harness = TreeActorHarness::new(
            single_node_tree(root_id, Vec::new(), Attrs::default()),
            100,
            100,
        );
        let base = Instant::now();

        harness.send(TreeMsg::Batch(vec![
            TreeMsg::AnimationPulse {
                presented_at: base,
                predicted_next_present_at: base + Duration::from_millis(16),
                trace: Some(AnimationPulseTrace {
                    sequence: 7,
                    sent_at: base,
                }),
            },
            TreeMsg::PatchTree {
                bytes: encode_set_attrs_patch(root_id, rgba_background_attrs_raw(255, 0, 0, 255)),
                submitted_at: Some(base),
            },
        ]));

        let trace = assert_scene_trace(harness.recv_scene(), 7);
        assert_eq!(trace.sample_time, base + Duration::from_millis(16));
        assert_eq!(trace.presented_at, Some(base));
        assert_eq!(
            trace.predicted_next_present_at,
            Some(base + Duration::from_millis(16))
        );
        assert!(trace.pulse_requested_sample);
        harness.stop();
    }

    #[test]
    fn mixed_pulse_and_resize_keeps_active_animation_sample_monotonic() {
        let case = AnimatedNearbyHitCase::width_move_in_front();
        let harness = TreeActorHarness::new(
            case.source_tree(false),
            case.constraint.max_width(0.0) as u32,
            case.constraint.max_height(0.0) as u32,
        );
        let base = Instant::now();

        harness.send(TreeMsg::AnimationPulse {
            presented_at: base,
            predicted_next_present_at: base + Duration::from_millis(67),
            trace: Some(AnimationPulseTrace {
                sequence: 11,
                sent_at: base,
            }),
        });
        let first = assert_scene_trace(harness.recv_scene(), 11);

        harness.send(TreeMsg::Batch(vec![
            TreeMsg::AnimationPulse {
                presented_at: base + Duration::from_millis(20),
                predicted_next_present_at: base + Duration::from_millis(36),
                trace: Some(AnimationPulseTrace {
                    sequence: 12,
                    sent_at: base + Duration::from_millis(20),
                }),
            },
            TreeMsg::Resize {
                width: case.constraint.max_width(0.0),
                height: case.constraint.max_height(0.0),
                scale: 1.0,
            },
        ]));
        let second = assert_scene_trace(harness.recv_scene(), 12);

        assert_eq!(second.previous_sample_time, Some(first.sample_time));
        assert_eq!(second.sample_time, first.sample_time);
        harness.stop();
    }

    #[test]
    fn resize_scale_relayout_rebuilds_registry_hit_regions() {
        let root_id = NodeId::from_u64(101);
        let harness = TreeActorHarness::new(fixed_listener_tree(root_id), 128, 64);

        harness.send(TreeMsg::RebuildRegistry);
        let initial_rebuild = harness.recv_registry_update();
        let _ = harness.recv_scene();

        assert!(registry_matches_cursor_pos(
            &initial_rebuild,
            root_id,
            48.0,
            24.0
        ));
        assert!(
            !registry_matches_cursor_pos(&initial_rebuild, root_id, 100.0, 40.0),
            "unscaled listener region should not include the post-scale point"
        );

        harness.send(TreeMsg::Resize {
            width: 256.0,
            height: 128.0,
            scale: 2.0,
        });
        let scaled_rebuild = harness.recv_registry_update();
        let _ = harness.recv_scene();

        assert!(
            registry_matches_cursor_pos(&scaled_rebuild, root_id, 100.0, 40.0),
            "resize/scale relayout must publish fresh registry geometry"
        );
        harness.stop();
    }

    #[test]
    fn explicit_registry_rebuild_publishes_after_local_paint_refresh() {
        let root_id = NodeId::from_u64(102);
        let mut tree = fixed_listener_tree(root_id);
        let element = tree.get_mut(&root_id).expect("root should exist");
        element.spec.declared.on_mouse_down = Some(true);
        element.spec.declared.on_mouse_up = Some(true);
        element.spec.declared.mouse_down = Some(MouseOverAttrs {
            background: Some(Background::Color(Color::Rgb { r: 1, g: 2, b: 3 })),
            ..MouseOverAttrs::default()
        });
        element.layout.effective = element.spec.declared.clone();
        let harness = TreeActorHarness::new(tree, 128, 64);

        harness.send(TreeMsg::RebuildRegistry);
        let _ = harness.recv_registry_update();
        let _ = harness.recv_scene();

        harness.send(TreeMsg::Batch(vec![
            TreeMsg::SetMouseDownActive {
                element_id: root_id,
                active: true,
            },
            TreeMsg::RebuildRegistry,
        ]));

        let _ = harness.recv_registry_update();
        let _ = harness.recv_scene();
        harness.stop();
    }

    #[test]
    fn mixed_pulse_and_structure_upload_rebuilds_registry() {
        let root_id = NodeId::from_u64(1);
        let uploaded_id = NodeId::from_u64(2);
        let harness = TreeActorHarness::new(
            single_node_tree(root_id, Vec::new(), Attrs::default()),
            100,
            100,
        );
        let base = Instant::now();

        harness.send(TreeMsg::Batch(vec![
            TreeMsg::AnimationPulse {
                presented_at: base,
                predicted_next_present_at: base + Duration::from_millis(16),
                trace: Some(AnimationPulseTrace {
                    sequence: 21,
                    sent_at: base,
                }),
            },
            TreeMsg::UploadTree {
                bytes: encode_tree(&fixed_listener_tree(uploaded_id)),
                submitted_at: Some(base),
            },
        ]));

        let rebuild = harness.recv_registry_update();
        assert!(
            rebuild
                .base_registry
                .view()
                .find_precedence(|_| true)
                .is_some(),
            "uploaded listener tree should rebuild the event registry"
        );
        let trace = assert_scene_trace(harness.recv_scene(), 21);
        assert!(trace.pulse_requested_sample);
        harness.stop();
    }

    #[test]
    fn mixed_pulse_and_asset_state_change_publish_sampled_scene() {
        let root_id = NodeId::from_u64(3);
        let harness = TreeActorHarness::new(
            single_node_tree(root_id, Vec::new(), Attrs::default()),
            100,
            100,
        );
        let base = Instant::now();

        harness.send(TreeMsg::Batch(vec![
            TreeMsg::AnimationPulse {
                presented_at: base,
                predicted_next_present_at: base + Duration::from_millis(16),
                trace: Some(AnimationPulseTrace {
                    sequence: 31,
                    sent_at: base,
                }),
            },
            TreeMsg::AssetStateChanged,
        ]));

        let trace = assert_scene_trace(harness.recv_scene(), 31);
        assert_eq!(trace.sample_time, base + Duration::from_millis(16));
        harness.stop();
    }

    #[test]
    fn queue_layout_output_counts_actual_render_queue_overwrites() {
        let (render_tx, render_rx) = bounded(1);
        let render_sender = RenderSender {
            tx: render_tx,
            drop_rx: render_rx.clone(),
            log_render: false,
        };
        let render_counter = Arc::new(AtomicU64::new(0));
        let window_wake = BackendWakeHandle::noop();
        let stats = RendererStatsCollector::new();

        for _ in 0..2 {
            queue_layout_output(
                LayoutOutputPublishTargets {
                    render_sender: &render_sender,
                    render_counter: &render_counter,
                    window_wake: &window_wake,
                    stats: Some(&stats),
                },
                LayoutOutput {
                    scene: RenderScene::default(),
                    event_rebuild: RegistryRebuildPayload::default(),
                    event_rebuild_changed: false,
                    ime_enabled: false,
                    ime_cursor_area: None,
                    ime_text_state: None,
                    animations_active: false,
                },
                None,
                None,
                None,
            );
        }

        let snapshot = stats.peek();
        assert_eq!(snapshot.pipeline.scenes_constructed, 0); // construction is recorded by the actor, not queued twice
        assert_eq!(snapshot.pipeline.render_queue_overwrites, 1);
        assert_eq!(render_rx.len(), 1);
    }

    fn coupled_publication_tree(ghost: bool) -> ElementTree {
        use crate::tree::animation::{AnimationCurve, AnimationRepeat, AnimationSpec};
        let mut tree = ElementTree::new();
        tree.insert(Element::with_attrs(
            NodeId(1),
            ElementKind::Row,
            vec![],
            Attrs {
                width: Some(Length::Fill),
                height: Some(Length::Fill),
                ..Default::default()
            },
        ));
        for (id, from, duration, color) in [
            (NodeId(2), 40.0, 1000.0, Color::Rgb { r: 255, g: 0, b: 0 }),
            (NodeId(3), 200.0, 2000.0, Color::Rgb { r: 0, g: 0, b: 255 }),
        ] {
            let spec = AnimationSpec {
                keyframes: vec![
                    Attrs {
                        width: Some(Length::Px(from)),
                        ..Default::default()
                    },
                    Attrs {
                        width: Some(Length::Fill),
                        ..Default::default()
                    },
                ],
                duration_ms: duration,
                curve: AnimationCurve::Linear,
                repeat: if id == NodeId(3) {
                    AnimationRepeat::Loop
                } else {
                    AnimationRepeat::Once
                },
            };
            let mut attrs = Attrs {
                width: Some(Length::Px(from)),
                height: Some(Length::Fill),
                on_mouse_move: Some(true),
                background: Some(Background::Color(color)),
                ..Default::default()
            };
            if ghost && id == NodeId(2) {
                attrs.animate_exit = Some(spec);
            } else {
                attrs.animate = Some(spec);
            }
            tree.insert(Element::with_attrs(id, ElementKind::El, vec![], attrs));
        }
        tree.set_children(&NodeId(1), vec![NodeId(2), NodeId(3)])
            .unwrap();
        tree.set_root_id(NodeId(1));
        tree
    }

    fn scene_pixels(
        renderer: &mut crate::renderer::SceneRenderer,
        scene: RenderScene,
        version: u64,
    ) -> Vec<u8> {
        let info = skia_safe::ImageInfo::new(
            (600, 60),
            skia_safe::ColorType::RGBA8888,
            skia_safe::AlphaType::Premul,
            None,
        );
        let mut surface = skia_safe::surfaces::raster(&info, None, None).unwrap();
        renderer.render(
            &mut crate::renderer::RenderFrame::new(&mut surface, None),
            &crate::renderer::RenderState::new(
                scene,
                skia_safe::Color::TRANSPARENT,
                version,
                false,
            ),
        );
        let mut bytes = vec![0; 600 * 60 * 4];
        assert!(surface.read_pixels(&info, &mut bytes, 600 * 4, (0, 0)));
        bytes
    }

    #[test]
    fn coupled_frames_ghost_cleanup_and_final_hits_match_direct_engine() {
        use crate::events::registry_builder::assert_registry_rebuild_payloads_equivalent;
        use crate::renderer::SceneRenderer;
        let assets = crate::assets::AssetRuntime::new();
        let _assets = assets.enter();
        for ghost in [false, true] {
            let tree = coupled_publication_tree(ghost);
            let mut direct = TreeUpdateEngine::new(tree.clone(), 600, 60);
            let actor = TreeActorHarness::new(tree, 600, 60);
            let start = Instant::now();
            let options = || TreeUpdateOptions::new(None, TreeUpdateDecodePolicy::ReturnErr);
            let mut a = SceneRenderer::new();
            let mut b = SceneRenderer::new();
            let mut prior = None;
            // Acknowledge the first ghost presentation at t=0 before advancing
            // time; transient starts are anchored to presentation, not wall time.
            for (step, us) in [0, 0, 0, 500_000, 1_100_000, 1_100_001, 1_200_000]
                .into_iter()
                .enumerate()
            {
                if (step == 1 || step == 2) && !ghost {
                    continue;
                }
                let at = start + Duration::from_micros(us);
                let patch = if step == 1 {
                    Some([vec![4], NodeId(2).to_wire_u64().to_be_bytes().to_vec()].concat())
                } else if step == 6 {
                    // Plain width/height Fill, blue background, move handler; no loop.
                    Some(encode_set_attrs_patch(
                        NodeId(3),
                        vec![0, 4, 1, 0, 2, 0, 12, 0, 1, 0, 0, 255, 255, 45, 1],
                    ))
                } else {
                    None
                };
                let messages = || {
                    patch
                        .iter()
                        .map(|bytes| TreeMsg::PatchTree {
                            bytes: bytes.clone(),
                            submitted_at: None,
                        })
                        .chain(std::iter::once(TreeMsg::AnimationPulse {
                            presented_at: at,
                            predicted_next_present_at: at,
                            trace: None,
                        }))
                        .collect::<Vec<_>>()
                };
                let TreeUpdateEffect::Layout { output, .. } =
                    direct.process_messages(messages(), options()).unwrap()
                else {
                    panic!("expected layout {ghost}/{step}");
                };
                actor.send(TreeMsg::Batch(messages()));
                if output.event_rebuild_changed {
                    assert_registry_rebuild_payloads_equivalent(
                        &output.event_rebuild,
                        &actor.recv_registry_update(),
                    );
                }
                let RenderMsg::Scene {
                    scene,
                    version,
                    animate,
                    ..
                } = actor.recv_scene()
                else {
                    panic!("expected scene");
                };
                assert_eq!(animate, output.animations_active);
                let actual = scene_pixels(&mut a, *scene, version);
                assert_eq!(actual, scene_pixels(&mut b, output.scene.clone(), version));
                if step == 3 {
                    prior = Some((output.scene.clone(), actual));
                }
                if step == 6 {
                    assert!(!animate, "ghost={ghost} step={step}");
                    assert!(direct.tree().length_runtime.is_none());
                    let (old, pixels) = prior.take().unwrap();
                    assert_eq!(pixels, scene_pixels(&mut b, old, version + 1));
                    assert!(matches!(
                        direct.process_messages(vec![], options()).unwrap(),
                        TreeUpdateEffect::Skip
                    ));
                }
            }
            actor.stop();
        }
    }
}
