//! Element types for Emerge UI trees.

use super::animation::AnimationSpec;
#[cfg(test)]
use super::attrs::MouseOverAttrs;
use super::attrs::{
    AlignX, AlignY, Attrs, BorderWidth, Font, FontStyle, FontWeight, ImageFit, ImageSource, Length,
    Padding, ScrollbarHoverAxis, TextAlign, TextFragment, supports_mouse_over_tracking,
};
use super::invalidation::{TreeInvalidation, classify_interaction_style};
use crate::events::registry_builder::RegistrySubtreeCache;
use crate::render_scene::RenderNode;
use crate::stats::LayoutCacheStats;
#[cfg(test)]
use std::cell::{Cell, RefCell};
use std::collections::{HashMap, hash_map::DefaultHasher};
use std::hash::{Hash, Hasher};

pub type NodeIx = usize;

const DETACHED_LAYOUT_CACHE_LIMIT: usize = 16;
const DETACHED_LAYOUT_CACHE_MAX_NODES: usize = 128;

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

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
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
#[derive(Clone, Copy, Debug, Default, PartialEq)]
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

#[derive(Clone, Debug)]
pub struct IntrinsicMeasureCache {
    pub key: IntrinsicMeasureCacheKey,
    pub frame: Frame,
}

#[derive(Clone, Debug)]
pub struct SubtreeMeasureCache {
    pub key: SubtreeMeasureCacheKey,
    pub frame: Frame,
}

