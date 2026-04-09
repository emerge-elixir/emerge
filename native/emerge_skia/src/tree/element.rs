//! Element types for Emerge UI trees.

use super::animation::AnimationSpec;
#[cfg(test)]
use super::attrs::MouseOverAttrs;
use super::attrs::{Attrs, ScrollbarHoverAxis, supports_mouse_over_tracking};
use std::collections::HashMap;

/// Unique identifier for an element, derived from Erlang term.
/// Stored as the raw bytes of the serialized Erlang term for exact matching.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct ElementId(pub Vec<u8>);

impl ElementId {
    pub fn from_term_bytes(bytes: Vec<u8>) -> Self {
        Self(bytes)
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum TextInputContentOrigin {
    Event,
    #[default]
    TreePatch,
}

/// The type/kind of an element.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ElementKind {
    Row,
    WrappedRow,
    Column,
    TextColumn,
    El,
    Text,
    TextInput,
    Multiline,
    Image,
    Video,
    None,
    Paragraph,
}

impl ElementKind {
    /// Decode from the type tag byte used in serialization.
    pub fn from_tag(tag: u8) -> Option<Self> {
        match tag {
            1 => Some(Self::Row),
            2 => Some(Self::WrappedRow),
            3 => Some(Self::Column),
            4 => Some(Self::El),
            5 => Some(Self::Text),
            6 => Some(Self::None),
            7 => Some(Self::Paragraph),
            8 => Some(Self::TextColumn),
            9 => Some(Self::Image),
            10 => Some(Self::TextInput),
            11 => Some(Self::Video),
            12 => Some(Self::Multiline),
            _ => None,
        }
    }

    pub fn is_text_input_family(self) -> bool {
        matches!(self, Self::TextInput | Self::Multiline)
    }
}

/// Frame representing the computed layout bounds.
#[derive(Clone, Copy, Debug, Default)]
pub struct Frame {
    /// X position relative to parent.
    pub x: f32,
    /// Y position relative to parent.
    pub y: f32,
    /// Visible width (may be smaller than content for scrollable areas).
    pub width: f32,
    /// Visible height (may be smaller than content for scrollable areas).
    pub height: f32,
    /// Actual content width (for scroll extent calculation).
    pub content_width: f32,
    /// Actual content height (for scroll extent calculation).
    pub content_height: f32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum NearbySlot {
    BehindContent,
    Above,
    OnRight,
    Below,
    OnLeft,
    InFront,
}

impl NearbySlot {
    pub fn from_tag(tag: u8) -> Option<Self> {
        match tag {
            1 => Some(Self::BehindContent),
            2 => Some(Self::Above),
            3 => Some(Self::OnRight),
            4 => Some(Self::Below),
            5 => Some(Self::OnLeft),
            6 => Some(Self::InFront),
            _ => None,
        }
    }

    pub fn tag(self) -> u8 {
        match self {
            Self::BehindContent => 1,
            Self::Above => 2,
            Self::OnRight => 3,
            Self::Below => 4,
            Self::OnLeft => 5,
            Self::InFront => 6,
        }
    }

