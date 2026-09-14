use super::*;

struct BlockedActor {
    tx: Sender<TreeMsg>,
    registry: Receiver<EventMsg>,
    renders: Receiver<RenderMsg>,
    stats: Arc<RendererStatsCollector>,
    handle: thread::JoinHandle<()>,
}
impl BlockedActor {
    fn new(tree: ElementTree) -> Self {
        let (tx, rx) = bounded(64);
        let (event_tx, registry) = bounded(1);
        let (render_tx, renders) = bounded(1);
        event_tx
            .send(EventMsg::RegistryUpdate {
                rebuild: RegistryRebuildPayload::default(),
            })
            .unwrap();
        let stats = Arc::new(RendererStatsCollector::new());
        let handle = spawn_tree_actor_with_initial_tree(
            rx,
            TreeActorConfig {
                render_sender: RenderSender {
                    tx: render_tx,
                    drop_rx: renders.clone(),
                    log_render: false,
                },
                event_tx,
                render_counter: Arc::new(AtomicU64::new(0)),
                stats: Some(Arc::clone(&stats)),
                log_input: false,
                window_wake: BackendWakeHandle::noop(),
                initial_width: 600,
                initial_height: 60,
                asset_context: crate::assets::AssetRuntime::new().context(),
            },
            tree,
        );
        Self {
            tx,
            registry,
            renders,
            stats,
            handle,
        }
    }
    fn wait_constructed(&self, count: u64) {
        let deadline = Instant::now() + Duration::from_secs(5);
        while self.stats.peek().pipeline.scenes_constructed < count {
            assert!(
                Instant::now() < deadline,
                "actor must process input while registry delivery is blocked"
            );
            thread::yield_now();
        }
    }
    fn stop(self) {
        self.tx.send(TreeMsg::Stop).unwrap();
        let (done, rx) = bounded(1);
        thread::spawn(move || {
            let result = self.handle.join();
            let _ = done.send(result);
        });
        rx.recv_timeout(Duration::from_secs(5))
            .expect("Stop cannot wait for the registry consumer")
            .unwrap();
        // Keep both receivers alive, intentionally never draining pending output.
        drop((self.registry, self.renders));
    }
}

#[test]
fn stop_is_received_with_a_permanently_full_or_disconnected_registry_channel() {
    let actor = BlockedActor::new(coupled_publication_tree(false));
    let now = Instant::now();
    actor
        .tx
        .send(TreeMsg::AnimationPulse {
            presented_at: now,
            predicted_next_present_at: now,
            trace: None,
        })
        .unwrap();
    actor.wait_constructed(1);
    assert!(actor.renders.is_empty());
    assert_eq!(actor.registry.len(), 1);
    actor.stop();

    let mut actor = BlockedActor::new(coupled_publication_tree(false));
    actor
        .tx
        .send(TreeMsg::AnimationPulse {
            presented_at: now,
            predicted_next_present_at: now,
            trace: None,
        })
        .unwrap();
    actor.wait_constructed(1);
    let (_, disconnected) = bounded(0);
    actor.registry = disconnected;
    assert!(matches!(
        actor.renders.recv_timeout(Duration::from_secs(5)).unwrap(),
        RenderMsg::Scene { .. }
    ));
    actor.stop();
}

#[test]
fn blocked_registry_coalesces_to_direct_final_and_ghost_cleanup_output() {
    use crate::events::registry_builder::assert_registry_rebuild_payloads_equivalent;
    let assets = crate::assets::AssetRuntime::new();
    let _assets = assets.enter();
    for ghost in [false, true] {
        let tree = coupled_publication_tree(ghost);
        let mut direct = TreeUpdateEngine::new(tree.clone(), 600, 60);
        let actor = BlockedActor::new(tree);
        let start = Instant::now();
        let mut count = 0;
        let mut final_output = None;
        for (step, us) in [0, 0, 0, 500_000, 1_100_000, 1_100_001, 1_200_000]
            .into_iter()
            .enumerate()
        {
            if !ghost && (step == 1 || step == 2) {
                continue;
            }
            let patch = if step == 1 {
                Some([vec![4], NodeId(2).to_wire_u64().to_be_bytes().to_vec()].concat())
            } else if step == 6 {
                Some(encode_set_attrs_patch(
                    NodeId(3),
                    vec![0, 4, 1, 0, 2, 0, 12, 0, 1, 0, 0, 255, 255, 45, 1],
                ))
            } else {
                None
            };
            let at = start + Duration::from_micros(us);
            let messages = patch
                .into_iter()
                .map(|bytes| TreeMsg::PatchTree {
                    bytes,
                    submitted_at: None,
                })
                .chain(std::iter::once(TreeMsg::AnimationPulse {
                    presented_at: at,
                    predicted_next_present_at: at,
                    trace: None,
                }))
                .collect::<Vec<_>>();
            let TreeUpdateEffect::Layout { output, .. } = direct
                .process_messages(
                    messages.clone(),
                    TreeUpdateOptions::new(None, TreeUpdateDecodePolicy::ReturnErr),
                )
                .unwrap()
            else {
                panic!("expected native output");
            };
            actor.tx.send(TreeMsg::Batch(messages)).unwrap();
            count += 1;
            actor.wait_constructed(count);
            assert!(actor.renders.is_empty());
            final_output = Some(output);
        }
        let expected = final_output.unwrap();
        assert!(!expected.animations_active);
        actor.registry.recv().unwrap(); // remove the prefilled sentinel, not a native update
        let EventMsg::RegistryUpdate { rebuild } =
            actor.registry.recv_timeout(Duration::from_secs(5)).unwrap()
        else {
            panic!("registry");
        };
        assert_registry_rebuild_payloads_equivalent(&expected.event_rebuild, &rebuild);
        let RenderMsg::Scene {
            scene,
            version,
            animate,
            ..
        } = actor.renders.recv_timeout(Duration::from_secs(5)).unwrap()
        else {
            panic!("scene");
        };
        assert!(!animate);
        assert_eq!(version, 1);
        assert!(actor.renders.is_empty());
        assert_eq!(
            scene_pixels(
                &mut crate::renderer::SceneRenderer::new(),
                expected.scene,
                1
            ),
            scene_pixels(&mut crate::renderer::SceneRenderer::new(), *scene, version)
        );
        actor.stop();
    }
}

