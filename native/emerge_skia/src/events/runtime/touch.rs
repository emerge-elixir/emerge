//! Direct-touch virtual keys have contact-local lifetimes, not a shared cursor.
use super::*;
use crate::input::TouchPhase;
use registry_builder::{VirtualKeyPhase, VirtualKeyTracker};

pub(super) struct TouchKey {
    tracker: VirtualKeyTracker,
    deadline: Option<Instant>,
    hold_fired: bool,
    styled: bool,
    focus: Option<(NodeId, u64)>,
}

fn tracker_in(actions: &[ListenerAction]) -> Option<VirtualKeyTracker> {
    actions.iter().find_map(|action| match action {
        ListenerAction::RuntimeChange(RuntimeChange::StartVirtualKeyTracker { tracker }) => {
            Some(tracker.clone())
        }
        _ => None,
    })
}
fn pointer(action: u8, x: f32, y: f32) -> InputEvent {
    InputEvent::CursorButton {
        button: "left".into(),
        action,
        mods: 0,
        x,
        y,
    }
}

impl DirectEventRuntime {
    fn base_actions(&mut self, event: InputEvent) -> Vec<ListenerAction> {
        let mut ctx = RuntimeListenerComputeCtx {
            base_registry: &self.base_registry,
            overlay_registry: &self.overlay_registry,
            focused_id: self.focused_id.as_ref(),
            hover_stack: &self.hover_stack,
            text_states: &self.text_states,
            slider_states: &self.slider_states,
            text_commit_suppressions: &mut self.text_commit_suppressions,
            clipboard: &mut self.clipboard,
        };
        ctx.dispatch_base(&ListenerInput::Raw(event))
    }

    pub(super) fn dispatch_touch(
        &mut self,
        id: u64,
        phase: TouchPhase,
        x: f32,
        y: f32,
        tx: &dyn TreeSink,
        log: bool,
    ) {
        if phase == TouchPhase::Down {
            if self.touch_keys.contains_key(&id) || self.primary_touch == Some(id) {
                return;
            }
            let actions = self.base_actions(pointer(ACTION_PRESS, x, y));
            if let Some(tracker) = tracker_in(&actions) {
                let styled = actions.iter().any(|action| matches!(action,
                    ListenerAction::TreeMsg(TreeMsg::SetMouseDownActive { element_id, active: true }) if *element_id == tracker.element_id
                ));
                let deadline = (tracker.hold != crate::tree::attrs::VirtualKeyHoldMode::None)
                    .then(|| Instant::now() + Duration::from_millis(u64::from(tracker.hold_ms)));
                let focus = self.focused_id.and_then(|id| {
                    self.base_registry
                        .source_mount(&id)
                        .map(|mount| (id, mount))
                });
                self.touch_keys.insert(
                    id,
                    TouchKey {
                        tracker,
                        deadline,
                        hold_fired: false,
                        styled,
                        focus,
                    },
                );
                // Only the contact-local tracker owns this key. Do not overwrite
                // mouse/primary-touch click, drag or virtual-key captures.
                let actions = actions
                    .into_iter()
                    .filter(|action| {
                        !matches!(
                            action,
                            ListenerAction::RuntimeChange(
                                RuntimeChange::StartVirtualKeyTracker { .. }
                                    | RuntimeChange::StartClickPressTracker { .. }
                            )
                        )
                    })
                    .collect();
                self.apply_listener_actions(actions, tx, log, DispatchMode::Normal);
                return;
            }
            if self.primary_touch.is_none() {
                self.primary_touch = Some(id);
                let event = pointer(ACTION_PRESS, x, y);
                self.record_pointer_snapshot(&event);
                self.cancel_inertial_scroll_for_input(&event);
                self.dispatch_event(event, tx, log, DispatchMode::Normal);
            }
            return;
        }

        if let Some(mut key) = self.touch_keys.remove(&id) {
            let inside = key.tracker.region.contains(x, y);
            if phase == TouchPhase::Move {
                let cancelled = !inside && key.tracker.phase != VirtualKeyPhase::Cancelled;
                if cancelled {
                    key.tracker.phase = VirtualKeyPhase::Cancelled;
                    key.deadline = None;
                }
                let (element, styled) = (key.tracker.element_id, key.styled);
                self.touch_keys.insert(id, key);
                if cancelled && styled {
                    self.clear_touch_feedback(element, tx, log);
                }
            } else {
                let commit = phase == TouchPhase::Up
                    && inside
                    && key.tracker.phase == VirtualKeyPhase::Armed
                    && !key.hold_fired;
                if key.styled {
                    self.clear_touch_feedback(key.tracker.element_id, tx, log);
                }
                if commit {
                    self.inject_synthetic_inputs(
                        registry_builder::synthetic_input_sequence_for_virtual_key_tap(
                            &key.tracker.tap,
                        ),
                        tx,
                        log,
                    );
                }
            }
            return;
        }

        if self.primary_touch == Some(id) {
            let event = match phase {
                TouchPhase::Move => InputEvent::CursorPos { x, y },
                TouchPhase::Up => {
                    self.primary_touch = None;
                    pointer(crate::input::ACTION_RELEASE, x, y)
                }
                TouchPhase::Cancel => {
                    self.primary_touch = None;
                    // Leaving cancels capture without activating a button or fling.
                    InputEvent::CursorEntered { entered: false }
                }
                TouchPhase::Down => return,
            };
            self.record_pointer_snapshot(&event);
            self.cancel_inertial_scroll_for_input(&event);
            self.dispatch_event(event, tx, log, DispatchMode::Normal);
        }
    }

