#![allow(dead_code)]

use std::collections::HashMap;

use crate::input::{
    EVENT_CLICK, EVENT_FOCUSABLE, EVENT_MOUSE_DOWN, EVENT_MOUSE_DOWN_STYLE, EVENT_MOUSE_ENTER,
    EVENT_MOUSE_LEAVE, EVENT_MOUSE_MOVE, EVENT_MOUSE_OVER_STYLE, EVENT_MOUSE_UP, EVENT_PRESS,
    EVENT_SCROLL_X_NEG, EVENT_SCROLL_X_POS, EVENT_SCROLL_Y_NEG, EVENT_SCROLL_Y_POS,
    EVENT_TEXT_INPUT, MOD_ALT, MOD_CTRL, MOD_META, MOD_SHIFT, SCROLL_LINE_PIXELS,
};
use crate::tree::element::ElementId;

use super::{EventNode, TextInputCommandRequest, TextInputEditRequest, TextInputPreeditRequest};

pub type NodeIdx = u32;
pub type DispatchRuleId = u32;

#[repr(u16)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum TriggerId {
    // Pointer
    CursorButtonLeftPress,
    CursorButtonLeftRelease,
    CursorButtonMiddlePress,
    CursorMove,
    CursorEnter,
    CursorLeave,
    CursorScrollXNeg,
    CursorScrollXPos,
    CursorScrollYNeg,
    CursorScrollYPos,
    // Keyboard
    KeyLeftPress,
    KeyRightPress,
    KeyUpPress,
    KeyDownPress,
    KeyTabPress,
    KeyEnterPress,
    KeyHomePress,
    KeyEndPress,
    KeyBackspacePress,
    KeyDeletePress,
    // Text/IME
    TextCommit,
    TextPreedit,
    TextPreeditClear,
    // Window/state
    WindowFocusLost,
    WindowResized,
}

impl TriggerId {
    pub const COUNT: usize = TriggerId::WindowResized as usize + 1;