#[test]
fn coalesced_mount_focus_survives_revision_changes_but_not_remount_or_removal() {
    let assets = crate::assets::AssetRuntime::new();
    let _assets = assets.enter();
    let mut tree = single_node_tree(
        NodeId(1),
        vec![],
        Attrs {
            width: Some(Length::Px(40.0)),
            height: Some(Length::Px(20.0)),
            focus_on_mount: Some(true),
            on_focus: Some(true),
            ..Default::default()
        },
    );
    tree.set_revision(1);
    // Insert at revision one so this is a genuine mount-time request.
    tree.remove_node(&NodeId(1));
    tree.insert(Element::with_attrs(
        NodeId(1),
        ElementKind::El,
        vec![],
        Attrs {
            width: Some(Length::Px(40.0)),
            height: Some(Length::Px(20.0)),
            focus_on_mount: Some(true),
            on_focus: Some(true),
            ..Default::default()
        },
    ));
    tree.set_root_id(NodeId(1));
    tree.stamp_all_mounted_at_revision(1);
    let first = crate::tree::layout::layout_and_refresh_default(
        &mut tree,
        crate::tree::layout::Constraint::new(600.0, 60.0),
        1.0,
    )
    .event_rebuild;
    assert!(first.focus_on_mount.is_some());
    tree.set_revision(2);
    let next = crate::tree::layout::layout_and_refresh_default(
        &mut tree,
        crate::tree::layout::Constraint::new(300.0, 60.0),
        1.0,
    )
    .event_rebuild;
    assert!(next.focus_on_mount.is_none());
    let merged = coalesce_registry(&tree, Some(first), next);
    assert!(merged.focus_on_mount.is_some());
    let pending = merged.clone();
    let mut previously_focused = merged.clone();
    previously_focused.focused_id = Some(NodeId(2));
    let mut lost_focus = merged.clone();
    lost_focus.focused_id = None;
    lost_focus.focus_on_mount = None;
    assert!(
        coalesce_registry(&tree, Some(previously_focused), lost_focus)
            .focus_on_mount
            .is_some(),
        "removing an unrelated focused node must not lose the eligible pending mount"
    );
    let mut explicitly_focused = merged.clone();
    explicitly_focused.focus_on_mount = None;
    explicitly_focused.focused_id = Some(NodeId(2));
    assert!(
        coalesce_registry(&tree, Some(merged.clone()), explicitly_focused)
            .focus_on_mount
            .is_none(),
        "a later explicit focus choice supersedes the unsent mount request"
    );
    tree.remove_node(&NodeId(1));
    let absent = coalesce_registry(&tree, Some(merged), RegistryRebuildPayload::default());
    assert!(absent.focus_on_mount.is_none());
    tree.insert(Element::with_attrs(
        NodeId(1),
        ElementKind::El,
        vec![],
        Attrs {
            focus_on_mount: Some(true),
            on_focus: Some(true),
            ..Default::default()
        },
    ));
    tree.set_root_id(NodeId(1));
    tree.stamp_all_mounted_at_revision(2);
    tree.set_revision(3);
    let remounted = crate::tree::layout::layout_and_refresh_default(
        &mut tree,
        crate::tree::layout::Constraint::new(300.0, 60.0),
        1.0,
    )
    .event_rebuild;
    assert!(
        coalesce_registry(&tree, Some(pending), remounted)
            .focus_on_mount
            .is_none()
    );
}