    pub(super) fn touch_key_pressed(&self, element: NodeId) -> bool {
        self.touch_keys.values().any(|key| {
            key.tracker.element_id == element && key.tracker.phase != VirtualKeyPhase::Cancelled
        })
    }

    fn key_still_pressed(&self, element: NodeId) -> bool {
        self.touch_key_pressed(element)
            || self
                .runtime_overlay
                .virtual_key
                .as_ref()
                .is_some_and(|key| {
                    key.element_id == element && key.phase != VirtualKeyPhase::Cancelled
                })
    }

    fn clear_touch_feedback(&mut self, element: NodeId, tx: &dyn TreeSink, log: bool) {
        if !self.key_still_pressed(element) {
            self.apply_listener_actions(
                vec![ListenerAction::TreeMsg(TreeMsg::SetMouseDownActive {
                    element_id: element,
                    active: false,
                })],
                tx,
                log,
                DispatchMode::Normal,
            );
        }
    }

    pub(super) fn cancel_touch_keys(&mut self, tx: &dyn TreeSink, log: bool) {
        let keys = std::mem::take(&mut self.touch_keys);
        let mut styled: Vec<_> = keys
            .into_values()
            .filter(|key| key.styled)
            .map(|key| key.tracker.element_id)
            .collect();
        styled.sort_unstable_by_key(|id| id.0);
        styled.dedup();
        for element in styled {
            self.clear_touch_feedback(element, tx, log);
        }
    }

    pub(super) fn reconcile_touch_keys(
        &mut self,
        previous: &registry_builder::Registry,
        focus: Option<NodeId>,
        tx: &dyn TreeSink,
        log: bool,
    ) {
        if self.touch_keys.is_empty() {
            return;
        }
        let focus = focus.and_then(|id| {
            self.base_registry
                .source_mount(&id)
                .map(|mount| (id, mount))
        });
        // One registry pass, rather than a full listener scan for every contact.
        let sources: HashMap<_, _> = self
            .base_registry
            .view()
            .iter_precedence()
            .filter_map(|listener| {
                let actions = match &listener.compute {
                    registry_builder::ListenerCompute::Static { actions }
                    | registry_builder::ListenerCompute::StaticWithLeftPressRuntimeAugment {
                        actions,
                        ..
                    } => actions,
                    _ => return None,
                };
                tracker_in(actions).map(|tracker| (tracker.element_id, tracker))
            })
            .collect();
        let base = &self.base_registry;
        let mut clear = Vec::new();
        self.touch_keys.retain(|_, key| {
            let same_mount = base.same_mount(previous, &key.tracker.element_id);
            let source = sources.get(&key.tracker.element_id);
            let keep = same_mount
                && key.focus == focus
                && source.is_some_and(|source| {
                    source.tap == key.tracker.tap && source.hold == key.tracker.hold
                });
            if keep {
                key.tracker.region = source.unwrap().region.clone();
            } else if same_mount && key.styled {
                clear.push(key.tracker.element_id);
            }
            keep
        });
        clear.sort_unstable_by_key(|id| id.0);
        clear.dedup();
        for element in clear {
            self.clear_touch_feedback(element, tx, log);
        }
    }

    pub(super) fn handle_touch_key_timers(&mut self, now: Instant, tx: &dyn TreeSink, log: bool) {
        let mut due: Vec<_> = self
            .touch_keys
            .iter()
            .filter_map(|(id, key)| {
                key.deadline
                    .filter(|deadline| *deadline <= now)
                    .map(|deadline| (deadline, *id))
            })
            .collect();
        due.sort_unstable();
        for (_, id) in due {
            let Some(key) = self.touch_keys.get_mut(&id) else {
                continue;
            };
            let actions = if key.tracker.phase == VirtualKeyPhase::Cancelled {
                key.deadline = None;
                vec![]
            } else {
                key.hold_fired = true;
                match key.tracker.hold {
                    crate::tree::attrs::VirtualKeyHoldMode::None => {
                        key.deadline = None;
                        vec![]
                    }
                    crate::tree::attrs::VirtualKeyHoldMode::Event => {
                        key.deadline = None;
                        vec![ListenerAction::ElixirEvent(registry_builder::ElixirEvent {
                            element_id: key.tracker.element_id,
                            kind: ElementEventKind::VirtualKeyHold,
                            payload: None,
                        })]
                    }
                    crate::tree::attrs::VirtualKeyHoldMode::Repeat => {
                        key.tracker.phase = VirtualKeyPhase::Repeating;
                        key.deadline =
                            Some(now + Duration::from_millis(u64::from(key.tracker.repeat_ms)));
                        vec![ListenerAction::SyntheticInput(
                            registry_builder::synthetic_input_sequence_for_virtual_key_tap(
                                &key.tracker.tap,
                            ),
                        )]
                    }
                }
            };
            self.apply_listener_actions(actions, tx, log, DispatchMode::Normal);
        }
    }

    pub(super) fn touch_key_deadline(&self) -> Option<Instant> {
        self.touch_keys
            .values()
            .filter_map(|key| key.deadline)
            .min()
    }
}