    #[inline]
    pub const fn index(self) -> usize {
        self as usize
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScrollDirection {
    Left,
    Right,
    Up,
    Down,
}

impl ScrollDirection {
    #[inline]
    pub const fn index(self) -> usize {
        match self {
            ScrollDirection::Left => 0,
            ScrollDirection::Right => 1,
            ScrollDirection::Up => 2,
            ScrollDirection::Down => 3,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
pub struct PriorityKey {
    pub depth: u32,
    pub paint_order: u32,
    pub insertion_order: u32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RuleScope {
    Target(NodeIdx),
    Focused(NodeIdx),
    PointerHit,
    NoFocus,
    Any,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct DispatchRulePredicate {
    pub required_mods: u8,
    pub forbidden_mods: u8,
    pub runtime_flags_all: u32,
    pub runtime_flags_none: u32,
}

#[derive(Clone, Debug, PartialEq)]
pub enum DispatchRuleAction {
    EmitElementEvent {
        element: NodeIdx,
    },
    ScrollRequest {
        element: NodeIdx,
        dx: f32,
        dy: f32,
    },
    FocusChange {
        next: Option<NodeIdx>,
    },
    TextCommand {
        element: NodeIdx,
        request: TextInputCommandRequest,
    },
    TextEdit {
        element: NodeIdx,
        request: TextInputEditRequest,
    },
    TextPreedit {
        element: NodeIdx,
        request: TextInputPreeditRequest,
    },
}

#[derive(Clone, Debug, PartialEq)]
pub struct DispatchRule {
    pub scope: RuleScope,
    pub predicate: DispatchRulePredicate,
    pub actions: Vec<DispatchRuleAction>,
    pub priority: PriorityKey,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct DispatchCtx {
    pub mods: u8,
    pub focused: Option<NodeIdx>,
    pub cursor: Option<(f32, f32)>,
    pub runtime_flags: u32,
}

#[derive(Clone, Debug, PartialEq)]
pub enum DispatchJob {
    Targeted {
        trigger: TriggerId,
        target: NodeIdx,
        ctx: DispatchCtx,
    },
    Pointed {
        trigger: TriggerId,
        x: f32,
        y: f32,
        ctx: DispatchCtx,
    },
    Untargeted {
        trigger: TriggerId,
        ctx: DispatchCtx,
    },
}

#[derive(Clone, Debug, Default)]
pub struct TriggerBucket {
    pub targeted: HashMap<NodeIdx, Vec<DispatchRuleId>>,
    pub ordered: Vec<DispatchRuleId>,
}

#[derive(Clone, Debug)]
pub struct NodeMeta {
    pub id: ElementId,
    pub visible: bool,
    pub flags: u16,
}

#[derive(Clone, Debug)]
pub struct PointerIndexPhase1 {
    pub candidates_by_trigger: Vec<Vec<NodeIdx>>,
}

impl Default for PointerIndexPhase1 {
    fn default() -> Self {
        Self {
            candidates_by_trigger: vec![Vec::new(); TriggerId::COUNT],
        }
    }
}

#[derive(Clone, Debug)]
pub struct EventRegistryV2 {
    pub nodes: Vec<NodeMeta>,
    pub id_to_idx: HashMap<ElementId, NodeIdx>,
    pub dispatch_rules: Vec<DispatchRule>,
    pub buckets: Vec<TriggerBucket>,
    pub pointer_index: PointerIndexPhase1,
    pub focus_order: Vec<NodeIdx>,
    pub focus_pos: Vec<u32>,
    pub first_visible_scrollable_by_dir: [Option<NodeIdx>; 4],
}

impl Default for EventRegistryV2 {
    fn default() -> Self {
        Self {
            nodes: Vec::new(),
            id_to_idx: HashMap::new(),
            dispatch_rules: Vec::new(),
            buckets: vec![TriggerBucket::default(); TriggerId::COUNT],
            pointer_index: PointerIndexPhase1::default(),
            focus_order: Vec::new(),
            focus_pos: Vec::new(),
            first_visible_scrollable_by_dir: [None; 4],
        }
    }
}

impl EventRegistryV2 {
    const FOCUS_NONE: u32 = u32::MAX;

    pub fn from_event_nodes(event_nodes: &[EventNode]) -> Self {
        let mut out = Self {
            nodes: Vec::with_capacity(event_nodes.len()),
            id_to_idx: HashMap::with_capacity(event_nodes.len()),
            ..Self::default()
        };

        for node in event_nodes {
            let idx = out.nodes.len() as NodeIdx;
            out.id_to_idx.insert(node.id.clone(), idx);
            out.nodes.push(NodeMeta {
                id: node.id.clone(),
                visible: node.visible,
                flags: node.flags,
            });
        }

        out.focus_pos = vec![Self::FOCUS_NONE; out.nodes.len()];

        for (idx_usize, node) in out.nodes.iter().enumerate() {
            let idx = idx_usize as NodeIdx;

            if node.flags & EVENT_FOCUSABLE != 0 {
                let pos = out.focus_order.len() as u32;
                out.focus_order.push(idx);
                out.focus_pos[idx_usize] = pos;
            }

            if !node.visible {
                continue;
            }

            if out.first_visible_scrollable_by_dir[ScrollDirection::Left.index()].is_none()
                && node.flags & EVENT_SCROLL_X_NEG != 0
            {
                out.first_visible_scrollable_by_dir[ScrollDirection::Left.index()] = Some(idx);
            }
            if out.first_visible_scrollable_by_dir[ScrollDirection::Right.index()].is_none()
                && node.flags & EVENT_SCROLL_X_POS != 0
            {
                out.first_visible_scrollable_by_dir[ScrollDirection::Right.index()] = Some(idx);
            }
            if out.first_visible_scrollable_by_dir[ScrollDirection::Up.index()].is_none()
                && node.flags & EVENT_SCROLL_Y_NEG != 0
            {
                out.first_visible_scrollable_by_dir[ScrollDirection::Up.index()] = Some(idx);
            }
            if out.first_visible_scrollable_by_dir[ScrollDirection::Down.index()].is_none()
                && node.flags & EVENT_SCROLL_Y_POS != 0
            {
                out.first_visible_scrollable_by_dir[ScrollDirection::Down.index()] = Some(idx);
            }
        }

        for (idx_usize, node) in out.nodes.iter().enumerate().rev() {
            if !node.visible {
                continue;
            }

            let idx = idx_usize as NodeIdx;
            for trigger in POINTER_TRIGGERS {
                if pointer_trigger_matches(node.flags, trigger) {
                    out.pointer_index.candidates_by_trigger[trigger.index()].push(idx);
                }
            }
        }

        out.populate_keyboard_focus_rules(event_nodes);

        out
    }

    fn populate_keyboard_focus_rules(&mut self, event_nodes: &[EventNode]) {
        let blocked_mods = MOD_CTRL | MOD_ALT | MOD_META;
        let mut node_by_id: HashMap<ElementId, &EventNode> =
            HashMap::with_capacity(event_nodes.len());
        for node in event_nodes {
            node_by_id.insert(node.id.clone(), node);
        }

        for node in event_nodes {
            let Some(source_idx) = self.node_idx(&node.id) else {
                continue;
            };

            if node.flags & EVENT_FOCUSABLE == 0 {
                continue;
            }

            if node.flags & EVENT_TEXT_INPUT != 0 {
                if let Some(descriptor) = node.text_input.as_ref() {
                    let has_selection = descriptor
                        .selection_anchor
                        .is_some_and(|anchor| anchor != descriptor.cursor);

                    let can_move_left_plain = descriptor.cursor > 0 || has_selection;
                    let can_move_left_extend = descriptor.cursor > 0;
                    let can_move_right_plain =
                        descriptor.cursor < descriptor.content_len || has_selection;
                    let can_move_right_extend = descriptor.cursor < descriptor.content_len;
                    let can_move_home_plain = descriptor.cursor > 0 || has_selection;
                    let can_move_home_extend = descriptor.cursor > 0;
                    let can_move_end_plain =
                        descriptor.cursor < descriptor.content_len || has_selection;
                    let can_move_end_extend = descriptor.cursor < descriptor.content_len;

                    if can_move_left_plain {
                        self.push_targeted_rule(
                            TriggerId::KeyLeftPress,
                            source_idx,
                            DispatchRule {
                                scope: RuleScope::Focused(source_idx),
                                predicate: DispatchRulePredicate {
                                    forbidden_mods: blocked_mods | MOD_SHIFT,
                                    ..DispatchRulePredicate::default()
                                },
                                actions: vec![DispatchRuleAction::TextEdit {
                                    element: source_idx,
                                    request: TextInputEditRequest::MoveLeft {
                                        extend_selection: false,
                                    },
                                }],
                                priority: self.next_priority(),
                            },
                        );
                    }

                    if can_move_left_extend {
                        self.push_targeted_rule(
                            TriggerId::KeyLeftPress,
                            source_idx,
                            DispatchRule {
                                scope: RuleScope::Focused(source_idx),
                                predicate: DispatchRulePredicate {
                                    required_mods: MOD_SHIFT,
                                    forbidden_mods: blocked_mods,
                                    ..DispatchRulePredicate::default()
                                },
                                actions: vec![DispatchRuleAction::TextEdit {
                                    element: source_idx,
                                    request: TextInputEditRequest::MoveLeft {
                                        extend_selection: true,
                                    },
                                }],
                                priority: self.next_priority(),
                            },
                        );
                    }

                    if can_move_right_plain {
                        self.push_targeted_rule(
                            TriggerId::KeyRightPress,
                            source_idx,
                            DispatchRule {
                                scope: RuleScope::Focused(source_idx),
                                predicate: DispatchRulePredicate {
                                    forbidden_mods: blocked_mods | MOD_SHIFT,
                                    ..DispatchRulePredicate::default()
                                },
                                actions: vec![DispatchRuleAction::TextEdit {
                                    element: source_idx,
                                    request: TextInputEditRequest::MoveRight {
                                        extend_selection: false,
                                    },
                                }],
                                priority: self.next_priority(),
                            },
                        );
                    }

                    if can_move_right_extend {
                        self.push_targeted_rule(
                            TriggerId::KeyRightPress,
                            source_idx,
                            DispatchRule {
                                scope: RuleScope::Focused(source_idx),
                                predicate: DispatchRulePredicate {
                                    required_mods: MOD_SHIFT,
                                    forbidden_mods: blocked_mods,
                                    ..DispatchRulePredicate::default()
                                },
                                actions: vec![DispatchRuleAction::TextEdit {
                                    element: source_idx,
                                    request: TextInputEditRequest::MoveRight {
                                        extend_selection: true,
                                    },
                                }],
                                priority: self.next_priority(),
                            },
                        );
                    }

                    if can_move_home_plain {
                        self.push_targeted_rule(
                            TriggerId::KeyHomePress,
                            source_idx,
                            DispatchRule {
                                scope: RuleScope::Focused(source_idx),
                                predicate: DispatchRulePredicate {
                                    forbidden_mods: blocked_mods | MOD_SHIFT,
                                    ..DispatchRulePredicate::default()
                                },
                                actions: vec![DispatchRuleAction::TextEdit {
                                    element: source_idx,
                                    request: TextInputEditRequest::MoveHome {
                                        extend_selection: false,
                                    },
                                }],
                                priority: self.next_priority(),
                            },
                        );
                    }

                    if can_move_home_extend {
                        self.push_targeted_rule(
                            TriggerId::KeyHomePress,
                            source_idx,
                            DispatchRule {
                                scope: RuleScope::Focused(source_idx),
                                predicate: DispatchRulePredicate {
                                    required_mods: MOD_SHIFT,
                                    forbidden_mods: blocked_mods,
                                    ..DispatchRulePredicate::default()
                                },
                                actions: vec![DispatchRuleAction::TextEdit {
                                    element: source_idx,
                                    request: TextInputEditRequest::MoveHome {
                                        extend_selection: true,
                                    },
                                }],
                                priority: self.next_priority(),
                            },
                        );
                    }

                    if can_move_end_plain {
                        self.push_targeted_rule(
                            TriggerId::KeyEndPress,
                            source_idx,
                            DispatchRule {
                                scope: RuleScope::Focused(source_idx),
                                predicate: DispatchRulePredicate {
                                    forbidden_mods: blocked_mods | MOD_SHIFT,
                                    ..DispatchRulePredicate::default()
                                },
                                actions: vec![DispatchRuleAction::TextEdit {
                                    element: source_idx,
                                    request: TextInputEditRequest::MoveEnd {
                                        extend_selection: false,
                                    },
                                }],
                                priority: self.next_priority(),
                            },
                        );
                    }

                    if can_move_end_extend {
                        self.push_targeted_rule(
                            TriggerId::KeyEndPress,
                            source_idx,
                            DispatchRule {
                                scope: RuleScope::Focused(source_idx),
                                predicate: DispatchRulePredicate {
                                    required_mods: MOD_SHIFT,
                                    forbidden_mods: blocked_mods,
                                    ..DispatchRulePredicate::default()
                                },
                                actions: vec![DispatchRuleAction::TextEdit {
                                    element: source_idx,
                                    request: TextInputEditRequest::MoveEnd {
                                        extend_selection: true,
                                    },
                                }],
                                priority: self.next_priority(),
                            },
                        );
                    }
                }

                self.push_targeted_rule(
                    TriggerId::KeyBackspacePress,
                    source_idx,
                    DispatchRule {
                        scope: RuleScope::Focused(source_idx),
                        predicate: DispatchRulePredicate::default(),
                        actions: vec![DispatchRuleAction::TextEdit {
                            element: source_idx,
                            request: TextInputEditRequest::Backspace,
                        }],
                        priority: self.next_priority(),
                    },
                );

                self.push_targeted_rule(
                    TriggerId::KeyDeletePress,
                    source_idx,
                    DispatchRule {
                        scope: RuleScope::Focused(source_idx),
                        predicate: DispatchRulePredicate::default(),
                        actions: vec![DispatchRuleAction::TextEdit {
                            element: source_idx,
                            request: TextInputEditRequest::Delete,
                        }],
                        priority: self.next_priority(),
                    },
                );
            }

            if node.flags & EVENT_PRESS != 0 {
                let rule = DispatchRule {
                    scope: RuleScope::Focused(source_idx),
                    predicate: DispatchRulePredicate {
                        forbidden_mods: blocked_mods,
                        ..DispatchRulePredicate::default()
                    },
                    actions: vec![DispatchRuleAction::EmitElementEvent {
                        element: source_idx,
                    }],
                    priority: self.next_priority(),
                };
                self.push_targeted_rule(TriggerId::KeyEnterPress, source_idx, rule);
            }

            if let Some(target_id) = node.key_scroll_targets.left.as_ref()
                && let Some(target_idx) = self.node_idx(target_id)
            {
                self.push_targeted_rule(
                    TriggerId::KeyLeftPress,
                    source_idx,
                    DispatchRule {
                        scope: RuleScope::Focused(source_idx),
                        predicate: DispatchRulePredicate {
                            forbidden_mods: blocked_mods,
                            ..DispatchRulePredicate::default()
                        },
                        actions: vec![DispatchRuleAction::ScrollRequest {
                            element: target_idx,
                            dx: SCROLL_LINE_PIXELS,
                            dy: 0.0,
                        }],
                        priority: self.next_priority(),
                    },
                );
            }

            if let Some(target_id) = node.key_scroll_targets.right.as_ref()
                && let Some(target_idx) = self.node_idx(target_id)
            {
                self.push_targeted_rule(
                    TriggerId::KeyRightPress,
                    source_idx,
                    DispatchRule {
                        scope: RuleScope::Focused(source_idx),
                        predicate: DispatchRulePredicate {
                            forbidden_mods: blocked_mods,
                            ..DispatchRulePredicate::default()
                        },
                        actions: vec![DispatchRuleAction::ScrollRequest {
                            element: target_idx,
                            dx: -SCROLL_LINE_PIXELS,
                            dy: 0.0,
                        }],
                        priority: self.next_priority(),
                    },
                );
            }

            if let Some(target_id) = node.key_scroll_targets.up.as_ref()
                && let Some(target_idx) = self.node_idx(target_id)
            {
                self.push_targeted_rule(
                    TriggerId::KeyUpPress,
                    source_idx,
                    DispatchRule {
                        scope: RuleScope::Focused(source_idx),
                        predicate: DispatchRulePredicate {
                            forbidden_mods: blocked_mods,
                            ..DispatchRulePredicate::default()
                        },
                        actions: vec![DispatchRuleAction::ScrollRequest {
                            element: target_idx,
                            dx: 0.0,
                            dy: SCROLL_LINE_PIXELS,
                        }],
                        priority: self.next_priority(),
                    },
                );
            }

            if let Some(target_id) = node.key_scroll_targets.down.as_ref()
                && let Some(target_idx) = self.node_idx(target_id)
            {
                self.push_targeted_rule(
                    TriggerId::KeyDownPress,
                    source_idx,
                    DispatchRule {
                        scope: RuleScope::Focused(source_idx),
                        predicate: DispatchRulePredicate {
                            forbidden_mods: blocked_mods,
                            ..DispatchRulePredicate::default()
                        },
                        actions: vec![DispatchRuleAction::ScrollRequest {
                            element: target_idx,
                            dx: 0.0,
                            dy: -SCROLL_LINE_PIXELS,
                        }],
                        priority: self.next_priority(),
                    },
                );
            }
        }

        if let Some(target_idx) = self.first_visible_scrollable(ScrollDirection::Left) {
            self.push_ordered_rule(
                TriggerId::KeyLeftPress,
                DispatchRule {
                    scope: RuleScope::Any,
                    predicate: DispatchRulePredicate {
                        forbidden_mods: blocked_mods,
                        ..DispatchRulePredicate::default()
                    },
                    actions: vec![DispatchRuleAction::ScrollRequest {
                        element: target_idx,
                        dx: SCROLL_LINE_PIXELS,
                        dy: 0.0,
                    }],
                    priority: self.next_priority(),
                },
            );
        }
        if let Some(target_idx) = self.first_visible_scrollable(ScrollDirection::Right) {
            self.push_ordered_rule(
                TriggerId::KeyRightPress,
                DispatchRule {
                    scope: RuleScope::Any,
                    predicate: DispatchRulePredicate {
                        forbidden_mods: blocked_mods,
                        ..DispatchRulePredicate::default()
                    },
                    actions: vec![DispatchRuleAction::ScrollRequest {
                        element: target_idx,
                        dx: -SCROLL_LINE_PIXELS,
                        dy: 0.0,
                    }],
                    priority: self.next_priority(),
                },
            );
        }
        if let Some(target_idx) = self.first_visible_scrollable(ScrollDirection::Up) {
            self.push_ordered_rule(
                TriggerId::KeyUpPress,
                DispatchRule {
                    scope: RuleScope::Any,
                    predicate: DispatchRulePredicate {
                        forbidden_mods: blocked_mods,
                        ..DispatchRulePredicate::default()
                    },
                    actions: vec![DispatchRuleAction::ScrollRequest {
                        element: target_idx,
                        dx: 0.0,
                        dy: SCROLL_LINE_PIXELS,
                    }],
                    priority: self.next_priority(),
                },
            );
        }
        if let Some(target_idx) = self.first_visible_scrollable(ScrollDirection::Down) {
            self.push_ordered_rule(
                TriggerId::KeyDownPress,
                DispatchRule {
                    scope: RuleScope::Any,
                    predicate: DispatchRulePredicate {
                        forbidden_mods: blocked_mods,
                        ..DispatchRulePredicate::default()
                    },
                    actions: vec![DispatchRuleAction::ScrollRequest {
                        element: target_idx,
                        dx: 0.0,
                        dy: -SCROLL_LINE_PIXELS,
                    }],
                    priority: self.next_priority(),
                },
            );
        }

        if self.focus_order.is_empty() {
            return;
        }

        let focus_order = self.focus_order.clone();
        for (position, focused_idx) in focus_order.iter().enumerate() {
            let next_position = (position + 1) % focus_order.len();
            let prev_position = if position == 0 {
                focus_order.len() - 1
            } else {
                position - 1
            };
            let next_idx = focus_order[next_position];
            let prev_idx = focus_order[prev_position];

            self.push_targeted_rule(
                TriggerId::KeyTabPress,
                *focused_idx,
                DispatchRule {
                    scope: RuleScope::Focused(*focused_idx),
                    predicate: DispatchRulePredicate {
                        forbidden_mods: blocked_mods | MOD_SHIFT,
                        ..DispatchRulePredicate::default()
                    },
                    actions: self.focus_change_actions(next_idx, &node_by_id),
                    priority: self.next_priority(),
                },
            );

            self.push_targeted_rule(
                TriggerId::KeyTabPress,
                *focused_idx,
                DispatchRule {
                    scope: RuleScope::Focused(*focused_idx),
                    predicate: DispatchRulePredicate {
                        required_mods: MOD_SHIFT,
                        forbidden_mods: blocked_mods,
                        ..DispatchRulePredicate::default()
                    },
                    actions: self.focus_change_actions(prev_idx, &node_by_id),
                    priority: self.next_priority(),
                },
            );
        }

        let first_idx = self.focus_order[0];
        self.push_ordered_rule(
            TriggerId::KeyTabPress,
            DispatchRule {
                scope: RuleScope::NoFocus,
                predicate: DispatchRulePredicate {
                    forbidden_mods: blocked_mods | MOD_SHIFT,
                    ..DispatchRulePredicate::default()
                },
                actions: self.focus_change_actions(first_idx, &node_by_id),
                priority: self.next_priority(),
            },
        );

        let last_idx = *self.focus_order.last().unwrap_or(&first_idx);
        self.push_ordered_rule(
            TriggerId::KeyTabPress,
            DispatchRule {
                scope: RuleScope::NoFocus,
                predicate: DispatchRulePredicate {
                    required_mods: MOD_SHIFT,
                    forbidden_mods: blocked_mods,
                    ..DispatchRulePredicate::default()
                },
                actions: self.focus_change_actions(last_idx, &node_by_id),
                priority: self.next_priority(),
            },
        );
    }

    fn focus_change_actions(
        &self,
        next_idx: NodeIdx,
        node_by_id: &HashMap<ElementId, &EventNode>,
    ) -> Vec<DispatchRuleAction> {
        let mut actions = vec![DispatchRuleAction::FocusChange {
            next: Some(next_idx),
        }];

        let Some(next_id) = self.node_id(next_idx) else {
            return actions;
        };

        let Some(node) = node_by_id.get(next_id) else {
            return actions;
        };

        for request in &node.focus_reveal_scrolls {
            if let Some(target_idx) = self.node_idx(&request.element_id) {
                actions.push(DispatchRuleAction::ScrollRequest {
                    element: target_idx,
                    dx: request.dx,
                    dy: request.dy,
                });
            }
        }

        actions
    }

    fn next_priority(&self) -> PriorityKey {
        PriorityKey {
            insertion_order: self.dispatch_rules.len() as u32,
            ..PriorityKey::default()
        }
    }

    fn push_targeted_rule(
        &mut self,
        trigger: TriggerId,
        target: NodeIdx,
        rule: DispatchRule,
    ) -> DispatchRuleId {
        let rule_id = self.dispatch_rules.len() as DispatchRuleId;
        self.dispatch_rules.push(rule);
        self.buckets[trigger.index()]
            .targeted
            .entry(target)
            .or_default()
            .push(rule_id);
        rule_id
    }

    fn push_ordered_rule(&mut self, trigger: TriggerId, rule: DispatchRule) -> DispatchRuleId {
        let rule_id = self.dispatch_rules.len() as DispatchRuleId;
        self.dispatch_rules.push(rule);
        self.buckets[trigger.index()].ordered.push(rule_id);
        rule_id
    }

    pub fn node_idx(&self, id: &ElementId) -> Option<NodeIdx> {
        self.id_to_idx.get(id).copied()
    }

    pub fn node_id(&self, idx: NodeIdx) -> Option<&ElementId> {
        self.nodes.get(idx as usize).map(|node| &node.id)
    }

    pub fn focus_order_ids(&self) -> Vec<ElementId> {
        self.focus_order
            .iter()
            .filter_map(|idx| self.node_id(*idx).cloned())
            .collect()
    }

    pub fn focus_order_next(&self, current: Option<NodeIdx>, reverse: bool) -> Option<NodeIdx> {
        if self.focus_order.is_empty() {
            return None;
        }

        let next_index = match current
            .and_then(|idx| self.focus_pos.get(idx as usize).copied())
            .filter(|pos| *pos != Self::FOCUS_NONE)
        {
            Some(pos) if reverse => {
                if pos == 0 {
                    self.focus_order.len() as u32 - 1
                } else {
                    pos - 1
                }
            }
            Some(pos) => (pos + 1) % self.focus_order.len() as u32,
            None if reverse => self.focus_order.len() as u32 - 1,
            None => 0,
        };

        self.focus_order.get(next_index as usize).copied()
    }

    pub fn first_visible_scrollable(&self, direction: ScrollDirection) -> Option<NodeIdx> {
        self.first_visible_scrollable_by_dir[direction.index()]
    }

    pub fn first_visible_scrollable_id(&self, direction: ScrollDirection) -> Option<ElementId> {
        self.first_visible_scrollable(direction)
            .and_then(|idx| self.node_id(idx).cloned())
    }

    pub fn pointer_candidates(&self, trigger: TriggerId) -> &[NodeIdx] {
        &self.pointer_index.candidates_by_trigger[trigger.index()]
    }

    pub fn debug_bucket_sizes(&self, trigger: TriggerId) -> (usize, usize) {
        let bucket = &self.buckets[trigger.index()];
        let targeted = bucket.targeted.values().map(Vec::len).sum();
        let ordered = bucket.ordered.len();
        (targeted, ordered)
    }

    pub fn resolve_winner_for_job(&self, job: &DispatchJob) -> Option<DispatchRuleId> {
        match job {
            DispatchJob::Targeted {
                trigger,
                target,
                ctx,
            } => {
                let bucket = &self.buckets[trigger.index()];

                if let Some(targeted) = bucket.targeted.get(target)
                    && let Some(winner) = self.resolve_in_candidates(targeted, Some(*target), ctx)
                {
                    return Some(winner);
                }

                self.resolve_in_candidates(&bucket.ordered, Some(*target), ctx)
            }
            DispatchJob::Untargeted { trigger, ctx }
            | DispatchJob::Pointed { trigger, ctx, .. } => {
                let bucket = &self.buckets[trigger.index()];
                self.resolve_in_candidates(&bucket.ordered, None, ctx)
            }
        }
    }

    pub fn resolve_actions_for_job(&self, job: &DispatchJob) -> Option<&[DispatchRuleAction]> {
        let winner = self.resolve_winner_for_job(job)?;
        self.dispatch_rules
            .get(winner as usize)
            .map(|rule| rule.actions.as_slice())
    }

    fn resolve_in_candidates(
        &self,
        candidate_rule_ids: &[DispatchRuleId],
        target: Option<NodeIdx>,
        ctx: &DispatchCtx,
    ) -> Option<DispatchRuleId> {
        candidate_rule_ids.iter().copied().find(|rule_id| {
            self.dispatch_rules
                .get(*rule_id as usize)
                .is_some_and(|rule| self.rule_matches(rule, target, ctx))
        })
    }

    fn rule_matches(
        &self,
        rule: &DispatchRule,
        target: Option<NodeIdx>,
        ctx: &DispatchCtx,
    ) -> bool {
        if ctx.mods & rule.predicate.required_mods != rule.predicate.required_mods {
            return false;
        }
        if ctx.mods & rule.predicate.forbidden_mods != 0 {
            return false;
        }
        if ctx.runtime_flags & rule.predicate.runtime_flags_all != rule.predicate.runtime_flags_all
        {
            return false;
        }
        if ctx.runtime_flags & rule.predicate.runtime_flags_none != 0 {
            return false;
        }

        match rule.scope {
            RuleScope::Target(expected) => target == Some(expected),
            RuleScope::Focused(expected) => ctx.focused == Some(expected),
            RuleScope::PointerHit => target.is_some(),
            RuleScope::NoFocus => ctx.focused.is_none(),
            RuleScope::Any => true,
        }
    }
}

const POINTER_TRIGGERS: [TriggerId; 10] = [
    TriggerId::CursorButtonLeftPress,
    TriggerId::CursorButtonLeftRelease,
    TriggerId::CursorButtonMiddlePress,
    TriggerId::CursorMove,
    TriggerId::CursorEnter,
    TriggerId::CursorLeave,
    TriggerId::CursorScrollXNeg,
    TriggerId::CursorScrollXPos,
    TriggerId::CursorScrollYNeg,
    TriggerId::CursorScrollYPos,
];

fn pointer_trigger_matches(flags: u16, trigger: TriggerId) -> bool {
    let mask = match trigger {
        TriggerId::CursorButtonLeftPress => {
            EVENT_CLICK
                | EVENT_PRESS
                | EVENT_MOUSE_DOWN
                | EVENT_FOCUSABLE
                | EVENT_TEXT_INPUT
                | EVENT_MOUSE_OVER_STYLE
                | EVENT_MOUSE_DOWN_STYLE
        }
        TriggerId::CursorButtonLeftRelease => {
            EVENT_CLICK
                | EVENT_PRESS
                | EVENT_MOUSE_UP
                | EVENT_MOUSE_OVER_STYLE
                | EVENT_MOUSE_DOWN_STYLE
        }
        TriggerId::CursorButtonMiddlePress => EVENT_TEXT_INPUT | EVENT_FOCUSABLE,
        TriggerId::CursorMove => {
            EVENT_MOUSE_ENTER | EVENT_MOUSE_LEAVE | EVENT_MOUSE_MOVE | EVENT_MOUSE_OVER_STYLE
        }
        TriggerId::CursorEnter | TriggerId::CursorLeave => {
            EVENT_MOUSE_ENTER | EVENT_MOUSE_LEAVE | EVENT_MOUSE_OVER_STYLE | EVENT_MOUSE_DOWN_STYLE
        }
        TriggerId::CursorScrollXNeg => EVENT_SCROLL_X_NEG,
        TriggerId::CursorScrollXPos => EVENT_SCROLL_X_POS,
        TriggerId::CursorScrollYNeg => EVENT_SCROLL_Y_NEG,
        TriggerId::CursorScrollYPos => EVENT_SCROLL_Y_POS,
        _ => return false,
    };

    flags & mask != 0
}
