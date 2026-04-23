//! Element types for Emerge UI trees.

use super::animation::AnimationSpec;
#[cfg(test)]
use super::attrs::MouseOverAttrs;
use super::attrs::{Attrs, ScrollbarHoverAxis, supports_mouse_over_tracking};
use std::cell::{Cell, RefCell};
use std::collections::HashMap;

pub type NodeIx = usize;

/// Unique identifier for an element.
/// Stored as the numeric runtime node id shared with Elixir.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct NodeId(pub u64);

impl NodeId {
    pub fn from_u64(value: u64) -> Self {
        Self(value)
    }

    pub fn from_wire_u64(value: u64) -> Self {
        Self(value)
    }

    pub fn to_wire_u64(self) -> u64 {
        self.0
    }

    pub fn to_be_bytes(self) -> [u8; 8] {
        self.0.to_be_bytes()
    }

    pub fn from_term_bytes(bytes: Vec<u8>) -> Self {
        assert!(
            bytes.len() <= 8,
            "NodeId test helper only supports ids up to 8 bytes"
        );
        let mut padded = [0u8; 8];
        let start = 8 - bytes.len();
        padded[start..].copy_from_slice(&bytes);
        Self(u64::from_be_bytes(padded))
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
    pub id: NodeId,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NearbyMountIx {
    pub slot: NearbySlot,
    pub ix: NodeIx,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ParentLink {
    Child { parent: NodeIx },
    Nearby { host: NodeIx, slot: NearbySlot },
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
    pub fn ids(&self, slot: NearbySlot) -> impl DoubleEndedIterator<Item = &NodeId> {
        self.mounts.iter().filter_map(move |mount| {
            if mount.slot == slot {
                Some(&mount.id)
            } else {
                None
            }
        })
    }

    pub fn push(&mut self, slot: NearbySlot, id: NodeId) {
        self.mounts.push(NearbyMount { slot, id });
    }

    pub fn insert(&mut self, index: usize, slot: NearbySlot, id: NodeId) {
        self.mounts
            .insert(index.min(self.mounts.len()), NearbyMount { slot, id });
    }

    #[cfg(test)]
    pub fn set(&mut self, slot: NearbySlot, id: Option<NodeId>) {
        self.mounts.retain(|mount| mount.slot != slot);
        if let Some(id) = id {
            self.mounts.push(NearbyMount { slot, id });
        }
    }

    pub fn set_mounts(&mut self, mounts: Vec<NearbyMount>) {
        self.mounts = mounts;
    }

    pub fn remove(&mut self, id: &NodeId) -> bool {
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
        parent_id: NodeId,
        live_index: usize,
        seq: u64,
    },
    Nearby {
        host_id: NodeId,
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
pub struct RetainedChildRef {
    pub ix: NodeIx,
    pub id: NodeId,
    pub mode: RetainedChildMode,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RetainedNearbyMountRef {
    pub slot: NearbySlot,
    pub ix: NodeIx,
    pub id: NodeId,
}

#[derive(Clone, Copy, Debug)]
pub enum RetainedLocalBranchRef {
    Nearby(RetainedNearbyMountRef),
    Child(RetainedChildRef),
}

/// A single element in the UI tree.
#[derive(Clone, Debug)]
pub struct Element {
    /// Unique identifier for this element.
    pub id: NodeId,

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

    /// Runtime-only pending patch content for focused text inputs.
    pub patch_content: Option<String>,

    /// Child element IDs (order matters).
    #[cfg(test)]
    pub children: Vec<NodeId>,

    /// Computed post-layout child paint order.
    #[cfg(test)]
    pub paint_children: Vec<NodeId>,

    /// Host-owned nearby mount roots.
    #[cfg(test)]
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

#[derive(Clone, Debug, Default)]
struct TreeTopology {
    parents: Vec<Option<ParentLink>>,
    children: Vec<Vec<NodeIx>>,
    paint_children: Vec<Vec<NodeIx>>,
    nearby: Vec<Vec<NearbyMountIx>>,
}

impl Element {
    /// Create an element with decoded attributes.
    /// The attrs are stored as base_attrs (original) and cloned to attrs (for scaling).
    pub fn with_attrs(id: NodeId, kind: ElementKind, attrs_raw: Vec<u8>, attrs: Attrs) -> Self {
        Self {
            id,
            kind,
            attrs_raw,
            base_attrs: attrs.clone(),
            attrs,
            text_input_content_origin: TextInputContentOrigin::TreePatch,
            patch_content: None,
            #[cfg(test)]
            children: Vec::new(),
            #[cfg(test)]
            paint_children: Vec::new(),
            #[cfg(test)]
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

    #[cfg(test)]
    pub fn paint_child_ids(&self) -> &[NodeId] {
        if self.paint_children.is_empty() {
            &self.children
        } else {
            &self.paint_children
        }
    }

    #[cfg(test)]
    pub fn local_nearby_mounts(&self) -> impl Iterator<Item = &NearbyMount> {
        self.nearby
            .iter()
            .filter(|mount| mount.slot == NearbySlot::BehindContent)
    }

    #[cfg(test)]
    pub fn escape_nearby_mounts(&self) -> impl Iterator<Item = &NearbyMount> {
        self.nearby
            .iter()
            .filter(|mount| mount.slot != NearbySlot::BehindContent)
    }

    pub fn for_each_retained_child(&self, tree: &ElementTree, mut f: impl FnMut(RetainedChildRef)) {
        let Some(ix) = tree.ix_of(&self.id) else {
            return;
        };

        if self.kind == ElementKind::Paragraph {
            for child_ix in tree.child_ixs(ix) {
                if paragraph_child_mode(tree, child_ix) == RetainedChildMode::Scope {
                    if let Some(id) = tree.id_of(child_ix) {
                        f(RetainedChildRef {
                            ix: child_ix,
                            id,
                            mode: RetainedChildMode::Scope,
                        });
                    }
                }
            }

            for child_ix in tree.child_ixs(ix) {
                if paragraph_child_mode(tree, child_ix) == RetainedChildMode::InlineEventOnly {
                    if let Some(id) = tree.id_of(child_ix) {
                        f(RetainedChildRef {
                            ix: child_ix,
                            id,
                            mode: RetainedChildMode::InlineEventOnly,
                        });
                    }
                }
            }
        } else {
            for child_ix in tree.paint_child_ixs(ix) {
                if let Some(id) = tree.id_of(child_ix) {
                    f(RetainedChildRef {
                        ix: child_ix,
                        id,
                        mode: RetainedChildMode::Scope,
                    });
                }
            }
        }
    }

    pub fn for_each_retained_local_branch(
        &self,
        tree: &ElementTree,
        mut f: impl FnMut(RetainedLocalBranchRef),
    ) {
        let Some(ix) = tree.ix_of(&self.id) else {
            return;
        };

        for mount in tree.local_nearby_mounts_ix(ix) {
            if let Some(id) = tree.id_of(mount.ix) {
                f(RetainedLocalBranchRef::Nearby(RetainedNearbyMountRef {
                    slot: mount.slot,
                    ix: mount.ix,
                    id,
                }));
            }
        }

        self.for_each_retained_child(tree, |child| f(RetainedLocalBranchRef::Child(child)));
    }
}

fn paragraph_child_mode(tree: &ElementTree, child_ix: NodeIx) -> RetainedChildMode {
    let is_float_child = tree.get_ix(child_ix).is_some_and(|child| {
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
    pub root: Option<NodeId>,

    /// Dense node storage indexed by NodeIx.
    pub nodes: Vec<Option<Element>>,

    /// Shared node-id to internal index map.
    pub id_to_ix: HashMap<NodeId, NodeIx>,

    /// Reusable free slots in the arena.
    pub free_list: Vec<NodeIx>,

    topology: RefCell<TreeTopology>,
    topology_dirty: Cell<bool>,
}

impl Default for ElementTree {
    fn default() -> Self {
        Self {
            revision: 0,
            next_ghost_seq: 0,
            current_scale: 1.0,
            root: None,
            nodes: Vec::new(),
            id_to_ix: HashMap::new(),
            free_list: Vec::new(),
            topology: RefCell::new(TreeTopology::default()),
            topology_dirty: Cell::new(false),
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

    #[cfg(test)]
    pub(crate) fn mark_topology_dirty(&self) {
        self.topology_dirty.set(true);
    }

    #[cfg(test)]
    pub fn ensure_topology(&self) {
        if !self.topology_dirty.get() {
            return;
        }

        let mut topology = TreeTopology {
            parents: vec![None; self.nodes.len()],
            children: vec![Vec::new(); self.nodes.len()],
            paint_children: vec![Vec::new(); self.nodes.len()],
            nearby: vec![Vec::new(); self.nodes.len()],
        };

        for (parent_ix, node) in self
            .nodes
            .iter()
            .enumerate()
            .filter_map(|(ix, slot)| slot.as_ref().map(|node| (ix, node)))
        {
            let child_ixs: Vec<NodeIx> = node
                .children
                .iter()
                .filter_map(|child_id| self.id_to_ix.get(child_id).copied())
                .collect();
            topology.children[parent_ix] = child_ixs.clone();

            if !node.paint_children.is_empty() {
                topology.paint_children[parent_ix] = node
                    .paint_children
                    .iter()
                    .filter_map(|child_id| self.id_to_ix.get(child_id).copied())
                    .collect();
            }

            let nearby_ixs: Vec<NearbyMountIx> = node
                .nearby
                .iter()
                .filter_map(|mount| {
                    self.id_to_ix
                        .get(&mount.id)
                        .copied()
                        .map(|ix| NearbyMountIx {
                            slot: mount.slot,
                            ix,
                        })
                })
                .collect();
            topology.nearby[parent_ix] = nearby_ixs.clone();

            for child_ix in child_ixs {
                topology.parents[child_ix] = Some(ParentLink::Child { parent: parent_ix });
            }

            for mount in nearby_ixs {
                topology.parents[mount.ix] = Some(ParentLink::Nearby {
                    host: parent_ix,
                    slot: mount.slot,
                });
            }
        }

        *self.topology.borrow_mut() = topology;
        self.topology_dirty.set(false);
    }

    #[cfg(not(test))]
    pub fn ensure_topology(&self) {}

    pub fn root_ix(&self) -> Option<NodeIx> {
        #[cfg(test)]
        self.ensure_topology();
        self.root
            .as_ref()
            .and_then(|id| self.id_to_ix.get(id).copied())
    }

    pub fn ix_of(&self, id: &NodeId) -> Option<NodeIx> {
        self.id_to_ix.get(id).copied()
    }

    pub fn id_of(&self, ix: NodeIx) -> Option<NodeId> {
        self.get_ix(ix).map(|element| element.id)
    }

    pub fn get_ix(&self, ix: NodeIx) -> Option<&Element> {
        self.nodes.get(ix).and_then(|slot| slot.as_ref())
    }

    pub fn get_ix_mut(&mut self, ix: NodeIx) -> Option<&mut Element> {
        self.nodes.get_mut(ix).and_then(|slot| slot.as_mut())
    }

    pub fn parent_link_of(&self, ix: NodeIx) -> Option<ParentLink> {
        #[cfg(test)]
        self.ensure_topology();
        self.topology.borrow().parents.get(ix).copied().flatten()
    }

    pub fn child_ixs(&self, ix: NodeIx) -> Vec<NodeIx> {
        #[cfg(test)]
        self.ensure_topology();
        self.topology
            .borrow()
            .children
            .get(ix)
            .cloned()
            .unwrap_or_default()
    }

    pub fn paint_child_ixs(&self, ix: NodeIx) -> Vec<NodeIx> {
        #[cfg(test)]
        self.ensure_topology();
        let topology = self.topology.borrow();
        let paint = topology.paint_children.get(ix).cloned().unwrap_or_default();
        if paint.is_empty() {
            topology.children.get(ix).cloned().unwrap_or_default()
        } else {
            paint
        }
    }

    pub fn nearby_ixs(&self, ix: NodeIx) -> Vec<NearbyMountIx> {
        #[cfg(test)]
        self.ensure_topology();
        self.topology
            .borrow()
            .nearby
            .get(ix)
            .cloned()
            .unwrap_or_default()
    }

    pub fn local_nearby_mounts_ix(&self, ix: NodeIx) -> Vec<NearbyMountIx> {
        self.nearby_ixs(ix)
            .into_iter()
            .filter(|mount| mount.slot == NearbySlot::BehindContent)
            .collect()
    }

    pub fn escape_nearby_mounts_ix(&self, ix: NodeIx) -> Vec<NearbyMountIx> {
        self.nearby_ixs(ix)
            .into_iter()
            .filter(|mount| mount.slot != NearbySlot::BehindContent)
            .collect()
    }

    pub fn iter_nodes(&self) -> impl Iterator<Item = &Element> {
        self.nodes.iter().filter_map(|slot| slot.as_ref())
    }

    pub fn iter_nodes_mut(&mut self) -> impl Iterator<Item = &mut Element> {
        #[cfg(test)]
        self.mark_topology_dirty();
        self.nodes.iter_mut().filter_map(|slot| slot.as_mut())
    }

    pub fn iter_node_pairs(&self) -> impl Iterator<Item = (NodeId, &Element)> {
        self.nodes
            .iter()
            .filter_map(|slot| slot.as_ref().map(|element| (element.id, element)))
    }

    /// Get an element by ID.
    pub fn get(&self, id: &NodeId) -> Option<&Element> {
        self.ix_of(id).and_then(|ix| self.get_ix(ix))
    }

    /// Get a mutable element by ID.
    pub fn get_mut(&mut self, id: &NodeId) -> Option<&mut Element> {
        let ix = self.ix_of(id)?;
        self.get_ix_mut(ix)
    }

    /// Insert or update an element.
    pub fn insert(&mut self, element: Element) {
        if let Some(&ix) = self.id_to_ix.get(&element.id) {
            self.nodes[ix] = Some(element);
        } else if let Some(ix) = self.free_list.pop() {
            self.id_to_ix.insert(element.id, ix);
            self.nodes[ix] = Some(element);
            self.ensure_topology_capacity(ix);
        } else {
            let ix = self.nodes.len();
            self.id_to_ix.insert(element.id, ix);
            self.nodes.push(Some(element));
            self.ensure_topology_capacity(ix);
        }

        #[cfg(test)]
        self.mark_topology_dirty();
    }

    pub fn remove_node(&mut self, id: &NodeId) -> Option<Element> {
        let ix = self.id_to_ix.remove(id)?;
        let removed = self.nodes.get_mut(ix).and_then(|slot| slot.take());
        self.free_list.push(ix);

        let topology = self.topology.get_mut();
        if let Some(parent) = topology.parents.get_mut(ix) {
            *parent = None;
        }
        if let Some(children) = topology.children.get_mut(ix) {
            children.clear();
        }
        if let Some(children) = topology.paint_children.get_mut(ix) {
            children.clear();
        }
        if let Some(nearby) = topology.nearby.get_mut(ix) {
            nearby.clear();
        }

        #[cfg(test)]
        self.mark_topology_dirty();
        removed
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

    pub fn mint_ghost_id(&mut self) -> NodeId {
        let seq = self.next_ghost_seq();
        NodeId((1u64 << 63) | seq)
    }

    /// Stamp every live node as mounted at the provided revision.
    pub fn stamp_all_mounted_at_revision(&mut self, revision: u64) {
        self.iter_nodes_mut()
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
        #[cfg(test)]
        self.mark_topology_dirty();
    }

    /// Returns true when the element was mounted after the provided revision.
    pub fn was_mounted_after(&self, id: &NodeId, revision: u64) -> bool {
        self.get(id)
            .is_some_and(|element| element.mounted_at_revision > revision)
    }

    /// Check if tree is empty.
    pub fn is_empty(&self) -> bool {
        self.root.is_none()
    }

    /// Get the number of nodes.
    pub fn len(&self) -> usize {
        self.id_to_ix.len()
    }

    /// Clear the tree.
    pub fn clear(&mut self) {
        self.bump_revision();
        self.root = None;
        self.id_to_ix.clear();
        self.nodes.clear();
        self.free_list.clear();
        *self.topology.borrow_mut() = TreeTopology::default();
        self.topology_dirty.set(false);
    }

    pub fn set_root_id(&mut self, id: NodeId) {
        self.root = Some(id);

        #[cfg(test)]
        self.mark_topology_dirty();
    }

    pub fn child_ids(&self, parent_id: &NodeId) -> Vec<NodeId> {
        self.ix_of(parent_id)
            .map(|parent_ix| {
                self.child_ixs(parent_ix)
                    .into_iter()
                    .filter_map(|ix| self.id_of(ix))
                    .collect()
            })
            .unwrap_or_default()
    }

    pub fn paint_child_ids_for(&self, parent_id: &NodeId) -> Vec<NodeId> {
        self.ix_of(parent_id)
            .map(|parent_ix| {
                self.paint_child_ixs(parent_ix)
                    .into_iter()
                    .filter_map(|ix| self.id_of(ix))
                    .collect()
            })
            .unwrap_or_default()
    }

    pub fn nearby_mounts_for(&self, host_id: &NodeId) -> Vec<NearbyMount> {
        self.ix_of(host_id)
            .map(|host_ix| {
                self.nearby_ixs(host_ix)
                    .into_iter()
                    .filter_map(|mount| {
                        self.id_of(mount.ix).map(|id| NearbyMount {
                            slot: mount.slot,
                            id,
                        })
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

    pub fn set_children(
        &mut self,
        parent_id: &NodeId,
        child_ids: Vec<NodeId>,
    ) -> Result<(), String> {
        let parent_ix = self
            .ix_of(parent_id)
            .ok_or_else(|| format!("parent not found: {:?}", parent_id.0))?;
        let child_ixs = self.resolve_child_ixs(&child_ids)?;

        self.set_children_ix(parent_ix, child_ixs.clone());
        self.set_paint_children_ix(parent_ix, child_ixs);

        #[cfg(test)]
        if let Some(parent) = self.get_mut(parent_id) {
            parent.children = child_ids.clone();
            parent.paint_children = child_ids;
        }

        Ok(())
    }

    pub fn set_paint_children(
        &mut self,
        parent_id: &NodeId,
        child_ids: Vec<NodeId>,
    ) -> Result<(), String> {
        let parent_ix = self
            .ix_of(parent_id)
            .ok_or_else(|| format!("parent not found: {:?}", parent_id.0))?;
        let child_ixs = self.resolve_child_ixs(&child_ids)?;

        self.set_paint_children_ix(parent_ix, child_ixs);

        #[cfg(test)]
        if let Some(parent) = self.get_mut(parent_id) {
            parent.paint_children = child_ids;
        }

        Ok(())
    }

    pub fn set_nearby_mounts(
        &mut self,
        host_id: &NodeId,
        mounts: Vec<NearbyMount>,
    ) -> Result<(), String> {
        let host_ix = self
            .ix_of(host_id)
            .ok_or_else(|| format!("host not found: {:?}", host_id.0))?;

        let nearby_ixs = mounts
            .iter()
            .map(|mount| {
                self.ix_of(&mount.id)
                    .map(|ix| NearbyMountIx {
                        slot: mount.slot,
                        ix,
                    })
                    .ok_or_else(|| format!("nearby mount not found: {:?}", mount.id.0))
            })
            .collect::<Result<Vec<_>, _>>()?;

        self.set_nearby_ixs(host_ix, nearby_ixs);

        #[cfg(test)]
        if let Some(host) = self.get_mut(host_id) {
            host.nearby.set_mounts(mounts);
        }

        Ok(())
    }

    pub fn live_child_ids(&self, parent_id: &NodeId) -> Vec<NodeId> {
        self.ix_of(parent_id)
            .map(|parent_ix| {
                self.child_ixs(parent_ix)
                    .into_iter()
                    .filter_map(|child_ix| {
                        self.get_ix(child_ix)
                            .filter(|child| child.is_live())
                            .map(|child| child.id)
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

    pub fn merge_live_children_with_ghosts(
        &self,
        parent_id: &NodeId,
        new_live_ids: Vec<NodeId>,
    ) -> Vec<NodeId> {
        let mut ghosts: Vec<(NodeId, usize, u64)> = self
            .ix_of(parent_id)
            .map(|parent_ix| {
                self.child_ixs(parent_ix)
                    .into_iter()
                    .filter_map(|child_ix| {
                        let child = self.get_ix(child_ix)?;
                        match child.ghost_attachment.as_ref() {
                            Some(GhostAttachment::Child {
                                parent_id: ghost_parent_id,
                                live_index,
                                seq,
                            }) if ghost_parent_id == parent_id => {
                                Some((child.id, *live_index, *seq))
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

    pub fn live_nearby_mounts(&self, host_id: &NodeId) -> Vec<NearbyMount> {
        self.ix_of(host_id)
            .map(|host_ix| {
                self.nearby_ixs(host_ix)
                    .into_iter()
                    .filter_map(|mount| {
                        self.get_ix(mount.ix)
                            .filter(|nearby| nearby.is_live())
                            .map(|nearby| NearbyMount {
                                slot: mount.slot,
                                id: nearby.id,
                            })
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

    pub fn merge_live_nearby_with_ghosts(
        &self,
        host_id: &NodeId,
        new_live_mounts: Vec<NearbyMount>,
    ) -> Vec<NearbyMount> {
        let mut ghosts: Vec<(NearbyMount, usize, u64)> = self
            .ix_of(host_id)
            .map(|host_ix| {
                self.nearby_ixs(host_ix)
                    .into_iter()
                    .filter_map(|mount| {
                        let nearby = self.get_ix(mount.ix)?;
                        match nearby.ghost_attachment.as_ref() {
                            Some(GhostAttachment::Nearby {
                                host_id: ghost_host_id,
                                mount_index,
                                seq,
                                ..
                            }) if ghost_host_id == host_id => Some((
                                NearbyMount {
                                    slot: mount.slot,
                                    id: nearby.id,
                                },
                                *mount_index,
                                *seq,
                            )),
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

    fn ensure_topology_capacity(&mut self, ix: NodeIx) {
        let topology = self.topology.get_mut();
        while topology.parents.len() <= ix {
            topology.parents.push(None);
            topology.children.push(Vec::new());
            topology.paint_children.push(Vec::new());
            topology.nearby.push(Vec::new());
        }
    }

    fn resolve_child_ixs(&self, child_ids: &[NodeId]) -> Result<Vec<NodeIx>, String> {
        child_ids
            .iter()
            .map(|child_id| {
                self.ix_of(child_id)
                    .ok_or_else(|| format!("child not found: {:?}", child_id.0))
            })
            .collect()
    }

    fn set_children_ix(&mut self, parent_ix: NodeIx, child_ixs: Vec<NodeIx>) {
        self.ensure_topology_capacity(parent_ix);
        let topology = self.topology.get_mut();

        for old_child_ix in topology.children[parent_ix].drain(..) {
            if matches!(topology.parents[old_child_ix], Some(ParentLink::Child { parent }) if parent == parent_ix)
            {
                topology.parents[old_child_ix] = None;
            }
        }

        for &child_ix in &child_ixs {
            topology.parents[child_ix] = Some(ParentLink::Child { parent: parent_ix });
        }

        topology.children[parent_ix] = child_ixs;
    }

    fn set_paint_children_ix(&mut self, parent_ix: NodeIx, child_ixs: Vec<NodeIx>) {
        self.ensure_topology_capacity(parent_ix);
        self.topology.get_mut().paint_children[parent_ix] = child_ixs;
    }

    fn set_nearby_ixs(&mut self, host_ix: NodeIx, mounts: Vec<NearbyMountIx>) {
        self.ensure_topology_capacity(host_ix);
        let topology = self.topology.get_mut();

        for old_mount in topology.nearby[host_ix].drain(..) {
            if matches!(topology.parents[old_mount.ix], Some(ParentLink::Nearby { host, .. }) if host == host_ix)
            {
                topology.parents[old_mount.ix] = None;
            }
        }

        for mount in &mounts {
            topology.parents[mount.ix] = Some(ParentLink::Nearby {
                host: host_ix,
                slot: mount.slot,
            });
        }

        topology.nearby[host_ix] = mounts;
    }

    /// Apply scroll delta to an element. Returns true if scroll changed.
    pub fn apply_scroll(&mut self, id: &NodeId, dx: f32, dy: f32) -> bool {
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
    pub fn apply_scroll_x(&mut self, id: &NodeId, dx: f32) -> bool {
        self.apply_scroll_axis(id, dx, ScrollAxis::X)
    }

    /// Apply vertical scroll delta to an element. Returns true if scroll changed.
    pub fn apply_scroll_y(&mut self, id: &NodeId, dy: f32) -> bool {
        self.apply_scroll_axis(id, dy, ScrollAxis::Y)
    }

    /// Set horizontal scrollbar thumb hover state. Returns true when state changes.
    pub fn set_scrollbar_x_hover(&mut self, id: &NodeId, hovered: bool) -> bool {
        self.set_scrollbar_hover_axis(id, ScrollbarHoverAxis::X, hovered)
    }

    /// Set vertical scrollbar thumb hover state. Returns true when state changes.
    pub fn set_scrollbar_y_hover(&mut self, id: &NodeId, hovered: bool) -> bool {
        self.set_scrollbar_hover_axis(id, ScrollbarHoverAxis::Y, hovered)
    }

    /// Set mouse_over active state. Returns true when state changes.
    pub fn set_mouse_over_active(&mut self, id: &NodeId, active: bool) -> bool {
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
    pub fn set_mouse_down_active(&mut self, id: &NodeId, active: bool) -> bool {
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
    pub fn set_focused_active(&mut self, id: &NodeId, active: bool) -> bool {
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

    pub fn set_text_input_content(&mut self, id: &NodeId, content: String) -> bool {
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

        if element.patch_content.take().is_some() {
            changed = true;
        }

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
        id: &NodeId,
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

    fn apply_scroll_axis(&mut self, id: &NodeId, delta: f32, axis: ScrollAxis) -> bool {
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
        id: &NodeId,
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
        let id = NodeId::from_term_bytes(vec![1]);
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
        let id = NodeId::from_term_bytes(vec![1]);
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
        let id = NodeId::from_term_bytes(vec![1]);
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
        let id = NodeId::from_term_bytes(vec![1]);
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
        let id = NodeId::from_term_bytes(vec![1]);
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
        let id = NodeId::from_term_bytes(vec![1]);
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
        let id = NodeId::from_term_bytes(vec![2]);
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
        let id = NodeId::from_term_bytes(vec![11]);
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
        let id = NodeId::from_term_bytes(vec![12]);
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
        let id = NodeId::from_term_bytes(vec![13]);
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
        let id = NodeId::from_term_bytes(vec![2]);
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
        let id = NodeId::from_term_bytes(vec![14]);
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
        let id = NodeId::from_term_bytes(vec![3]);
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
        let id = NodeId::from_term_bytes(vec![4]);
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
        let paragraph_id = NodeId::from_term_bytes(vec![10]);
        let inline_id = NodeId::from_term_bytes(vec![11]);
        let float_id = NodeId::from_term_bytes(vec![12]);

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
        let row_id = NodeId::from_term_bytes(vec![20]);
        let first_id = NodeId::from_term_bytes(vec![21]);
        let second_id = NodeId::from_term_bytes(vec![22]);

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
        let existing_id = NodeId::from_term_bytes(vec![1]);
        let mut tree = ElementTree::new();
        tree.insert(Element::with_attrs(
            existing_id,
            ElementKind::El,
            Vec::new(),
            Attrs::default(),
        ));
        let first_revision = tree.bump_revision();
        tree.stamp_all_mounted_at_revision(first_revision);

        let uploaded_id = NodeId::from_term_bytes(vec![2]);
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
        let id = NodeId::from_term_bytes(vec![3]);
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
