//! Rubber-band overscroll: drags past a content edge move the content with resistance, flings
//! overshoot the edge, and a critically damped spring returns both to the edge. Opt-in through
//! `scroll_bounce`; the session position stays clamped to [0, 1].
use super::*;

impl DirectEventRuntime {
    pub(super) fn overscroll_spring(&self) -> Option<&OverscrollSpring> {
        self.overscroll.as_ref()?.spring.as_ref()
    }

    /// Runs after a drag dispatch, before its tree messages are sent.
    pub(super) fn apply_drag_overscroll(
        &mut self,
        tree_msgs: &mut Vec<TreeMsg>,
        changes: &[RuntimeChange],
    ) {
        if !self.scroll_bounce {
            return;
        }
        let Some((axis, motion_element)) = self
            .drag_motion
            .as_ref()
            .map(|motion| (motion.axis, motion.element_id))
        else {
            return;
        };
        let registry_builder::DragTrackerState::Active { last_x, last_y, .. } =
            self.runtime_overlay.drag
        else {
            return;
        };
        let Some((x, y)) = changes.iter().find_map(|change| match change {
            RuntimeChange::UpdateDragTrackerPointer { last_x, last_y, .. } => {
                Some((*last_x, *last_y))
            }
            _ => None,
        }) else {
            return;
        };
        let pointer_delta =
            Self::pointer_axis_value(axis, x, y) - Self::pointer_axis_value(axis, last_x, last_y);
        if pointer_delta.abs() <= f32::EPSILON {
            return;
        }
        let request = tree_msgs
            .iter()
            .find_map(|message| scroll_request_axis_delta(message, axis));

        if let Some(overscroll) = self.overscroll.as_mut() {
            let element_id = overscroll.element_id;
            let related = overscroll.axis == axis
                && (element_id == motion_element
                    || self.drag_scroll_target == Some(element_id)
                    || request.is_some_and(|(id, _)| id == element_id));
            if !related {
                self.start_overscroll_spring(Instant::now());
                return;
            }
            let delta = request
                .filter(|(id, _)| *id == element_id)
                .map_or(pointer_delta, |(_, delta)| delta);
            let raw = overscroll.raw;
            let next = raw - delta;
            set_axis_scroll_requests(tree_msgs, axis, None);
            if next * raw > 0.0 {
                overscroll.raw = next;
            } else {
                // Back inside the content: the rest of the movement scrolls it.
                overscroll.raw = 0.0;
                set_axis_scroll_requests(tree_msgs, axis, Some((element_id, delta - raw)));
            }
            if let Some(message) = self.take_overscroll_update() {
                tree_msgs.push(message);
            }
            return;
        }

        let (element_id, delta, room) = match request {
            Some((element_id, delta)) => {
                let Some(room) = self.scroll_room(element_id, axis, delta) else {
                    return;
                };
                if delta.abs() <= room + 0.01 {
                    return;
                }
                (element_id, delta, room)
            }
            None => {
                let target = [Some(motion_element), self.drag_scroll_target]
                    .into_iter()
                    .flatten()
                    .find(|id| {
                        self.scroll_room(*id, axis, pointer_delta)
                            .is_some_and(|room| room <= 0.01)
                    });
                let Some(element_id) = target else {
                    return;
                };
                (element_id, pointer_delta, 0.0)
            }
        };
        let excess = delta - delta.signum() * room;
        self.overscroll = Some(OverscrollState {
            element_id,
            axis,
            viewport: self.overscroll_viewport(element_id, axis),
            raw: -excess,
            spring: None,
            sent: 0.0,
        });
        if let Some(message) = self.take_overscroll_update() {
            tree_msgs.push(message);
        }
    }

    /// A press stops the spring at its current offset; a release starts it again.
    pub(super) fn overscroll_for_input(&mut self, event: &InputEvent) {
        let Some(overscroll) = self.overscroll.as_mut() else {
            return;
        };
        let now = Instant::now();
        match event {
            InputEvent::CursorButton {
                action: crate::input::ACTION_PRESS,
                ..
            }
            | InputEvent::Touch {
                phase: crate::input::TouchPhase::Down,
                ..
            } => {
                if let Some(spring) = overscroll.spring.take() {
                    overscroll.raw = spring.raw_at(now);
                }
            }
            InputEvent::CursorButton {
                action: crate::input::ACTION_RELEASE,
                ..
            }
            | InputEvent::Touch {
                phase: crate::input::TouchPhase::Up | crate::input::TouchPhase::Cancel,
                ..
            }
            | InputEvent::CursorEntered { entered: false }
            | InputEvent::Focused { focused: false }
                if overscroll.spring.is_none() =>
            {
                self.start_overscroll_spring(now);
            }
            _ => {}
        }
    }

    /// A fling that reaches the edge continues past it on the spring and returns.
    /// `excess_delta` and `velocity` are in scroll-request space (positive moves content down).
    pub(super) fn start_overshoot(
        &mut self,
        inertia: &InertialScrollState,
        excess_delta: f32,
        velocity: f32,
        now: Instant,
    ) {
        if !self.scroll_bounce {
            return;
        }
        let viewport = self.overscroll_viewport(inertia.element_id, inertia.axis);
        // The spring from rest peaks at v / (omega * e); this caps the peak at one viewport.
        let cap = viewport * OVERSCROLL_SPRING_OMEGA * std::f32::consts::E;
        let from = -excess_delta;
        self.overscroll = Some(OverscrollState {
            element_id: inertia.element_id,
            axis: inertia.axis,
            viewport,
            raw: from,
            spring: Some(OverscrollSpring {
                started_at: now,
                from,
                velocity: (-velocity).clamp(-cap, cap),
                watchdog_deadline: self.next_inertial_watchdog_deadline(now),
            }),
            sent: 0.0,
        });
        self.backend_wake.request_redraw();
    }