    pub fn spec(self) -> NearbySlotSpec {
        match self {
            Self::BehindContent => NearbySlotSpec {
                phase: RetainedPaintPhase::BehindContent,
                constraint_kind: NearbyConstraintKind::Box,
                align_x_active: true,
                align_y_active: true,
            },
            Self::Above => NearbySlotSpec {
                phase: RetainedPaintPhase::Overlay(self),
                constraint_kind: NearbyConstraintKind::WidthBand,
                align_x_active: true,
                align_y_active: false,
            },
            Self::OnRight => NearbySlotSpec {
                phase: RetainedPaintPhase::Overlay(self),
                constraint_kind: NearbyConstraintKind::HeightBand,
                align_x_active: false,
                align_y_active: true,
            },
            Self::Below => NearbySlotSpec {
                phase: RetainedPaintPhase::Overlay(self),
                constraint_kind: NearbyConstraintKind::WidthBand,
                align_x_active: true,
                align_y_active: false,
            },
            Self::OnLeft => NearbySlotSpec {
                phase: RetainedPaintPhase::Overlay(self),
                constraint_kind: NearbyConstraintKind::HeightBand,
                align_x_active: false,
                align_y_active: true,
            },
            Self::InFront => NearbySlotSpec {
                phase: RetainedPaintPhase::Overlay(self),
                constraint_kind: NearbyConstraintKind::Box,
                align_x_active: true,
                align_y_active: true,
            },
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NearbyMount {
    pub slot: NearbySlot,
    pub id: ElementId,
}

#[derive(Clone, Debug, Default)]
pub struct NearbyMounts {
    pub mounts: Vec<NearbyMount>,
}

impl NearbyMounts {
    pub fn iter(&self) -> impl Iterator<Item = &NearbyMount> {
        self.mounts.iter()
    }

    #[cfg(test)]
    pub fn ids(&self, slot: NearbySlot) -> impl DoubleEndedIterator<Item = &ElementId> {
        self.mounts.iter().filter_map(move |mount| {
            if mount.slot == slot {
                Some(&mount.id)
            } else {
                None
            }
        })
    }

    pub fn push(&mut self, slot: NearbySlot, id: ElementId) {
        self.mounts.push(NearbyMount { slot, id });
    }

    pub fn insert(&mut self, index: usize, slot: NearbySlot, id: ElementId) {
        self.mounts
            .insert(index.min(self.mounts.len()), NearbyMount { slot, id });
    }

    #[cfg(test)]
    pub fn set(&mut self, slot: NearbySlot, id: Option<ElementId>) {
        self.mounts.retain(|mount| mount.slot != slot);
        if let Some(id) = id {
            self.mounts.push(NearbyMount { slot, id });
        }
    }

    pub fn set_mounts(&mut self, mounts: Vec<NearbyMount>) {
        self.mounts = mounts;
    }

    pub fn remove(&mut self, id: &ElementId) -> bool {
        let len_before = self.mounts.len();
        self.mounts.retain(|mount| &mount.id != id);
        len_before != self.mounts.len()
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub enum NodeResidency {
    #[default]
    Live,
    Ghost,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum GhostAttachment {
    Child {
        parent_id: ElementId,
        live_index: usize,
        seq: u64,
    },
    Nearby {
        host_id: ElementId,
        mount_index: usize,
        slot: NearbySlot,
        seq: u64,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RetainedPaintPhase {
    BehindContent,
    Children,
    Overlay(NearbySlot),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NearbyConstraintKind {
    Box,
    WidthBand,
    HeightBand,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NearbySlotSpec {
    pub phase: RetainedPaintPhase,
    pub constraint_kind: NearbyConstraintKind,
    pub align_x_active: bool,
    pub align_y_active: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RetainedChildMode {
    Scope,
    InlineEventOnly,
}

#[derive(Clone, Copy, Debug)]
pub struct RetainedChildRef<'a> {
    pub id: &'a ElementId,
    pub mode: RetainedChildMode,
}

#[derive(Clone, Copy, Debug)]
pub enum RetainedLocalBranchRef<'a> {
    Nearby(&'a NearbyMount),
    Child(RetainedChildRef<'a>),
}

/// A single element in the UI tree.
#[derive(Clone, Debug)]
pub struct Element {
    /// Unique identifier for this element.
    pub id: ElementId,

    /// The type of element (row, column, el, text, etc).
    pub kind: ElementKind,

    /// Raw attributes as binary (EMRG format).
    pub attrs_raw: Vec<u8>,

    /// Original unscaled attributes (as received from Elixir).
    pub base_attrs: Attrs,

    /// Scaled attributes (populated by layout pass, used by render).
    pub attrs: Attrs,

    /// Runtime-only origin label for current text-input content.
    pub text_input_content_origin: TextInputContentOrigin,

    /// Child element IDs (order matters).
    pub children: Vec<ElementId>,

    /// Computed post-layout child paint order.
    pub paint_children: Vec<ElementId>,

    /// Host-owned nearby mount roots.
    pub nearby: NearbyMounts,

    /// Computed layout frame (populated after layout pass).
    pub frame: Option<Frame>,

    /// Intrinsic frame captured during measurement pass before resolution mutates `frame`.
    pub measured_frame: Option<Frame>,

    /// Tree revision when this element was last mounted into the tree.
    pub mounted_at_revision: u64,

    /// Runtime residency marker.
    pub residency: NodeResidency,

    /// Runtime-only attachment metadata for ghost roots.
    pub ghost_attachment: Option<GhostAttachment>,

    /// Runtime-only scale active when this ghost snapshot was captured.
    pub ghost_capture_scale: Option<f32>,

    /// Runtime-only exit animation captured for this ghost root.
    pub ghost_exit_animation: Option<AnimationSpec>,
}

impl Element {
    /// Create an element with decoded attributes.
    /// The attrs are stored as base_attrs (original) and cloned to attrs (for scaling).
    pub fn with_attrs(id: ElementId, kind: ElementKind, attrs_raw: Vec<u8>, attrs: Attrs) -> Self {
        Self {
            id,
            kind,
            attrs_raw,
            base_attrs: attrs.clone(),
            attrs,
            text_input_content_origin: TextInputContentOrigin::TreePatch,
            children: Vec::new(),
            paint_children: Vec::new(),
            nearby: NearbyMounts::default(),
            frame: None,
            measured_frame: None,
            mounted_at_revision: 0,
            residency: NodeResidency::Live,
            ghost_attachment: None,
            ghost_capture_scale: None,
            ghost_exit_animation: None,
        }
    }

    pub fn is_live(&self) -> bool {
        matches!(self.residency, NodeResidency::Live)
    }

    pub fn is_ghost(&self) -> bool {
        matches!(self.residency, NodeResidency::Ghost)
    }

    pub fn is_ghost_root(&self) -> bool {
        self.is_ghost() && self.ghost_attachment.is_some()
    }

    pub fn paint_child_ids(&self) -> &[ElementId] {
        if self.paint_children.is_empty() {
            &self.children
        } else {
            &self.paint_children
        }
    }

    pub fn local_nearby_mounts(&self) -> impl Iterator<Item = &NearbyMount> {
        self.nearby
            .iter()
            .filter(|mount| mount.slot == NearbySlot::BehindContent)
    }

    pub fn escape_nearby_mounts(&self) -> impl Iterator<Item = &NearbyMount> {
        self.nearby
            .iter()
            .filter(|mount| mount.slot != NearbySlot::BehindContent)
    }

    pub fn for_each_retained_child(
        &self,
        tree: &ElementTree,
        mut f: impl FnMut(RetainedChildRef<'_>),
    ) {
        if self.kind == ElementKind::Paragraph {
            for id in &self.children {
                if paragraph_child_mode(tree, id) == RetainedChildMode::Scope {
                    f(RetainedChildRef {
                        id,
                        mode: RetainedChildMode::Scope,
                    });
                }
            }

            for id in &self.children {
                if paragraph_child_mode(tree, id) == RetainedChildMode::InlineEventOnly {
                    f(RetainedChildRef {
                        id,
                        mode: RetainedChildMode::InlineEventOnly,
                    });
                }
            }
        } else {
            for id in self.paint_child_ids() {
                f(RetainedChildRef {
                    id,
                    mode: RetainedChildMode::Scope,
                });
            }
        }
    }

    pub fn for_each_retained_local_branch(
        &self,
        tree: &ElementTree,
        mut f: impl FnMut(RetainedLocalBranchRef<'_>),
    ) {
        for mount in self.local_nearby_mounts() {
            f(RetainedLocalBranchRef::Nearby(mount));
        }

        self.for_each_retained_child(tree, |child| f(RetainedLocalBranchRef::Child(child)));
    }
}

fn paragraph_child_mode(tree: &ElementTree, child_id: &ElementId) -> RetainedChildMode {
    let is_float_child = tree.get(child_id).is_some_and(|child| {
        matches!(
            child.attrs.align_x,
            Some(super::attrs::AlignX::Left | super::attrs::AlignX::Right)
        )
    });

    if is_float_child {
        RetainedChildMode::Scope
    } else {
        RetainedChildMode::InlineEventOnly
    }
}

/// The complete element tree with indexed access.
#[derive(Clone, Debug)]
pub struct ElementTree {
    /// Monotonic revision for tree mutations.
    pub revision: u64,

    /// Monotonic sequence used to order ghost attachments.
    pub next_ghost_seq: u64,

    /// Last layout scale applied to the tree.
    pub current_scale: f32,

    /// Root element ID (if tree is non-empty).
    pub root: Option<ElementId>,

    /// All elements indexed by ID for O(1) lookup.
    pub nodes: HashMap<ElementId, Element>,
}

impl Default for ElementTree {
    fn default() -> Self {
        Self {
            revision: 0,
            next_ghost_seq: 0,
            current_scale: 1.0,
            root: None,
            nodes: HashMap::new(),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ScrollAxis {
    X,
    Y,
}

impl ElementTree {
    pub fn new() -> Self {
        Self::default()
    }

    /// Get an element by ID.
    pub fn get(&self, id: &ElementId) -> Option<&Element> {
        self.nodes.get(id)
    }

    /// Get a mutable element by ID.
    pub fn get_mut(&mut self, id: &ElementId) -> Option<&mut Element> {
        self.nodes.get_mut(id)
    }

    /// Insert or update an element.
    pub fn insert(&mut self, element: Element) {
        self.nodes.insert(element.id.clone(), element);
    }

    /// Current tree revision.
    pub fn revision(&self) -> u64 {
        self.revision
    }

    /// Advance the tree revision and return the new value.
    pub fn bump_revision(&mut self) -> u64 {
        self.revision = self.revision.saturating_add(1);
        self.revision
    }

    /// Override the tree revision.
    pub fn set_revision(&mut self, revision: u64) {
        self.revision = revision;
    }

    pub fn current_scale(&self) -> f32 {
        self.current_scale
    }

    pub fn set_current_scale(&mut self, scale: f32) {
        self.current_scale = scale.max(f32::EPSILON);
    }

    pub fn next_ghost_seq(&mut self) -> u64 {
        let seq = self.next_ghost_seq;
        self.next_ghost_seq = self.next_ghost_seq.saturating_add(1);
        seq
    }

    pub fn mint_ghost_id(&mut self) -> ElementId {
        let seq = self.next_ghost_seq();
        ElementId(format!("__emerge_ghost:{seq}").into_bytes())
    }

    /// Stamp every live node as mounted at the provided revision.
    pub fn stamp_all_mounted_at_revision(&mut self, revision: u64) {
        self.nodes
            .values_mut()
            .for_each(|element| element.mounted_at_revision = revision);
    }

    /// Replace this tree with a fully uploaded tree, advancing revision once.
    pub fn replace_with_uploaded(&mut self, mut uploaded: ElementTree) {
        let revision = self.revision.saturating_add(1);
        uploaded.set_revision(revision);
        uploaded.next_ghost_seq = self.next_ghost_seq;
        uploaded.current_scale = self.current_scale;
        uploaded.stamp_all_mounted_at_revision(revision);
        *self = uploaded;
    }

    /// Returns true when the element was mounted after the provided revision.
    pub fn was_mounted_after(&self, id: &ElementId, revision: u64) -> bool {
        self.get(id)
            .is_some_and(|element| element.mounted_at_revision > revision)
    }

    /// Check if tree is empty.
    pub fn is_empty(&self) -> bool {
        self.root.is_none()
    }

    /// Get the number of nodes.
    pub fn len(&self) -> usize {
        self.nodes.len()
    }

    /// Clear the tree.
    pub fn clear(&mut self) {
        self.bump_revision();
        self.root = None;
        self.nodes.clear();
    }

    pub fn live_child_ids(&self, parent_id: &ElementId) -> Vec<ElementId> {
        self.get(parent_id)
            .map(|element| {
                element
                    .children
                    .iter()
                    .filter(|child_id| self.get(child_id).is_some_and(Element::is_live))
                    .cloned()
                    .collect()
            })
            .unwrap_or_default()
    }

    pub fn merge_live_children_with_ghosts(
        &self,
        parent_id: &ElementId,
        new_live_ids: Vec<ElementId>,
    ) -> Vec<ElementId> {
        let mut ghosts: Vec<(ElementId, usize, u64)> = self
            .get(parent_id)
            .map(|parent| {
                parent
                    .children
                    .iter()
                    .filter_map(|child_id| {
                        let child = self.get(child_id)?;
                        match child.ghost_attachment.as_ref() {
                            Some(GhostAttachment::Child {
                                parent_id: ghost_parent_id,
                                live_index,
                                seq,
                            }) if ghost_parent_id == parent_id => {
                                Some((child_id.clone(), *live_index, *seq))
                            }
                            _ => None,
                        }
                    })
                    .collect()
            })
            .unwrap_or_default();

        ghosts.sort_by_key(|(_, live_index, seq)| (*live_index, *seq));

        let mut merged = Vec::with_capacity(new_live_ids.len() + ghosts.len());
        let mut ghost_index = 0;

        for live_index in 0..=new_live_ids.len() {
            while ghost_index < ghosts.len() {
                let (ghost_id, anchor, _) = &ghosts[ghost_index];
                if (*anchor).min(new_live_ids.len()) != live_index {
                    break;
                }

                merged.push(ghost_id.clone());
                ghost_index += 1;
            }

            if let Some(live_id) = new_live_ids.get(live_index) {
                merged.push(live_id.clone());
            }
        }

        merged
    }

    pub fn live_nearby_mounts(&self, host_id: &ElementId) -> Vec<NearbyMount> {
        self.get(host_id)
            .map(|host| {
                host.nearby
                    .iter()
                    .filter(|mount| self.get(&mount.id).is_some_and(Element::is_live))
                    .cloned()
                    .collect()
            })
            .unwrap_or_default()
    }

    pub fn merge_live_nearby_with_ghosts(
        &self,
        host_id: &ElementId,
        new_live_mounts: Vec<NearbyMount>,
    ) -> Vec<NearbyMount> {
        let mut ghosts: Vec<(NearbyMount, usize, u64)> = self
            .get(host_id)
            .map(|host| {
                host.nearby
                    .iter()
                    .filter_map(|mount| {
                        let nearby = self.get(&mount.id)?;
                        match nearby.ghost_attachment.as_ref() {
                            Some(GhostAttachment::Nearby {
                                host_id: ghost_host_id,
                                mount_index,
                                seq,
                                ..
                            }) if ghost_host_id == host_id => {
                                Some((mount.clone(), *mount_index, *seq))
                            }
                            _ => None,
                        }
                    })
                    .collect()
            })
            .unwrap_or_default();

        ghosts.sort_by_key(|(_, mount_index, seq)| (*mount_index, *seq));

        let mut merged = Vec::with_capacity(new_live_mounts.len() + ghosts.len());
        let mut ghost_index = 0;

        for live_index in 0..=new_live_mounts.len() {
            while ghost_index < ghosts.len() {
                let (ghost_mount, anchor, _) = &ghosts[ghost_index];
                if (*anchor).min(new_live_mounts.len()) != live_index {
                    break;
                }

                merged.push(ghost_mount.clone());
                ghost_index += 1;
            }

            if let Some(live_mount) = new_live_mounts.get(live_index) {
                merged.push(live_mount.clone());
            }
        }

        merged
    }

    /// Apply scroll delta to an element. Returns true if scroll changed.
    pub fn apply_scroll(&mut self, id: &ElementId, dx: f32, dy: f32) -> bool {
        let mut changed = false;
        if dx != 0.0 {
            changed |= self.apply_scroll_x(id, dx);
        }
        if dy != 0.0 {
            changed |= self.apply_scroll_y(id, dy);
        }
        changed
    }

    /// Apply horizontal scroll delta to an element. Returns true if scroll changed.
    pub fn apply_scroll_x(&mut self, id: &ElementId, dx: f32) -> bool {
        self.apply_scroll_axis(id, dx, ScrollAxis::X)
    }

    /// Apply vertical scroll delta to an element. Returns true if scroll changed.
    pub fn apply_scroll_y(&mut self, id: &ElementId, dy: f32) -> bool {
        self.apply_scroll_axis(id, dy, ScrollAxis::Y)
    }

    /// Set horizontal scrollbar thumb hover state. Returns true when state changes.
    pub fn set_scrollbar_x_hover(&mut self, id: &ElementId, hovered: bool) -> bool {
        self.set_scrollbar_hover_axis(id, ScrollbarHoverAxis::X, hovered)
    }

    /// Set vertical scrollbar thumb hover state. Returns true when state changes.
    pub fn set_scrollbar_y_hover(&mut self, id: &ElementId, hovered: bool) -> bool {
        self.set_scrollbar_hover_axis(id, ScrollbarHoverAxis::Y, hovered)
    }

    /// Set mouse_over active state. Returns true when state changes.
    pub fn set_mouse_over_active(&mut self, id: &ElementId, active: bool) -> bool {
        let Some(element) = self.get_mut(id) else {
            return false;
        };

        if !supports_mouse_over_tracking(&element.attrs) {
            if element.attrs.mouse_over_active.take().is_some() {
                return true;
            }
            return false;
        }

        let current = element.attrs.mouse_over_active.unwrap_or(false);
        if current == active {
            return false;
        }

        element.attrs.mouse_over_active = Some(active);
        true
    }

    /// Set mouse_down active state. Returns true when state changes.
    pub fn set_mouse_down_active(&mut self, id: &ElementId, active: bool) -> bool {
        let Some(element) = self.get_mut(id) else {
            return false;
        };

        if element.attrs.mouse_down.is_none() {
            if element.attrs.mouse_down_active.take().is_some() {
                return true;
            }
            return false;
        }

        let current = element.attrs.mouse_down_active.unwrap_or(false);
        if current == active {
            return false;
        }

        element.attrs.mouse_down_active = Some(active);
        true
    }

    /// Set focused active state. Returns true when state changes.
    pub fn set_focused_active(&mut self, id: &ElementId, active: bool) -> bool {
        let Some(element) = self.get_mut(id) else {
            return false;
        };

        let current = element.attrs.focused_active.unwrap_or(false);
        if current == active {
            return false;
        }

        element.attrs.focused_active = Some(active);
        true
    }

    pub fn set_text_input_content(&mut self, id: &ElementId, content: String) -> bool {
        let Some(element) = self.get_mut(id) else {
            return false;
        };

        if !element.kind.is_text_input_family() {
            return false;
        }

        let prev_base = element.base_attrs.content.as_deref().unwrap_or("");
        let prev_attrs = element.attrs.content.as_deref().unwrap_or("");
        let mut changed = prev_base != content || prev_attrs != content;

        element.base_attrs.content = Some(content.clone());
        element.attrs.content = Some(content.clone());

        if element.text_input_content_origin != TextInputContentOrigin::Event {
            element.text_input_content_origin = TextInputContentOrigin::Event;
            changed = true;
        }

        let len = text_char_len(&content);
        if let Some(cursor) = element.attrs.text_input_cursor {
            let clamped = cursor.min(len);
            if clamped != cursor {
                element.attrs.text_input_cursor = Some(clamped);
                changed = true;
            }
        }

        if let Some(anchor) = element.attrs.text_input_selection_anchor {
            let clamped = anchor.min(len);
            let cursor = element.attrs.text_input_cursor.unwrap_or(len);
            let next = if clamped == cursor {
                None
            } else {
                Some(clamped)
            };
            if next != element.attrs.text_input_selection_anchor {
                element.attrs.text_input_selection_anchor = next;
                changed = true;
            }
        }

        let had_preedit = element.attrs.text_input_preedit.take().is_some();
        let had_preedit_cursor = element.attrs.text_input_preedit_cursor.take().is_some();
        if had_preedit || had_preedit_cursor {
            changed = true;
        }

        changed
    }

    pub fn set_text_input_runtime(
        &mut self,
        id: &ElementId,
        focused: bool,
        cursor: Option<u32>,
        selection_anchor: Option<u32>,
        preedit: Option<String>,
        preedit_cursor: Option<(u32, u32)>,
    ) -> bool {
        let Some(element) = self.get_mut(id) else {
            return false;
        };

        if !element.kind.is_text_input_family() {
            return false;
        }

        let content = element.base_attrs.content.as_deref().unwrap_or("");
        let len = text_char_len(content);

        let mut next_cursor = cursor.or(element.attrs.text_input_cursor);
        if focused {
            next_cursor = Some(next_cursor.unwrap_or(len).min(len));
        } else {
            next_cursor = next_cursor.map(|value| value.min(len));
        }

        let cursor_value = next_cursor.unwrap_or(len);
        let mut next_anchor = selection_anchor.map(|value| value.min(len));
        if !focused {
            next_anchor = None;
        } else if let Some(anchor) = next_anchor
            && anchor == cursor_value
        {
            next_anchor = None;
        }

        let next_preedit = if focused {
            preedit.filter(|value| !value.is_empty())
        } else {
            None
        };
        let next_preedit_cursor = if focused {
            normalize_preedit_cursor(next_preedit.as_deref(), preedit_cursor)
        } else {
            None
        };

        let mut changed = false;

        if element.attrs.text_input_focused != Some(focused) {
            element.attrs.text_input_focused = Some(focused);
            changed = true;
        }

        if element.attrs.text_input_cursor != next_cursor {
            element.attrs.text_input_cursor = next_cursor;
            changed = true;
        }

        if element.attrs.text_input_selection_anchor != next_anchor {
            element.attrs.text_input_selection_anchor = next_anchor;
            changed = true;
        }

        if element.attrs.text_input_preedit != next_preedit {
            element.attrs.text_input_preedit = next_preedit;
            changed = true;
        }

        if element.attrs.text_input_preedit_cursor != next_preedit_cursor {
            element.attrs.text_input_preedit_cursor = next_preedit_cursor;
            changed = true;
        }

        changed
    }

    fn apply_scroll_axis(&mut self, id: &ElementId, delta: f32, axis: ScrollAxis) -> bool {
        let Some(element) = self.get_mut(id) else {
            return false;
        };
        let Some(frame) = element.frame else {
            return false;
        };

        let (current, max) = match axis {
            ScrollAxis::X => (
                element.attrs.scroll_x.unwrap_or(0.0) as f32,
                (frame.content_width - frame.width).max(0.0),
            ),
            ScrollAxis::Y => (
                element.attrs.scroll_y.unwrap_or(0.0) as f32,
                (frame.content_height - frame.height).max(0.0),
            ),
        };
        let next = (current - delta).clamp(0.0, max);

        if (next - current).abs() < f32::EPSILON {
            return false;
        }

        match axis {
            ScrollAxis::X => element.attrs.scroll_x = Some(next as f64),
            ScrollAxis::Y => element.attrs.scroll_y = Some(next as f64),
        }
        true
    }

    fn set_scrollbar_hover_axis(
        &mut self,
        id: &ElementId,
        axis: ScrollbarHoverAxis,
        hovered: bool,
    ) -> bool {
        let Some(element) = self.get_mut(id) else {
            return false;
        };

        let current = element.attrs.scrollbar_hover_axis;
        let axis_enabled = match axis {
            ScrollbarHoverAxis::X => element.attrs.scrollbar_x.unwrap_or(false),
            ScrollbarHoverAxis::Y => element.attrs.scrollbar_y.unwrap_or(false),
        };

        if hovered {
            if !axis_enabled || current == Some(axis) {
                return false;
            }
            element.attrs.scrollbar_hover_axis = Some(axis);
            return true;
        }

        if current == Some(axis) {
            element.attrs.scrollbar_hover_axis = None;
            return true;
        }

        false
    }
}

fn text_char_len(content: &str) -> u32 {
    content.chars().count() as u32
}

fn normalize_preedit_cursor(text: Option<&str>, cursor: Option<(u32, u32)>) -> Option<(u32, u32)> {
    let text_len = text.map(text_char_len)?;
    let (mut start, mut end) = cursor?;
    start = start.min(text_len);
    end = end.min(text_len);
    if start > end {
        std::mem::swap(&mut start, &mut end);
    }
    Some((start, end))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tree::attrs::AlignX;

    #[test]
    fn test_element_kind_from_tag() {
        assert_eq!(ElementKind::from_tag(1), Some(ElementKind::Row));
        assert_eq!(ElementKind::from_tag(2), Some(ElementKind::WrappedRow));
        assert_eq!(ElementKind::from_tag(3), Some(ElementKind::Column));
        assert_eq!(ElementKind::from_tag(4), Some(ElementKind::El));
        assert_eq!(ElementKind::from_tag(5), Some(ElementKind::Text));
        assert_eq!(ElementKind::from_tag(6), Some(ElementKind::None));
        assert_eq!(ElementKind::from_tag(7), Some(ElementKind::Paragraph));
        assert_eq!(ElementKind::from_tag(8), Some(ElementKind::TextColumn));
        assert_eq!(ElementKind::from_tag(9), Some(ElementKind::Image));
        assert_eq!(ElementKind::from_tag(10), Some(ElementKind::TextInput));
        assert_eq!(ElementKind::from_tag(11), Some(ElementKind::Video));
        assert_eq!(ElementKind::from_tag(12), Some(ElementKind::Multiline));
    }

    #[test]
    fn test_scrollbar_hover_axis_is_tri_state() {
        let id = ElementId::from_term_bytes(vec![1]);
        let mut attrs = Attrs::default();
        attrs.scrollbar_x = Some(true);
        attrs.scrollbar_y = Some(true);
        let mut element = Element::with_attrs(id.clone(), ElementKind::El, Vec::new(), attrs);
        element.frame = Some(Frame {
            x: 0.0,
            y: 0.0,
            width: 100.0,
            height: 100.0,
            content_width: 200.0,
            content_height: 200.0,
        });

        let mut tree = ElementTree::new();
        tree.root = Some(id.clone());
        tree.insert(element);

        assert!(tree.set_scrollbar_x_hover(&id, true));
        assert_eq!(
            tree.get(&id).unwrap().attrs.scrollbar_hover_axis,
            Some(ScrollbarHoverAxis::X)
        );

        assert!(tree.set_scrollbar_y_hover(&id, true));
        assert_eq!(
            tree.get(&id).unwrap().attrs.scrollbar_hover_axis,
            Some(ScrollbarHoverAxis::Y)
        );

        assert!(!tree.set_scrollbar_x_hover(&id, false));
        assert!(tree.set_scrollbar_y_hover(&id, false));
        assert_eq!(tree.get(&id).unwrap().attrs.scrollbar_hover_axis, None);
    }

    #[test]
    fn test_apply_scroll_axis_helpers() {
        let id = ElementId::from_term_bytes(vec![1]);
        let mut attrs = Attrs::default();
        attrs.scrollbar_x = Some(true);
        attrs.scrollbar_y = Some(true);
        let mut element = Element::with_attrs(id.clone(), ElementKind::El, Vec::new(), attrs);
        element.frame = Some(Frame {
            x: 0.0,
            y: 0.0,
            width: 100.0,
            height: 100.0,
            content_width: 200.0,
            content_height: 200.0,
        });

        let mut tree = ElementTree::new();
        tree.root = Some(id.clone());
        tree.insert(element);

        assert!(tree.apply_scroll_x(&id, -30.0));
        assert_eq!(tree.get(&id).unwrap().attrs.scroll_x, Some(30.0));

        assert!(tree.apply_scroll_y(&id, -25.0));
        assert_eq!(tree.get(&id).unwrap().attrs.scroll_y, Some(25.0));

        assert!(!tree.apply_scroll_x(&id, 0.0));
        assert!(!tree.apply_scroll_y(&id, 0.0));
    }

    #[test]
    fn test_apply_scroll_axis_helpers_clamp_to_bounds() {
        let id = ElementId::from_term_bytes(vec![1]);
        let mut attrs = Attrs::default();
        attrs.scrollbar_x = Some(true);
        attrs.scrollbar_y = Some(true);
        let mut element = Element::with_attrs(id.clone(), ElementKind::El, Vec::new(), attrs);
        element.frame = Some(Frame {
            x: 0.0,
            y: 0.0,
            width: 100.0,
            height: 100.0,
            content_width: 180.0,
            content_height: 170.0,
        });

        let mut tree = ElementTree::new();
        tree.root = Some(id.clone());
        tree.insert(element);

        assert!(tree.apply_scroll_x(&id, -500.0));
        assert!(tree.apply_scroll_y(&id, -500.0));
        assert_eq!(tree.get(&id).unwrap().attrs.scroll_x, Some(80.0));
        assert_eq!(tree.get(&id).unwrap().attrs.scroll_y, Some(70.0));

        assert!(tree.apply_scroll_x(&id, 500.0));
        assert!(tree.apply_scroll_y(&id, 500.0));
        assert_eq!(tree.get(&id).unwrap().attrs.scroll_x, Some(0.0));
        assert_eq!(tree.get(&id).unwrap().attrs.scroll_y, Some(0.0));
    }

    #[test]
    fn test_set_scrollbar_hover_axis_noop_when_axis_disabled() {
        let id = ElementId::from_term_bytes(vec![1]);
        let attrs = Attrs::default();
        let mut element = Element::with_attrs(id.clone(), ElementKind::El, Vec::new(), attrs);
        element.frame = Some(Frame {
            x: 0.0,
            y: 0.0,
            width: 100.0,
            height: 100.0,
            content_width: 100.0,
            content_height: 100.0,
        });

        let mut tree = ElementTree::new();
        tree.root = Some(id.clone());
        tree.insert(element);

        assert!(!tree.set_scrollbar_x_hover(&id, true));
        assert!(!tree.set_scrollbar_y_hover(&id, true));
        assert_eq!(tree.get(&id).unwrap().attrs.scrollbar_hover_axis, None);
    }

    #[test]
    fn test_set_mouse_over_active_requires_mouse_over_attrs() {
        let id = ElementId::from_term_bytes(vec![1]);
        let mut element =
            Element::with_attrs(id.clone(), ElementKind::El, Vec::new(), Attrs::default());
        element.frame = Some(Frame {
            x: 0.0,
            y: 0.0,
            width: 100.0,
            height: 100.0,
            content_width: 100.0,
            content_height: 100.0,
        });

        let mut tree = ElementTree::new();
        tree.root = Some(id.clone());
        tree.insert(element);

        assert!(!tree.set_mouse_over_active(&id, true));
        assert_eq!(tree.get(&id).unwrap().attrs.mouse_over_active, None);
    }

    #[test]
    fn test_set_mouse_over_active_toggles_state() {
        let id = ElementId::from_term_bytes(vec![1]);
        let mut attrs = Attrs::default();
        attrs.mouse_over = Some(MouseOverAttrs {
            alpha: Some(0.6),
            ..Default::default()
        });
        let mut element = Element::with_attrs(id.clone(), ElementKind::El, Vec::new(), attrs);
        element.frame = Some(Frame {
            x: 0.0,
            y: 0.0,
            width: 100.0,
            height: 100.0,
            content_width: 100.0,
            content_height: 100.0,
        });

        let mut tree = ElementTree::new();
        tree.root = Some(id.clone());
        tree.insert(element);

        assert!(tree.set_mouse_over_active(&id, true));
        assert_eq!(tree.get(&id).unwrap().attrs.mouse_over_active, Some(true));

        assert!(!tree.set_mouse_over_active(&id, true));

        assert!(tree.set_mouse_over_active(&id, false));
        assert_eq!(tree.get(&id).unwrap().attrs.mouse_over_active, Some(false));
    }

    #[test]
    fn test_set_mouse_over_active_tracks_event_only_hover() {
        let id = ElementId::from_term_bytes(vec![2]);
        let mut attrs = Attrs::default();
        attrs.on_mouse_enter = Some(true);
        attrs.on_mouse_leave = Some(true);
        let mut element = Element::with_attrs(id.clone(), ElementKind::El, Vec::new(), attrs);
        element.frame = Some(Frame {
            x: 0.0,
            y: 0.0,
            width: 100.0,
            height: 100.0,
            content_width: 100.0,
            content_height: 100.0,
        });

        let mut tree = ElementTree::new();
        tree.root = Some(id.clone());
        tree.insert(element);

        assert!(tree.set_mouse_over_active(&id, true));
        assert_eq!(tree.get(&id).unwrap().attrs.mouse_over_active, Some(true));

        assert!(!tree.set_mouse_over_active(&id, true));

        assert!(tree.set_mouse_over_active(&id, false));
        assert_eq!(tree.get(&id).unwrap().attrs.mouse_over_active, Some(false));
    }

    #[test]
    fn test_set_mouse_down_active_requires_mouse_down_attrs() {
        let id = ElementId::from_term_bytes(vec![11]);
        let mut element =
            Element::with_attrs(id.clone(), ElementKind::El, Vec::new(), Attrs::default());
        element.frame = Some(Frame {
            x: 0.0,
            y: 0.0,
            width: 100.0,
            height: 100.0,
            content_width: 100.0,
            content_height: 100.0,
        });

        let mut tree = ElementTree::new();
        tree.root = Some(id.clone());
        tree.insert(element);

        assert!(!tree.set_mouse_down_active(&id, true));
        assert_eq!(tree.get(&id).unwrap().attrs.mouse_down_active, None);
    }

    #[test]
    fn test_set_mouse_down_active_toggles_state() {
        let id = ElementId::from_term_bytes(vec![12]);
        let mut attrs = Attrs::default();
        attrs.mouse_down = Some(MouseOverAttrs {
            alpha: Some(0.7),
            ..Default::default()
        });
        let mut element = Element::with_attrs(id.clone(), ElementKind::El, Vec::new(), attrs);
        element.frame = Some(Frame {
            x: 0.0,
            y: 0.0,
            width: 100.0,
            height: 100.0,
            content_width: 100.0,
            content_height: 100.0,
        });

        let mut tree = ElementTree::new();
        tree.root = Some(id.clone());
        tree.insert(element);

        assert!(tree.set_mouse_down_active(&id, true));
        assert_eq!(tree.get(&id).unwrap().attrs.mouse_down_active, Some(true));

        assert!(!tree.set_mouse_down_active(&id, true));

        assert!(tree.set_mouse_down_active(&id, false));
        assert_eq!(tree.get(&id).unwrap().attrs.mouse_down_active, Some(false));
    }

    #[test]
    fn test_set_focused_active_toggles_state() {
        let id = ElementId::from_term_bytes(vec![13]);
        let mut element =
            Element::with_attrs(id.clone(), ElementKind::El, Vec::new(), Attrs::default());
        element.frame = Some(Frame {
            x: 0.0,
            y: 0.0,
            width: 100.0,
            height: 40.0,
            content_width: 100.0,
            content_height: 40.0,
        });

        let mut tree = ElementTree::new();
        tree.root = Some(id.clone());
        tree.insert(element);

        assert!(tree.set_focused_active(&id, true));
        assert_eq!(tree.get(&id).unwrap().attrs.focused_active, Some(true));

        assert!(!tree.set_focused_active(&id, true));

        assert!(tree.set_focused_active(&id, false));
        assert_eq!(tree.get(&id).unwrap().attrs.focused_active, Some(false));
    }

    #[test]
    fn test_set_text_input_content_updates_and_clamps_runtime() {
        let id = ElementId::from_term_bytes(vec![2]);
        let mut attrs = Attrs::default();
        attrs.content = Some("hello".to_string());
        attrs.text_input_cursor = Some(10);
        attrs.text_input_selection_anchor = Some(10);
        attrs.text_input_preedit = Some("pre".to_string());
        attrs.text_input_preedit_cursor = Some((2, 2));
        let mut element =
            Element::with_attrs(id.clone(), ElementKind::TextInput, Vec::new(), attrs);
        element.frame = Some(Frame {
            x: 0.0,
            y: 0.0,
            width: 120.0,
            height: 24.0,
            content_width: 120.0,
            content_height: 24.0,
        });

        let mut tree = ElementTree::new();
        tree.root = Some(id.clone());
        tree.insert(element);

        assert!(tree.set_text_input_content(&id, "hey".to_string()));
        let node = tree.get(&id).unwrap();
        assert_eq!(node.base_attrs.content.as_deref(), Some("hey"));
        assert_eq!(node.attrs.content.as_deref(), Some("hey"));
        assert_eq!(
            node.text_input_content_origin,
            TextInputContentOrigin::Event
        );
        assert_eq!(node.attrs.text_input_cursor, Some(3));
        assert_eq!(node.attrs.text_input_selection_anchor, None);
        assert_eq!(node.attrs.text_input_preedit, None);
        assert_eq!(node.attrs.text_input_preedit_cursor, None);

        assert!(!tree.set_text_input_content(&id, "hey".to_string()));
    }

    #[test]
    fn test_set_text_input_content_marks_event_origin_without_content_change() {
        let id = ElementId::from_term_bytes(vec![14]);
        let mut attrs = Attrs::default();
        attrs.content = Some("same".to_string());
        let element = Element::with_attrs(id.clone(), ElementKind::TextInput, Vec::new(), attrs);

        let mut tree = ElementTree::new();
        tree.root = Some(id.clone());
        tree.insert(element);

        assert!(tree.set_text_input_content(&id, "same".to_string()));
        assert_eq!(
            tree.get(&id).unwrap().text_input_content_origin,
            TextInputContentOrigin::Event
        );
        assert!(!tree.set_text_input_content(&id, "same".to_string()));
    }

    #[test]
    fn test_set_text_input_runtime_normalizes_focus_selection_and_preedit() {
        let id = ElementId::from_term_bytes(vec![3]);
        let mut attrs = Attrs::default();
        attrs.content = Some("abcd".to_string());
        let mut element =
            Element::with_attrs(id.clone(), ElementKind::TextInput, Vec::new(), attrs);
        element.frame = Some(Frame {
            x: 0.0,
            y: 0.0,
            width: 120.0,
            height: 24.0,
            content_width: 120.0,
            content_height: 24.0,
        });

        let mut tree = ElementTree::new();
        tree.root = Some(id.clone());
        tree.insert(element);

        assert!(tree.set_text_input_runtime(
            &id,
            true,
            Some(99),
            Some(1),
            Some("ka".to_string()),
            Some((7, 2)),
        ));

        let focused = tree.get(&id).unwrap();
        assert_eq!(focused.attrs.text_input_focused, Some(true));
        assert_eq!(focused.attrs.text_input_cursor, Some(4));
        assert_eq!(focused.attrs.text_input_selection_anchor, Some(1));
        assert_eq!(focused.attrs.text_input_preedit.as_deref(), Some("ka"));
        assert_eq!(focused.attrs.text_input_preedit_cursor, Some((2, 2)));

        assert!(tree.set_text_input_runtime(
            &id,
            false,
            Some(2),
            Some(0),
            Some("ignored".to_string()),
            Some((1, 1)),
        ));

        let blurred = tree.get(&id).unwrap();
        assert_eq!(blurred.attrs.text_input_focused, Some(false));
        assert_eq!(blurred.attrs.text_input_cursor, Some(2));
        assert_eq!(blurred.attrs.text_input_selection_anchor, None);
        assert_eq!(blurred.attrs.text_input_preedit, None);
        assert_eq!(blurred.attrs.text_input_preedit_cursor, None);
    }

    #[test]
    fn test_set_text_input_runtime_ignores_non_text_input_nodes() {
        let id = ElementId::from_term_bytes(vec![4]);
        let mut element =
            Element::with_attrs(id.clone(), ElementKind::El, Vec::new(), Attrs::default());
        element.frame = Some(Frame {
            x: 0.0,
            y: 0.0,
            width: 50.0,
            height: 20.0,
            content_width: 50.0,
            content_height: 20.0,
        });

        let mut tree = ElementTree::new();
        tree.root = Some(id.clone());
        tree.insert(element);

        assert!(!tree.set_text_input_content(&id, "nope".to_string()));
        assert!(!tree.set_text_input_runtime(
            &id,
            true,
            Some(0),
            None,
            Some("x".to_string()),
            None,
        ));
    }

    #[test]
    fn test_for_each_retained_child_paragraph_orders_float_before_inline() {
        let paragraph_id = ElementId::from_term_bytes(vec![10]);
        let inline_id = ElementId::from_term_bytes(vec![11]);
        let float_id = ElementId::from_term_bytes(vec![12]);

        let mut paragraph = Element::with_attrs(
            paragraph_id.clone(),
            ElementKind::Paragraph,
            Vec::new(),
            Attrs::default(),
        );
        paragraph.children = vec![inline_id.clone(), float_id.clone()];

        let inline = Element::with_attrs(
            inline_id.clone(),
            ElementKind::Text,
            Vec::new(),
            Attrs::default(),
        );

        let mut float_attrs = Attrs::default();
        float_attrs.align_x = Some(AlignX::Left);
        let float = Element::with_attrs(float_id.clone(), ElementKind::El, Vec::new(), float_attrs);

        let mut tree = ElementTree::new();
        tree.root = Some(paragraph_id.clone());
        tree.insert(paragraph);
        tree.insert(inline);
        tree.insert(float);

        let mut visited = Vec::new();
        tree.get(&paragraph_id)
            .expect("paragraph should exist")
            .for_each_retained_child(&tree, |child| visited.push((child.id.clone(), child.mode)));

        assert_eq!(
            visited,
            vec![
                (float_id, RetainedChildMode::Scope),
                (inline_id, RetainedChildMode::InlineEventOnly),
            ]
        );
    }

    #[test]
    fn test_for_each_retained_child_non_paragraph_preserves_order_as_scope() {
        let row_id = ElementId::from_term_bytes(vec![20]);
        let first_id = ElementId::from_term_bytes(vec![21]);
        let second_id = ElementId::from_term_bytes(vec![22]);

        let mut row = Element::with_attrs(
            row_id.clone(),
            ElementKind::Row,
            Vec::new(),
            Attrs::default(),
        );
        row.children = vec![first_id.clone(), second_id.clone()];

        let first = Element::with_attrs(
            first_id.clone(),
            ElementKind::El,
            Vec::new(),
            Attrs::default(),
        );
        let second = Element::with_attrs(
            second_id.clone(),
            ElementKind::El,
            Vec::new(),
            Attrs::default(),
        );

        let mut tree = ElementTree::new();
        tree.root = Some(row_id.clone());
        tree.insert(row);
        tree.insert(first);
        tree.insert(second);

        let mut visited = Vec::new();
        tree.get(&row_id)
            .expect("row should exist")
            .for_each_retained_child(&tree, |child| visited.push((child.id.clone(), child.mode)));

        assert_eq!(
            visited,
            vec![
                (first_id, RetainedChildMode::Scope),
                (second_id, RetainedChildMode::Scope),
            ]
        );
    }

    #[test]
    fn test_replace_with_uploaded_advances_revision_and_restamps_nodes() {
        let existing_id = ElementId::from_term_bytes(vec![1]);
        let mut tree = ElementTree::new();
        tree.insert(Element::with_attrs(
            existing_id,
            ElementKind::El,
            Vec::new(),
            Attrs::default(),
        ));
        let first_revision = tree.bump_revision();
        tree.stamp_all_mounted_at_revision(first_revision);

        let uploaded_id = ElementId::from_term_bytes(vec![2]);
        let mut uploaded = ElementTree::new();
        uploaded.root = Some(uploaded_id.clone());
        uploaded.insert(Element::with_attrs(
            uploaded_id.clone(),
            ElementKind::El,
            Vec::new(),
            Attrs::default(),
        ));

        tree.replace_with_uploaded(uploaded);

        assert_eq!(tree.revision(), first_revision + 1);
        assert_eq!(
            tree.get(&uploaded_id)
                .expect("uploaded node should exist")
                .mounted_at_revision,
            tree.revision()
        );
    }

    #[test]
    fn test_clear_advances_revision_without_resetting_to_zero() {
        let mut tree = ElementTree::new();
        let first_revision = tree.bump_revision();

        tree.clear();

        assert!(tree.is_empty());
        assert!(tree.revision() > first_revision);
    }

    #[test]
    fn test_was_mounted_after_uses_node_mount_revision() {
        let id = ElementId::from_term_bytes(vec![3]);
        let mut tree = ElementTree::new();
        tree.root = Some(id.clone());
        tree.insert(Element::with_attrs(
            id.clone(),
            ElementKind::El,
            Vec::new(),
            Attrs::default(),
        ));

        let revision = tree.bump_revision();
        tree.stamp_all_mounted_at_revision(revision);

        assert!(tree.was_mounted_after(&id, revision - 1));
        assert!(!tree.was_mounted_after(&id, revision));
    }
}
