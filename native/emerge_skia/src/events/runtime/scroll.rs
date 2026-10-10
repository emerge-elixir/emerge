use super::*;
use crate::events::scroll::{ScrollNode, ScrollSession};

impl DirectEventRuntime {
    pub(super) fn reconcile_scrolls(
        &mut self,
        nodes: HashMap<NodeId, ScrollNode>,
        tx: &dyn TreeSink,
        log: bool,
    ) -> bool {
        self.scroll_sessions.retain(|id, session| {
            nodes
                .get(id)
                .is_some_and(|node| node.mount == session.node.mount)
        });
        let now = Instant::now();
        nodes.into_iter().fold(false, |changed, (id, node)| {
            let session = self
                .scroll_sessions
                .entry(id)
                .or_insert_with(|| ScrollSession::new(node.clone()));
            let before = session.position;
            let unresolved = session.has_new_unresolved_target(&node);
            let replaced = session.reconcile(node, now);
            let position = session.position;
            self.refresh_scrollbar_offsets(id, position);
            if unresolved {
                self.log_event_diagnostic(log, || format!("unresolved scroll target for {id:?}: expected a live descendant in the same scroll content; preserving position"));
            }
            if replaced.iter().any(|axis| *axis) {
                if self.drag_motion.as_ref().is_some_and(|motion| {
                    motion.element_id == id
                        && replaced[usize::from(motion.axis == ScrollbarAxis::Y)]
                }) {
                    // Continue relative pointer tracking for safe release, but
                    // never restart pre-jump momentum when that drag ends.
                    self.drag_motion = None;
                    self.suppress_drag_release_inertia = true;
                }
                if self.inertial_scroll.as_ref().is_some_and(|inertia| {
                    inertia.element_id == id
                        && replaced[usize::from(inertia.axis == ScrollbarAxis::Y)]
                }) {
                    self.cancel_inertial_scroll();
                }
                if self
                    .runtime_overlay
                    .scrollbar
                    .as_ref()
                    .is_some_and(|tracker| {
                        tracker.element_id == id
                            && replaced[usize::from(tracker.axis == ScrollbarAxis::Y)]
                    })
                {
                    self.runtime_overlay.scrollbar = None;
                }
            }
            if before != position {
                send_tree(
                    tx,
                    TreeMsg::SetScrollPosition {
                        element_id: id,
                        position,
                    },
                    log,
                );
                true
            } else {
                changed
            }
        })
    }

    // Inertia and drag anchors must not regress to an older native receipt.
    fn refresh_scrollbar_offsets(&mut self, id: NodeId, position: [f64; 2]) {
        for (axis, slot) in [ScrollbarAxis::X, ScrollbarAxis::Y].into_iter().enumerate() {
            if let Some(node) = self.scrollbar_nodes.get_mut(&(id, slot)) {
                node.scroll_offset = (position[axis] * node.scroll_range as f64) as f32;
            }
        }
    }

    // The driver's operation boundary covers wheel, keys, gestures, timers and
    // focus reveal alike. Relative requests never reach the tree from this path.
    // Coalesce callbacks per container, after both axes of a dispatch are applied.
    pub(super) fn resolve_scroll_commands(&mut self, messages: Vec<TreeMsg>) -> Vec<TreeMsg> {
        let mut changed = HashMap::new();
        let other: Vec<_> = messages
            .into_iter()
            .filter_map(|message| {
                let (id, delta) = match message {
                    TreeMsg::ScrollRequest { element_id, dx, dy } => {
                        (element_id, [dx as f64, dy as f64])
                    }
                    TreeMsg::ScrollbarThumbDragX { element_id, dx } => {
                        (element_id, [dx as f64, 0.0])
                    }
                    TreeMsg::ScrollbarThumbDragY { element_id, dy } => {
                        (element_id, [0.0, dy as f64])
                    }
                    message => return Some(message),
                };
                if let Some(session) = self.scroll_sessions.get_mut(&id) {
                    let before = session.position;
                    if session.move_by(delta) {
                        changed.entry(id).or_insert(before);
                    }
                }
                None
            })
            .collect();
        let mut changes: Vec<_> = changed.into_iter().collect();
        changes.sort_unstable_by_key(|(id, _)| id.0);
        let updates: Vec<_> = changes
            .into_iter()
            .filter_map(|(id, before)| {
                let session = self.scroll_sessions.get_mut(&id)?;
                if session.position == before {
                    return None;
                }
                let position = session.position;
                let emit = session.node.emit;
                if emit {
                    session.expect_echo(Instant::now());
                }
                self.refresh_scrollbar_offsets(id, position);
                if emit {
                    self.send_elixir_event(registry_builder::ElixirEvent {
                        element_id: id,
                        kind: ElementEventKind::Scroll,
                        payload: Some(ElixirEventPayload::Scroll(crate::events::ScrollPayload {
                            x: position[0],
                            y: position[1],
                        })),
                    });
                }
                Some(TreeMsg::SetScrollPosition {
                    element_id: id,
                    position,
                })
            })
            .collect();
        other.into_iter().chain(updates).collect()
    }
}