    pub(super) fn start_overscroll_spring(&mut self, now: Instant) {
        let deadline = self.next_inertial_watchdog_deadline(now);
        let Some(overscroll) = self.overscroll.as_mut() else {
            return;
        };
        overscroll.spring = Some(OverscrollSpring {
            started_at: now,
            from: overscroll.raw,
            velocity: 0.0,
            watchdog_deadline: deadline,
        });
        self.backend_wake.request_redraw();
    }

    pub(super) fn step_overscroll(&mut self, now: Instant, tree_tx: &dyn TreeSink, log: bool) {
        let deadline = self.next_inertial_watchdog_deadline(now);
        let Some(overscroll) = self.overscroll.as_mut() else {
            return;
        };
        let Some(spring) = overscroll.spring.as_mut() else {
            return;
        };
        let elapsed = now.saturating_duration_since(spring.started_at);
        let raw = spring.raw_at(now);
        let settled = elapsed.as_secs_f32() * OVERSCROLL_SPRING_OMEGA >= 1.0
            && rubber_band(raw, overscroll.viewport).abs() < OVERSCROLL_STOP_TOLERANCE;
        if settled || elapsed >= OVERSCROLL_MAX_DURATION {
            overscroll.raw = 0.0;
        } else {
            spring.watchdog_deadline = deadline;
            overscroll.raw = raw;
        }
        let running = overscroll.raw != 0.0;
        if let Some(message) = self.take_overscroll_update() {
            send_tree(tree_tx, message, log);
        }
        if running {
            self.backend_wake.request_redraw();
        }
    }

    /// Puts the content back at the edge at once.
    pub(super) fn clear_overscroll(&mut self, tree_tx: &dyn TreeSink, log: bool) {
        if let Some(overscroll) = self.overscroll.as_mut() {
            overscroll.raw = 0.0;
        }
        if let Some(message) = self.take_overscroll_update() {
            send_tree(tree_tx, message, log);
        }
    }

    /// The tree message for a changed displayed offset; drops the state once it is back at zero.
    fn take_overscroll_update(&mut self) -> Option<TreeMsg> {
        let overscroll = self.overscroll.as_mut()?;
        // Whole pixels keep the subpixel phase of the cached scroll payload, so no subpixel updates go out.
        let displayed = overscroll.displayed().round();
        let message = (displayed != overscroll.sent).then(|| {
            overscroll.sent = displayed;
            TreeMsg::SetScrollOverscroll {
                element_id: overscroll.element_id,
                offset: match overscroll.axis {
                    ScrollbarAxis::X => [displayed, 0.0],
                    ScrollbarAxis::Y => [0.0, displayed],
                },
            }
        });
        if overscroll.raw == 0.0 {
            self.overscroll = None;
        }
        message
    }

    /// Pixels the container can still scroll in the direction of a request delta, or `None` when
    /// it does not scroll on this axis.
    fn scroll_room(&self, element_id: NodeId, axis: ScrollbarAxis, delta: f32) -> Option<f32> {
        let session = self.scroll_sessions.get(&element_id)?;
        let index = usize::from(axis == ScrollbarAxis::Y);
        let range = session.node.range[index];
        if !session.node.enabled[index] {
            return None;
        }
        if range <= 0.0 {
            return Some(0.0);
        }
        let position = session.position[index];
        Some(if delta > 0.0 {
            position * range
        } else {
            (1.0 - position) * range
        } as f32)
    }

    /// A scroll container on `axis` whose content fits its viewport.
    pub(super) fn fits_on_axis(&self, element_id: NodeId, axis: ScrollbarAxis) -> bool {
        let index = usize::from(axis == ScrollbarAxis::Y);
        self.scroll_sessions
            .get(&element_id)
            .is_some_and(|session| session.node.enabled[index] && session.node.range[index] <= 0.0)
    }

    fn overscroll_viewport(&self, element_id: NodeId, axis: ScrollbarAxis) -> f32 {
        let index = usize::from(axis == ScrollbarAxis::Y);
        self.scroll_sessions
            .get(&element_id)
            .map_or(0.0, |session| session.node.viewport[index] as f32)
    }
}

/// Clears the axis component of every scroll request, then adds `replacement` if given.
fn set_axis_scroll_requests(
    tree_msgs: &mut Vec<TreeMsg>,
    axis: ScrollbarAxis,
    replacement: Option<(NodeId, f32)>,
) {
    for message in tree_msgs.iter_mut() {
        if let TreeMsg::ScrollRequest { dx, dy, .. } = message {
            match axis {
                ScrollbarAxis::X => *dx = 0.0,
                ScrollbarAxis::Y => *dy = 0.0,
            }
        }
    }
    tree_msgs.retain(|message| {
        !matches!(message, TreeMsg::ScrollRequest { dx, dy, .. } if *dx == 0.0 && *dy == 0.0)
    });
    if let Some((element_id, delta)) = replacement.filter(|(_, delta)| *delta != 0.0) {
        let (dx, dy) = match axis {
            ScrollbarAxis::X => (delta, 0.0),
            ScrollbarAxis::Y => (0.0, delta),
        };
        tree_msgs.push(TreeMsg::ScrollRequest { element_id, dx, dy });
    }
}
