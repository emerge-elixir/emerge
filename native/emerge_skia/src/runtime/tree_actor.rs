use std::{
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    thread,
    time::Instant,
};

use crossbeam_channel::{Receiver, Sender, TrySendError};

use crate::{
    RenderSender,
    actors::{AnimationFrameTraceSeed, EventMsg, RenderMsg, TreeMsg},
    backend::wake::BackendWakeHandle,
    events,
    stats::RendererStatsCollector,
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
}

#[cfg_attr(
    not(any(
        all(feature = "wayland", target_os = "linux"),
        all(feature = "drm", target_os = "linux")
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
        } = config;

        let mut engine = TreeUpdateEngine::new(initial_tree, initial_width, initial_height);

        loop {
            let msg = match tree_rx.recv() {
                Ok(msg) => msg,
                Err(_) => return,
            };
            let mut messages = vec![msg];
            while let Ok(next) = tree_rx.try_recv() {
                messages.push(next);
            }

            match engine.process_messages(
                messages,
                TreeUpdateOptions::new(stats.as_ref(), TreeUpdateDecodePolicy::LogAndContinue),
            ) {
                Ok(TreeUpdateEffect::Stop) => return,
                Ok(TreeUpdateEffect::Skip) => {}
                Ok(TreeUpdateEffect::RegistryUpdate { rebuild }) => {
                    send_registry_update(&event_tx, rebuild, log_input);
                }
                Ok(TreeUpdateEffect::Layout {
                    output,
                    pipeline_submitted_at,
                    tree_batch_started_at,
                    animation_trace,
                }) => {
                    publish_layout_output(
                        LayoutOutputPublishTargets {
                            event_tx: &event_tx,
                            render_sender: &render_sender,
                            render_counter: &render_counter,
                            window_wake: &window_wake,
                            stats: stats.as_ref(),
                            log_input,
                        },
                        *output,
                        pipeline_submitted_at,
                        pipeline_submitted_at.map(|_| tree_batch_started_at),
                        animation_trace,
                    );
                }
                Err(err) => {
                    eprintln!("tree update failed: {err}");
                }
            }
        }
    })
}

pub(crate) fn send_registry_update(
    event_tx: &Sender<EventMsg>,
    rebuild: events::RegistryRebuildPayload,
    log_input: bool,
) {
    match event_tx.try_send(EventMsg::RegistryUpdate { rebuild }) {
        Ok(()) => {}
        Err(TrySendError::Full(_)) => {
            if log_input {
                eprintln!("event channel full, dropping registry update");
            }
            crate::debug_trace::hover_trace!(
                "event_channel",
                "event channel full, dropping registry update"
            );
        }
        Err(TrySendError::Disconnected(_)) => {}
    }
}

struct LayoutOutputPublishTargets<'a> {
    event_tx: &'a Sender<EventMsg>,
    render_sender: &'a RenderSender,
    render_counter: &'a Arc<AtomicU64>,
    window_wake: &'a BackendWakeHandle,
    stats: Option<&'a Arc<RendererStatsCollector>>,
    log_input: bool,
}

fn publish_layout_output(
    targets: LayoutOutputPublishTargets<'_>,
    output: LayoutOutput,
    pipeline_submitted_at: Option<Instant>,
    pipeline_tree_started_at: Option<Instant>,
    animation_trace: Option<AnimationFrameTraceSeed>,
) {
    if output.event_rebuild_changed {
        send_registry_update(targets.event_tx, output.event_rebuild, targets.log_input);
    }

    let version = targets.render_counter.fetch_add(1, Ordering::Relaxed) + 1;
    let render_queued_at = Instant::now();
    let pipeline_render_queued_at = pipeline_submitted_at.map(|_| render_queued_at);
    if let (Some(stats), Some(tree_started_at), Some(render_queued_at)) = (
        targets.stats,
        pipeline_tree_started_at,
        pipeline_render_queued_at,
    ) {
        stats.record_pipeline_tree(tree_started_at, render_queued_at);
    }
    targets.render_sender.send_latest(RenderMsg::Scene {
        scene: Box::new(output.scene),
        version,
        pipeline_submitted_at,
        pipeline_render_queued_at,
        animation_trace: animation_trace.map(|trace| Box::new(trace.queued_at(render_queued_at))),
        animate: output.animations_active,
        ime_enabled: output.ime_enabled,
        ime_cursor_area: output.ime_cursor_area,
        ime_text_state: Box::new(output.ime_text_state),
    });

    targets.window_wake.request_redraw();
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        RenderSender,
        actors::AnimationPulseTrace,
        events::{RegistryRebuildPayload, test_support::AnimatedNearbyHitCase},
        render_scene::RenderScene,
        tree::{
            attrs::{Attrs, Length},
            element::{Element, ElementKind, NodeId},
            serialize::encode_tree,
        },
    };
    use crossbeam_channel::{Receiver, bounded};
    use std::sync::{Arc, atomic::AtomicU64};
    use std::time::Duration;

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
        let mut attrs = Attrs::default();
        attrs.width = Some(Length::Px(64.0));
        attrs.height = Some(Length::Px(32.0));
        attrs.on_mouse_move = Some(true);
        single_node_tree(id, attrs_raw, attrs)
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
    fn publish_layout_output_skips_registry_send_when_output_is_clean() {
        let (event_tx, event_rx) = bounded(1);
        let (render_tx, render_rx) = bounded(1);
        let render_sender = RenderSender {
            tx: render_tx,
            drop_rx: render_rx.clone(),
            log_render: false,
        };
        let render_counter = Arc::new(AtomicU64::new(0));
        let window_wake = BackendWakeHandle::noop();

        publish_layout_output(
            LayoutOutputPublishTargets {
                event_tx: &event_tx,
                render_sender: &render_sender,
                render_counter: &render_counter,
                window_wake: &window_wake,
                stats: None,
                log_input: false,
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
            Some(Instant::now()),
            Some(Instant::now()),
            None,
        );

        assert!(event_rx.try_recv().is_err());

        match render_rx
            .try_recv()
            .expect("scene should still be published")
        {
            RenderMsg::Scene {
                version,
                pipeline_submitted_at,
                pipeline_render_queued_at,
                ..
            } => {
                assert_eq!(version, 1);
                assert!(pipeline_submitted_at.is_some());
                assert!(pipeline_render_queued_at.is_some());
            }
            RenderMsg::Stop => panic!("expected scene render message"),
        }
    }
}