#[derive(Clone, Debug)]
pub struct ResolveCache {
    pub key: ResolveCacheKey,
    pub extent: ResolveExtent,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ResolveExtent {
    pub width: f32,
    pub height: f32,
    pub content_width: f32,
    pub content_height: f32,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ResolveCacheKey {
    pub kind: ElementKind,
    pub attrs: ResolveAttrs,
    pub inherited: InheritedMeasureFontKey,
    pub measured_frame: Option<Frame>,
    pub constraint: ResolveConstraintKey,
    pub topology: TopologyDependencyKey,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ResolveConstraintKey {
    pub width: ResolveAvailableSpaceKey,
    pub height: ResolveAvailableSpaceKey,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ResolveAvailableSpaceKey {
    Definite(f32),
    MinContent,
    MaxContent,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ResolveAttrs {
    pub width: Option<Length>,
    pub height: Option<Length>,
    pub padding: Option<Padding>,
    pub border_width: Option<BorderWidth>,
    pub spacing: Option<f64>,
    pub spacing_x: Option<f64>,
    pub spacing_y: Option<f64>,
    pub align_x: Option<AlignX>,
    pub align_y: Option<AlignY>,
    pub scrollbar_y: Option<bool>,
    pub scrollbar_x: Option<bool>,
    pub ghost_scrollbar_y: Option<bool>,
    pub ghost_scrollbar_x: Option<bool>,
    pub scroll_x: Option<f64>,
    pub scroll_y: Option<f64>,
    pub clip_nearby: Option<bool>,
    pub content: Option<String>,
    pub font_size: Option<f64>,
    pub font: Option<Font>,
    pub font_weight: Option<FontWeight>,
    pub font_style: Option<FontStyle>,
    pub font_letter_spacing: Option<f64>,
    pub font_word_spacing: Option<f64>,
    pub image_src: Option<ImageSource>,
    pub image_fit: Option<ImageFit>,
    pub image_size: Option<(f64, f64)>,
    pub text_align: Option<TextAlign>,
    pub snap_layout: Option<bool>,
    pub snap_text_metrics: Option<bool>,
    pub space_evenly: Option<bool>,
    pub has_animation_attrs: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct SubtreeMeasureCacheKey {
    pub kind: ElementKind,
    pub attrs: SubtreeMeasureAttrs,
    pub inherited: InheritedMeasureFontKey,
    pub topology: TopologyDependencyKey,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct LayoutTopologyVersions {
    pub children: u64,
    pub paint_children: u64,
    pub nearby: u64,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TopologyDependencyKey {
    pub children_version: u64,
    pub nearby_version: u64,
    pub child_count: usize,
    pub nearby_count: usize,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RenderTopologyDependencyKey {
    pub children_version: u64,
    pub paint_children_version: u64,
    pub nearby_version: u64,
    pub child_count: usize,
    pub paint_child_count: usize,
    pub nearby_count: usize,
}

#[derive(Clone, Debug, PartialEq)]
pub struct RenderSubtreeKey {
    pub kind: ElementKind,
    pub attrs_hash: u64,
    pub runtime_hash: u64,
    pub frame: Option<Frame>,
    pub scroll_x: f32,
    pub scroll_y: f32,
    pub scroll_x_max: f32,
    pub scroll_y_max: f32,
    pub inherited_hash: u64,
    pub scene_context_hash: u64,
    pub render_context_hash: u64,
    pub topology: RenderTopologyDependencyKey,
    pub paragraph_fragments_hash: u64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct RenderSubtreeCache {
    pub key: RenderSubtreeKey,
    pub local: Vec<RenderNode>,
    pub escapes: Vec<RenderNode>,
    pub text_input_focused: bool,
    pub text_input_cursor_area: Option<(f32, f32, f32, f32)>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct SubtreeMeasureAttrs {
    pub width: Option<Length>,
    pub height: Option<Length>,
    pub padding: Option<Padding>,
    pub border_width: Option<BorderWidth>,
    pub spacing: Option<f64>,
    pub spacing_x: Option<f64>,
    pub spacing_y: Option<f64>,
    pub scrollbar_y: Option<bool>,
    pub scrollbar_x: Option<bool>,
    pub ghost_scrollbar_y: Option<bool>,
    pub ghost_scrollbar_x: Option<bool>,
    pub scroll_x: Option<f64>,
    pub scroll_y: Option<f64>,
    pub clip_nearby: Option<bool>,
    pub content: Option<String>,
    pub font_size: Option<f64>,
    pub font: Option<Font>,
    pub font_weight: Option<FontWeight>,
    pub font_style: Option<FontStyle>,
    pub font_letter_spacing: Option<f64>,
    pub font_word_spacing: Option<f64>,
    pub image_src: Option<ImageSource>,
    pub image_fit: Option<ImageFit>,
    pub image_size: Option<(f64, f64)>,
    pub text_align: Option<TextAlign>,
    pub snap_layout: Option<bool>,
    pub snap_text_metrics: Option<bool>,
    pub space_evenly: Option<bool>,
    pub has_animation_attrs: bool,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct InheritedMeasureFontKey {
    pub family: Option<String>,
    pub weight: Option<u16>,
    pub italic: Option<bool>,
    pub font_size: Option<f32>,
    pub letter_spacing: Option<f32>,
    pub word_spacing: Option<f32>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum IntrinsicMeasureCacheKey {
    Text {
        kind: ElementKind,
        content: Option<String>,
        width: Option<Length>,
        height: Option<Length>,
        padding: Option<Padding>,
        border_width: Option<BorderWidth>,
        family: String,
        weight: u16,
        italic: bool,
        font_size: f32,
        letter_spacing: f32,
        word_spacing: f32,
    },
    Media {
        kind: ElementKind,
        width: Option<Length>,
        height: Option<Length>,
        padding: Option<Padding>,
        border_width: Option<BorderWidth>,
        image_src: Option<ImageSource>,
        image_size: Option<(f64, f64)>,
        resolved_source_size: Option<(u32, u32)>,
    },
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

    pub spec: NodeSpec,
    pub runtime: NodeRuntime,
    pub layout: NodeLayoutState,
    pub refresh: NodeRefreshState,
    pub lifecycle: NodeLifecycle,

    /// Child element IDs (order matters).
    #[cfg(test)]
    pub children: Vec<NodeId>,

    /// Computed post-layout child paint order.
    #[cfg(test)]
    pub paint_children: Vec<NodeId>,

    /// Host-owned nearby mount roots.
    #[cfg(test)]
    pub nearby: NearbyMounts,
}

pub type NodeRecord = Element;

#[derive(Clone, Debug)]
pub struct NodeSpec {
    /// The type of element (row, column, el, text, etc).
    pub kind: ElementKind,

    /// Raw attributes as binary (EMRG format).
    pub attrs_raw: Vec<u8>,

    /// Original unscaled attributes (as received from Elixir).
    pub declared: Attrs,
}

#[derive(Clone, Debug, Default, Hash)]
pub struct NodeRuntime {
    /// Runtime-only origin label for current text-input content.
    pub text_input_content_origin: TextInputContentOrigin,

    /// Runtime-only pending patch content for focused text inputs.
    pub patch_content: Option<String>,

    pub text_input_focused: bool,
    pub text_input_cursor: Option<u32>,
    pub text_input_selection_anchor: Option<u32>,
    pub text_input_preedit: Option<String>,
    pub text_input_preedit_cursor: Option<(u32, u32)>,

    pub mouse_over_active: bool,
    pub mouse_down_active: bool,
    pub focused_active: bool,
    pub scrollbar_hover_axis: Option<ScrollbarHoverAxis>,
}

#[derive(Clone, Debug)]
pub struct NodeRefreshState {
    pub render_dirty: bool,
    pub render_descendant_dirty: bool,
    pub registry_dirty: bool,
    pub registry_descendant_dirty: bool,
    pub render_cache: Option<RenderSubtreeCache>,
    pub registry_cache: Option<RegistrySubtreeCache>,
}

impl Default for NodeRefreshState {
    fn default() -> Self {
        Self {
            render_dirty: true,
            render_descendant_dirty: false,
            registry_dirty: true,
            registry_descendant_dirty: false,
            render_cache: None,
            registry_cache: None,
        }
    }
}

impl NodeRefreshState {
    fn clear_render(&mut self) {
        self.render_dirty = false;
        self.render_descendant_dirty = false;
    }

    fn clear_registry(&mut self) {
        self.registry_dirty = false;
        self.registry_descendant_dirty = false;
    }

    fn has_render_damage(&self) -> bool {
        self.render_dirty || self.render_descendant_dirty
    }

    fn has_registry_damage(&self) -> bool {
        self.registry_dirty || self.registry_descendant_dirty
    }
}

#[derive(Clone, Debug)]
pub struct NodeLayoutState {
    /// Scaled attributes (populated by layout pass, used by render).
    pub effective: Attrs,

    /// Computed layout frame (populated after layout pass).
    pub frame: Option<Frame>,

    /// Intrinsic frame captured during measurement pass before resolution mutates `frame`.
    pub measured_frame: Option<Frame>,

    pub scroll_x: f32,
    pub scroll_y: f32,
    pub scroll_x_max: f32,
    pub scroll_y_max: f32,
    pub paragraph_fragments: Option<Vec<TextFragment>>,
    pub topology_versions: LayoutTopologyVersions,
    pub intrinsic_measure_cache: Option<IntrinsicMeasureCache>,
    pub subtree_measure_cache: Option<SubtreeMeasureCache>,
    pub measure_dirty: bool,
    pub measure_descendant_dirty: bool,
    pub resolve_cache: Option<ResolveCache>,
    pub resolve_dirty: bool,
    pub resolve_descendant_dirty: bool,
}

impl Default for NodeLayoutState {
    fn default() -> Self {
        Self {
            effective: Attrs::default(),
            frame: None,
            measured_frame: None,
            scroll_x: 0.0,
            scroll_y: 0.0,
            scroll_x_max: 0.0,
            scroll_y_max: 0.0,
            paragraph_fragments: None,
            topology_versions: LayoutTopologyVersions::default(),
            intrinsic_measure_cache: None,
            subtree_measure_cache: None,
            measure_dirty: true,
            measure_descendant_dirty: false,
            resolve_cache: None,
            resolve_dirty: true,
            resolve_descendant_dirty: false,
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct NodeLifecycle {
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
struct NodeTopology {
    parent: Option<ParentLink>,
    children: Vec<NodeIx>,
    paint_children: Vec<NodeIx>,
    nearby: Vec<NearbyMountIx>,
}

#[derive(Clone, Debug)]
struct DetachedLayoutSubtreeCache {
    signature: u64,
    scale_bits: u32,
    states: Vec<NodeLayoutState>,
}

#[derive(Clone, Debug, Default)]
struct TreeTopology {
    nodes: Vec<NodeTopology>,
}

impl Element {
    /// Create an element with decoded attributes.
    /// The attrs are stored as base_attrs (original) and cloned to attrs (for scaling).
    pub fn with_attrs(id: NodeId, kind: ElementKind, attrs_raw: Vec<u8>, attrs: Attrs) -> Self {
        Self {
            id,
            spec: NodeSpec {
                kind,
                attrs_raw,
                declared: attrs.clone(),
            },
            runtime: NodeRuntime {
                text_input_content_origin: TextInputContentOrigin::TreePatch,
                patch_content: None,
                #[cfg(test)]
                text_input_focused: attrs.text_input_focused.unwrap_or(false),
                #[cfg(not(test))]
                text_input_focused: false,
                #[cfg(test)]
                text_input_cursor: attrs.text_input_cursor,
                #[cfg(not(test))]
                text_input_cursor: None,
                #[cfg(test)]
                text_input_selection_anchor: attrs.text_input_selection_anchor,
                #[cfg(not(test))]
                text_input_selection_anchor: None,
                #[cfg(test)]
                text_input_preedit: attrs.text_input_preedit.clone(),
                #[cfg(not(test))]
                text_input_preedit: None,
                #[cfg(test)]
                text_input_preedit_cursor: attrs.text_input_preedit_cursor,
                #[cfg(not(test))]
                text_input_preedit_cursor: None,
                #[cfg(test)]
                mouse_over_active: attrs.mouse_over_active.unwrap_or(false),
                #[cfg(not(test))]
                mouse_over_active: false,
                #[cfg(test)]
                mouse_down_active: attrs.mouse_down_active.unwrap_or(false),
                #[cfg(not(test))]
                mouse_down_active: false,
                #[cfg(test)]
                focused_active: attrs.focused_active.unwrap_or(false),
                #[cfg(not(test))]
                focused_active: false,
                #[cfg(test)]
                scrollbar_hover_axis: attrs.scrollbar_hover_axis,
                #[cfg(not(test))]
                scrollbar_hover_axis: None,
            },
            layout: NodeLayoutState {
                scroll_x: attrs.scroll_x.unwrap_or(0.0) as f32,
                scroll_y: attrs.scroll_y.unwrap_or(0.0) as f32,
                #[cfg(test)]
                scroll_x_max: attrs.scroll_x_max.unwrap_or(0.0) as f32,
                #[cfg(not(test))]
                scroll_x_max: 0.0,
                #[cfg(test)]
                scroll_y_max: attrs.scroll_y_max.unwrap_or(0.0) as f32,
                #[cfg(not(test))]
                scroll_y_max: 0.0,
                #[cfg(test)]
                paragraph_fragments: attrs.paragraph_fragments.clone(),
                #[cfg(not(test))]
                paragraph_fragments: None,
                topology_versions: LayoutTopologyVersions::default(),
                effective: attrs,
                frame: None,
                measured_frame: None,
                intrinsic_measure_cache: None,
                subtree_measure_cache: None,
                measure_dirty: true,
                measure_descendant_dirty: false,
                resolve_cache: None,
                resolve_dirty: true,
                resolve_descendant_dirty: false,
            },
            refresh: NodeRefreshState::default(),
            lifecycle: NodeLifecycle {
                mounted_at_revision: 0,
                residency: NodeResidency::Live,
                ghost_attachment: None,
                ghost_capture_scale: None,
                ghost_exit_animation: None,
            },
            #[cfg(test)]
            children: Vec::new(),
            #[cfg(test)]
            paint_children: Vec::new(),
            #[cfg(test)]
            nearby: NearbyMounts::default(),
        }
    }

    pub(crate) fn render_snapshot(&self) -> Self {
        Self {
            id: self.id,
            spec: self.spec.clone(),
            runtime: self.runtime.clone(),
            layout: NodeLayoutState {
                effective: self.layout.effective.clone(),
                frame: self.layout.frame,
                measured_frame: self.layout.measured_frame,
                scroll_x: self.layout.scroll_x,
                scroll_y: self.layout.scroll_y,
                scroll_x_max: self.layout.scroll_x_max,
                scroll_y_max: self.layout.scroll_y_max,
                paragraph_fragments: self.layout.paragraph_fragments.clone(),
                topology_versions: self.layout.topology_versions,
                intrinsic_measure_cache: None,
                subtree_measure_cache: None,
                measure_dirty: self.layout.measure_dirty,
                measure_descendant_dirty: self.layout.measure_descendant_dirty,
                resolve_cache: None,
                resolve_dirty: self.layout.resolve_dirty,
                resolve_descendant_dirty: self.layout.resolve_descendant_dirty,
            },
            refresh: NodeRefreshState {
                render_dirty: self.refresh.render_dirty,
                render_descendant_dirty: self.refresh.render_descendant_dirty,
                registry_dirty: self.refresh.registry_dirty,
                registry_descendant_dirty: self.refresh.registry_descendant_dirty,
                render_cache: None,
                registry_cache: None,
            },
            lifecycle: self.lifecycle.clone(),
            #[cfg(test)]
            children: self.children.clone(),
            #[cfg(test)]
            paint_children: self.paint_children.clone(),
            #[cfg(test)]
            nearby: self.nearby.clone(),
        }
    }

    pub fn is_live(&self) -> bool {
        matches!(self.lifecycle.residency, NodeResidency::Live)
    }

    pub fn is_ghost(&self) -> bool {
        matches!(self.lifecycle.residency, NodeResidency::Ghost)
    }

    pub fn is_ghost_root(&self) -> bool {
        self.is_ghost() && self.lifecycle.ghost_attachment.is_some()
    }

    pub fn normalize_extracted_state(&mut self) {
        let attrs = &mut self.layout.effective;

        if self.spec.kind.is_text_input_family() {
            let content_len = attrs
                .content
                .as_ref()
                .map(|content| text_char_len(content))
                .unwrap_or(0);

            if let Some(cursor) = self.runtime.text_input_cursor {
                self.runtime.text_input_cursor = Some(cursor.min(content_len));
            } else if self.runtime.text_input_focused {
                self.runtime.text_input_cursor = Some(content_len);
            }

            let cursor = self.runtime.text_input_cursor.unwrap_or(content_len);

            if let Some(anchor) = self.runtime.text_input_selection_anchor {
                let clamped = anchor.min(content_len);
                self.runtime.text_input_selection_anchor =
                    if !self.runtime.text_input_focused || clamped == cursor {
                        None
                    } else {
                        Some(clamped)
                    };
            } else if !self.runtime.text_input_focused {
                self.runtime.text_input_selection_anchor = None;
            }

            if !self.runtime.text_input_focused {
                self.runtime.text_input_preedit = None;
                self.runtime.text_input_preedit_cursor = None;
            } else {
                self.runtime.text_input_preedit = self
                    .runtime
                    .text_input_preedit
                    .take()
                    .filter(|value| !value.is_empty());
                self.runtime.text_input_preedit_cursor = normalize_preedit_cursor(
                    self.runtime.text_input_preedit.as_deref(),
                    self.runtime.text_input_preedit_cursor,
                );
            }
        } else {
            self.runtime.text_input_focused = false;
            self.runtime.text_input_cursor = None;
            self.runtime.text_input_selection_anchor = None;
            self.runtime.text_input_preedit = None;
            self.runtime.text_input_preedit_cursor = None;
        }

        self.runtime.scrollbar_hover_axis = match self.runtime.scrollbar_hover_axis {
            Some(ScrollbarHoverAxis::X) if attrs.scrollbar_x.unwrap_or(false) => {
                Some(ScrollbarHoverAxis::X)
            }
            Some(ScrollbarHoverAxis::Y) if attrs.scrollbar_y.unwrap_or(false) => {
                Some(ScrollbarHoverAxis::Y)
            }
            _ => None,
        };

        if let Some(scroll_x) = attrs.scroll_x {
            self.layout.scroll_x = scroll_x as f32;
        }
        if let Some(scroll_y) = attrs.scroll_y {
            self.layout.scroll_y = scroll_y as f32;
        }

        if !supports_mouse_over_tracking(attrs) {
            self.runtime.mouse_over_active = false;
        }

        if attrs.mouse_down.is_none() {
            self.runtime.mouse_down_active = false;
        }

        #[cfg(test)]
        {
            if self.spec.kind.is_text_input_family() {
                attrs.text_input_focused = Some(self.runtime.text_input_focused);
                attrs.text_input_cursor = self.runtime.text_input_cursor;
                attrs.text_input_selection_anchor = self.runtime.text_input_selection_anchor;
                attrs.text_input_preedit = self.runtime.text_input_preedit.clone();
                attrs.text_input_preedit_cursor = self.runtime.text_input_preedit_cursor;
            } else {
                attrs.text_input_focused = None;
                attrs.text_input_cursor = None;
                attrs.text_input_selection_anchor = None;
                attrs.text_input_preedit = None;
                attrs.text_input_preedit_cursor = None;
            }
            attrs.scrollbar_hover_axis = self.runtime.scrollbar_hover_axis;
            attrs.scroll_x_max = Some(self.layout.scroll_x_max as f64);
            attrs.scroll_y_max = Some(self.layout.scroll_y_max as f64);
            attrs.mouse_over_active = Some(self.runtime.mouse_over_active);
            attrs.mouse_down_active = Some(self.runtime.mouse_down_active);
            attrs.focused_active = Some(self.runtime.focused_active);
            attrs.paragraph_fragments = self.layout.paragraph_fragments.clone();
        }
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

        if self.spec.kind == ElementKind::Paragraph {
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
            child.layout.effective.align_x,
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

    /// Root element index (if tree is non-empty).
    root: Option<NodeIx>,

    /// Dense node storage indexed by NodeIx.
    pub nodes: Vec<Option<NodeRecord>>,

    /// Shared node-id to internal index map.
    pub id_to_ix: HashMap<NodeId, NodeIx>,

    /// Reusable free slots in the arena.
    pub free_list: Vec<NodeIx>,

    layout_cache_stats: LayoutCacheStats,
    layout_cache_stats_enabled: bool,
    detached_layout_cache: Vec<DetachedLayoutSubtreeCache>,

    pending_root_id: Option<NodeId>,

    #[cfg(test)]
    topology: RefCell<TreeTopology>,
    #[cfg(not(test))]
    topology: TreeTopology,

    #[cfg(test)]
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
            layout_cache_stats: LayoutCacheStats::default(),
            layout_cache_stats_enabled: false,
            detached_layout_cache: Vec::new(),
            pending_root_id: None,
            #[cfg(test)]
            topology: RefCell::new(TreeTopology::default()),
            #[cfg(not(test))]
            topology: TreeTopology::default(),
            #[cfg(test)]
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

    pub fn reset_layout_cache_stats(&mut self) {
        if self.layout_cache_stats_enabled {
            self.layout_cache_stats = LayoutCacheStats::default();
        }
    }

    #[inline]
    pub fn set_layout_cache_stats_enabled(&mut self, enabled: bool) {
        if self.layout_cache_stats_enabled == enabled {
            return;
        }

        self.layout_cache_stats_enabled = enabled;
        if !enabled {
            self.layout_cache_stats = LayoutCacheStats::default();
        }
    }

    #[inline]
    pub fn layout_cache_stats_enabled(&self) -> bool {
        self.layout_cache_stats_enabled
    }

    pub fn layout_cache_stats(&self) -> LayoutCacheStats {
        self.layout_cache_stats
    }

    #[inline]
    pub fn record_layout_cache_stats(&mut self, record: impl FnOnce(&mut LayoutCacheStats)) {
        if self.layout_cache_stats_enabled {
            record(&mut self.layout_cache_stats);
        }
    }

    pub(crate) fn store_detached_layout_subtree_cache(&mut self, id: &NodeId) {
        let Some(root_ix) = self.ix_of(id) else {
            return;
        };
        let Some(signature) = self.detached_layout_signature_ix(root_ix) else {
            return;
        };
        let ixs = self.detached_layout_ixs(root_ix);

        if ixs.is_empty() || ixs.len() > DETACHED_LAYOUT_CACHE_MAX_NODES {
            return;
        }

        let states: Vec<NodeLayoutState> = ixs
            .iter()
            .filter_map(|&ix| self.get_ix(ix).map(|element| element.layout.clone()))
            .collect();

        if states.len() != ixs.len() {
            return;
        }

        let scale_bits = self.current_scale.to_bits();
        self.detached_layout_cache
            .retain(|cache| cache.signature != signature || cache.scale_bits != scale_bits);
        self.detached_layout_cache.push(DetachedLayoutSubtreeCache {
            signature,
            scale_bits,
            states,
        });

        if self.detached_layout_cache.len() > DETACHED_LAYOUT_CACHE_LIMIT {
            let overflow = self.detached_layout_cache.len() - DETACHED_LAYOUT_CACHE_LIMIT;
            self.detached_layout_cache.drain(0..overflow);
        }
    }

    pub(crate) fn restore_detached_layout_subtree_cache(&mut self, id: &NodeId) -> bool {
        let Some(root_ix) = self.ix_of(id) else {
            return false;
        };
        let Some(signature) = self.detached_layout_signature_ix(root_ix) else {
            return false;
        };
        let scale_bits = self.current_scale.to_bits();
        let Some(position) = self
            .detached_layout_cache
            .iter()
            .position(|cache| cache.signature == signature && cache.scale_bits == scale_bits)
        else {
            return false;
        };

        let cache = self.detached_layout_cache.remove(position);
        let ixs = self.detached_layout_ixs(root_ix);
        if ixs.len() != cache.states.len() {
            return false;
        }

        let restore_plan: Vec<_> = ixs
            .iter()
            .zip(cache.states.iter())
            .filter_map(|(&ix, state)| {
                let versions = self.get_ix(ix)?.layout.topology_versions;
                Some((
                    ix,
                    state.clone(),
                    versions,
                    self.measure_topology_dependency_key_ix(ix),
                    self.topology_dependency_key_ix(ix),
                ))
            })
            .collect();

        if restore_plan.len() != cache.states.len() {
            return false;
        }

        for (ix, mut layout, versions, measure_topology, resolve_topology) in restore_plan {
            layout.topology_versions = versions;
            if let Some(cache) = layout.subtree_measure_cache.as_mut() {
                cache.key.topology = measure_topology;
            }
            if let Some(cache) = layout.resolve_cache.as_mut() {
                cache.key.topology = resolve_topology;
            }
            if let Some(element) = self.get_ix_mut(ix) {
                element.layout = layout;
            }
        }

        true
    }

    fn detached_layout_signature_ix(&self, ix: NodeIx) -> Option<u64> {
        let mut hasher = DefaultHasher::new();
        self.hash_detached_layout_signature_ix(ix, &mut hasher)?;
        Some(hasher.finish())
    }

    fn hash_detached_layout_signature_ix(&self, ix: NodeIx, state: &mut impl Hasher) -> Option<()> {
        let element = self.get_ix(ix)?;
        if element.spec.declared.animate.is_some()
            || element.spec.declared.animate_enter.is_some()
            || element.spec.declared.animate_exit.is_some()
        {
            return None;
        }

        std::mem::discriminant(&element.spec.kind).hash(state);
        element.spec.attrs_raw.hash(state);
        element.runtime.hash(state);

        let child_ixs = self.child_ixs(ix);
        child_ixs.len().hash(state);
        for child_ix in child_ixs {
            self.hash_detached_layout_signature_ix(child_ix, state)?;
        }

        let nearby_ixs = self.nearby_ixs(ix);
        nearby_ixs.len().hash(state);
        for mount in nearby_ixs {
            mount.slot.hash(state);
            self.hash_detached_layout_signature_ix(mount.ix, state)?;
        }

        Some(())
    }

    fn detached_layout_ixs(&self, root_ix: NodeIx) -> Vec<NodeIx> {
        let mut ixs = Vec::new();
        self.collect_detached_layout_ixs(root_ix, &mut ixs);
        ixs
    }

    fn collect_detached_layout_ixs(&self, ix: NodeIx, ixs: &mut Vec<NodeIx>) {
        ixs.push(ix);
        for child_ix in self.child_ixs(ix) {
            self.collect_detached_layout_ixs(child_ix, ixs);
        }
        for mount in self.nearby_ixs(ix) {
            self.collect_detached_layout_ixs(mount.ix, ixs);
        }
    }

    pub fn mark_refresh_dirty_for_invalidation(
        &mut self,
        id: &NodeId,
        invalidation: TreeInvalidation,
    ) {
        let Some(ix) = self.ix_of(id) else {
            return;
        };

        match invalidation {
            TreeInvalidation::None => {}
            TreeInvalidation::Registry => self.mark_registry_refresh_dirty_ix(ix),
            TreeInvalidation::Paint => self.mark_render_refresh_dirty_ix(ix),
            TreeInvalidation::Resolve | TreeInvalidation::Measure | TreeInvalidation::Structure => {
                self.mark_render_and_registry_refresh_dirty_ix(ix)
            }
        }
    }

    pub fn mark_registry_refresh_dirty(&mut self, id: &NodeId) {
        if let Some(ix) = self.ix_of(id) {
            self.mark_registry_refresh_dirty_ix(ix);
        }
    }

    pub fn mark_render_and_registry_refresh_dirty(&mut self, id: &NodeId) {
        if let Some(ix) = self.ix_of(id) {
            self.mark_render_and_registry_refresh_dirty_ix(ix);
        }
    }

    pub fn has_render_refresh_damage(&self) -> bool {
        self.iter_nodes()
            .any(|element| element.refresh.has_render_damage())
    }

    pub fn has_registry_refresh_damage(&self) -> bool {
        self.iter_nodes()
            .any(|element| element.refresh.has_registry_damage())
    }

    pub fn has_render_subtree_cache(&self) -> bool {
        self.iter_nodes()
            .any(|element| element.refresh.render_cache.is_some())
    }

    pub fn has_registry_subtree_cache(&self) -> bool {
        self.iter_nodes()
            .any(|element| element.refresh.registry_cache.is_some())
    }

    pub fn clear_render_refresh_dirty(&mut self) {
        self.iter_nodes_mut()
            .for_each(|element| element.refresh.clear_render());
    }

    pub fn clear_registry_refresh_dirty(&mut self) {
        self.iter_nodes_mut()
            .for_each(|element| element.refresh.clear_registry());
    }

    pub fn clear_refresh_dirty(&mut self) {
        self.iter_nodes_mut().for_each(|element| {
            element.refresh.clear_render();
            element.refresh.clear_registry();
        });
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

        let mut topology = TreeTopology::default();
        topology
            .nodes
            .resize(self.nodes.len(), NodeTopology::default());

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
            topology.nodes[parent_ix].children = child_ixs.clone();

            if !node.paint_children.is_empty() {
                topology.nodes[parent_ix].paint_children = node
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
            topology.nodes[parent_ix].nearby = nearby_ixs.clone();

            for child_ix in child_ixs {
                topology.nodes[child_ix].parent = Some(ParentLink::Child { parent: parent_ix });
            }

            for mount in nearby_ixs {
                topology.nodes[mount.ix].parent = Some(ParentLink::Nearby {
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
        self.root
    }

    pub fn root_id(&self) -> Option<NodeId> {
        self.root
            .and_then(|ix| self.id_of(ix))
            .or(self.pending_root_id)
    }

    pub fn set_root_ix(&mut self, ix: NodeIx) {
        assert!(self.get_ix(ix).is_some(), "root node not found at ix {ix}");
        self.root = Some(ix);
        self.pending_root_id = None;

        #[cfg(test)]
        self.mark_topology_dirty();
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
        {
            self.ensure_topology();
            self.topology
                .borrow()
                .nodes
                .get(ix)
                .and_then(|node| node.parent)
        }
        #[cfg(not(test))]
        {
            self.topology.nodes.get(ix).and_then(|node| node.parent)
        }
    }

    pub fn child_ixs(&self, ix: NodeIx) -> Vec<NodeIx> {
        #[cfg(test)]
        {
            self.ensure_topology();
            self.topology
                .borrow()
                .nodes
                .get(ix)
                .map(|node| node.children.clone())
                .unwrap_or_default()
        }
        #[cfg(not(test))]
        {
            self.topology
                .nodes
                .get(ix)
                .map(|node| node.children.clone())
                .unwrap_or_default()
        }
    }

    pub fn paint_child_ixs(&self, ix: NodeIx) -> Vec<NodeIx> {
        #[cfg(test)]
        let paint = {
            self.ensure_topology();
            self.topology
                .borrow()
                .nodes
                .get(ix)
                .map(|node| node.paint_children.clone())
                .unwrap_or_default()
        };
        #[cfg(not(test))]
        let paint = self
            .topology
            .nodes
            .get(ix)
            .map(|node| node.paint_children.clone())
            .unwrap_or_default();

        if paint.is_empty() {
            self.child_ixs(ix)
        } else {
            paint
        }
    }

    pub fn nearby_ixs(&self, ix: NodeIx) -> Vec<NearbyMountIx> {
        #[cfg(test)]
        {
            self.ensure_topology();
            self.topology
                .borrow()
                .nodes
                .get(ix)
                .map(|node| node.nearby.clone())
                .unwrap_or_default()
        }
        #[cfg(not(test))]
        {
            self.topology
                .nodes
                .get(ix)
                .map(|node| node.nearby.clone())
                .unwrap_or_default()
        }
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

    pub fn has_escape_nearby_mounts(&self) -> bool {
        #[cfg(test)]
        {
            self.ensure_topology();
            self.topology.borrow().nodes.iter().any(|node| {
                node.nearby
                    .iter()
                    .any(|mount| mount.slot != NearbySlot::BehindContent)
            })
        }
        #[cfg(not(test))]
        {
            self.topology.nodes.iter().any(|node| {
                node.nearby
                    .iter()
                    .any(|mount| mount.slot != NearbySlot::BehindContent)
            })
        }
    }

    pub(crate) fn subtree_affects_registry(&self, id: &NodeId) -> bool {
        self.ix_of(id)
            .is_some_and(|ix| self.subtree_affects_registry_ix(ix))
    }

    pub(crate) fn nearby_subtree_can_skip_layout(&self, id: &NodeId) -> bool {
        self.ix_of(id).is_some_and(|ix| {
            let Some(element) = self.get_ix(ix) else {
                return false;
            };

            element.spec.kind == ElementKind::None
                && self.child_ixs(ix).is_empty()
                && self.nearby_ixs(ix).is_empty()
                && !element_affects_registry(element)
        })
    }

    pub(crate) fn mark_nearby_subtree_layout_clean_for_refresh_only(&mut self, id: &NodeId) {
        let Some(ix) = self.ix_of(id) else {
            return;
        };

        if let Some(element) = self.get_ix_mut(ix) {
            element
                .layout
                .measured_frame
                .get_or_insert_with(Frame::default);
            element.layout.frame.get_or_insert_with(Frame::default);
            element.layout.measure_dirty = false;
            element.layout.measure_descendant_dirty = false;
            element.layout.resolve_dirty = false;
            element.layout.resolve_descendant_dirty = false;
        }
    }

    pub(crate) fn clear_registry_refresh_dirty_for_subtree(&mut self, id: &NodeId) {
        if let Some(ix) = self.ix_of(id) {
            self.clear_registry_refresh_dirty_for_subtree_ix(ix);
        }
    }

    fn clear_registry_refresh_dirty_for_subtree_ix(&mut self, ix: NodeIx) {
        if let Some(element) = self.get_ix_mut(ix) {
            element.refresh.clear_registry();
        }

        for child_ix in self.child_ixs(ix) {
            self.clear_registry_refresh_dirty_for_subtree_ix(child_ix);
        }
        for mount in self.nearby_ixs(ix) {
            self.clear_registry_refresh_dirty_for_subtree_ix(mount.ix);
        }
    }

    pub(crate) fn recompute_layout_descendant_dirty(&mut self) {
        if let Some(root_ix) = self.root_ix() {
            self.recompute_layout_descendant_dirty_ix(root_ix);
        }
    }

    fn recompute_layout_descendant_dirty_ix(&mut self, ix: NodeIx) -> (bool, bool) {
        let child_ixs = self.child_ixs(ix);
        let nearby_ixs: Vec<NodeIx> = self
            .nearby_ixs(ix)
            .into_iter()
            .map(|mount| mount.ix)
            .collect();

        let (child_measure_dirty, child_resolve_dirty) = child_ixs
            .into_iter()
            .chain(nearby_ixs)
            .map(|child_ix| self.recompute_layout_descendant_dirty_ix(child_ix))
            .fold(
                (false, false),
                |(measure_acc, resolve_acc), (measure, resolve)| {
                    (measure_acc || measure, resolve_acc || resolve)
                },
            );

        let Some(element) = self.get_ix_mut(ix) else {
            return (child_measure_dirty, child_resolve_dirty);
        };

        if element.layout.measure_dirty {
            element.layout.measure_descendant_dirty = false;
        } else {
            element.layout.measure_descendant_dirty = child_measure_dirty;
        }

        if element.layout.resolve_dirty {
            element.layout.resolve_descendant_dirty = false;
        } else {
            element.layout.resolve_descendant_dirty = child_resolve_dirty;
        }

        (
            element.layout.measure_dirty || element.layout.measure_descendant_dirty,
            element.layout.resolve_dirty || element.layout.resolve_descendant_dirty,
        )
    }

    fn subtree_affects_registry_ix(&self, ix: NodeIx) -> bool {
        let Some(element) = self.get_ix(ix) else {
            return false;
        };

        element_affects_registry(element)
            || self
                .child_ixs(ix)
                .into_iter()
                .any(|child_ix| self.subtree_affects_registry_ix(child_ix))
            || self.nearby_ixs(ix).into_iter().any(|mount| {
                mount.slot == NearbySlot::InFront || self.subtree_affects_registry_ix(mount.ix)
            })
    }

    fn nearby_registry_dirty_for_change(
        &self,
        old_mounts: &[NearbyMountIx],
        new_mounts: &[NearbyMountIx],
    ) -> bool {
        old_mounts.iter().chain(new_mounts.iter()).any(|mount| {
            mount.slot == NearbySlot::InFront || self.subtree_affects_registry_ix(mount.ix)
        })
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
        let element_id = element.id;
        if let Some(&ix) = self.id_to_ix.get(&element.id) {
            self.nodes[ix] = Some(element);
            self.mark_measure_dirty_ix(ix);
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

        if self.pending_root_id == Some(element_id) {
            let ix = self
                .ix_of(&element_id)
                .expect("newly inserted root id should resolve to ix");
            self.root = Some(ix);
            self.pending_root_id = None;
        }

        #[cfg(test)]
        self.mark_topology_dirty();
    }

    pub fn remove_node(&mut self, id: &NodeId) -> Option<Element> {
        let ix = self.id_to_ix.remove(id)?;
        let dirty_parent = self
            .parent_link_of(ix)
            .map(|parent_link| match parent_link {
                ParentLink::Child { parent } => parent,
                ParentLink::Nearby { host, .. } => host,
            });
        let removed = self.nodes.get_mut(ix).and_then(|slot| slot.take());
        self.free_list.push(ix);

        #[cfg(test)]
        if let Some(node) = self.topology.get_mut().nodes.get_mut(ix) {
            *node = NodeTopology::default();
        }

        #[cfg(not(test))]
        if let Some(node) = self.topology.nodes.get_mut(ix) {
            *node = NodeTopology::default();
        }

        #[cfg(test)]
        self.mark_topology_dirty();

        if let Some(parent_ix) = dirty_parent {
            self.mark_measure_dirty_ix(parent_ix);
        }
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
        let next = scale.max(f32::EPSILON);
        if (self.current_scale - next).abs() > f32::EPSILON {
            self.mark_all_measure_dirty();
        }
        self.current_scale = next;
    }

    pub fn mark_measure_dirty(&mut self, id: &NodeId) {
        if let Some(ix) = self.ix_of(id) {
            self.mark_measure_dirty_ix(ix);
        }
    }

    pub fn mark_measure_dirty_for_invalidation(
        &mut self,
        id: &NodeId,
        invalidation: TreeInvalidation,
    ) {
        self.mark_refresh_dirty_for_invalidation(id, invalidation);

        if invalidation == TreeInvalidation::Structure {
            self.mark_measure_dirty(id);
        } else if invalidation.requires_measure() {
            self.mark_measure_dirty_with_boundaries(id);
        } else if invalidation.requires_resolve() {
            self.mark_resolve_dirty(id);
        }
    }

    pub fn mark_resolve_dirty(&mut self, id: &NodeId) {
        if let Some(ix) = self.ix_of(id) {
            self.mark_resolve_dirty_ix(ix);
        }
    }

    pub fn mark_all_measure_dirty(&mut self) {
        self.iter_nodes_mut().for_each(|element| {
            element.layout.measure_dirty = true;
            element.layout.measure_descendant_dirty = false;
            element.layout.resolve_dirty = true;
            element.layout.resolve_descendant_dirty = false;
            element.refresh.render_dirty = true;
            element.refresh.render_descendant_dirty = false;
            element.refresh.render_cache = None;
            element.refresh.registry_dirty = true;
            element.refresh.registry_descendant_dirty = false;
            element.refresh.registry_cache = None;
        });
    }

    pub fn mark_all_resolve_dirty(&mut self) {
        self.iter_nodes_mut().for_each(|element| {
            element.layout.resolve_dirty = true;
            element.layout.resolve_descendant_dirty = false;
            element.refresh.render_dirty = true;
            element.refresh.render_descendant_dirty = false;
            element.refresh.render_cache = None;
            element.refresh.registry_dirty = true;
            element.refresh.registry_descendant_dirty = false;
            element.refresh.registry_cache = None;
        });
    }

    fn mark_measure_dirty_ix(&mut self, ix: NodeIx) {
        self.mark_render_and_registry_refresh_dirty_ix(ix);
        self.mark_dirty_ix(ix, true);
    }

    fn mark_measure_dirty_with_boundaries(&mut self, id: &NodeId) {
        if let Some(ix) = self.ix_of(id) {
            self.mark_measure_dirty_with_boundaries_ix(ix);
        }
    }

    fn mark_resolve_dirty_ix(&mut self, ix: NodeIx) {
        self.mark_render_and_registry_refresh_dirty_ix(ix);
        self.mark_dirty_ix(ix, false);
    }

    fn mark_render_refresh_dirty_ix(&mut self, ix: NodeIx) {
        self.mark_refresh_dirty_ix(ix, true, false);
    }

    fn mark_registry_refresh_dirty_ix(&mut self, ix: NodeIx) {
        self.mark_refresh_dirty_ix(ix, false, true);
    }

    fn mark_render_and_registry_refresh_dirty_ix(&mut self, ix: NodeIx) {
        self.mark_refresh_dirty_ix(ix, true, true);
    }

    fn mark_refresh_dirty_ix(&mut self, ix: NodeIx, render: bool, registry: bool) {
        let mut current_ix = Some(ix);
        let mut origin = true;

        while let Some(ix) = current_ix {
            if let Some(element) = self.get_ix_mut(ix) {
                if render {
                    element.refresh.render_cache = None;
                    if origin {
                        element.refresh.render_dirty = true;
                        element.refresh.render_descendant_dirty = false;
                    } else if !element.refresh.render_dirty {
                        element.refresh.render_descendant_dirty = true;
                    }
                }

                if registry {
                    element.refresh.registry_cache = None;
                    if origin {
                        element.refresh.registry_dirty = true;
                        element.refresh.registry_descendant_dirty = false;
                    } else if !element.refresh.registry_dirty {
                        element.refresh.registry_descendant_dirty = true;
                    }
                }
            }

            origin = false;
            current_ix = parent_ix_from_link(self.parent_link_of(ix));
        }
    }

    fn mark_nearby_topology_dirty_ix(
        &mut self,
        host_ix: NodeIx,
        newly_attached_mounts: &[NodeIx],
        registry_dirty: bool,
    ) {
        if registry_dirty {
            self.mark_render_and_registry_refresh_dirty_ix(host_ix);
        } else {
            self.mark_render_refresh_dirty_ix(host_ix);
        }

        for &mount_ix in newly_attached_mounts {
            self.mark_measure_dirty_local_ix(mount_ix);
            if registry_dirty {
                self.mark_render_and_registry_refresh_dirty_ix(mount_ix);
            } else {
                self.mark_render_refresh_dirty_ix(mount_ix);
            }
        }

        let mut current_ix = Some(host_ix);
        while let Some(ix) = current_ix {
            if let Some(element) = self.get_ix_mut(ix) {
                if !element.layout.measure_dirty {
                    element.layout.measure_descendant_dirty = true;
                }

                if !element.layout.resolve_dirty {
                    element.layout.resolve_descendant_dirty = true;
                }
            }

            current_ix = parent_ix_from_link(self.parent_link_of(ix));
        }
    }

    fn mark_measure_dirty_local_ix(&mut self, ix: NodeIx) {
        if let Some(element) = self.get_ix_mut(ix) {
            element.layout.measure_dirty = true;
            element.layout.measure_descendant_dirty = false;
            element.layout.resolve_dirty = true;
            element.layout.resolve_descendant_dirty = false;
        }
    }

    fn mark_dirty_ix(&mut self, ix: NodeIx, measure_dirty: bool) {
        let mut current_ix = Some(ix);

        while let Some(ix) = current_ix {
            if let Some(element) = self.get_ix_mut(ix) {
                if measure_dirty {
                    element.layout.measure_dirty = true;
                    element.layout.measure_descendant_dirty = false;
                }
                element.layout.resolve_dirty = true;
                element.layout.resolve_descendant_dirty = false;
            }

            current_ix = parent_ix_from_link(self.parent_link_of(ix));
        }
    }

    fn mark_measure_dirty_with_boundaries_ix(&mut self, ix: NodeIx) {
        if let Some(element) = self.get_ix_mut(ix) {
            element.layout.measure_dirty = true;
            element.layout.measure_descendant_dirty = false;
            element.layout.resolve_dirty = true;
            element.layout.resolve_descendant_dirty = false;
        }

        let mut current_link = self.parent_link_of(ix);
        let mut measure_propagates = true;

        while let Some(parent_link) = current_link {
            let parent_ix = parent_ix_from_link(Some(parent_link))
                .expect("parent link should always have a parent ix");
            let parent_depends_on_child_measure = match parent_link {
                ParentLink::Child { .. } if measure_propagates => self
                    .get_ix(parent_ix)
                    .map_or(true, parent_measure_depends_on_child_measure),
                ParentLink::Child { .. } => false,
                ParentLink::Nearby { .. } => true,
            };
            let mark_parent_measure_dirty = measure_propagates && parent_depends_on_child_measure;

            if let Some(parent) = self.get_ix_mut(parent_ix) {
                if mark_parent_measure_dirty {
                    parent.layout.measure_dirty = true;
                    parent.layout.measure_descendant_dirty = false;
                } else if !parent.layout.measure_dirty {
                    parent.layout.measure_descendant_dirty = true;
                }
                parent.layout.resolve_dirty = true;
                parent.layout.resolve_descendant_dirty = false;
            }

            measure_propagates = mark_parent_measure_dirty;
            current_link = self.parent_link_of(parent_ix);
        }
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
            .for_each(|element| element.lifecycle.mounted_at_revision = revision);
    }

    /// Replace this tree with a fully uploaded tree, advancing revision once.
    pub fn replace_with_uploaded(&mut self, mut uploaded: ElementTree) {
        let revision = self.revision.saturating_add(1);
        let layout_cache_stats_enabled = self.layout_cache_stats_enabled;
        uploaded.set_revision(revision);
        uploaded.next_ghost_seq = self.next_ghost_seq;
        uploaded.current_scale = self.current_scale;
        uploaded.set_layout_cache_stats_enabled(layout_cache_stats_enabled);
        uploaded.stamp_all_mounted_at_revision(revision);
        *self = uploaded;

        #[cfg(test)]
        self.mark_topology_dirty();
    }

    /// Returns true when the element was mounted after the provided revision.
    pub fn was_mounted_after(&self, id: &NodeId, revision: u64) -> bool {
        self.get(id)
            .is_some_and(|element| element.lifecycle.mounted_at_revision > revision)
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
        self.pending_root_id = None;
        self.id_to_ix.clear();
        self.nodes.clear();
        self.free_list.clear();
        self.reset_layout_cache_stats();
        self.reset_topology();
    }

    pub fn clear_root(&mut self) {
        self.root = None;
        self.pending_root_id = None;
    }

    pub fn set_root_id(&mut self, id: NodeId) {
        match self.ix_of(&id) {
            Some(ix) => self.set_root_ix(ix),
            None => {
                self.root = None;
                self.pending_root_id = Some(id);

                #[cfg(test)]
                self.mark_topology_dirty();
            }
        }
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
        self.ensure_topology();

        #[cfg(test)]
        if let Some(parent) = self.get_mut(parent_id) {
            parent.children = child_ids.clone();
            parent.paint_children = child_ids;
        }

        self.set_children_ix(parent_ix, child_ixs.clone());
        self.set_paint_children_ix(parent_ix, child_ixs);

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
        self.ensure_topology();

        #[cfg(test)]
        if let Some(parent) = self.get_mut(parent_id) {
            parent.paint_children = child_ids;
        }

        self.set_paint_children_ix(parent_ix, child_ixs);

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
        self.ensure_topology();

        #[cfg(test)]
        if let Some(host) = self.get_mut(host_id) {
            host.nearby.set_mounts(mounts);
        }

        self.set_nearby_ixs(host_ix, nearby_ixs);

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
                        match child.lifecycle.ghost_attachment.as_ref() {
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
                        match nearby.lifecycle.ghost_attachment.as_ref() {
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
        #[cfg(test)]
        {
            let topology = self.topology.get_mut();
            while topology.nodes.len() <= ix {
                topology.nodes.push(NodeTopology::default());
            }
        }

        #[cfg(not(test))]
        while self.topology.nodes.len() <= ix {
            self.topology.nodes.push(NodeTopology::default());
        }
    }

    #[cfg(test)]
    fn reset_topology(&mut self) {
        *self.topology.borrow_mut() = TreeTopology::default();
        self.topology_dirty.set(false);
    }

    #[cfg(not(test))]
    fn reset_topology(&mut self) {
        self.topology = TreeTopology::default();
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
        for &child_ix in &child_ixs {
            self.ensure_topology_capacity(child_ix);
        }

        let changed = {
            #[cfg(test)]
            let topology = self.topology.get_mut();
            #[cfg(not(test))]
            let topology = &mut self.topology;

            let changed = topology.nodes[parent_ix].children != child_ixs;

            for old_child_ix in std::mem::take(&mut topology.nodes[parent_ix].children) {
                if matches!(topology.nodes[old_child_ix].parent, Some(ParentLink::Child { parent }) if parent == parent_ix)
                {
                    topology.nodes[old_child_ix].parent = None;
                }
            }

            for &child_ix in &child_ixs {
                topology.nodes[child_ix].parent = Some(ParentLink::Child { parent: parent_ix });
            }

            topology.nodes[parent_ix].children = child_ixs;
            changed
        };

        if changed {
            self.bump_children_version(parent_ix);
        }

        self.mark_measure_dirty_ix(parent_ix);
    }

    fn set_paint_children_ix(&mut self, parent_ix: NodeIx, child_ixs: Vec<NodeIx>) {
        self.ensure_topology_capacity(parent_ix);

        let changed = {
            #[cfg(test)]
            let topology = self.topology.get_mut();
            #[cfg(not(test))]
            let topology = &mut self.topology;

            let changed = topology.nodes[parent_ix].paint_children != child_ixs;
            topology.nodes[parent_ix].paint_children = child_ixs;
            changed
        };

        if changed {
            self.bump_paint_children_version(parent_ix);
        }
    }

    fn set_nearby_ixs(&mut self, host_ix: NodeIx, mounts: Vec<NearbyMountIx>) {
        self.ensure_topology_capacity(host_ix);
        for mount in &mounts {
            self.ensure_topology_capacity(mount.ix);
        }

        let old_mounts = self.nearby_ixs(host_ix);
        let changed = old_mounts != mounts;
        let registry_dirty = changed && self.nearby_registry_dirty_for_change(&old_mounts, &mounts);
        let newly_attached_mounts: Vec<NodeIx> = mounts
            .iter()
            .filter(|mount| !old_mounts.iter().any(|old| old.ix == mount.ix))
            .map(|mount| mount.ix)
            .collect();

        {
            #[cfg(test)]
            let topology = self.topology.get_mut();
            #[cfg(not(test))]
            let topology = &mut self.topology;

            for old_mount in std::mem::take(&mut topology.nodes[host_ix].nearby) {
                if matches!(topology.nodes[old_mount.ix].parent, Some(ParentLink::Nearby { host, .. }) if host == host_ix)
                {
                    topology.nodes[old_mount.ix].parent = None;
                }
            }

            for mount in &mounts {
                topology.nodes[mount.ix].parent = Some(ParentLink::Nearby {
                    host: host_ix,
                    slot: mount.slot,
                });
            }

            topology.nodes[host_ix].nearby = mounts;
        }

        if changed {
            self.bump_nearby_version(host_ix);
            self.mark_nearby_topology_dirty_ix(host_ix, &newly_attached_mounts, registry_dirty);
        }
    }

    pub fn topology_dependency_key_for(&self, id: &NodeId) -> TopologyDependencyKey {
        self.ix_of(id)
            .map(|ix| self.topology_dependency_key_ix(ix))
            .unwrap_or_default()
    }

    pub fn topology_dependency_key_ix(&self, ix: NodeIx) -> TopologyDependencyKey {
        self.ensure_topology();

        let versions = self
            .get_ix(ix)
            .map(|element| element.layout.topology_versions)
            .unwrap_or_default();

        let (child_count, _, nearby_count) = self.topology_dependency_counts(ix);

        TopologyDependencyKey {
            children_version: versions.children,
            nearby_version: versions.nearby,
            child_count,
            nearby_count,
        }
    }

    pub fn measure_topology_dependency_key_for(&self, id: &NodeId) -> TopologyDependencyKey {
        self.ix_of(id)
            .map(|ix| self.measure_topology_dependency_key_ix(ix))
            .unwrap_or_default()
    }

    pub fn measure_topology_dependency_key_ix(&self, ix: NodeIx) -> TopologyDependencyKey {
        self.ensure_topology();

        let versions = self
            .get_ix(ix)
            .map(|element| element.layout.topology_versions)
            .unwrap_or_default();
        let (child_count, _, _) = self.topology_dependency_counts(ix);

        TopologyDependencyKey {
            children_version: versions.children,
            nearby_version: 0,
            child_count,
            nearby_count: 0,
        }
    }

    pub fn render_topology_dependency_key_ix(&self, ix: NodeIx) -> RenderTopologyDependencyKey {
        self.ensure_topology();

        let versions = self
            .get_ix(ix)
            .map(|element| element.layout.topology_versions)
            .unwrap_or_default();
        let (child_count, paint_child_count, nearby_count) = self.topology_dependency_counts(ix);

        RenderTopologyDependencyKey {
            children_version: versions.children,
            paint_children_version: versions.paint_children,
            nearby_version: versions.nearby,
            child_count,
            paint_child_count,
            nearby_count,
        }
    }

    fn topology_dependency_counts(&self, ix: NodeIx) -> (usize, usize, usize) {
        #[cfg(test)]
        {
            let topology = self.topology.borrow();
            topology
                .nodes
                .get(ix)
                .map(|node| {
                    (
                        node.children.len(),
                        node.paint_children.len(),
                        node.nearby.len(),
                    )
                })
                .unwrap_or_default()
        }

        #[cfg(not(test))]
        {
            self.topology
                .nodes
                .get(ix)
                .map(|node| {
                    (
                        node.children.len(),
                        node.paint_children.len(),
                        node.nearby.len(),
                    )
                })
                .unwrap_or_default()
        }
    }

    fn bump_children_version(&mut self, ix: NodeIx) {
        if let Some(element) = self.get_ix_mut(ix) {
            element.layout.topology_versions.children =
                element.layout.topology_versions.children.saturating_add(1);
        }
    }

    fn bump_paint_children_version(&mut self, ix: NodeIx) {
        if let Some(element) = self.get_ix_mut(ix) {
            element.layout.topology_versions.paint_children = element
                .layout
                .topology_versions
                .paint_children
                .saturating_add(1);
        }
    }

    fn bump_nearby_version(&mut self, ix: NodeIx) {
        if let Some(element) = self.get_ix_mut(ix) {
            element.layout.topology_versions.nearby =
                element.layout.topology_versions.nearby.saturating_add(1);
        }
    }

    /// Apply scroll delta to an element. Returns the required invalidation.
    pub fn apply_scroll(&mut self, id: &NodeId, dx: f32, dy: f32) -> TreeInvalidation {
        let mut invalidation = TreeInvalidation::None;
        if dx != 0.0 {
            invalidation.add(self.apply_scroll_x(id, dx));
        }
        if dy != 0.0 {
            invalidation.add(self.apply_scroll_y(id, dy));
        }
        invalidation
    }

    /// Apply horizontal scroll delta to an element. Returns the required invalidation.
    pub fn apply_scroll_x(&mut self, id: &NodeId, dx: f32) -> TreeInvalidation {
        self.apply_scroll_axis(id, dx, ScrollAxis::X)
    }

    /// Apply vertical scroll delta to an element. Returns the required invalidation.
    pub fn apply_scroll_y(&mut self, id: &NodeId, dy: f32) -> TreeInvalidation {
        self.apply_scroll_axis(id, dy, ScrollAxis::Y)
    }

    /// Set horizontal scrollbar thumb hover state. Returns the required invalidation.
    pub fn set_scrollbar_x_hover(&mut self, id: &NodeId, hovered: bool) -> TreeInvalidation {
        self.set_scrollbar_hover_axis(id, ScrollbarHoverAxis::X, hovered)
    }

    /// Set vertical scrollbar thumb hover state. Returns the required invalidation.
    pub fn set_scrollbar_y_hover(&mut self, id: &NodeId, hovered: bool) -> TreeInvalidation {
        self.set_scrollbar_hover_axis(id, ScrollbarHoverAxis::Y, hovered)
    }

    /// Set mouse_over active state. Returns the required invalidation.
    pub fn set_mouse_over_active(&mut self, id: &NodeId, active: bool) -> TreeInvalidation {
        let invalidation = {
            let Some(element) = self.get_mut(id) else {
                return TreeInvalidation::None;
            };

            if !supports_mouse_over_tracking(&element.layout.effective) {
                let changed = element.runtime.mouse_over_active;
                element.runtime.mouse_over_active = false;
                TreeInvalidation::when_changed(changed, TreeInvalidation::Registry)
            } else {
                let current = element.runtime.mouse_over_active;
                if current == active {
                    TreeInvalidation::None
                } else {
                    element.runtime.mouse_over_active = active;
                    classify_interaction_style(element.layout.effective.mouse_over.as_ref())
                        .join(TreeInvalidation::Registry)
                }
            }
        };

        self.mark_measure_dirty_for_invalidation(id, invalidation);
        if invalidation.is_dirty() {
            self.mark_registry_refresh_dirty(id);
        }
        invalidation
    }

    /// Set mouse_down active state. Returns the required invalidation.
    pub fn set_mouse_down_active(&mut self, id: &NodeId, active: bool) -> TreeInvalidation {
        let invalidation = {
            let Some(element) = self.get_mut(id) else {
                return TreeInvalidation::None;
            };

            if element.layout.effective.mouse_down.is_none() {
                let changed = element.runtime.mouse_down_active;
                element.runtime.mouse_down_active = false;
                TreeInvalidation::when_changed(changed, TreeInvalidation::Registry)
            } else {
                let current = element.runtime.mouse_down_active;
                if current == active {
                    TreeInvalidation::None
                } else {
                    element.runtime.mouse_down_active = active;
                    classify_interaction_style(element.layout.effective.mouse_down.as_ref())
                        .join(TreeInvalidation::Registry)
                }
            }
        };

        self.mark_measure_dirty_for_invalidation(id, invalidation);
        if invalidation.is_dirty() {
            self.mark_registry_refresh_dirty(id);
        }
        invalidation
    }

    /// Set focused active state. Returns the required invalidation.
    pub fn set_focused_active(&mut self, id: &NodeId, active: bool) -> TreeInvalidation {
        let invalidation = {
            let Some(element) = self.get_mut(id) else {
                return TreeInvalidation::None;
            };

            let current = element.runtime.focused_active;
            if current == active {
                TreeInvalidation::None
            } else {
                element.runtime.focused_active = active;
                classify_interaction_style(element.layout.effective.focused.as_ref())
                    .join(TreeInvalidation::Registry)
            }
        };

        self.mark_measure_dirty_for_invalidation(id, invalidation);
        if invalidation.is_dirty() {
            self.mark_registry_refresh_dirty(id);
        }
        invalidation
    }

    pub fn set_text_input_content(&mut self, id: &NodeId, content: String) -> TreeInvalidation {
        let changed = {
            let Some(element) = self.get_mut(id) else {
                return TreeInvalidation::None;
            };

            if !element.spec.kind.is_text_input_family() {
                return TreeInvalidation::None;
            }

            let prev_base = element.spec.declared.content.as_deref().unwrap_or("");
            let prev_attrs = element.layout.effective.content.as_deref().unwrap_or("");
            let mut changed = prev_base != content || prev_attrs != content;

            element.spec.declared.content = Some(content.clone());
            element.layout.effective.content = Some(content.clone());

            if element.runtime.patch_content.take().is_some() {
                changed = true;
            }

            if element.runtime.text_input_content_origin != TextInputContentOrigin::Event {
                element.runtime.text_input_content_origin = TextInputContentOrigin::Event;
                changed = true;
            }

            let len = text_char_len(&content);
            if let Some(cursor) = element.runtime.text_input_cursor {
                let clamped = cursor.min(len);
                if clamped != cursor {
                    element.runtime.text_input_cursor = Some(clamped);
                    changed = true;
                }
            }

            if let Some(anchor) = element.runtime.text_input_selection_anchor {
                let clamped = anchor.min(len);
                let cursor = element.runtime.text_input_cursor.unwrap_or(len);
                let next = if clamped == cursor {
                    None
                } else {
                    Some(clamped)
                };
                if next != element.runtime.text_input_selection_anchor {
                    element.runtime.text_input_selection_anchor = next;
                    changed = true;
                }
            }

            let had_preedit = element.runtime.text_input_preedit.take().is_some();
            let had_preedit_cursor = element.runtime.text_input_preedit_cursor.take().is_some();
            if had_preedit || had_preedit_cursor {
                changed = true;
            }

            element.normalize_extracted_state();

            changed
        };

        if changed {
            self.mark_measure_dirty(id);
            TreeInvalidation::Measure
        } else {
            TreeInvalidation::None
        }
    }

    pub fn set_text_input_runtime(
        &mut self,
        id: &NodeId,
        focused: bool,
        cursor: Option<u32>,
        selection_anchor: Option<u32>,
        preedit: Option<String>,
        preedit_cursor: Option<(u32, u32)>,
    ) -> TreeInvalidation {
        let Some(element) = self.get_mut(id) else {
            return TreeInvalidation::None;
        };

        if !element.spec.kind.is_text_input_family() {
            return TreeInvalidation::None;
        }

        let content = element.spec.declared.content.as_deref().unwrap_or("");
        let len = text_char_len(content);

        let mut next_cursor = cursor.or(element.runtime.text_input_cursor);
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

        if element.runtime.text_input_focused != focused {
            element.runtime.text_input_focused = focused;
            changed = true;
        }

        if element.runtime.text_input_cursor != next_cursor {
            element.runtime.text_input_cursor = next_cursor;
            changed = true;
        }

        if element.runtime.text_input_selection_anchor != next_anchor {
            element.runtime.text_input_selection_anchor = next_anchor;
            changed = true;
        }

        if element.runtime.text_input_preedit != next_preedit {
            element.runtime.text_input_preedit = next_preedit;
            changed = true;
        }

        if element.runtime.text_input_preedit_cursor != next_preedit_cursor {
            element.runtime.text_input_preedit_cursor = next_preedit_cursor;
            changed = true;
        }

        element.normalize_extracted_state();

        let invalidation = TreeInvalidation::when_changed(changed, TreeInvalidation::Paint);
        if invalidation.is_dirty() {
            self.mark_render_and_registry_refresh_dirty(id);
        }
        invalidation
    }

    fn apply_scroll_axis(&mut self, id: &NodeId, delta: f32, axis: ScrollAxis) -> TreeInvalidation {
        let Some(element) = self.get_mut(id) else {
            return TreeInvalidation::None;
        };
        let Some(frame) = element.layout.frame else {
            return TreeInvalidation::None;
        };

        let (current, max) = match axis {
            ScrollAxis::X => (
                element.layout.scroll_x,
                (frame.content_width - frame.width).max(0.0),
            ),
            ScrollAxis::Y => (
                element.layout.scroll_y,
                (frame.content_height - frame.height).max(0.0),
            ),
        };
        let next = (current - delta).clamp(0.0, max);

        if (next - current).abs() < f32::EPSILON {
            return TreeInvalidation::None;
        }

        match axis {
            ScrollAxis::X => element.layout.scroll_x = next,
            ScrollAxis::Y => element.layout.scroll_y = next,
        }
        self.mark_render_and_registry_refresh_dirty(id);
        TreeInvalidation::Paint
    }

    fn set_scrollbar_hover_axis(
        &mut self,
        id: &NodeId,
        axis: ScrollbarHoverAxis,
        hovered: bool,
    ) -> TreeInvalidation {
        let Some(element) = self.get_mut(id) else {
            return TreeInvalidation::None;
        };

        let current = element.runtime.scrollbar_hover_axis;
        let axis_enabled = match axis {
            ScrollbarHoverAxis::X => element.layout.effective.scrollbar_x.unwrap_or(false),
            ScrollbarHoverAxis::Y => element.layout.effective.scrollbar_y.unwrap_or(false),
        };

        let changed = if hovered {
            if !axis_enabled || current == Some(axis) {
                false
            } else {
                element.runtime.scrollbar_hover_axis = Some(axis);
                true
            }
        } else if current == Some(axis) {
            element.runtime.scrollbar_hover_axis = None;
            true
        } else {
            false
        };

        let invalidation = TreeInvalidation::when_changed(changed, TreeInvalidation::Paint);
        if invalidation.is_dirty() {
            self.mark_render_and_registry_refresh_dirty(id);
        }
        invalidation
    }
}

fn parent_ix_from_link(parent_link: Option<ParentLink>) -> Option<NodeIx> {
    parent_link.map(|parent_link| match parent_link {
        ParentLink::Child { parent } => parent,
        ParentLink::Nearby { host, .. } => host,
    })
}

fn element_affects_registry(element: &Element) -> bool {
    let attrs = &element.spec.declared;
    element.spec.kind.is_text_input_family()
        || element.runtime.text_input_focused
        || element.runtime.mouse_over_active
        || element.runtime.mouse_down_active
        || element.runtime.focused_active
        || element.runtime.scrollbar_hover_axis.is_some()
        || attrs.on_click.unwrap_or(false)
        || attrs.on_mouse_down.unwrap_or(false)
        || attrs.on_mouse_up.unwrap_or(false)
        || attrs.on_mouse_enter.unwrap_or(false)
        || attrs.on_mouse_leave.unwrap_or(false)
        || attrs.on_mouse_move.unwrap_or(false)
        || attrs.on_press.unwrap_or(false)
        || attrs.on_swipe_up.unwrap_or(false)
        || attrs.on_swipe_down.unwrap_or(false)
        || attrs.on_swipe_left.unwrap_or(false)
        || attrs.on_swipe_right.unwrap_or(false)
        || attrs.on_change.unwrap_or(false)
        || attrs.on_focus.unwrap_or(false)
        || attrs.on_blur.unwrap_or(false)
        || attrs.focus_on_mount.unwrap_or(false)
        || attrs.on_key_down.is_some()
        || attrs.on_key_up.is_some()
        || attrs.on_key_press.is_some()
        || attrs.virtual_key.is_some()
        || attrs.mouse_over.is_some()
        || attrs.mouse_down.is_some()
        || attrs.scrollbar_x.unwrap_or(false)
        || attrs.scrollbar_y.unwrap_or(false)
        || attrs.ghost_scrollbar_x.unwrap_or(false)
        || attrs.ghost_scrollbar_y.unwrap_or(false)
}

fn parent_measure_depends_on_child_measure(parent: &Element) -> bool {
    !matches!(parent.spec.kind, ElementKind::El | ElementKind::None)
        || !measure_length_is_child_independent(parent.layout.effective.width.as_ref())
        || !measure_length_is_child_independent(parent.layout.effective.height.as_ref())
}

fn measure_length_is_child_independent(length: Option<&Length>) -> bool {
    match length {
        Some(Length::Px(_)) => true,
        Some(Length::Minimum(_, inner)) | Some(Length::Maximum(_, inner)) => {
            measure_length_is_child_independent(Some(inner))
        }
        Some(Length::Content) | Some(Length::Fill) | Some(Length::FillWeighted(_)) | None => false,
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
    fn topology_versions_bump_for_child_order_changes_but_not_noop_writes() {
        let parent_id = NodeId::from_term_bytes(vec![1]);
        let first_id = NodeId::from_term_bytes(vec![2]);
        let second_id = NodeId::from_term_bytes(vec![3]);

        let mut tree = ElementTree::new();
        tree.insert(Element::with_attrs(
            parent_id,
            ElementKind::Column,
            Vec::new(),
            Attrs::default(),
        ));
        tree.insert(Element::with_attrs(
            first_id,
            ElementKind::Text,
            Vec::new(),
            Attrs::default(),
        ));
        tree.insert(Element::with_attrs(
            second_id,
            ElementKind::Text,
            Vec::new(),
            Attrs::default(),
        ));
        tree.set_root_id(parent_id);

        tree.set_children(&parent_id, vec![first_id, second_id])
            .unwrap();
        let first_key = tree.topology_dependency_key_for(&parent_id);
        assert_eq!(first_key.child_count, 2);
        assert_eq!(first_key.nearby_count, 0);

        tree.set_children(&parent_id, vec![first_id, second_id])
            .unwrap();
        assert_eq!(tree.topology_dependency_key_for(&parent_id), first_key);

        tree.set_children(&parent_id, vec![second_id, first_id])
            .unwrap();
        let reordered_key = tree.topology_dependency_key_for(&parent_id);
        assert_eq!(reordered_key.child_count, 2);
        assert!(reordered_key.children_version > first_key.children_version);
    }

    #[test]
    fn topology_versions_bump_for_nearby_slot_changes_but_not_noop_writes() {
        let host_id = NodeId::from_term_bytes(vec![11]);
        let nearby_id = NodeId::from_term_bytes(vec![12]);

        let mut tree = ElementTree::new();
        tree.insert(Element::with_attrs(
            host_id,
            ElementKind::El,
            Vec::new(),
            Attrs::default(),
        ));
        tree.insert(Element::with_attrs(
            nearby_id,
            ElementKind::El,
            Vec::new(),
            Attrs::default(),
        ));
        tree.set_root_id(host_id);

        tree.set_nearby_mounts(
            &host_id,
            vec![NearbyMount {
                slot: NearbySlot::Below,
                id: nearby_id,
            }],
        )
        .unwrap();
        let first_key = tree.topology_dependency_key_for(&host_id);
        assert_eq!(first_key.child_count, 0);
        assert_eq!(first_key.nearby_count, 1);

        tree.set_nearby_mounts(
            &host_id,
            vec![NearbyMount {
                slot: NearbySlot::Below,
                id: nearby_id,
            }],
        )
        .unwrap();
        assert_eq!(tree.topology_dependency_key_for(&host_id), first_key);

        tree.set_nearby_mounts(
            &host_id,
            vec![NearbyMount {
                slot: NearbySlot::Above,
                id: nearby_id,
            }],
        )
        .unwrap();
        let changed_key = tree.topology_dependency_key_for(&host_id);
        assert_eq!(changed_key.nearby_count, 1);
        assert!(changed_key.nearby_version > first_key.nearby_version);
    }

    #[test]
    fn refresh_damage_tracks_render_registry_and_descendant_paths() {
        let parent_id = NodeId::from_term_bytes(vec![21]);
        let child_id = NodeId::from_term_bytes(vec![22]);

        let mut tree = ElementTree::new();
        tree.insert(Element::with_attrs(
            parent_id,
            ElementKind::Column,
            Vec::new(),
            Attrs::default(),
        ));
        tree.insert(Element::with_attrs(
            child_id,
            ElementKind::Text,
            Vec::new(),
            Attrs::default(),
        ));
        tree.set_root_id(parent_id);
        tree.set_children(&parent_id, vec![child_id]).unwrap();
        tree.clear_refresh_dirty();

        tree.mark_refresh_dirty_for_invalidation(&child_id, TreeInvalidation::Paint);
        assert!(tree.has_render_refresh_damage());
        assert!(!tree.has_registry_refresh_damage());
        assert!(tree.get(&child_id).unwrap().refresh.render_dirty);
        assert!(
            tree.get(&parent_id)
                .unwrap()
                .refresh
                .render_descendant_dirty
        );

        tree.clear_refresh_dirty();
        tree.mark_refresh_dirty_for_invalidation(&child_id, TreeInvalidation::Registry);
        assert!(!tree.has_render_refresh_damage());
        assert!(tree.has_registry_refresh_damage());
        assert!(tree.get(&child_id).unwrap().refresh.registry_dirty);
        assert!(
            tree.get(&parent_id)
                .unwrap()
                .refresh
                .registry_descendant_dirty
        );

        tree.clear_refresh_dirty();
        tree.mark_refresh_dirty_for_invalidation(&child_id, TreeInvalidation::Measure);
        assert!(tree.has_render_refresh_damage());
        assert!(tree.has_registry_refresh_damage());
        assert!(tree.get(&child_id).unwrap().refresh.render_dirty);
        assert!(tree.get(&child_id).unwrap().refresh.registry_dirty);
    }

    #[test]
    fn test_scrollbar_hover_axis_is_tri_state() {
        let id = NodeId::from_term_bytes(vec![1]);
        let mut attrs = Attrs::default();
        attrs.scrollbar_x = Some(true);
        attrs.scrollbar_y = Some(true);
        let mut element = Element::with_attrs(id.clone(), ElementKind::El, Vec::new(), attrs);
        element.layout.frame = Some(Frame {
            x: 0.0,
            y: 0.0,
            width: 100.0,
            height: 100.0,
            content_width: 200.0,
            content_height: 200.0,
        });

        let mut tree = ElementTree::new();
        tree.insert(element);
        tree.set_root_id(id);

        assert!(tree.set_scrollbar_x_hover(&id, true).is_dirty());
        assert_eq!(
            tree.get(&id).unwrap().runtime.scrollbar_hover_axis,
            Some(ScrollbarHoverAxis::X)
        );

        assert!(tree.set_scrollbar_y_hover(&id, true).is_dirty());
        assert_eq!(
            tree.get(&id).unwrap().runtime.scrollbar_hover_axis,
            Some(ScrollbarHoverAxis::Y)
        );

        assert!(tree.set_scrollbar_x_hover(&id, false).is_none());
        assert!(tree.set_scrollbar_y_hover(&id, false).is_dirty());
        assert_eq!(tree.get(&id).unwrap().runtime.scrollbar_hover_axis, None);
    }

    #[test]
    fn test_apply_scroll_axis_helpers() {
        let id = NodeId::from_term_bytes(vec![1]);
        let mut attrs = Attrs::default();
        attrs.scrollbar_x = Some(true);
        attrs.scrollbar_y = Some(true);
        let mut element = Element::with_attrs(id.clone(), ElementKind::El, Vec::new(), attrs);
        element.layout.frame = Some(Frame {
            x: 0.0,
            y: 0.0,
            width: 100.0,
            height: 100.0,
            content_width: 200.0,
            content_height: 200.0,
        });

        let mut tree = ElementTree::new();
        tree.insert(element);
        tree.set_root_id(id);

        assert!(tree.apply_scroll_x(&id, -30.0).is_dirty());
        assert_eq!(tree.get(&id).unwrap().layout.scroll_x, 30.0);

        assert!(tree.apply_scroll_y(&id, -25.0).is_dirty());
        assert_eq!(tree.get(&id).unwrap().layout.scroll_y, 25.0);

        assert!(tree.apply_scroll_x(&id, 0.0).is_none());
        assert!(tree.apply_scroll_y(&id, 0.0).is_none());
    }

    #[test]
    fn test_apply_scroll_axis_helpers_clamp_to_bounds() {
        let id = NodeId::from_term_bytes(vec![1]);
        let mut attrs = Attrs::default();
        attrs.scrollbar_x = Some(true);
        attrs.scrollbar_y = Some(true);
        let mut element = Element::with_attrs(id.clone(), ElementKind::El, Vec::new(), attrs);
        element.layout.frame = Some(Frame {
            x: 0.0,
            y: 0.0,
            width: 100.0,
            height: 100.0,
            content_width: 180.0,
            content_height: 170.0,
        });

        let mut tree = ElementTree::new();
        tree.insert(element);
        tree.set_root_id(id);

        assert!(tree.apply_scroll_x(&id, -500.0).is_dirty());
        assert!(tree.apply_scroll_y(&id, -500.0).is_dirty());
        assert_eq!(tree.get(&id).unwrap().layout.scroll_x, 80.0);
        assert_eq!(tree.get(&id).unwrap().layout.scroll_y, 70.0);

        assert!(tree.apply_scroll_x(&id, 500.0).is_dirty());
        assert!(tree.apply_scroll_y(&id, 500.0).is_dirty());
        assert_eq!(tree.get(&id).unwrap().layout.scroll_x, 0.0);
        assert_eq!(tree.get(&id).unwrap().layout.scroll_y, 0.0);
    }

    #[test]
    fn test_set_scrollbar_hover_axis_noop_when_axis_disabled() {
        let id = NodeId::from_term_bytes(vec![1]);
        let attrs = Attrs::default();
        let mut element = Element::with_attrs(id.clone(), ElementKind::El, Vec::new(), attrs);
        element.layout.frame = Some(Frame {
            x: 0.0,
            y: 0.0,
            width: 100.0,
            height: 100.0,
            content_width: 100.0,
            content_height: 100.0,
        });

        let mut tree = ElementTree::new();
        tree.insert(element);
        tree.set_root_id(id);

        assert!(tree.set_scrollbar_x_hover(&id, true).is_none());
        assert!(tree.set_scrollbar_y_hover(&id, true).is_none());
        assert_eq!(tree.get(&id).unwrap().runtime.scrollbar_hover_axis, None);
    }

    #[test]
    fn test_set_mouse_over_active_requires_mouse_over_attrs() {
        let id = NodeId::from_term_bytes(vec![1]);
        let mut element =
            Element::with_attrs(id.clone(), ElementKind::El, Vec::new(), Attrs::default());
        element.layout.frame = Some(Frame {
            x: 0.0,
            y: 0.0,
            width: 100.0,
            height: 100.0,
            content_width: 100.0,
            content_height: 100.0,
        });

        let mut tree = ElementTree::new();
        tree.insert(element);
        tree.set_root_id(id);

        assert!(tree.set_mouse_over_active(&id, true).is_none());
        assert!(!tree.get(&id).unwrap().runtime.mouse_over_active);
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
        element.layout.frame = Some(Frame {
            x: 0.0,
            y: 0.0,
            width: 100.0,
            height: 100.0,
            content_width: 100.0,
            content_height: 100.0,
        });

        let mut tree = ElementTree::new();
        tree.insert(element);
        tree.set_root_id(id);

        assert!(tree.set_mouse_over_active(&id, true).is_dirty());
        assert!(tree.get(&id).unwrap().runtime.mouse_over_active);

        assert!(tree.set_mouse_over_active(&id, true).is_none());

        assert!(tree.set_mouse_over_active(&id, false).is_dirty());
        assert!(!tree.get(&id).unwrap().runtime.mouse_over_active);
    }

    #[test]
    fn test_set_mouse_over_active_tracks_event_only_hover() {
        let id = NodeId::from_term_bytes(vec![2]);
        let mut attrs = Attrs::default();
        attrs.on_mouse_enter = Some(true);
        attrs.on_mouse_leave = Some(true);
        let mut element = Element::with_attrs(id.clone(), ElementKind::El, Vec::new(), attrs);
        element.layout.frame = Some(Frame {
            x: 0.0,
            y: 0.0,
            width: 100.0,
            height: 100.0,
            content_width: 100.0,
            content_height: 100.0,
        });

        let mut tree = ElementTree::new();
        tree.insert(element);
        tree.set_root_id(id);

        assert!(tree.set_mouse_over_active(&id, true).is_dirty());
        assert!(tree.get(&id).unwrap().runtime.mouse_over_active);

        assert!(tree.set_mouse_over_active(&id, true).is_none());

        assert!(tree.set_mouse_over_active(&id, false).is_dirty());
        assert!(!tree.get(&id).unwrap().runtime.mouse_over_active);
    }

    #[test]
    fn test_set_mouse_down_active_requires_mouse_down_attrs() {
        let id = NodeId::from_term_bytes(vec![11]);
        let mut element =
            Element::with_attrs(id.clone(), ElementKind::El, Vec::new(), Attrs::default());
        element.layout.frame = Some(Frame {
            x: 0.0,
            y: 0.0,
            width: 100.0,
            height: 100.0,
            content_width: 100.0,
            content_height: 100.0,
        });

        let mut tree = ElementTree::new();
        tree.insert(element);
        tree.set_root_id(id);

        assert!(tree.set_mouse_down_active(&id, true).is_none());
        assert!(!tree.get(&id).unwrap().runtime.mouse_down_active);
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
        element.layout.frame = Some(Frame {
            x: 0.0,
            y: 0.0,
            width: 100.0,
            height: 100.0,
            content_width: 100.0,
            content_height: 100.0,
        });

        let mut tree = ElementTree::new();
        tree.insert(element);
        tree.set_root_id(id.clone());

        assert!(tree.set_mouse_down_active(&id, true).is_dirty());
        assert!(tree.get(&id).unwrap().runtime.mouse_down_active);

        assert!(tree.set_mouse_down_active(&id, true).is_none());

        assert!(tree.set_mouse_down_active(&id, false).is_dirty());
        assert!(!tree.get(&id).unwrap().runtime.mouse_down_active);
    }

    #[test]
    fn test_set_focused_active_toggles_state() {
        let id = NodeId::from_term_bytes(vec![13]);
        let mut element =
            Element::with_attrs(id.clone(), ElementKind::El, Vec::new(), Attrs::default());
        element.layout.frame = Some(Frame {
            x: 0.0,
            y: 0.0,
            width: 100.0,
            height: 40.0,
            content_width: 100.0,
            content_height: 40.0,
        });

        let mut tree = ElementTree::new();
        tree.insert(element);
        tree.set_root_id(id.clone());

        assert!(tree.set_focused_active(&id, true).is_dirty());
        assert!(tree.get(&id).unwrap().runtime.focused_active);

        assert!(tree.set_focused_active(&id, true).is_none());

        assert!(tree.set_focused_active(&id, false).is_dirty());
        assert!(!tree.get(&id).unwrap().runtime.focused_active);
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
        element.layout.frame = Some(Frame {
            x: 0.0,
            y: 0.0,
            width: 120.0,
            height: 24.0,
            content_width: 120.0,
            content_height: 24.0,
        });

        let mut tree = ElementTree::new();
        tree.insert(element);
        tree.set_root_id(id.clone());

        assert!(
            tree.set_text_input_content(&id, "hey".to_string())
                .is_dirty()
        );
        let node = tree.get(&id).unwrap();
        assert_eq!(node.spec.declared.content.as_deref(), Some("hey"));
        assert_eq!(node.layout.effective.content.as_deref(), Some("hey"));
        assert_eq!(
            node.runtime.text_input_content_origin,
            TextInputContentOrigin::Event
        );
        assert_eq!(node.layout.effective.text_input_cursor, Some(3));
        assert_eq!(node.layout.effective.text_input_selection_anchor, None);
        assert_eq!(node.layout.effective.text_input_preedit, None);
        assert_eq!(node.layout.effective.text_input_preedit_cursor, None);

        assert!(
            tree.set_text_input_content(&id, "hey".to_string())
                .is_none()
        );
    }

    #[test]
    fn test_set_text_input_content_marks_event_origin_without_content_change() {
        let id = NodeId::from_term_bytes(vec![14]);
        let mut attrs = Attrs::default();
        attrs.content = Some("same".to_string());
        let element = Element::with_attrs(id.clone(), ElementKind::TextInput, Vec::new(), attrs);

        let mut tree = ElementTree::new();
        tree.insert(element);
        tree.set_root_id(id);

        assert!(
            tree.set_text_input_content(&id, "same".to_string())
                .is_dirty()
        );
        assert_eq!(
            tree.get(&id).unwrap().runtime.text_input_content_origin,
            TextInputContentOrigin::Event
        );
        assert!(
            tree.set_text_input_content(&id, "same".to_string())
                .is_none()
        );
    }

    #[test]
    fn test_set_text_input_runtime_normalizes_focus_selection_and_preedit() {
        let id = NodeId::from_term_bytes(vec![3]);
        let mut attrs = Attrs::default();
        attrs.content = Some("abcd".to_string());
        let mut element =
            Element::with_attrs(id.clone(), ElementKind::TextInput, Vec::new(), attrs);
        element.layout.frame = Some(Frame {
            x: 0.0,
            y: 0.0,
            width: 120.0,
            height: 24.0,
            content_width: 120.0,
            content_height: 24.0,
        });

        let mut tree = ElementTree::new();
        tree.insert(element);
        tree.set_root_id(id);

        assert!(
            tree.set_text_input_runtime(
                &id,
                true,
                Some(99),
                Some(1),
                Some("ka".to_string()),
                Some((7, 2)),
            )
            .is_dirty()
        );

        let focused = tree.get(&id).unwrap();
        assert_eq!(focused.layout.effective.text_input_focused, Some(true));
        assert_eq!(focused.layout.effective.text_input_cursor, Some(4));
        assert_eq!(
            focused.layout.effective.text_input_selection_anchor,
            Some(1)
        );
        assert_eq!(
            focused.layout.effective.text_input_preedit.as_deref(),
            Some("ka")
        );
        assert_eq!(
            focused.layout.effective.text_input_preedit_cursor,
            Some((2, 2))
        );

        assert!(
            tree.set_text_input_runtime(
                &id,
                false,
                Some(2),
                Some(0),
                Some("ignored".to_string()),
                Some((1, 1)),
            )
            .is_dirty()
        );

        let blurred = tree.get(&id).unwrap();
        assert_eq!(blurred.layout.effective.text_input_focused, Some(false));
        assert_eq!(blurred.layout.effective.text_input_cursor, Some(2));
        assert_eq!(blurred.layout.effective.text_input_selection_anchor, None);
        assert_eq!(blurred.layout.effective.text_input_preedit, None);
        assert_eq!(blurred.layout.effective.text_input_preedit_cursor, None);
    }

    #[test]
    fn test_set_text_input_runtime_ignores_non_text_input_nodes() {
        let id = NodeId::from_term_bytes(vec![4]);
        let mut element =
            Element::with_attrs(id.clone(), ElementKind::El, Vec::new(), Attrs::default());
        element.layout.frame = Some(Frame {
            x: 0.0,
            y: 0.0,
            width: 50.0,
            height: 20.0,
            content_width: 50.0,
            content_height: 20.0,
        });

        let mut tree = ElementTree::new();
        tree.insert(element);
        tree.set_root_id(id);

        assert!(
            tree.set_text_input_content(&id, "nope".to_string())
                .is_none()
        );
        assert!(
            tree.set_text_input_runtime(&id, true, Some(0), None, Some("x".to_string()), None,)
                .is_none()
        );
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
        tree.insert(paragraph);
        tree.insert(inline);
        tree.insert(float);
        tree.set_root_id(paragraph_id.clone());

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
        tree.insert(row);
        tree.insert(first);
        tree.insert(second);
        tree.set_root_id(row_id.clone());

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
        uploaded.insert(Element::with_attrs(
            uploaded_id.clone(),
            ElementKind::El,
            Vec::new(),
            Attrs::default(),
        ));
        uploaded.set_root_id(uploaded_id.clone());

        tree.replace_with_uploaded(uploaded);

        assert_eq!(tree.revision(), first_revision + 1);
        assert_eq!(
            tree.get(&uploaded_id)
                .expect("uploaded node should exist")
                .lifecycle
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
        tree.insert(Element::with_attrs(
            id.clone(),
            ElementKind::El,
            Vec::new(),
            Attrs::default(),
        ));
        tree.set_root_id(id.clone());

        let revision = tree.bump_revision();
        tree.stamp_all_mounted_at_revision(revision);

        assert!(tree.was_mounted_after(&id, revision - 1));
        assert!(!tree.was_mounted_after(&id, revision));
    }
}
