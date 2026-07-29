//! Patch decoding and application for incremental tree updates.
//!
//! Patch binary format:
//! - Stream of operations, each starting with a tag byte:
//!   - 1: set_attrs - id(u64) + attr_len(4) + attrs
//!   - 2: set_children - id(u64) + count(2) + [child_id(u64)]...
//!   - 3: insert_subtree - parent_id(u64, 0=nil) + index(2) + tree_len(4) + tree_bytes
//!   - 4: remove - id(u64)
//!   - 5: set_nearby_mounts - host_id(u64) + count(2) + [slot(1) + id(u64)]...
//!   - 6: insert_nearby_subtree - host_id(u64) + index(2) + slot(1) + tree_len(4) + tree_bytes

use super::animation::{
    AnimationSpec, retarget_exit_animation_spec_to_current_visual, scale_animation_spec,
};
use super::attrs::{
    AlignX, Attrs, BorderWidth, Length, Padding, decode_attrs, effective_scrollbar_x,
    effective_scrollbar_y,
};
use super::deserialize::{DecodeError, decode_tree};
#[cfg(test)]
use super::element::NearbyMounts;
use super::element::{
    Element, ElementKind, ElementTree, Frame, GhostAttachment, NearbyMount, NearbySlot, NodeId,
    NodeIx, NodeResidency, ParentLink, SliderValueOrigin, TextInputContentOrigin,
};
use super::invalidation::{
    TreeInvalidation, attrs_change_affects_registry_refresh, classify_attrs_change,
    downgrade_content_measure_when_layout_independent,
};
use super::layout::{
    FontContext, SkiaTextMeasurer, TextMeasurer, effective_layout_scale_for_node,
    font_info_with_inheritance, layout_nearby_mounts_for_refresh,
};
use std::collections::{HashMap, HashSet};

/// A single patch operation.
#[derive(Debug, Clone)]
pub enum Patch {
    /// Update attributes for an existing node.
    SetAttrs { id: NodeId, attrs_raw: Vec<u8> },

    /// Replace the children list for an existing node.
    SetChildren { id: NodeId, children: Vec<NodeId> },

    SetNearbyMounts {
        host_id: NodeId,
        mounts: Vec<NearbyMount>,
    },

    /// Insert a new subtree.
    InsertSubtree {
        /// Parent ID (None if inserting as new root).
        parent_id: Option<NodeId>,
        /// Index in parent's children list.
        index: usize,
        /// The subtree to insert.
        subtree: ElementTree,
    },

    /// Insert a nearby-mounted subtree onto a host slot.
    InsertNearbySubtree {
        host_id: NodeId,
        index: usize,
        slot: NearbySlot,
        subtree: ElementTree,
    },

    /// Remove a node and its descendants.
    Remove { id: NodeId },
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum AttachmentPoint {
    Root,
    Child {
        parent_id: NodeId,
        live_index: usize,
    },
    Nearby {
        host_id: NodeId,
        mount_index: usize,
        slot: NearbySlot,
    },
}

struct GhostTopology {
    id: NodeId,
    children: Vec<NodeId>,
    paint_children: Vec<NodeId>,
    nearby: Vec<NearbyMount>,
}

/// A cursor for reading patch binary data.
struct Cursor<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> Cursor<'a> {
    fn new(data: &'a [u8]) -> Self {
        Self { data, pos: 0 }
    }

    fn is_empty(&self) -> bool {
        self.pos >= self.data.len()
    }

    fn read_bytes(&mut self, len: usize) -> Result<&'a [u8], DecodeError> {
        if self.pos + len > self.data.len() {
            return Err(DecodeError::UnexpectedEof);
        }
        let bytes = &self.data[self.pos..self.pos + len];
        self.pos += len;
        Ok(bytes)
    }

    fn read_u8(&mut self) -> Result<u8, DecodeError> {
        let bytes = self.read_bytes(1)?;
        Ok(bytes[0])
    }

    fn read_u16_be(&mut self) -> Result<u16, DecodeError> {
        let bytes = self.read_bytes(2)?;
        Ok(u16::from_be_bytes([bytes[0], bytes[1]]))
    }

    fn read_u32_be(&mut self) -> Result<u32, DecodeError> {
        let bytes = self.read_bytes(4)?;
        Ok(u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
    }

    fn read_u64_be(&mut self) -> Result<u64, DecodeError> {
        let bytes = self.read_bytes(8)?;
        Ok(u64::from_be_bytes([
            bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7],
        ]))
    }

    fn read_length_prefixed(&mut self) -> Result<Vec<u8>, DecodeError> {
        let len = self.read_u32_be()? as usize;
        let bytes = self.read_bytes(len)?;
        Ok(bytes.to_vec())
    }
}

/// Decode a stream of patches from binary data.
pub fn decode_patches(data: &[u8]) -> Result<Vec<Patch>, DecodeError> {
    let mut cursor = Cursor::new(data);
    let mut patches = Vec::new();

    while !cursor.is_empty() {
        let patch = decode_patch(&mut cursor)?;
        patches.push(patch);
    }

    Ok(patches)
}

/// Decode a single patch from the cursor.
fn decode_patch(cursor: &mut Cursor) -> Result<Patch, DecodeError> {
    let tag = cursor.read_u8()?;

    match tag {
        1 => decode_set_attrs(cursor),
        2 => decode_set_children(cursor),
        5 => decode_set_nearby_mounts(cursor),
        3 => decode_insert_subtree(cursor),
        4 => decode_remove(cursor),
        6 => decode_insert_nearby_subtree(cursor),
        _ => Err(DecodeError::InvalidStructure(format!(
            "unknown patch tag: {}",
            tag
        ))),
    }
}

fn decode_set_attrs(cursor: &mut Cursor) -> Result<Patch, DecodeError> {
    let id = NodeId::from_wire_u64(cursor.read_u64_be()?);

    let attrs_raw = cursor.read_length_prefixed()?;

    Ok(Patch::SetAttrs { id, attrs_raw })
}

fn decode_set_children(cursor: &mut Cursor) -> Result<Patch, DecodeError> {
    let id = NodeId::from_wire_u64(cursor.read_u64_be()?);

    let count = cursor.read_u16_be()? as usize;
    let mut children = Vec::with_capacity(count);

    for _ in 0..count {
        children.push(NodeId::from_wire_u64(cursor.read_u64_be()?));
    }

    Ok(Patch::SetChildren { id, children })
}

fn decode_set_nearby_mounts(cursor: &mut Cursor) -> Result<Patch, DecodeError> {
    let host_id = NodeId::from_wire_u64(cursor.read_u64_be()?);

    let count = cursor.read_u16_be()? as usize;
    let mut mounts = Vec::with_capacity(count);

    for _ in 0..count {
        let slot_tag = cursor.read_u8()?;
        let slot = NearbySlot::from_tag(slot_tag).ok_or_else(|| {
            DecodeError::InvalidStructure(format!("unknown nearby slot tag: {}", slot_tag))
        })?;
        mounts.push(NearbyMount {
            slot,
            id: NodeId::from_wire_u64(cursor.read_u64_be()?),
        });
    }

    Ok(Patch::SetNearbyMounts { host_id, mounts })
}

fn decode_insert_subtree(cursor: &mut Cursor) -> Result<Patch, DecodeError> {
    let parent_id = cursor.read_u64_be()?;

    let parent_id = if parent_id == 0 {
        None
    } else {
        Some(NodeId::from_wire_u64(parent_id))
    };

    let index = cursor.read_u16_be()? as usize;

    let tree_len = cursor.read_u32_be()? as usize;
    let tree_bytes = cursor.read_bytes(tree_len)?;
    let subtree = decode_tree(tree_bytes)?;

    Ok(Patch::InsertSubtree {
        parent_id,
        index,
        subtree,
    })
}

fn decode_remove(cursor: &mut Cursor) -> Result<Patch, DecodeError> {
    let id = NodeId::from_wire_u64(cursor.read_u64_be()?);

    Ok(Patch::Remove { id })
}

fn decode_insert_nearby_subtree(cursor: &mut Cursor) -> Result<Patch, DecodeError> {
    let host_id = NodeId::from_wire_u64(cursor.read_u64_be()?);
    let index = cursor.read_u16_be()? as usize;
    let slot_tag = cursor.read_u8()?;
    let slot = NearbySlot::from_tag(slot_tag).ok_or_else(|| {
        DecodeError::InvalidStructure(format!("unknown nearby slot tag: {}", slot_tag))
    })?;

    let tree_len = cursor.read_u32_be()? as usize;
    let tree_bytes = cursor.read_bytes(tree_len)?;
    let subtree = decode_tree(tree_bytes)?;

    Ok(Patch::InsertNearbySubtree {
        host_id,
        index,
        slot,
        subtree,
    })
}

/// Apply a list of patches to an element tree.
pub fn apply_patches(
    tree: &mut ElementTree,
    patches: Vec<Patch>,
) -> Result<TreeInvalidation, String> {
    if patches.is_empty() {
        return Ok(TreeInvalidation::None);
    }

    let patches = filter_descendant_remove_patches(tree, patches);

    let batch_revision = tree.bump_revision();
    let mut invalidation = TreeInvalidation::None;

    for patch in patches {
        invalidation.add(apply_patch(tree, patch, batch_revision)?);
    }
    Ok(invalidation)
}

#[derive(Clone, Debug)]
struct TextContentPatchLayoutSkipPath {
    text_ix: NodeIx,
    wrapper_ixs: Vec<NodeIx>,
    column_ix: NodeIx,
}

fn text_content_patch_layout_skip_path(
    tree: &ElementTree,
    id: &NodeId,
) -> Option<TextContentPatchLayoutSkipPath> {
    let text_ix = tree.ix_of(id)?;
    let mut current_ix = text_ix;
    let mut wrapper_ixs = Vec::new();

    loop {
        let parent_ix = tree
            .parent_link_of(current_ix)
            .and_then(|link| match link {
                ParentLink::Child { parent } => Some(parent),
                ParentLink::Nearby { .. } => None,
            })?;
        let parent = tree.get_ix(parent_ix)?;
        if plain_single_child_wrapper_contains(tree, parent_ix, current_ix, parent) {
            wrapper_ixs.push(parent_ix);
            current_ix = parent_ix;
            continue;
        }

        return (parent.spec.kind == ElementKind::Column
            && parent.layout.effective.align_x.unwrap_or(AlignX::Left) == AlignX::Left
            && !attrs_affect_interaction_or_registry(&parent.layout.effective))
        .then_some(TextContentPatchLayoutSkipPath {
            text_ix,
            wrapper_ixs,
            column_ix: parent_ix,
        });
    }
}

fn plain_single_child_wrapper_contains(
    tree: &ElementTree,
    wrapper_ix: NodeIx,
    child_ix: NodeIx,
    wrapper: &Element,
) -> bool {
    matches!(wrapper.spec.kind, ElementKind::El | ElementKind::None)
        && plain_wrapper_attrs(&wrapper.layout.effective)
        && tree.child_ixs(wrapper_ix).as_slice() == [child_ix]
        && tree.nearby_ixs(wrapper_ix).is_empty()
}

fn plain_wrapper_attrs(attrs: &Attrs) -> bool {
    attrs.width.is_none()
        && attrs.height.is_none()
        && attrs.layout_scale.is_none()
        && attrs.layout_rotate.is_none()
        && attrs.padding.is_none()
        && attrs.spacing.is_none()
        && attrs.spacing_x.is_none()
        && attrs.spacing_y.is_none()
        && attrs.scrollbar_y != Some(true)
        && attrs.scrollbar_x != Some(true)
        && attrs.ghost_scrollbar_y != Some(true)
        && attrs.ghost_scrollbar_x != Some(true)
        && attrs.clip_nearby != Some(true)
        && attrs.background.is_none()
        && attrs.border_radius.is_none()
        && attrs.border_width.is_none()
        && attrs.border_color.is_none()
        && attrs.box_shadows.is_none()
        && attrs.move_x.is_none()
        && attrs.move_y.is_none()
        && attrs.rotate.is_none()
        && attrs.scale.is_none()
        && attrs.alpha.is_none()
        && !attrs_affect_interaction_or_registry(attrs)
}

fn plain_text_content_change_can_paint_without_layout(
    element: &Element,
    before: &Attrs,
    after: &Attrs,
) -> bool {
    element.spec.kind == ElementKind::Text
        && before.content != after.content
        && content_is_single_line(before.content.as_deref())
        && content_is_single_line(after.content.as_deref())
        && plain_text_leaf_attrs(before)
        && plain_text_leaf_attrs(after)
        && attrs_equal_except_content(before, after)
}

fn text_input_content_change_can_paint_without_layout(
    element: &Element,
    before: &Attrs,
    after: &Attrs,
) -> bool {
    element.spec.kind == ElementKind::TextInput
        && before.content != after.content
        && content_is_single_line(before.content.as_deref())
        && content_is_single_line(after.content.as_deref())
        && !length_depends_on_text_input_content(after.width.as_ref())
        && attrs_equal_except_content(before, after)
}

fn length_depends_on_text_input_content(length: Option<&super::attrs::Length>) -> bool {
    match length {
        None | Some(super::attrs::Length::Content) => true,
        Some(super::attrs::Length::Fill)
        | Some(super::attrs::Length::FillWeighted(_))
        | Some(super::attrs::Length::Px(_)) => false,
        Some(super::attrs::Length::Min(left, right))
        | Some(super::attrs::Length::Max(left, right)) => {
            length_depends_on_text_input_content(Some(left.as_ref()))
                || length_depends_on_text_input_content(Some(right.as_ref()))
        }
    }
}

fn content_is_single_line(content: Option<&str>) -> bool {
    content.is_none_or(|content| !content.contains('\n'))
}

fn plain_text_leaf_attrs(attrs: &Attrs) -> bool {
    attrs.width.is_none()
        && attrs.height.is_none()
        && attrs.layout_scale.is_none()
        && attrs.layout_rotate.is_none()
        && attrs.padding.is_none()
        && attrs.text_align.is_none()
        && attrs.clip_nearby != Some(true)
        && attrs.background.is_none()
        && attrs.border_radius.is_none()
        && attrs.border_width.is_none()
        && attrs.border_color.is_none()
        && attrs.box_shadows.is_none()
        && attrs.move_x.is_none()
        && attrs.move_y.is_none()
        && attrs.rotate.is_none()
        && attrs.scale.is_none()
        && attrs.alpha.is_none()
        && !attrs_affect_interaction_or_registry(attrs)
}

fn attrs_affect_interaction_or_registry(attrs: &Attrs) -> bool {
    attrs.on_click.unwrap_or(false)
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
        || attrs.focused.is_some()
        || attrs.mouse_down.is_some()
}

fn refresh_text_content_layout_skip_frames(
    tree: &mut ElementTree,
    path: &TextContentPatchLayoutSkipPath,
) -> bool {
    if !path
        .wrapper_ixs
        .iter()
        .copied()
        .chain([path.text_ix, path.column_ix])
        .all(|ix| {
            tree.get_ix(ix)
                .is_some_and(|element| element.layout.frame.is_some())
        })
    {
        return false;
    }

    let Some(text_frame) = refreshed_text_content_frame(tree, path.text_ix) else {
        return false;
    };
    resize_all_layout_frames_ix(tree, path.text_ix, text_frame.width, text_frame.height);

    let mut child_width = text_frame.width;
    let mut child_height = text_frame.height;
    for wrapper_ix in path.wrapper_ixs.iter().copied() {
        resize_all_layout_frames_ix(tree, wrapper_ix, child_width, child_height);
        child_width = tree
            .get_ix(wrapper_ix)
            .and_then(|element| element.layout.frame)
            .map(|frame| frame.width)
            .unwrap_or(child_width);
        child_height = tree
            .get_ix(wrapper_ix)
            .and_then(|element| element.layout.frame)
            .map(|frame| frame.height)
            .unwrap_or(child_height);
    }

    refresh_left_column_content_width(tree, path.column_ix);
    true
}

fn refreshed_text_content_frame(tree: &ElementTree, text_ix: NodeIx) -> Option<Frame> {
    let element = tree.get_ix(text_ix)?;
    let frame = element.layout.frame?;
    let attrs = &element.layout.effective;
    let content = attrs.content.as_deref().unwrap_or("");
    let inherited = inherited_font_context_for_ix(tree, text_ix);
    let font_size = attrs
        .font_size
        .map(|size| size as f32)
        .or(inherited.font_size)
        .unwrap_or(16.0);
    let (family, weight, italic) = font_info_with_inheritance(attrs, &inherited);
    let letter_spacing = attrs
        .font_letter_spacing
        .map(|spacing| spacing as f32)
        .or(inherited.font_letter_spacing)
        .unwrap_or(0.0);
    let word_spacing = attrs
        .font_word_spacing
        .map(|spacing| spacing as f32)
        .or(inherited.font_word_spacing)
        .unwrap_or(0.0);
    let measurer = SkiaTextMeasurer;
    let width = measure_text_width_with_spacing_for_patch(
        &measurer,
        content,
        font_size,
        &family,
        weight,
        italic,
        (letter_spacing, word_spacing),
    );
    Some(Frame {
        width,
        height: frame.height,
        content_width: width,
        content_height: frame.height,
        ..frame
    })
}

fn inherited_font_context_for_ix(tree: &ElementTree, ix: NodeIx) -> FontContext {
    let mut ancestors = Vec::new();
    let mut current = Some(ix);
    while let Some(current_ix) = current {
        current = tree.parent_link_of(current_ix).and_then(|link| match link {
            ParentLink::Child { parent } => Some(parent),
            ParentLink::Nearby { .. } => None,
        });
        if let Some(parent_ix) = current {
            ancestors.push(parent_ix);
        }
    }

    ancestors
        .into_iter()
        .rev()
        .filter_map(|ancestor_ix| tree.get_ix(ancestor_ix))
        .fold(FontContext::default(), |context, ancestor| {
            context.merge_with_attrs(&ancestor.layout.effective)
        })
}

fn measure_text_width_with_spacing_for_patch<M: TextMeasurer>(
    measurer: &M,
    text: &str,
    font_size: f32,
    family: &str,
    weight: u16,
    italic: bool,
    spacing: (f32, f32),
) -> f32 {
    let (letter_spacing, word_spacing) = spacing;

    if text.is_empty() {
        return 0.0;
    }

    if letter_spacing == 0.0 && word_spacing == 0.0 {
        return measurer.measure_visual_width_with_font(text, font_size, family, weight, italic);
    }

    let mut total = 0.0;
    let mut chars = text.chars().peekable();

    while let Some(ch) = chars.next() {
        total += measurer
            .measure_with_font(&ch.to_string(), font_size, family, weight, italic)
            .0;

        if chars.peek().is_some() {
            total += letter_spacing;
            if ch.is_whitespace() {
                total += word_spacing;
            }
        }
    }

    total
}

fn refresh_left_column_content_width(tree: &mut ElementTree, column_ix: NodeIx) {
    let Some(column) = tree.get_ix(column_ix) else {
        return;
    };
    let attrs = column.layout.effective.clone();
    let Some(frame) = column.layout.frame else {
        return;
    };
    let insets = patch_content_insets(&attrs);
    let max_child_width = tree
        .child_ixs(column_ix)
        .into_iter()
        .filter_map(|ix| tree.get_ix(ix).and_then(|child| child.layout.frame))
        .map(|frame| frame.width)
        .fold(0.0, f32::max);
    let intrinsic_width = resolve_intrinsic_length_for_patch(
        attrs.width.as_ref(),
        max_child_width + insets.horizontal(),
    );
    let resolved_width =
        resolve_frame_width_for_patch(attrs.width.as_ref(), intrinsic_width, frame.width);

    resize_measured_layout_frames_ix(tree, column_ix, intrinsic_width, frame.height);
    resize_resolved_layout_frames_ix(tree, column_ix, resolved_width, frame.height);
}

fn resize_all_layout_frames_ix(tree: &mut ElementTree, ix: NodeIx, width: f32, height: f32) {
    resize_measured_layout_frames_ix(tree, ix, width, height);
    resize_resolved_layout_frames_ix(tree, ix, width, height);
}

fn resize_measured_layout_frames_ix(tree: &mut ElementTree, ix: NodeIx, width: f32, height: f32) {
    if let Some(element) = tree.get_ix_mut(ix) {
        resize_frame_option(&mut element.layout.measured_frame, width, height);
        resize_frame_option(&mut element.layout.measured_render_frame, width, height);
        element.layout.intrinsic_measure_cache = None;
        element.layout.subtree_measure_cache = None;
    }
}

fn resize_resolved_layout_frames_ix(tree: &mut ElementTree, ix: NodeIx, width: f32, height: f32) {
    if let Some(element) = tree.get_ix_mut(ix) {
        resize_frame_option(&mut element.layout.frame, width, height);
        resize_frame_option(&mut element.layout.render_frame, width, height);
        element.layout.resolve_cache = None;
    }
}

fn resize_frame_option(frame: &mut Option<Frame>, width: f32, height: f32) {
    if let Some(frame) = frame {
        frame.width = width;
        frame.height = height;
        frame.content_width = width;
        frame.content_height = height;
    }
}

#[derive(Clone, Copy, Debug, Default)]
struct PatchInsets {
    top: f32,
    right: f32,
    bottom: f32,
    left: f32,
}

impl PatchInsets {
    fn horizontal(self) -> f32 {
        self.left + self.right
    }
}

fn patch_content_insets(attrs: &Attrs) -> PatchInsets {
    let padding = patch_padding_insets(attrs.padding.as_ref());
    let border = patch_border_width_insets(attrs.border_width.as_ref());
    PatchInsets {
        top: padding.top + border.top,
        right: padding.right + border.right,
        bottom: padding.bottom + border.bottom,
        left: padding.left + border.left,
    }
}

fn patch_padding_insets(padding: Option<&Padding>) -> PatchInsets {
    match padding {
        Some(Padding::Uniform(value)) => {
            let value = *value as f32;
            PatchInsets {
                top: value,
                right: value,
                bottom: value,
                left: value,
            }
        }
        Some(Padding::Sides {
            top,
            right,
            bottom,
            left,
        }) => PatchInsets {
            top: *top as f32,
            right: *right as f32,
            bottom: *bottom as f32,
            left: *left as f32,
        },
        None => PatchInsets::default(),
    }
}

fn patch_border_width_insets(border_width: Option<&BorderWidth>) -> PatchInsets {
    match border_width {
        Some(BorderWidth::Uniform(value)) => {
            let value = *value as f32;
            PatchInsets {
                top: value,
                right: value,
                bottom: value,
                left: value,
            }
        }
        Some(BorderWidth::Sides {
            top,
            right,
            bottom,
            left,
        }) => PatchInsets {
            top: *top as f32,
            right: *right as f32,
            bottom: *bottom as f32,
            left: *left as f32,
        },
        None => PatchInsets::default(),
    }
}

fn resolve_intrinsic_length_for_patch(length: Option<&Length>, intrinsic: f32) -> f32 {
    match length {
        Some(Length::Px(px)) => *px as f32,
        Some(Length::Content) | None => intrinsic,
        Some(Length::Fill) | Some(Length::FillWeighted(_)) => intrinsic,
        Some(Length::Min(left, right)) => resolve_intrinsic_length_for_patch(Some(left), intrinsic)
            .min(resolve_intrinsic_length_for_patch(Some(right), intrinsic)),
        Some(Length::Max(left, right)) => resolve_intrinsic_length_for_patch(Some(left), intrinsic)
            .max(resolve_intrinsic_length_for_patch(Some(right), intrinsic)),
    }
}

fn resolve_frame_width_for_patch(length: Option<&Length>, intrinsic: f32, current: f32) -> f32 {
    match length {
        Some(Length::Px(px)) => *px as f32,
        Some(Length::Content) | None => intrinsic,
        Some(Length::Fill) | Some(Length::FillWeighted(_)) => current,
        Some(Length::Min(left, right)) => {
            resolve_frame_width_for_patch(Some(left), intrinsic, current).min(
                resolve_frame_width_for_patch(Some(right), intrinsic, current),
            )
        }
        Some(Length::Max(left, right)) => {
            resolve_frame_width_for_patch(Some(left), intrinsic, current).max(
                resolve_frame_width_for_patch(Some(right), intrinsic, current),
            )
        }
    }
}

fn attrs_equal_except_content(before: &Attrs, after: &Attrs) -> bool {
    let mut before = before.clone();
    let mut after = after.clone();
    before.content = None;
    after.content = None;
    attrs_equivalent_for_plain_text_paint(&before, &after)
}

fn attrs_equivalent_for_plain_text_paint(before: &Attrs, after: &Attrs) -> bool {
    before.width == after.width
        && before.height == after.height
        && before.layout_scale == after.layout_scale
        && before.layout_rotate == after.layout_rotate
        && before.padding == after.padding
        && before.spacing == after.spacing
        && before.spacing_x == after.spacing_x
        && before.spacing_y == after.spacing_y
        && before.align_x == after.align_x
        && before.align_y == after.align_y
        && before.background == after.background
        && before.border_radius == after.border_radius
        && before.border_width == after.border_width
        && before.border_style == after.border_style
        && before.border_color == after.border_color
        && before.box_shadows == after.box_shadows
        && before.font_size == after.font_size
        && before.font_color == after.font_color
        && before.font == after.font
        && before.font_weight == after.font_weight
        && before.font_style == after.font_style
        && before.font_underline == after.font_underline
        && before.font_strike == after.font_strike
        && before.font_letter_spacing == after.font_letter_spacing
        && before.font_word_spacing == after.font_word_spacing
        && before.text_align == after.text_align
        && before.move_x == after.move_x
        && before.move_y == after.move_y
        && before.rotate == after.rotate
        && before.scale == after.scale
        && before.alpha == after.alpha
        && before.svg_color == after.svg_color
        && before.svg_expected == after.svg_expected
}

fn text_paint_attrs_changed(before: &Attrs, after: &Attrs) -> bool {
    before.font_color != after.font_color
        || before.font_underline != after.font_underline
        || before.font_strike != after.font_strike
}

fn affected_paragraph_flow_roots(tree: &ElementTree, id: &NodeId) -> Vec<NodeId> {
    let mut ids = Vec::new();
    let mut seen = HashSet::new();

    if let Some(paragraph_id) = nearest_paragraph_ancestor_or_self(tree, id) {
        push_unique_node_id(&mut ids, &mut seen, paragraph_id);
    }
    collect_paragraph_descendants(tree, id, &mut ids, &mut seen);

    ids
}

fn nearest_paragraph_ancestor_or_self(tree: &ElementTree, id: &NodeId) -> Option<NodeId> {
    let mut current_ix = tree.ix_of(id)?;

    loop {
        let element = tree.get_ix(current_ix)?;
        if element.spec.kind == ElementKind::Paragraph {
            return Some(element.id);
        }

        current_ix = match tree.parent_link_of(current_ix)? {
            ParentLink::Child { parent } | ParentLink::Nearby { host: parent, .. } => parent,
        };
    }
}

fn collect_paragraph_descendants(
    tree: &ElementTree,
    id: &NodeId,
    ids: &mut Vec<NodeId>,
    seen: &mut HashSet<NodeId>,
) {
    if let Some(element) = tree.get(id)
        && element.spec.kind == ElementKind::Paragraph
    {
        push_unique_node_id(ids, seen, *id);
    }

    for child_id in tree.child_ids(id) {
        collect_paragraph_descendants(tree, &child_id, ids, seen);
    }

    for mount in tree.nearby_mounts_for(id) {
        collect_paragraph_descendants(tree, &mount.id, ids, seen);
    }
}

fn push_unique_node_id(ids: &mut Vec<NodeId>, seen: &mut HashSet<NodeId>, id: NodeId) {
    if seen.insert(id) {
        ids.push(id);
    }
}

fn subtree_has_animation_attrs(tree: &ElementTree) -> bool {
    tree.iter_node_pairs().any(|(_, element)| {
        element.spec.declared.animate.is_some()
            || element.spec.declared.animate_enter.is_some()
            || element.spec.declared.animate_exit.is_some()
    })
}

/// Apply a single patch to the tree.
fn apply_patch(
    tree: &mut ElementTree,
    patch: Patch,
    batch_revision: u64,
) -> Result<TreeInvalidation, String> {
    let invalidation = match patch {
        Patch::SetAttrs { id, attrs_raw } => {
            let text_content_layout_skip_path = text_content_patch_layout_skip_path(tree, &id);
            let before_declared_attrs = tree
                .get(&id)
                .ok_or_else(|| "SetAttrs: node not found".to_string())?
                .spec
                .declared
                .clone();
            let mut decoded = decode_attrs(&attrs_raw).map_err(|e| e.to_string())?;
            let paragraph_flow_dirty_ids =
                if text_paint_attrs_changed(&before_declared_attrs, &decoded) {
                    affected_paragraph_flow_roots(tree, &id)
                } else {
                    Vec::new()
                };

            let invalidation = {
                let element = tree
                    .get_mut(&id)
                    .ok_or_else(|| "SetAttrs: node not found".to_string())?;
                let before_attrs = before_declared_attrs.clone();
                let before_patch_content = element.runtime.patch_content.clone();
                let before_content_origin = element.runtime.text_input_content_origin;
                let before_slider_patch_value = element.runtime.slider_patch_value;
                let before_slider_value_origin = element.runtime.slider_value_origin;

                element.spec.attrs_raw = attrs_raw;
                let content_is_from_patch =
                    element.spec.kind.is_text_input_family() && decoded.content.is_some();
                let text_input_is_focused =
                    element.spec.kind.is_text_input_family() && element.runtime.text_input_focused;
                let slider_value_is_from_patch =
                    element.spec.kind == ElementKind::Slider && decoded.slider_value.is_some();
                let slider_value_is_runtime_owned = element.spec.kind == ElementKind::Slider
                    && element.runtime.slider_value_origin == SliderValueOrigin::Event;

                if content_is_from_patch && text_input_is_focused {
                    element.runtime.patch_content = decoded.content.clone();
                    decoded.content = element.spec.declared.content.clone();
                } else if element.spec.kind.is_text_input_family() && !text_input_is_focused {
                    element.runtime.patch_content = None;
                }

                if slider_value_is_from_patch && slider_value_is_runtime_owned {
                    element.runtime.slider_patch_value =
                        decoded.slider_value.and_then(|patch_value| {
                            (!slider_patch_matches_runtime_value(element, patch_value))
                                .then_some(patch_value.to_bits())
                        });
                    decoded.slider_value = element.spec.declared.slider_value;
                } else if element.spec.kind == ElementKind::Slider {
                    element.runtime.slider_patch_value = None;
                }

                element.spec.declared = decoded.clone();
                element.layout.effective = decoded;
                element.normalize_extracted_state();
                if content_is_from_patch && !text_input_is_focused {
                    element.runtime.text_input_content_origin = TextInputContentOrigin::TreePatch;
                }
                if slider_value_is_from_patch && !slider_value_is_runtime_owned {
                    element.runtime.slider_value_origin = SliderValueOrigin::TreePatch;
                }

                let mut invalidation = downgrade_content_measure_when_layout_independent(
                    &before_attrs,
                    &element.spec.declared,
                    classify_attrs_change(&before_attrs, &element.spec.declared),
                );
                let mut skipped_text_content_layout = false;
                if text_content_layout_skip_path.is_some()
                    && plain_text_content_change_can_paint_without_layout(
                        element,
                        &before_attrs,
                        &element.spec.declared,
                    )
                {
                    invalidation = TreeInvalidation::Paint;
                    skipped_text_content_layout = true;
                }
                if text_input_content_change_can_paint_without_layout(
                    element,
                    &before_attrs,
                    &element.spec.declared,
                ) {
                    invalidation = TreeInvalidation::Paint;
                }
                let registry_refresh_dirty =
                    attrs_change_affects_registry_refresh(&before_attrs, &element.spec.declared);
                if before_patch_content != element.runtime.patch_content
                    || before_content_origin != element.runtime.text_input_content_origin
                    || before_slider_patch_value != element.runtime.slider_patch_value
                    || before_slider_value_origin != element.runtime.slider_value_origin
                {
                    invalidation.add(TreeInvalidation::Paint);
                }
                (
                    invalidation,
                    registry_refresh_dirty,
                    before_attrs.layout_scale != element.spec.declared.layout_scale,
                    skipped_text_content_layout,
                )
            };
            let mut invalidation_kind = invalidation.0;
            if invalidation.3
                && !text_content_layout_skip_path
                    .as_ref()
                    .is_some_and(|path| refresh_text_content_layout_skip_frames(tree, path))
            {
                invalidation_kind = TreeInvalidation::Measure;
            }
            if !paragraph_flow_dirty_ids.is_empty() {
                invalidation_kind.add(TreeInvalidation::Resolve);
            }
            if invalidation.2 {
                tree.mark_layout_scale_dirty(&id);
            } else {
                tree.mark_measure_dirty_for_invalidation(&id, invalidation_kind);
            }
            for paragraph_id in paragraph_flow_dirty_ids {
                tree.mark_measure_dirty_for_invalidation(&paragraph_id, TreeInvalidation::Resolve);
            }
            if invalidation.1 {
                tree.mark_registry_refresh_dirty(&id);
            }
            invalidation_kind
        }

        Patch::SetChildren { id, children } => {
            let nearby_local_change = tree.is_inside_nearby_subtree(&id);
            let merged_children = tree.merge_live_children_with_ghosts(&id, children);
            tree.set_children(&id, merged_children)?;
            if nearby_local_change {
                TreeInvalidation::Resolve
            } else {
                TreeInvalidation::Structure
            }
        }

        Patch::SetNearbyMounts { host_id, mounts } => {
            let merged_mounts = tree.merge_live_nearby_with_ghosts(&host_id, mounts);
            let registry_relevant =
                tree.nearby_mount_change_affects_registry(&host_id, &merged_mounts);
            tree.set_nearby_mounts(&host_id, merged_mounts)?;
            if layout_nearby_mounts_for_refresh(tree, &host_id) {
                if registry_relevant {
                    TreeInvalidation::Registry
                } else {
                    TreeInvalidation::Paint
                }
            } else {
                TreeInvalidation::Resolve
            }
        }

        Patch::InsertSubtree {
            parent_id,
            index,
            mut subtree,
        } => {
            // Get the root of the subtree
            let subtree_root_id = subtree
                .root_id()
                .ok_or_else(|| "InsertSubtree: subtree has no root".to_string())?;

            let subtree_topology: Vec<_> = subtree
                .iter_node_pairs()
                .map(|(id, _)| {
                    (
                        id,
                        subtree.child_ids(&id),
                        subtree.paint_child_ids_for(&id),
                        subtree.nearby_mounts_for(&id),
                    )
                })
                .collect();

            subtree.stamp_all_mounted_at_revision(batch_revision);

            // Insert all nodes from subtree into main tree
            for element in subtree.nodes.into_iter().flatten() {
                tree.insert(element);
            }

            for (id, child_ids, paint_child_ids, nearby_mounts) in subtree_topology {
                tree.set_inserted_node_topology(&id, child_ids, paint_child_ids, nearby_mounts)?;
            }

            // Update parent's children or set as tree root
            match parent_id {
                Some(pid) => {
                    let mut live_children = tree.live_child_ids(&pid);
                    if !live_children.contains(&subtree_root_id) {
                        let insert_idx = index.min(live_children.len());
                        live_children.insert(insert_idx, subtree_root_id);
                        let merged_children =
                            tree.merge_live_children_with_ghosts(&pid, live_children);
                        tree.set_children(&pid, merged_children)?;
                    }
                }
                None => {
                    // Inserting as new root
                    tree.set_root_id(subtree_root_id);
                }
            }

            TreeInvalidation::Structure
        }

        Patch::InsertNearbySubtree {
            host_id,
            index,
            slot,
            mut subtree,
        } => {
            let subtree_root_id = subtree
                .root_id()
                .ok_or_else(|| "InsertNearbySubtree: subtree has no root".to_string())?;

            let subtree_topology: Vec<_> = subtree
                .iter_node_pairs()
                .map(|(id, _)| {
                    (
                        id,
                        subtree.child_ids(&id),
                        subtree.paint_child_ids_for(&id),
                        subtree.nearby_mounts_for(&id),
                    )
                })
                .collect();
            let has_animation_attrs = subtree_has_animation_attrs(&subtree);

            subtree.stamp_all_mounted_at_revision(batch_revision);

            for element in subtree.nodes.into_iter().flatten() {
                tree.insert(element);
            }

            for (id, child_ids, paint_child_ids, nearby_mounts) in subtree_topology {
                tree.set_inserted_node_topology(&id, child_ids, paint_child_ids, nearby_mounts)?;
            }

            let mut live_mounts = tree.live_nearby_mounts(&host_id);
            let insert_index = index.min(live_mounts.len());
            if !live_mounts.iter().any(|mount| mount.id == subtree_root_id) {
                live_mounts.insert(
                    insert_index,
                    NearbyMount {
                        slot,
                        id: subtree_root_id,
                    },
                );
            }

            let registry_relevant =
                slot.is_overlay() || tree.subtree_affects_registry(&subtree_root_id);
            let merged_mounts = tree.merge_live_nearby_with_ghosts(&host_id, live_mounts);
            tree.set_nearby_mounts(&host_id, merged_mounts)?;

            if has_animation_attrs {
                TreeInvalidation::Resolve
            } else {
                let restored_layout = tree.restore_detached_layout_subtree_cache(&subtree_root_id);
                let can_skip_layout =
                    restored_layout || tree.nearby_subtree_can_skip_layout(&subtree_root_id);

                if can_skip_layout {
                    if !restored_layout {
                        tree.mark_nearby_subtree_layout_clean_for_refresh_only(&subtree_root_id);
                    }
                    if !registry_relevant {
                        tree.clear_registry_refresh_dirty_for_subtree(&subtree_root_id);
                    }
                    tree.recompute_layout_descendant_dirty();
                    if registry_relevant {
                        TreeInvalidation::Registry
                    } else {
                        TreeInvalidation::Paint
                    }
                } else if layout_nearby_mounts_for_refresh(tree, &host_id) {
                    if !registry_relevant {
                        tree.clear_registry_refresh_dirty_for_subtree(&subtree_root_id);
                    }
                    if registry_relevant {
                        TreeInvalidation::Registry
                    } else {
                        TreeInvalidation::Paint
                    }
                } else {
                    TreeInvalidation::Resolve
                }
            }
        }

        Patch::Remove { id } => {
            let parent_link = tree.ix_of(&id).and_then(|ix| tree.parent_link_of(ix));
            let nearby_registry_relevant = match parent_link {
                Some(ParentLink::Nearby { slot, .. }) => {
                    slot.is_overlay() || tree.subtree_affects_registry(&id)
                }
                _ => false,
            };

            let ghost_root_id = maybe_capture_exit_ghost(tree, &id)?;
            if let Some(ghost_root_id) = ghost_root_id {
                attach_ghost_root(tree, &ghost_root_id)?;
            }

            remove_subtree(tree, &id);
            match parent_link {
                Some(ParentLink::Nearby { .. }) if ghost_root_id.is_none() => {
                    tree.recompute_layout_descendant_dirty();
                    if nearby_registry_relevant {
                        TreeInvalidation::Registry
                    } else {
                        TreeInvalidation::Paint
                    }
                }
                Some(ParentLink::Nearby { .. }) => TreeInvalidation::Resolve,
                _ => TreeInvalidation::Structure,
            }
        }
    };

    Ok(invalidation)
}

fn filter_descendant_remove_patches(tree: &ElementTree, patches: Vec<Patch>) -> Vec<Patch> {
    let remove_ids: Vec<NodeId> = patches
        .iter()
        .filter_map(|patch| match patch {
            Patch::Remove { id } if tree.get(id).is_some_and(Element::is_live) => Some(*id),
            _ => None,
        })
        .collect();

    if remove_ids.is_empty() {
        return patches;
    }

    let remove_set: HashSet<NodeId> = remove_ids.iter().cloned().collect();

    patches
        .into_iter()
        .filter(|patch| match patch {
            Patch::Remove { id } => {
                !tree.get(id).is_some_and(Element::is_live)
                    || !has_removed_live_ancestor(tree, id, &remove_set)
            }
            _ => true,
        })
        .collect()
}

fn slider_patch_matches_runtime_value(element: &Element, patch_value: f64) -> bool {
    let runtime_value = element
        .layout
        .effective
        .slider_value
        .or(element.spec.declared.slider_value)
        .unwrap_or(0.0);
    patch_value.to_bits() == runtime_value.to_bits()
        || (patch_value - runtime_value).abs() <= f64::EPSILON
}

fn has_removed_live_ancestor(
    tree: &ElementTree,
    id: &NodeId,
    remove_set: &HashSet<NodeId>,
) -> bool {
    let mut current_ix = tree.ix_of(id);

    while let Some(ix) = current_ix {
        let Some(parent_link) = tree.parent_link_of(ix) else {
            return false;
        };

        let parent_ix = match parent_link {
            ParentLink::Child { parent } => parent,
            ParentLink::Nearby { host, .. } => host,
        };

        if let Some(parent_id) = tree.id_of(parent_ix)
            && remove_set.contains(&parent_id)
        {
            return true;
        }

        current_ix = Some(parent_ix);
    }

    false
}

fn maybe_capture_exit_ghost(tree: &mut ElementTree, id: &NodeId) -> Result<Option<NodeId>, String> {
    let Some(element) = tree.get(id) else {
        return Ok(None);
    };

    if element.is_ghost() {
        return Ok(None);
    }

    let capture_scale = effective_layout_scale_for_node(tree, id, tree.current_scale());
    let Some(spec) = captured_exit_spec(element, capture_scale) else {
        return Ok(None);
    };

    let attachment = locate_attachment(tree, id).ok_or_else(|| {
        format!(
            "Remove: failed to locate surviving attachment for node {:?}",
            id.0
        )
    })?;

    if matches!(attachment, AttachmentPoint::Root) {
        return Ok(None);
    }

    let mut to_clone = Vec::new();
    collect_descendants(tree, id, &mut to_clone);
    let capture_scales: HashMap<NodeId, f32> = to_clone
        .iter()
        .map(|old_id| {
            (
                *old_id,
                effective_layout_scale_for_node(tree, old_id, tree.current_scale()),
            )
        })
        .collect();

    let mut id_map = HashMap::new();
    for old_id in &to_clone {
        id_map.insert(*old_id, tree.mint_ghost_id());
    }

    let ghost_root_id = id_map
        .get(id)
        .cloned()
        .ok_or_else(|| "Remove: missing ghost root id".to_string())?;

    let ghost_attachment = match attachment {
        AttachmentPoint::Child {
            parent_id,
            live_index,
        } => GhostAttachment::Child {
            parent_id,
            live_index,
            seq: tree.next_ghost_seq(),
        },
        AttachmentPoint::Nearby {
            host_id,
            mount_index,
            slot,
        } => GhostAttachment::Nearby {
            host_id,
            mount_index,
            slot,
            seq: tree.next_ghost_seq(),
        },
        AttachmentPoint::Root => {
            unreachable!("viewport root animate_exit should be rejected earlier")
        }
    };

    let ghost_nodes: Vec<Element> = to_clone
        .iter()
        .filter_map(|old_id| tree.get(old_id).cloned())
        .map(|old| {
            let capture_scale = capture_scales
                .get(&old.id)
                .copied()
                .unwrap_or(capture_scale);
            clone_as_ghost(
                &old,
                &id_map,
                capture_scale,
                &ghost_root_id,
                &ghost_attachment,
                &spec,
            )
        })
        .collect();
    let ghost_topology: Vec<GhostTopology> = to_clone
        .iter()
        .filter_map(|old_id| {
            let ghost_id = *id_map.get(old_id)?;
            let children = remap_child_ids(&tree.child_ids(old_id), &id_map);
            let paint_children = remap_child_ids(&tree.paint_child_ids_for(old_id), &id_map);
            let nearby = remap_nearby_mounts(&tree.nearby_mounts_for(old_id), &id_map);
            Some(GhostTopology {
                id: ghost_id,
                children,
                paint_children,
                nearby,
            })
        })
        .collect();

    for ghost in ghost_nodes {
        tree.insert(ghost);
    }

    for topology in ghost_topology {
        tree.set_inserted_node_topology(
            &topology.id,
            topology.children,
            topology.paint_children,
            topology.nearby,
        )?;
    }

    Ok(Some(ghost_root_id))
}

fn attach_ghost_root(tree: &mut ElementTree, ghost_root_id: &NodeId) -> Result<(), String> {
    let attachment = tree
        .get(ghost_root_id)
        .and_then(|ghost| ghost.lifecycle.ghost_attachment.clone())
        .ok_or_else(|| "attach_ghost_root: ghost root missing attachment".to_string())?;

    match attachment {
        GhostAttachment::Child { parent_id, .. } => {
            let live_children = tree.live_child_ids(&parent_id);
            let merged_children = tree.merge_live_children_with_ghosts_including(
                &parent_id,
                live_children,
                Some(*ghost_root_id),
            );
            tree.set_children(&parent_id, merged_children)?;
        }
        GhostAttachment::Nearby { host_id, .. } => {
            let live_mounts = tree.live_nearby_mounts(&host_id);
            let merged_mounts = tree.merge_live_nearby_with_ghosts_including(
                &host_id,
                live_mounts,
                Some(*ghost_root_id),
            );
            tree.set_nearby_mounts(&host_id, merged_mounts)?;
        }
    }

    Ok(())
}

fn captured_exit_spec(element: &Element, capture_scale: f32) -> Option<AnimationSpec> {
    let spec = element.layout.effective.animate_exit.clone().or_else(|| {
        element
            .spec
            .declared
            .animate_exit
            .as_ref()
            .map(|spec| scale_animation_spec(spec, capture_scale as f64))
    })?;

    Some(retarget_exit_animation_spec_to_current_visual(
        spec,
        &element.layout.effective,
    ))
}

fn locate_attachment(tree: &ElementTree, id: &NodeId) -> Option<AttachmentPoint> {
    if tree.root_id() == Some(*id) {
        return Some(AttachmentPoint::Root);
    }

    let ix = tree.ix_of(id)?;
    match tree.parent_link_of(ix)? {
        ParentLink::Child { parent } => {
            let parent_id = tree.id_of(parent)?;
            let live_index = tree
                .child_ids(&parent_id)
                .iter()
                .scan(0usize, |live_index, child_id| {
                    let current_live_index = *live_index;
                    let is_live = tree.get(child_id).is_some_and(Element::is_live);
                    let result = (*child_id, current_live_index, is_live);
                    if is_live {
                        *live_index += 1;
                    }
                    Some(result)
                })
                .find_map(|(child_id, child_live_index, _is_live)| {
                    (child_id == *id).then_some(child_live_index)
                })?;

            Some(AttachmentPoint::Child {
                parent_id,
                live_index,
            })
        }
        ParentLink::Nearby { host, slot } => {
            let host_id = tree.id_of(host)?;
            let mount_index = tree
                .nearby_mounts_for(&host_id)
                .iter()
                .scan(0usize, |live_index, mount| {
                    let current_live_index = *live_index;
                    let is_live = tree.get(&mount.id).is_some_and(Element::is_live);
                    let result = (mount.id, current_live_index, is_live);
                    if is_live {
                        *live_index += 1;
                    }
                    Some(result)
                })
                .find_map(|(mount_id, mount_live_index, _is_live)| {
                    (mount_id == *id).then_some(mount_live_index)
                })?;

            Some(AttachmentPoint::Nearby {
                host_id,
                mount_index,
                slot,
            })
        }
    }
}

fn clone_as_ghost(
    old: &Element,
    id_map: &HashMap<NodeId, NodeId>,
    capture_scale: f32,
    ghost_root_id: &NodeId,
    ghost_attachment: &GhostAttachment,
    exit_spec: &AnimationSpec,
) -> Element {
    let new_id = id_map
        .get(&old.id)
        .cloned()
        .expect("ghost id should exist for every cloned node");
    let (kind, attrs) = sanitize_ghost_visual(old.spec.kind, &old.layout.effective);

    let mut cloned = Element {
        id: new_id,
        spec: crate::tree::element::NodeSpec {
            kind,
            attrs_raw: Vec::new(),
            declared: attrs.clone(),
        },
        runtime: crate::tree::element::NodeRuntime {
            text_input_content_origin: old.runtime.text_input_content_origin,
            patch_content: old.runtime.patch_content.clone(),
            text_input_focused: false,
            text_input_cursor: None,
            text_input_selection_anchor: None,
            text_input_preedit: None,
            text_input_preedit_cursor: None,
            slider_value_origin: old.runtime.slider_value_origin,
            slider_patch_value: old.runtime.slider_patch_value,
            mouse_over_active: false,
            mouse_down_active: false,
            focused_active: false,
            scrollbar_hover_axis: None,
        },
        layout: crate::tree::element::NodeLayoutState {
            effective: attrs,
            frame: old.layout.frame,
            render_frame: old.layout.render_frame,
            measured_frame: old.layout.measured_frame,
            measured_render_frame: old.layout.measured_render_frame,
            scroll_x: old.layout.scroll_x,
            scroll_y: old.layout.scroll_y,
            scroll_x_max: old.layout.scroll_x_max,
            scroll_y_max: old.layout.scroll_y_max,
            paragraph_fragments: old.layout.paragraph_fragments.clone(),
            topology_versions: Default::default(),
            intrinsic_measure_cache: None,
            subtree_measure_cache: None,
            measure_dirty: true,
            measure_descendant_dirty: false,
            resolve_cache: None,
            resolve_dirty: true,
            resolve_descendant_dirty: false,
        },
        refresh: Default::default(),
        lifecycle: crate::tree::element::NodeLifecycle {
            mounted_at_revision: old.lifecycle.mounted_at_revision,
            residency: NodeResidency::Ghost,
            ghost_attachment: None,
            ghost_capture_scale: Some(capture_scale),
            ghost_exit_animation: None,
        },
        #[cfg(test)]
        children: Vec::new(),
        #[cfg(test)]
        paint_children: Vec::new(),
        #[cfg(test)]
        nearby: NearbyMounts::default(),
    };

    if new_id == *ghost_root_id {
        cloned.lifecycle.ghost_attachment = Some(ghost_attachment.clone());
        cloned.lifecycle.ghost_exit_animation = Some(exit_spec.clone());
    }

    cloned.normalize_extracted_state();

    cloned
}

fn remap_child_ids(child_ids: &[NodeId], id_map: &HashMap<NodeId, NodeId>) -> Vec<NodeId> {
    child_ids
        .iter()
        .filter_map(|child_id| id_map.get(child_id).copied())
        .collect()
}

fn remap_nearby_mounts(
    nearby: &[NearbyMount],
    id_map: &HashMap<NodeId, NodeId>,
) -> Vec<NearbyMount> {
    nearby
        .iter()
        .filter_map(|mount| {
            id_map.get(&mount.id).cloned().map(|id| NearbyMount {
                slot: mount.slot,
                id,
            })
        })
        .collect()
}

fn sanitize_ghost_visual(kind: ElementKind, source: &Attrs) -> (ElementKind, Attrs) {
    let mut attrs = source.clone();

    attrs.on_click = None;
    attrs.on_mouse_down = None;
    attrs.on_mouse_up = None;
    attrs.on_mouse_enter = None;
    attrs.on_mouse_leave = None;
    attrs.on_mouse_move = None;
    attrs.on_press = None;
    attrs.on_swipe_up = None;
    attrs.on_swipe_down = None;
    attrs.on_swipe_left = None;
    attrs.on_swipe_right = None;
    attrs.on_change = None;
    attrs.on_focus = None;
    attrs.on_blur = None;
    attrs.virtual_key = None;

    attrs.mouse_over = None;
    attrs.focused = None;
    attrs.mouse_down = None;
    attrs.ghost_scrollbar_x = effective_scrollbar_x(&attrs).then_some(true);
    attrs.ghost_scrollbar_y = effective_scrollbar_y(&attrs).then_some(true);
    attrs.scrollbar_x = None;
    attrs.scrollbar_y = None;

    attrs.animate = None;
    attrs.animate_enter = None;
    attrs.animate_exit = None;

    let ghost_kind = if kind.is_text_input_family() {
        ElementKind::Text
    } else {
        kind
    };

    (ghost_kind, attrs)
}

/// Recursively remove a node and all its descendants.
pub(crate) fn remove_subtree(tree: &mut ElementTree, id: &NodeId) {
    // First collect all descendant IDs
    let mut to_remove = Vec::new();
    collect_descendants(tree, id, &mut to_remove);

    let parent_link = tree.ix_of(id).and_then(|ix| tree.parent_link_of(ix));

    if matches!(parent_link, Some(ParentLink::Nearby { .. })) {
        tree.store_detached_layout_subtree_cache(id);
    }

    if let Some(parent_link) = parent_link {
        match parent_link {
            ParentLink::Child { parent } => {
                if let Some(parent_id) = tree.id_of(parent) {
                    let mut remaining = tree.child_ids(&parent_id);
                    remaining.retain(|child_id| child_id != id);
                    let _ = tree.set_children(&parent_id, remaining);
                }
            }
            ParentLink::Nearby { host, .. } => {
                if let Some(host_id) = tree.id_of(host) {
                    let mut remaining = tree.nearby_mounts_for(&host_id);
                    remaining.retain(|mount| &mount.id != id);
                    let _ = tree.set_nearby_mounts(&host_id, remaining);
                }
            }
        }
    }

    // Remove all collected nodes
    for remove_id in to_remove {
        tree.remove_node(&remove_id);
    }

    // If this was the root, clear it
    if tree.root_id() == Some(*id) {
        tree.clear_root();
    }
}

/// Collect a node and all its descendants.
fn collect_descendants(tree: &ElementTree, id: &NodeId, acc: &mut Vec<NodeId>) {
    acc.push(*id);

    for child_id in tree.child_ids(id) {
        collect_descendants(tree, &child_id, acc);
    }

    for mount in tree.nearby_mounts_for(id) {
        collect_descendants(tree, &mount.id, acc);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::events::registry_builder::ListenerMatcherKind;
    use crate::tree::animation::{AnimationCurve, AnimationRepeat, AnimationSpec};
    use crate::tree::attrs::{Attrs, Length};
    use crate::tree::element::{
        Element, ElementKind, Frame, NearbyMountIx, NearbySlot, NodeId, NodeIx, ParentLink,
        TextInputContentOrigin,
    };
    use crate::tree::layout::{Constraint, layout_tree_default};

    fn exit_alpha_spec() -> AnimationSpec {
        alpha_spec(1.0, 0.0)
    }

    fn alpha_spec(from_alpha: f64, to_alpha: f64) -> AnimationSpec {
        let from = Attrs {
            alpha: Some(from_alpha),
            ..Attrs::default()
        };

        let to = Attrs {
            alpha: Some(to_alpha),
            ..Attrs::default()
        };

        AnimationSpec {
            keyframes: vec![from, to],
            duration_ms: 200.0,
            curve: AnimationCurve::Linear,
            repeat: AnimationRepeat::Once,
        }
    }

    fn width_spec(from_width: f64, to_width: f64) -> AnimationSpec {
        AnimationSpec {
            keyframes: vec![
                Attrs {
                    width: Some(Length::Px(from_width)),
                    ..Attrs::default()
                },
                Attrs {
                    width: Some(Length::Px(to_width)),
                    ..Attrs::default()
                },
            ],
            duration_ms: 200.0,
            curve: AnimationCurve::Linear,
            repeat: AnimationRepeat::Once,
        }
    }

    fn text_frame(x: f32, y: f32, width: f32, height: f32) -> Frame {
        Frame {
            x,
            y,
            width,
            height,
            content_width: width,
            content_height: height,
        }
    }

    fn text_element(id: u8, content: &str) -> Element {
        let attrs = Attrs {
            content: Some(content.to_string()),
            ..Attrs::default()
        };
        let mut element = Element::with_attrs(
            NodeId::from_term_bytes(vec![id]),
            ElementKind::Text,
            Vec::new(),
            attrs,
        );
        element.layout.frame = Some(text_frame(0.0, 0.0, 64.0, 24.0));
        element
    }

    fn plain_element(id: u8) -> Element {
        Element::with_attrs(
            NodeId::from_term_bytes(vec![id]),
            ElementKind::El,
            Vec::new(),
            Attrs::default(),
        )
    }

    fn slider_value_attrs_raw(value: f64) -> Vec<u8> {
        let mut raw = vec![0, 1, 82];
        raw.extend_from_slice(&value.to_be_bytes());
        raw
    }

    fn node_ix(tree: &ElementTree, id: &NodeId) -> NodeIx {
        tree.ix_of(id).expect("node id should resolve to an ix")
    }

    fn assert_topology_links(tree: &ElementTree, id: &NodeId, expected_parent: Option<ParentLink>) {
        let ix = node_ix(tree, id);
        assert_eq!(tree.parent_link_of(ix), expected_parent);

        for child_id in tree.child_ids(id) {
            assert_topology_links(tree, &child_id, Some(ParentLink::Child { parent: ix }));
        }

        for mount in tree.nearby_mounts_for(id) {
            assert_topology_links(
                tree,
                &mount.id,
                Some(ParentLink::Nearby {
                    host: ix,
                    slot: mount.slot,
                }),
            );
        }
    }

    fn mount_revision(tree: &ElementTree, id: &NodeId) -> u64 {
        tree.get(id)
            .expect("node should exist")
            .lifecycle
            .mounted_at_revision
    }

    #[test]
    fn test_set_attrs_preserves_node_ix_and_mount_revision() {
        let id = NodeId::from_term_bytes(vec![1]);
        let mut element = plain_element(1);
        element.lifecycle.mounted_at_revision = 7;

        let mut tree = ElementTree::new();
        tree.set_root_id(id);
        tree.insert(element);

        let before_ix = node_ix(&tree, &id);
        let before_revision = mount_revision(&tree, &id);

        let invalidation = apply_patches(
            &mut tree,
            vec![Patch::SetAttrs {
                id,
                attrs_raw: vec![0, 1, 12, 0, 1, 255, 0, 0, 255],
            }],
        )
        .unwrap();

        assert_eq!(invalidation, TreeInvalidation::Paint);
        assert_eq!(node_ix(&tree, &id), before_ix);
        assert_eq!(mount_revision(&tree, &id), before_revision);
    }

    #[test]
    fn test_set_children_reorder_preserves_existing_child_ixs() {
        let parent_id = NodeId::from_term_bytes(vec![2]);
        let first_id = NodeId::from_term_bytes(vec![3]);
        let second_id = NodeId::from_term_bytes(vec![4]);
        let third_id = NodeId::from_term_bytes(vec![5]);

        let mut tree = ElementTree::new();
        tree.set_root_id(parent_id);
        tree.insert(plain_element(2));
        tree.insert(plain_element(3));
        tree.insert(plain_element(4));
        tree.insert(plain_element(5));
        tree.set_children(&parent_id, vec![first_id, second_id, third_id])
            .unwrap();

        let parent_ix = node_ix(&tree, &parent_id);
        let first_ix = node_ix(&tree, &first_id);
        let second_ix = node_ix(&tree, &second_id);
        let third_ix = node_ix(&tree, &third_id);

        let invalidation = apply_patches(
            &mut tree,
            vec![Patch::SetChildren {
                id: parent_id,
                children: vec![third_id, first_id, second_id],
            }],
        )
        .unwrap();

        assert_eq!(invalidation, TreeInvalidation::Structure);
        assert_eq!(node_ix(&tree, &first_id), first_ix);
        assert_eq!(node_ix(&tree, &second_id), second_ix);
        assert_eq!(node_ix(&tree, &third_id), third_ix);
        assert_eq!(
            tree.child_ixs(parent_ix),
            vec![third_ix, first_ix, second_ix]
        );
        assert_eq!(
            tree.parent_link_of(first_ix),
            Some(ParentLink::Child { parent: parent_ix })
        );
        assert_eq!(
            tree.parent_link_of(second_ix),
            Some(ParentLink::Child { parent: parent_ix })
        );
        assert_eq!(
            tree.parent_link_of(third_ix),
            Some(ParentLink::Child { parent: parent_ix })
        );
    }

    #[test]
    fn test_set_nearby_mounts_reorder_preserves_existing_mount_ixs() {
        let host_id = NodeId::from_term_bytes(vec![6]);
        let first_id = NodeId::from_term_bytes(vec![7]);
        let second_id = NodeId::from_term_bytes(vec![8]);

        let mut tree = ElementTree::new();
        tree.set_root_id(host_id);
        tree.insert(plain_element(6));
        tree.insert(plain_element(7));
        tree.insert(plain_element(8));
        tree.set_nearby_mounts(
            &host_id,
            vec![
                NearbyMount {
                    slot: NearbySlot::Above,
                    id: first_id,
                },
                NearbyMount {
                    slot: NearbySlot::Below,
                    id: second_id,
                },
            ],
        )
        .unwrap();

        let host_ix = node_ix(&tree, &host_id);
        let first_ix = node_ix(&tree, &first_id);
        let second_ix = node_ix(&tree, &second_id);

        let invalidation = apply_patches(
            &mut tree,
            vec![Patch::SetNearbyMounts {
                host_id,
                mounts: vec![
                    NearbyMount {
                        slot: NearbySlot::Below,
                        id: second_id,
                    },
                    NearbyMount {
                        slot: NearbySlot::Above,
                        id: first_id,
                    },
                ],
            }],
        )
        .unwrap();

        assert_eq!(invalidation, TreeInvalidation::Resolve);
        assert_eq!(node_ix(&tree, &first_id), first_ix);
        assert_eq!(node_ix(&tree, &second_id), second_ix);
        assert_eq!(
            tree.nearby_ixs(host_ix),
            vec![
                NearbyMountIx {
                    slot: NearbySlot::Below,
                    ix: second_ix,
                },
                NearbyMountIx {
                    slot: NearbySlot::Above,
                    ix: first_ix,
                },
            ]
        );
        assert_eq!(
            tree.parent_link_of(first_ix),
            Some(ParentLink::Nearby {
                host: host_ix,
                slot: NearbySlot::Above,
            })
        );
        assert_eq!(
            tree.parent_link_of(second_ix),
            Some(ParentLink::Nearby {
                host: host_ix,
                slot: NearbySlot::Below,
            })
        );
    }

    #[test]
    fn test_set_nearby_mounts_slot_change_preserves_node_ix() {
        let host_id = NodeId::from_term_bytes(vec![9]);
        let tip_id = NodeId::from_term_bytes(vec![10]);

        let mut tree = ElementTree::new();
        tree.set_root_id(host_id);
        tree.insert(plain_element(9));
        tree.insert(plain_element(10));
        tree.set_nearby_mounts(
            &host_id,
            vec![NearbyMount {
                slot: NearbySlot::Above,
                id: tip_id,
            }],
        )
        .unwrap();

        let host_ix = node_ix(&tree, &host_id);
        let tip_ix = node_ix(&tree, &tip_id);

        apply_patches(
            &mut tree,
            vec![Patch::SetNearbyMounts {
                host_id,
                mounts: vec![NearbyMount {
                    slot: NearbySlot::Below,
                    id: tip_id,
                }],
            }],
        )
        .unwrap();

        assert_eq!(node_ix(&tree, &tip_id), tip_ix);
        assert_eq!(
            tree.nearby_ixs(host_ix),
            vec![NearbyMountIx {
                slot: NearbySlot::Below,
                ix: tip_ix,
            }]
        );
        assert_eq!(
            tree.parent_link_of(tip_ix),
            Some(ParentLink::Nearby {
                host: host_ix,
                slot: NearbySlot::Below,
            })
        );
    }

    #[test]
    fn test_topology_links_stay_consistent_across_representative_mutation_batch() {
        let root_id = NodeId::from_term_bytes(vec![90]);
        let host_id = NodeId::from_term_bytes(vec![91]);
        let child_id = NodeId::from_term_bytes(vec![92]);
        let removed_id = NodeId::from_term_bytes(vec![93]);
        let nearby_id = NodeId::from_term_bytes(vec![94]);
        let inserted_child_id = NodeId::from_term_bytes(vec![95]);
        let inserted_child_leaf_id = NodeId::from_term_bytes(vec![96]);
        let inserted_nearby_id = NodeId::from_term_bytes(vec![97]);
        let inserted_nearby_leaf_id = NodeId::from_term_bytes(vec![98]);

        let mut tree = ElementTree::new();
        tree.set_root_id(root_id);
        tree.insert(plain_element(90));
        tree.insert(plain_element(91));
        tree.insert(plain_element(92));
        tree.insert(plain_element(93));
        tree.insert(plain_element(94));
        tree.set_children(&root_id, vec![host_id, removed_id])
            .unwrap();
        tree.set_children(&host_id, vec![child_id]).unwrap();
        tree.set_paint_children(&host_id, vec![child_id]).unwrap();
        tree.set_nearby_mounts(
            &host_id,
            vec![NearbyMount {
                slot: NearbySlot::Above,
                id: nearby_id,
            }],
        )
        .unwrap();

        let mut inserted_child = ElementTree::new();
        inserted_child.set_root_id(inserted_child_id);
        inserted_child.insert(plain_element(95));
        inserted_child.insert(text_element(96, "inserted child"));
        inserted_child
            .set_children(&inserted_child_id, vec![inserted_child_leaf_id])
            .unwrap();
        inserted_child
            .set_paint_children(&inserted_child_id, vec![inserted_child_leaf_id])
            .unwrap();

        let mut inserted_nearby = ElementTree::new();
        inserted_nearby.set_root_id(inserted_nearby_id);
        inserted_nearby.insert(plain_element(97));
        inserted_nearby.insert(text_element(98, "inserted nearby"));
        inserted_nearby
            .set_children(&inserted_nearby_id, vec![inserted_nearby_leaf_id])
            .unwrap();

        apply_patches(
            &mut tree,
            vec![
                Patch::Remove { id: removed_id },
                Patch::InsertSubtree {
                    parent_id: Some(host_id),
                    index: 1,
                    subtree: inserted_child,
                },
                Patch::InsertNearbySubtree {
                    host_id,
                    index: 1,
                    slot: NearbySlot::OnRight,
                    subtree: inserted_nearby,
                },
                Patch::SetNearbyMounts {
                    host_id,
                    mounts: vec![
                        NearbyMount {
                            slot: NearbySlot::OnRight,
                            id: inserted_nearby_id,
                        },
                        NearbyMount {
                            slot: NearbySlot::Above,
                            id: nearby_id,
                        },
                    ],
                },
            ],
        )
        .unwrap();

        assert_eq!(tree.child_ids(&root_id), vec![host_id]);
        assert_eq!(tree.child_ids(&host_id), vec![child_id, inserted_child_id]);
        assert_eq!(
            tree.paint_child_ids_for(&inserted_child_id),
            vec![inserted_child_leaf_id]
        );
        assert_eq!(
            tree.nearby_mounts_for(&host_id),
            vec![
                NearbyMount {
                    slot: NearbySlot::OnRight,
                    id: inserted_nearby_id,
                },
                NearbyMount {
                    slot: NearbySlot::Above,
                    id: nearby_id,
                },
            ]
        );

        assert_topology_links(&tree, &root_id, None);
    }

    #[test]
    fn test_insert_subtree_preserves_existing_ixs_and_stamps_new_nodes() {
        let parent_id = NodeId::from_term_bytes(vec![11]);
        let first_id = NodeId::from_term_bytes(vec![12]);
        let second_id = NodeId::from_term_bytes(vec![13]);
        let new_root_id = NodeId::from_term_bytes(vec![14]);
        let new_leaf_id = NodeId::from_term_bytes(vec![15]);

        let mut tree = ElementTree::new();
        tree.set_root_id(parent_id);
        tree.insert(plain_element(11));
        tree.insert(plain_element(12));
        tree.insert(plain_element(13));
        tree.set_children(&parent_id, vec![first_id, second_id])
            .unwrap();

        let parent_ix = node_ix(&tree, &parent_id);
        let first_ix = node_ix(&tree, &first_id);
        let second_ix = node_ix(&tree, &second_id);
        let first_revision = mount_revision(&tree, &first_id);
        let second_revision = mount_revision(&tree, &second_id);

        let mut subtree = ElementTree::new();
        subtree.set_root_id(new_root_id);
        subtree.insert(plain_element(14));
        subtree.insert(text_element(15, "new"));
        subtree
            .set_children(&new_root_id, vec![new_leaf_id])
            .unwrap();

        let invalidation = apply_patches(
            &mut tree,
            vec![Patch::InsertSubtree {
                parent_id: Some(parent_id),
                index: 1,
                subtree,
            }],
        )
        .unwrap();

        let new_root_ix = node_ix(&tree, &new_root_id);
        let new_leaf_ix = node_ix(&tree, &new_leaf_id);
        assert_eq!(invalidation, TreeInvalidation::Structure);
        assert_eq!(node_ix(&tree, &first_id), first_ix);
        assert_eq!(node_ix(&tree, &second_id), second_ix);
        assert_eq!(mount_revision(&tree, &first_id), first_revision);
        assert_eq!(mount_revision(&tree, &second_id), second_revision);
        assert_eq!(mount_revision(&tree, &new_root_id), tree.revision());
        assert_eq!(mount_revision(&tree, &new_leaf_id), tree.revision());
        assert_eq!(
            tree.child_ixs(parent_ix),
            vec![first_ix, new_root_ix, second_ix]
        );
        assert_eq!(tree.child_ixs(new_root_ix), vec![new_leaf_ix]);
    }

    #[test]
    fn test_insert_nearby_subtree_with_animation_requires_resolve_instead_of_refresh_layout() {
        let host_id = NodeId::from_term_bytes(vec![156]);
        let nearby_id = NodeId::from_term_bytes(vec![157]);

        let mut host = plain_element(156);
        host.layout.frame = Some(Frame {
            x: 0.0,
            y: 0.0,
            width: 320.0,
            height: 240.0,
            content_width: 320.0,
            content_height: 240.0,
        });

        let mut tree = ElementTree::new();
        tree.set_root_id(host_id);
        tree.insert(host);

        let mut nearby = plain_element(157);
        nearby.spec.declared.animate_enter = Some(width_spec(120.0, 180.0));
        nearby.layout.effective.animate_enter = nearby.spec.declared.animate_enter.clone();
        let mut subtree = ElementTree::new();
        subtree.set_root_id(nearby_id);
        subtree.insert(nearby);

        let invalidation = apply_patches(
            &mut tree,
            vec![Patch::InsertNearbySubtree {
                host_id,
                index: 0,
                slot: NearbySlot::InFront,
                subtree,
            }],
        )
        .unwrap();

        assert_eq!(invalidation, TreeInvalidation::Resolve);
    }

    #[test]
    fn test_insert_nearby_subtree_preserves_existing_ixs_and_stamps_new_nodes() {
        let host_id = NodeId::from_term_bytes(vec![56]);
        let first_id = NodeId::from_term_bytes(vec![57]);
        let second_id = NodeId::from_term_bytes(vec![58]);
        let new_root_id = NodeId::from_term_bytes(vec![59]);
        let new_leaf_id = NodeId::from_term_bytes(vec![60]);

        let mut tree = ElementTree::new();
        tree.set_root_id(host_id);
        tree.insert(plain_element(56));
        tree.insert(plain_element(57));
        tree.insert(plain_element(58));
        tree.set_nearby_mounts(
            &host_id,
            vec![
                NearbyMount {
                    slot: NearbySlot::Above,
                    id: first_id,
                },
                NearbyMount {
                    slot: NearbySlot::Below,
                    id: second_id,
                },
            ],
        )
        .unwrap();

        let host_ix = node_ix(&tree, &host_id);
        let first_ix = node_ix(&tree, &first_id);
        let second_ix = node_ix(&tree, &second_id);
        let first_revision = mount_revision(&tree, &first_id);
        let second_revision = mount_revision(&tree, &second_id);

        let mut subtree = ElementTree::new();
        subtree.set_root_id(new_root_id);
        subtree.insert(plain_element(59));
        subtree.insert(text_element(60, "new nearby"));
        subtree
            .set_children(&new_root_id, vec![new_leaf_id])
            .unwrap();

        let invalidation = apply_patches(
            &mut tree,
            vec![Patch::InsertNearbySubtree {
                host_id,
                index: 1,
                slot: NearbySlot::OnRight,
                subtree,
            }],
        )
        .unwrap();

        let new_root_ix = node_ix(&tree, &new_root_id);
        let new_leaf_ix = node_ix(&tree, &new_leaf_id);
        assert_eq!(invalidation, TreeInvalidation::Resolve);
        assert_eq!(node_ix(&tree, &host_id), host_ix);
        assert_eq!(node_ix(&tree, &first_id), first_ix);
        assert_eq!(node_ix(&tree, &second_id), second_ix);
        assert_eq!(mount_revision(&tree, &first_id), first_revision);
        assert_eq!(mount_revision(&tree, &second_id), second_revision);
        assert_eq!(mount_revision(&tree, &new_root_id), tree.revision());
        assert_eq!(mount_revision(&tree, &new_leaf_id), tree.revision());
        assert_eq!(
            tree.nearby_ixs(host_ix),
            vec![
                NearbyMountIx {
                    slot: NearbySlot::Above,
                    ix: first_ix,
                },
                NearbyMountIx {
                    slot: NearbySlot::OnRight,
                    ix: new_root_ix,
                },
                NearbyMountIx {
                    slot: NearbySlot::Below,
                    ix: second_ix,
                },
            ]
        );
        assert_eq!(tree.child_ixs(new_root_ix), vec![new_leaf_ix]);
        assert_eq!(
            tree.parent_link_of(new_root_ix),
            Some(ParentLink::Nearby {
                host: host_ix,
                slot: NearbySlot::OnRight,
            })
        );
        assert_eq!(
            tree.parent_link_of(new_leaf_ix),
            Some(ParentLink::Child {
                parent: new_root_ix
            })
        );
    }

    #[test]
    fn test_remove_prunes_old_id_mapping_before_slot_reuse() {
        let parent_id = NodeId::from_term_bytes(vec![16]);
        let old_id = NodeId::from_term_bytes(vec![17]);
        let new_id = NodeId::from_term_bytes(vec![18]);

        let mut tree = ElementTree::new();
        tree.set_root_id(parent_id);
        tree.insert(plain_element(16));
        tree.insert(plain_element(17));
        tree.set_children(&parent_id, vec![old_id]).unwrap();

        let old_ix = node_ix(&tree, &old_id);

        apply_patches(&mut tree, vec![Patch::Remove { id: old_id }]).unwrap();

        assert_eq!(tree.ix_of(&old_id), None);
        assert_eq!(tree.id_of(old_ix), None);

        let mut subtree = ElementTree::new();
        subtree.set_root_id(new_id);
        subtree.insert(plain_element(18));

        apply_patches(
            &mut tree,
            vec![Patch::InsertSubtree {
                parent_id: Some(parent_id),
                index: 0,
                subtree,
            }],
        )
        .unwrap();

        assert_eq!(tree.ix_of(&old_id), None);
        assert_eq!(tree.ix_of(&new_id), Some(old_ix));
        assert_eq!(tree.id_of(old_ix), Some(new_id));
    }

    #[test]
    fn test_remove_recursively_prunes_descendant_id_mappings() {
        let parent_id = NodeId::from_term_bytes(vec![61]);
        let removed_id = NodeId::from_term_bytes(vec![62]);
        let child_id = NodeId::from_term_bytes(vec![63]);
        let nearby_id = NodeId::from_term_bytes(vec![64]);

        let mut tree = ElementTree::new();
        tree.set_root_id(parent_id);
        tree.insert(plain_element(61));
        tree.insert(plain_element(62));
        tree.insert(plain_element(63));
        tree.insert(plain_element(64));
        tree.set_children(&parent_id, vec![removed_id]).unwrap();
        tree.set_children(&removed_id, vec![child_id]).unwrap();
        tree.set_nearby_mounts(
            &removed_id,
            vec![NearbyMount {
                slot: NearbySlot::Above,
                id: nearby_id,
            }],
        )
        .unwrap();

        let parent_ix = node_ix(&tree, &parent_id);
        let removed_ix = node_ix(&tree, &removed_id);
        let child_ix = node_ix(&tree, &child_id);
        let nearby_ix = node_ix(&tree, &nearby_id);

        apply_patches(&mut tree, vec![Patch::Remove { id: removed_id }]).unwrap();

        assert_eq!(tree.child_ixs(parent_ix), Vec::<NodeIx>::new());
        assert_eq!(tree.ix_of(&removed_id), None);
        assert_eq!(tree.ix_of(&child_id), None);
        assert_eq!(tree.ix_of(&nearby_id), None);
        assert_eq!(tree.id_of(removed_ix), None);
        assert_eq!(tree.id_of(child_ix), None);
        assert_eq!(tree.id_of(nearby_ix), None);
    }

    #[test]
    fn test_remove_then_insert_batch_preserves_sibling_ixs_and_stamps_new_nodes() {
        let parent_id = NodeId::from_term_bytes(vec![65]);
        let first_id = NodeId::from_term_bytes(vec![66]);
        let removed_id = NodeId::from_term_bytes(vec![67]);
        let third_id = NodeId::from_term_bytes(vec![68]);
        let new_root_id = NodeId::from_term_bytes(vec![69]);
        let new_leaf_id = NodeId::from_term_bytes(vec![70]);

        let mut tree = ElementTree::new();
        tree.set_root_id(parent_id);
        tree.insert(plain_element(65));
        tree.insert(text_element(66, "first"));
        tree.insert(text_element(67, "removed"));
        tree.insert(text_element(68, "third"));
        tree.set_children(&parent_id, vec![first_id, removed_id, third_id])
            .unwrap();

        let parent_ix = node_ix(&tree, &parent_id);
        let first_ix = node_ix(&tree, &first_id);
        let third_ix = node_ix(&tree, &third_id);
        let first_revision = mount_revision(&tree, &first_id);
        let third_revision = mount_revision(&tree, &third_id);

        let mut subtree = ElementTree::new();
        subtree.set_root_id(new_root_id);
        subtree.insert(plain_element(69));
        subtree.insert(text_element(70, "new"));
        subtree
            .set_children(&new_root_id, vec![new_leaf_id])
            .unwrap();

        let invalidation = apply_patches(
            &mut tree,
            vec![
                Patch::Remove { id: removed_id },
                Patch::InsertSubtree {
                    parent_id: Some(parent_id),
                    index: 1,
                    subtree,
                },
            ],
        )
        .unwrap();

        let new_root_ix = node_ix(&tree, &new_root_id);
        let new_leaf_ix = node_ix(&tree, &new_leaf_id);
        assert_eq!(invalidation, TreeInvalidation::Structure);
        assert_eq!(tree.ix_of(&removed_id), None);
        assert_eq!(node_ix(&tree, &first_id), first_ix);
        assert_eq!(node_ix(&tree, &third_id), third_ix);
        assert_eq!(mount_revision(&tree, &first_id), first_revision);
        assert_eq!(mount_revision(&tree, &third_id), third_revision);
        assert_eq!(mount_revision(&tree, &new_root_id), tree.revision());
        assert_eq!(mount_revision(&tree, &new_leaf_id), tree.revision());
        assert_eq!(
            tree.child_ixs(parent_ix),
            vec![first_ix, new_root_ix, third_ix]
        );
        assert_eq!(tree.child_ixs(new_root_ix), vec![new_leaf_ix]);
    }

    #[test]
    fn test_animated_remove_keeps_live_ixs_stable_while_adding_ghost() {
        let parent_id = NodeId::from_term_bytes(vec![19]);
        let first_id = NodeId::from_term_bytes(vec![20]);
        let removed_id = NodeId::from_term_bytes(vec![21]);
        let third_id = NodeId::from_term_bytes(vec![22]);

        let mut removed = text_element(21, "removed");
        removed.layout.effective.animate_exit = Some(exit_alpha_spec());
        removed.spec.declared.animate_exit = Some(exit_alpha_spec());

        let mut tree = ElementTree::new();
        tree.set_root_id(parent_id);
        tree.insert(plain_element(19));
        tree.insert(text_element(20, "first"));
        tree.insert(removed);
        tree.insert(text_element(22, "third"));
        tree.set_children(&parent_id, vec![first_id, removed_id, third_id])
            .unwrap();

        let parent_ix = node_ix(&tree, &parent_id);
        let first_ix = node_ix(&tree, &first_id);
        let third_ix = node_ix(&tree, &third_id);

        apply_patches(&mut tree, vec![Patch::Remove { id: removed_id }]).unwrap();

        assert_eq!(tree.ix_of(&removed_id), None);
        assert_eq!(node_ix(&tree, &first_id), first_ix);
        assert_eq!(node_ix(&tree, &third_id), third_ix);

        let child_ids = tree.child_ids(&parent_id);
        assert_eq!(child_ids.len(), 3);
        assert_eq!(child_ids[0], first_id);
        assert_eq!(child_ids[2], third_id);

        let ghost_id = child_ids[1];
        let ghost_ix = node_ix(&tree, &ghost_id);
        assert!(tree.get(&ghost_id).unwrap().is_ghost_root());
        assert_eq!(
            tree.child_ixs(parent_ix),
            vec![first_ix, ghost_ix, third_ix]
        );
        assert_eq!(
            tree.parent_link_of(first_ix),
            Some(ParentLink::Child { parent: parent_ix })
        );
        assert_eq!(
            tree.parent_link_of(third_ix),
            Some(ParentLink::Child { parent: parent_ix })
        );
    }

    #[test]
    fn test_animated_remove_retargets_exit_spec_to_current_visual_alpha() {
        let parent_id = NodeId::from_term_bytes(vec![75]);
        let removed_id = NodeId::from_term_bytes(vec![76]);

        let mut parent =
            Element::with_attrs(parent_id, ElementKind::El, Vec::new(), Attrs::default());
        parent.children = vec![removed_id];

        let mut removed = text_element(76, "closing");
        removed.layout.effective.animate_exit = Some(exit_alpha_spec());
        removed.spec.declared.animate_exit = Some(exit_alpha_spec());
        removed.layout.effective.alpha = Some(0.42);

        let mut tree = ElementTree::new();
        tree.set_root_id(parent_id);
        tree.insert(parent);
        tree.insert(removed);

        apply_patches(&mut tree, vec![Patch::Remove { id: removed_id }]).unwrap();

        let ghost_id = tree.child_ids(&parent_id)[0];
        let ghost = tree.get(&ghost_id).expect("ghost root should exist");
        let exit_spec = ghost
            .lifecycle
            .ghost_exit_animation
            .as_ref()
            .expect("exit animation should be captured");

        assert_eq!(ghost.layout.effective.alpha, Some(0.42));
        assert_eq!(exit_spec.keyframes[0].alpha, Some(0.42));
        assert_eq!(exit_spec.keyframes[1].alpha, Some(0.0));
    }

    #[test]
    fn test_animated_nearby_remove_keeps_live_ixs_stable_while_adding_ghost() {
        let host_id = NodeId::from_term_bytes(vec![71]);
        let first_id = NodeId::from_term_bytes(vec![72]);
        let removed_id = NodeId::from_term_bytes(vec![73]);
        let third_id = NodeId::from_term_bytes(vec![74]);

        let mut removed = text_element(73, "removed nearby");
        removed.layout.effective.animate_exit = Some(exit_alpha_spec());
        removed.spec.declared.animate_exit = Some(exit_alpha_spec());

        let mut tree = ElementTree::new();
        tree.set_root_id(host_id);
        tree.insert(plain_element(71));
        tree.insert(text_element(72, "first"));
        tree.insert(removed);
        tree.insert(text_element(74, "third"));
        tree.set_nearby_mounts(
            &host_id,
            vec![
                NearbyMount {
                    slot: NearbySlot::Above,
                    id: first_id,
                },
                NearbyMount {
                    slot: NearbySlot::Below,
                    id: removed_id,
                },
                NearbyMount {
                    slot: NearbySlot::InFront,
                    id: third_id,
                },
            ],
        )
        .unwrap();

        let host_ix = node_ix(&tree, &host_id);
        let first_ix = node_ix(&tree, &first_id);
        let third_ix = node_ix(&tree, &third_id);

        apply_patches(&mut tree, vec![Patch::Remove { id: removed_id }]).unwrap();

        assert_eq!(tree.ix_of(&removed_id), None);
        assert_eq!(node_ix(&tree, &first_id), first_ix);
        assert_eq!(node_ix(&tree, &third_id), third_ix);

        let mount_ids = tree.nearby_mounts_for(&host_id);
        assert_eq!(mount_ids.len(), 3);
        assert_eq!(mount_ids[0].id, first_id);
        assert_eq!(mount_ids[0].slot, NearbySlot::Above);
        assert_eq!(mount_ids[2].id, third_id);
        assert_eq!(mount_ids[2].slot, NearbySlot::InFront);

        let ghost_id = mount_ids[1].id;
        let ghost_ix = node_ix(&tree, &ghost_id);
        assert_eq!(mount_ids[1].slot, NearbySlot::Below);
        assert!(tree.get(&ghost_id).unwrap().is_ghost_root());
        assert_eq!(
            tree.nearby_ixs(host_ix),
            vec![
                NearbyMountIx {
                    slot: NearbySlot::Above,
                    ix: first_ix,
                },
                NearbyMountIx {
                    slot: NearbySlot::Below,
                    ix: ghost_ix,
                },
                NearbyMountIx {
                    slot: NearbySlot::InFront,
                    ix: third_ix,
                },
            ]
        );
        assert_eq!(
            tree.parent_link_of(first_ix),
            Some(ParentLink::Nearby {
                host: host_ix,
                slot: NearbySlot::Above,
            })
        );
        assert_eq!(
            tree.parent_link_of(third_ix),
            Some(ParentLink::Nearby {
                host: host_ix,
                slot: NearbySlot::InFront,
            })
        );
    }

    #[test]
    fn test_multiple_animated_nearby_removes_keep_all_ghost_mounts() {
        let host_id = NodeId::from_term_bytes(vec![175]);
        let nearby_ids: Vec<NodeId> = (0..6)
            .map(|index| NodeId::from_term_bytes(vec![176 + index]))
            .collect();

        let mut tree = ElementTree::new();
        tree.set_root_id(host_id);
        tree.insert(plain_element(175));

        for (index, nearby_id) in nearby_ids.iter().enumerate() {
            let mut nearby = text_element(176 + index as u8, &format!("nearby {index}"));
            if (1..=3).contains(&index) {
                nearby.layout.effective.animate_exit = Some(exit_alpha_spec());
                nearby.spec.declared.animate_exit = Some(exit_alpha_spec());
            }
            assert_eq!(nearby.id, *nearby_id);
            tree.insert(nearby);
        }

        tree.set_nearby_mounts(
            &host_id,
            nearby_ids
                .iter()
                .copied()
                .map(|id| NearbyMount {
                    slot: NearbySlot::OnRight,
                    id,
                })
                .collect(),
        )
        .unwrap();

        apply_patches(
            &mut tree,
            vec![
                Patch::Remove { id: nearby_ids[1] },
                Patch::Remove { id: nearby_ids[2] },
                Patch::Remove { id: nearby_ids[3] },
            ],
        )
        .unwrap();

        let mounts = tree.nearby_mounts_for(&host_id);
        let ghost_ids: Vec<NodeId> = mounts
            .iter()
            .map(|mount| mount.id)
            .filter(|id| tree.get(id).is_some_and(Element::is_ghost_root))
            .collect();

        assert_eq!(ghost_ids.len(), 3);
        assert!(ghost_ids.iter().all(|id| {
            tree.get(id)
                .and_then(|ghost| ghost.lifecycle.ghost_attachment.as_ref())
                .is_some_and(|attachment| {
                    matches!(
                        attachment,
                        GhostAttachment::Nearby {
                            host_id: ghost_host_id,
                            mount_index: 1,
                            slot: NearbySlot::OnRight,
                            ..
                        } if ghost_host_id == &host_id
                    )
                })
        }));
        assert_eq!(
            mounts.iter().map(|mount| mount.id).collect::<Vec<_>>(),
            vec![
                nearby_ids[0],
                ghost_ids[0],
                ghost_ids[1],
                ghost_ids[2],
                nearby_ids[4],
                nearby_ids[5],
            ]
        );
    }

    #[test]
    fn test_preserve_runtime_attrs_on_patch() {
        let id = NodeId::from_term_bytes(vec![1]);
        let attrs = Attrs {
            scroll_x: Some(12.0),
            scroll_y: Some(34.0),
            scroll_x_max: Some(50.0),
            scroll_y_max: Some(60.0),
            scrollbar_y: Some(true),
            scrollbar_hover_axis: Some(crate::tree::attrs::ScrollbarHoverAxis::Y),
            ..Attrs::default()
        };

        let element = Element::with_attrs(id, ElementKind::El, Vec::new(), attrs);
        let mut tree = ElementTree::new();
        tree.set_root_id(id);
        tree.insert(element);

        let patch = Patch::SetAttrs {
            id,
            attrs_raw: Vec::new(),
        };

        apply_patch(&mut tree, patch, 1).unwrap();

        let updated = tree.get(&id).unwrap();
        assert_eq!(updated.layout.scroll_x, 12.0);
        assert_eq!(updated.layout.scroll_y, 34.0);
        assert_eq!(updated.layout.scroll_x_max, 50.0);
        assert_eq!(updated.layout.scroll_y_max, 60.0);
        assert_eq!(updated.runtime.scrollbar_hover_axis, None);
    }

    #[test]
    fn test_preserve_runtime_attrs_on_patch_when_axis_present() {
        let id = NodeId::from_term_bytes(vec![1]);
        let attrs = Attrs {
            scroll_x: Some(12.0),
            scroll_y: Some(34.0),
            scroll_x_max: Some(50.0),
            scroll_y_max: Some(60.0),
            scrollbar_y: Some(true),
            scrollbar_hover_axis: Some(crate::tree::attrs::ScrollbarHoverAxis::Y),
            ..Attrs::default()
        };

        let element = Element::with_attrs(id, ElementKind::El, Vec::new(), attrs);
        let mut tree = ElementTree::new();
        tree.set_root_id(id);
        tree.insert(element);

        let patch = Patch::SetAttrs {
            id,
            attrs_raw: vec![0, 1, 7, 1],
        };

        apply_patch(&mut tree, patch, 1).unwrap();

        let updated = tree.get(&id).unwrap();
        assert_eq!(updated.layout.effective.scrollbar_y, Some(true));
        assert_eq!(
            updated.layout.effective.scrollbar_hover_axis,
            Some(crate::tree::attrs::ScrollbarHoverAxis::Y)
        );
    }

    #[test]
    fn test_patch_clears_mouse_over_active_when_mouse_over_removed() {
        let id = NodeId::from_term_bytes(vec![1]);
        let attrs = Attrs {
            mouse_over: Some(crate::tree::attrs::MouseOverAttrs::default()),
            mouse_over_active: Some(true),
            ..Attrs::default()
        };

        let element = Element::with_attrs(id, ElementKind::El, Vec::new(), attrs);
        let mut tree = ElementTree::new();
        tree.set_root_id(id);
        tree.insert(element);

        let patch = Patch::SetAttrs {
            id,
            attrs_raw: Vec::new(),
        };

        apply_patch(&mut tree, patch, 1).unwrap();

        let updated = tree.get(&id).unwrap();
        assert_eq!(updated.layout.effective.mouse_over, None);
        assert!(!updated.runtime.mouse_over_active);
    }

    #[test]
    fn test_apply_patches_advances_revision_once_per_batch() {
        let id = NodeId::from_term_bytes(vec![1]);
        let element = Element::with_attrs(id, ElementKind::El, Vec::new(), Attrs::default());
        let mut tree = ElementTree::new();
        tree.set_root_id(id);
        tree.insert(element);

        apply_patches(
            &mut tree,
            vec![Patch::SetAttrs {
                id,
                attrs_raw: Vec::new(),
            }],
        )
        .unwrap();

        assert_eq!(tree.revision(), 1);
    }

    #[test]
    fn test_apply_patches_classifies_event_attr_as_registry() {
        let id = NodeId::from_term_bytes(vec![1]);
        let element = Element::with_attrs(id, ElementKind::El, Vec::new(), Attrs::default());
        let mut tree = ElementTree::new();
        tree.set_root_id(id);
        tree.insert(element);

        let invalidation = apply_patches(
            &mut tree,
            vec![Patch::SetAttrs {
                id,
                attrs_raw: vec![0, 1, 40, 1],
            }],
        )
        .unwrap();

        assert_eq!(invalidation, TreeInvalidation::Registry);
    }

    #[test]
    fn test_apply_patches_classifies_visual_attr_as_paint() {
        let id = NodeId::from_term_bytes(vec![1]);
        let element = Element::with_attrs(id, ElementKind::El, Vec::new(), Attrs::default());
        let mut tree = ElementTree::new();
        tree.set_root_id(id);
        tree.insert(element);

        let invalidation = apply_patches(
            &mut tree,
            vec![Patch::SetAttrs {
                id,
                attrs_raw: vec![0, 1, 12, 0, 1, 255, 0, 0, 255],
            }],
        )
        .unwrap();

        assert_eq!(invalidation, TreeInvalidation::Paint);
    }

    #[test]
    fn test_apply_patches_invalidates_paragraph_for_inherited_text_paint_attr() {
        let root_id = NodeId::from_term_bytes(vec![1]);
        let paragraph_id = NodeId::from_term_bytes(vec![2]);
        let text_id = NodeId::from_term_bytes(vec![3]);
        let mut tree = ElementTree::new();
        tree.set_root_id(root_id);
        tree.insert(Element::with_attrs(
            root_id,
            ElementKind::El,
            Vec::new(),
            Attrs::default(),
        ));
        tree.insert(Element::with_attrs(
            paragraph_id,
            ElementKind::Paragraph,
            Vec::new(),
            Attrs {
                width: Some(Length::Px(200.0)),
                ..Attrs::default()
            },
        ));
        tree.insert(Element::with_attrs(
            text_id,
            ElementKind::Text,
            Vec::new(),
            Attrs {
                content: Some("Todo item".to_string()),
                ..Attrs::default()
            },
        ));
        tree.set_children(&root_id, vec![paragraph_id]).unwrap();
        tree.set_children(&paragraph_id, vec![text_id]).unwrap();
        layout_tree_default(&mut tree, Constraint::new(400.0, 200.0), 1.0);

        assert!(
            tree.get(&paragraph_id)
                .unwrap()
                .layout
                .paragraph_fragments
                .as_ref()
                .unwrap()
                .iter()
                .all(|fragment| !fragment.strike)
        );

        let invalidation = apply_patches(
            &mut tree,
            vec![Patch::SetAttrs {
                id: root_id,
                attrs_raw: vec![0, 1, 48, 1],
            }],
        )
        .unwrap();

        assert_eq!(invalidation, TreeInvalidation::Resolve);
        assert!(tree.get(&paragraph_id).unwrap().layout.resolve_dirty);

        layout_tree_default(&mut tree, Constraint::new(400.0, 200.0), 1.0);
        assert!(
            tree.get(&paragraph_id)
                .unwrap()
                .layout
                .paragraph_fragments
                .as_ref()
                .unwrap()
                .iter()
                .all(|fragment| fragment.strike)
        );
    }

    #[test]
    fn test_apply_patches_classifies_content_attr_as_measure() {
        let id = NodeId::from_term_bytes(vec![1]);
        let element = Element::with_attrs(id, ElementKind::Text, Vec::new(), Attrs::default());
        let mut tree = ElementTree::new();
        tree.set_root_id(id);
        tree.insert(element);

        let invalidation = apply_patches(
            &mut tree,
            vec![Patch::SetAttrs {
                id,
                attrs_raw: vec![0, 1, 21, 0, 3, b'a', b'b', b'c'],
            }],
        )
        .unwrap();

        assert_eq!(invalidation, TreeInvalidation::Measure);
    }

    #[test]
    fn test_apply_patches_classifies_fixed_box_content_attr_as_paint() {
        let id = NodeId::from_term_bytes(vec![1]);
        let element = Element::with_attrs(
            id,
            ElementKind::Text,
            Vec::new(),
            Attrs {
                width: Some(Length::Px(120.0)),
                height: Some(Length::Px(24.0)),
                content: Some("ab".to_string()),
                ..Attrs::default()
            },
        );
        let mut tree = ElementTree::new();
        tree.set_root_id(id);
        tree.insert(element);

        let invalidation = apply_patches(
            &mut tree,
            vec![Patch::SetAttrs {
                id,
                attrs_raw: fixed_content_attrs_raw("abc", 120.0, 24.0),
            }],
        )
        .unwrap();

        assert_eq!(invalidation, TreeInvalidation::Paint);
    }

    #[test]
    fn test_apply_patches_classifies_single_line_text_input_content_as_paint() {
        let id = NodeId::from_term_bytes(vec![7]);
        let element = Element::with_attrs(
            id,
            ElementKind::TextInput,
            Vec::new(),
            Attrs {
                width: Some(Length::Fill),
                content: Some("ab".to_string()),
                ..Attrs::default()
            },
        );
        let mut tree = ElementTree::new();
        tree.set_root_id(id);
        tree.insert(element);

        let mut raw = vec![0, 2, 1, 0];
        raw.extend_from_slice(&content_attr_raw_tail("abc"));
        let invalidation =
            apply_patches(&mut tree, vec![Patch::SetAttrs { id, attrs_raw: raw }]).unwrap();

        assert_eq!(invalidation, TreeInvalidation::Paint);
    }

    #[test]
    fn test_apply_patches_keeps_multiline_text_input_content_as_measure() {
        let id = NodeId::from_term_bytes(vec![8]);
        let element = Element::with_attrs(
            id,
            ElementKind::TextInput,
            Vec::new(),
            Attrs {
                width: Some(Length::Fill),
                content: Some("ab".to_string()),
                ..Attrs::default()
            },
        );
        let mut tree = ElementTree::new();
        tree.set_root_id(id);
        tree.insert(element);

        let mut raw = vec![0, 2, 1, 0];
        raw.extend_from_slice(&content_attr_raw_tail("ab\nc"));
        let invalidation =
            apply_patches(&mut tree, vec![Patch::SetAttrs { id, attrs_raw: raw }]).unwrap();

        assert_eq!(invalidation, TreeInvalidation::Measure);
    }

    #[test]
    fn test_apply_patches_classifies_plain_column_text_label_content_as_paint() {
        let root_id = NodeId::from_term_bytes(vec![1]);
        let wrapper_id = NodeId::from_term_bytes(vec![2]);
        let text_id = NodeId::from_term_bytes(vec![3]);
        let mut tree = ElementTree::new();
        tree.set_root_id(root_id);
        tree.insert(Element::with_attrs(
            root_id,
            ElementKind::Column,
            Vec::new(),
            Attrs::default(),
        ));
        tree.insert(Element::with_attrs(
            wrapper_id,
            ElementKind::El,
            Vec::new(),
            Attrs::default(),
        ));
        tree.insert(Element::with_attrs(
            text_id,
            ElementKind::Text,
            Vec::new(),
            Attrs {
                content: Some("Value: a".to_string()),
                ..Attrs::default()
            },
        ));
        tree.set_children(&root_id, vec![wrapper_id]).unwrap();
        tree.set_children(&wrapper_id, vec![text_id]).unwrap();
        layout_tree_default(&mut tree, Constraint::new(200.0, 200.0), 1.0);
        let before_text_width = tree
            .get(&text_id)
            .and_then(|element| element.layout.frame)
            .map(|frame| frame.width)
            .unwrap();
        let before_wrapper_width = tree
            .get(&wrapper_id)
            .and_then(|element| element.layout.frame)
            .map(|frame| frame.width)
            .unwrap();
        let before_column_width = tree
            .get(&root_id)
            .and_then(|element| element.layout.frame)
            .map(|frame| frame.width)
            .unwrap();

        let invalidation = apply_patches(
            &mut tree,
            vec![Patch::SetAttrs {
                id: text_id,
                attrs_raw: content_only_attrs_raw("Value: ab"),
            }],
        )
        .unwrap();

        let after_text_width = tree
            .get(&text_id)
            .and_then(|element| element.layout.frame)
            .map(|frame| frame.width)
            .unwrap();
        let after_wrapper_width = tree
            .get(&wrapper_id)
            .and_then(|element| element.layout.frame)
            .map(|frame| frame.width)
            .unwrap();
        let after_column_width = tree
            .get(&root_id)
            .and_then(|element| element.layout.frame)
            .map(|frame| frame.width)
            .unwrap();
        assert_eq!(invalidation, TreeInvalidation::Paint);
        assert!(after_text_width > before_text_width);
        assert!(after_wrapper_width > before_wrapper_width);
        assert!(after_column_width > before_column_width);
    }

    #[test]
    fn test_apply_patches_keeps_row_text_label_content_as_measure() {
        let root_id = NodeId::from_term_bytes(vec![4]);
        let wrapper_id = NodeId::from_term_bytes(vec![5]);
        let text_id = NodeId::from_term_bytes(vec![6]);
        let mut tree = ElementTree::new();
        tree.set_root_id(root_id);
        tree.insert(Element::with_attrs(
            root_id,
            ElementKind::Row,
            Vec::new(),
            Attrs::default(),
        ));
        tree.insert(Element::with_attrs(
            wrapper_id,
            ElementKind::El,
            Vec::new(),
            Attrs::default(),
        ));
        tree.insert(Element::with_attrs(
            text_id,
            ElementKind::Text,
            Vec::new(),
            Attrs {
                content: Some("Value: a".to_string()),
                ..Attrs::default()
            },
        ));
        tree.set_children(&root_id, vec![wrapper_id]).unwrap();
        tree.set_children(&wrapper_id, vec![text_id]).unwrap();

        let invalidation = apply_patches(
            &mut tree,
            vec![Patch::SetAttrs {
                id: text_id,
                attrs_raw: content_only_attrs_raw("Value: ab"),
            }],
        )
        .unwrap();

        assert_eq!(invalidation, TreeInvalidation::Measure);
    }

    fn fixed_content_attrs_raw(content: &str, width: f64, height: f64) -> Vec<u8> {
        let mut raw = vec![0, 3, 1, 2];
        raw.extend_from_slice(&width.to_be_bytes());
        raw.extend_from_slice(&[2, 2]);
        raw.extend_from_slice(&height.to_be_bytes());
        raw.extend_from_slice(&content_attr_raw_tail(content));
        raw
    }

    fn content_only_attrs_raw(content: &str) -> Vec<u8> {
        let mut raw = vec![0, 1];
        raw.extend_from_slice(&content_attr_raw_tail(content));
        raw
    }

    fn content_attr_raw_tail(content: &str) -> Vec<u8> {
        let mut raw = Vec::new();
        raw.push(21);
        raw.extend_from_slice(&(content.len() as u16).to_be_bytes());
        raw.extend_from_slice(content.as_bytes());
        raw
    }

    #[test]
    fn test_apply_patches_classifies_child_changes_as_structure() {
        let parent_id = NodeId::from_term_bytes(vec![1]);
        let child_id = NodeId::from_term_bytes(vec![2]);
        let parent = Element::with_attrs(parent_id, ElementKind::El, Vec::new(), Attrs::default());
        let child = Element::with_attrs(child_id, ElementKind::El, Vec::new(), Attrs::default());
        let mut tree = ElementTree::new();
        tree.set_root_id(parent_id);
        tree.insert(parent);
        tree.insert(child);

        let invalidation = apply_patches(
            &mut tree,
            vec![Patch::SetChildren {
                id: parent_id,
                children: vec![child_id],
            }],
        )
        .unwrap();

        assert_eq!(invalidation, TreeInvalidation::Structure);
    }

    #[test]
    fn test_insert_subtree_stamps_inserted_nodes_with_batch_revision() {
        let parent_id = NodeId::from_term_bytes(vec![1]);
        let child_id = NodeId::from_term_bytes(vec![2]);

        let parent = Element::with_attrs(parent_id, ElementKind::El, Vec::new(), Attrs::default());

        let child = Element::with_attrs(child_id, ElementKind::El, Vec::new(), Attrs::default());

        let mut subtree = ElementTree::new();
        subtree.set_root_id(child_id);
        subtree.insert(child);

        let mut tree = ElementTree::new();
        tree.set_root_id(parent_id);
        tree.insert(parent);

        apply_patches(
            &mut tree,
            vec![Patch::InsertSubtree {
                parent_id: Some(parent_id),
                index: 0,
                subtree,
            }],
        )
        .unwrap();

        let inserted = tree.get(&child_id).expect("inserted child should exist");
        assert_eq!(inserted.lifecycle.mounted_at_revision, tree.revision());
    }

    #[test]
    fn test_set_attrs_preserves_existing_mount_revision() {
        let id = NodeId::from_term_bytes(vec![7]);
        let mut element = Element::with_attrs(id, ElementKind::El, Vec::new(), Attrs::default());
        element.lifecycle.mounted_at_revision = 4;

        let mut tree = ElementTree::new();
        tree.set_root_id(id);
        tree.insert(element);

        apply_patches(
            &mut tree,
            vec![Patch::SetAttrs {
                id,
                attrs_raw: Vec::new(),
            }],
        )
        .unwrap();

        assert_eq!(tree.get(&id).unwrap().lifecycle.mounted_at_revision, 4);
    }

    #[test]
    fn test_set_attrs_marks_text_input_content_as_tree_patch_when_content_present() {
        let id = NodeId::from_term_bytes(vec![17]);
        let attrs = Attrs {
            content: Some("before".to_string()),
            ..Attrs::default()
        };
        let mut element = Element::with_attrs(id, ElementKind::TextInput, Vec::new(), attrs);
        element.runtime.text_input_content_origin = TextInputContentOrigin::Event;

        let mut tree = ElementTree::new();
        tree.set_root_id(id);
        tree.insert(element);

        apply_patches(
            &mut tree,
            vec![Patch::SetAttrs {
                id,
                attrs_raw: vec![0, 1, 21, 0, 5, b'a', b'f', b't', b'e', b'r'],
            }],
        )
        .unwrap();

        let updated = tree.get(&id).unwrap();
        assert_eq!(updated.spec.declared.content.as_deref(), Some("after"));
        assert_eq!(
            updated.runtime.text_input_content_origin,
            TextInputContentOrigin::TreePatch
        );
    }

    #[test]
    fn test_set_attrs_preserves_text_input_content_origin_when_content_absent() {
        let id = NodeId::from_term_bytes(vec![18]);
        let attrs = Attrs {
            content: Some("before".to_string()),
            ..Attrs::default()
        };
        let mut element = Element::with_attrs(id, ElementKind::TextInput, Vec::new(), attrs);
        element.runtime.text_input_content_origin = TextInputContentOrigin::Event;

        let mut tree = ElementTree::new();
        tree.set_root_id(id);
        tree.insert(element);

        apply_patches(
            &mut tree,
            vec![Patch::SetAttrs {
                id,
                attrs_raw: Vec::new(),
            }],
        )
        .unwrap();

        assert_eq!(
            tree.get(&id).unwrap().runtime.text_input_content_origin,
            TextInputContentOrigin::Event
        );
    }

    #[test]
    fn test_set_attrs_buffers_focused_text_input_patch_content() {
        let id = NodeId::from_term_bytes(vec![181]);
        let attrs = Attrs {
            content: Some("before".to_string()),
            text_input_focused: Some(true),
            ..Attrs::default()
        };
        let mut element = Element::with_attrs(id, ElementKind::TextInput, Vec::new(), attrs);
        element.runtime.text_input_content_origin = TextInputContentOrigin::Event;

        let mut tree = ElementTree::new();
        tree.set_root_id(id);
        tree.insert(element);

        apply_patches(
            &mut tree,
            vec![Patch::SetAttrs {
                id,
                attrs_raw: vec![0, 1, 21, 0, 5, b'a', b'f', b't', b'e', b'r'],
            }],
        )
        .unwrap();

        let updated = tree.get(&id).unwrap();
        assert_eq!(updated.spec.declared.content.as_deref(), Some("before"));
        assert_eq!(updated.layout.effective.content.as_deref(), Some("before"));
        assert_eq!(updated.runtime.patch_content.as_deref(), Some("after"));
        assert_eq!(
            updated.runtime.text_input_content_origin,
            TextInputContentOrigin::Event
        );
    }

    #[test]
    fn test_set_attrs_clears_patch_content_when_unfocused_text_input_accepts_patch() {
        let id = NodeId::from_term_bytes(vec![182]);
        let attrs = Attrs {
            content: Some("before".to_string()),
            ..Attrs::default()
        };
        let mut element = Element::with_attrs(id, ElementKind::TextInput, Vec::new(), attrs);
        element.runtime.patch_content = Some("stale".to_string());

        let mut tree = ElementTree::new();
        tree.set_root_id(id);
        tree.insert(element);

        apply_patches(
            &mut tree,
            vec![Patch::SetAttrs {
                id,
                attrs_raw: vec![0, 1, 21, 0, 5, b'a', b'f', b't', b'e', b'r'],
            }],
        )
        .unwrap();

        let updated = tree.get(&id).unwrap();
        assert_eq!(updated.spec.declared.content.as_deref(), Some("after"));
        assert_eq!(updated.runtime.patch_content, None);
        assert_eq!(
            updated.runtime.text_input_content_origin,
            TextInputContentOrigin::TreePatch
        );
    }

    #[test]
    fn test_runtime_owned_slider_patch_matching_current_value_is_noop() {
        let id = NodeId::from_term_bytes(vec![183]);
        let attrs = Attrs {
            slider_value: Some(60.0),
            ..Attrs::default()
        };
        let mut element = Element::with_attrs(id, ElementKind::Slider, Vec::new(), attrs);
        element.runtime.slider_value_origin = SliderValueOrigin::Event;

        let mut tree = ElementTree::new();
        tree.set_root_id(id);
        tree.insert(element);

        let invalidation = apply_patches(
            &mut tree,
            vec![Patch::SetAttrs {
                id,
                attrs_raw: slider_value_attrs_raw(60.0),
            }],
        )
        .unwrap();

        let updated = tree.get(&id).unwrap();
        assert_eq!(invalidation, TreeInvalidation::None);
        assert_eq!(updated.spec.declared.slider_value, Some(60.0));
        assert_eq!(updated.layout.effective.slider_value, Some(60.0));
        assert_eq!(updated.runtime.slider_patch_value, None);
        assert_eq!(
            updated.runtime.slider_value_origin,
            SliderValueOrigin::Event
        );
    }

    #[test]
    fn test_set_children_preserves_existing_mount_revisions() {
        let parent_id = NodeId::from_term_bytes(vec![8]);
        let first_id = NodeId::from_term_bytes(vec![9]);
        let second_id = NodeId::from_term_bytes(vec![10]);

        let mut parent =
            Element::with_attrs(parent_id, ElementKind::El, Vec::new(), Attrs::default());
        parent.children = vec![first_id, second_id];
        parent.lifecycle.mounted_at_revision = 2;

        let mut first =
            Element::with_attrs(first_id, ElementKind::El, Vec::new(), Attrs::default());
        first.lifecycle.mounted_at_revision = 2;

        let mut second =
            Element::with_attrs(second_id, ElementKind::El, Vec::new(), Attrs::default());
        second.lifecycle.mounted_at_revision = 3;

        let mut tree = ElementTree::new();
        tree.set_root_id(parent_id);
        tree.insert(parent);
        tree.insert(first);
        tree.insert(second);

        apply_patches(
            &mut tree,
            vec![Patch::SetChildren {
                id: parent_id,
                children: vec![second_id, first_id],
            }],
        )
        .unwrap();

        assert_eq!(
            tree.get(&first_id).unwrap().lifecycle.mounted_at_revision,
            2
        );
        assert_eq!(
            tree.get(&second_id).unwrap().lifecycle.mounted_at_revision,
            3
        );
    }

    #[test]
    fn test_remove_then_reinsert_stamps_new_mount_revision() {
        let parent_id = NodeId::from_term_bytes(vec![11]);
        let child_id = NodeId::from_term_bytes(vec![12]);

        let mut parent =
            Element::with_attrs(parent_id, ElementKind::El, Vec::new(), Attrs::default());
        parent.children = vec![child_id];

        let mut child =
            Element::with_attrs(child_id, ElementKind::El, Vec::new(), Attrs::default());
        child.lifecycle.mounted_at_revision = 1;

        let mut tree = ElementTree::new();
        tree.set_root_id(parent_id);
        tree.insert(parent);
        tree.insert(child);
        tree.set_revision(1);

        apply_patches(&mut tree, vec![Patch::Remove { id: child_id }]).unwrap();
        let removed_revision = tree.revision();

        let mut subtree = ElementTree::new();
        subtree.set_root_id(child_id);
        subtree.insert(Element::with_attrs(
            child_id,
            ElementKind::El,
            Vec::new(),
            Attrs::default(),
        ));

        apply_patches(
            &mut tree,
            vec![Patch::InsertSubtree {
                parent_id: Some(parent_id),
                index: 0,
                subtree,
            }],
        )
        .unwrap();

        assert!(tree.get(&child_id).unwrap().lifecycle.mounted_at_revision > removed_revision);
    }

    #[test]
    fn test_remove_with_animate_exit_creates_sanitized_child_ghost() {
        let parent_id = NodeId::from_term_bytes(vec![20]);
        let child_id = NodeId::from_term_bytes(vec![21]);

        let mut parent =
            Element::with_attrs(parent_id, ElementKind::El, Vec::new(), Attrs::default());
        parent.children = vec![child_id];

        let child_attrs = Attrs {
            content: Some("ghosted".to_string()),
            on_click: Some(true),
            mouse_over: Some(crate::tree::attrs::MouseOverAttrs::default()),
            mouse_over_active: Some(true),
            scrollbar_y: Some(true),
            scroll_y: Some(12.0),
            scroll_y_max: Some(40.0),
            text_input_focused: Some(true),
            text_input_cursor: Some(2),
            animate_exit: Some(exit_alpha_spec()),
            ..Attrs::default()
        };

        let mut child =
            Element::with_attrs(child_id, ElementKind::TextInput, Vec::new(), child_attrs);
        child.layout.frame = Some(Frame {
            x: 0.0,
            y: 0.0,
            width: 80.0,
            height: 24.0,
            content_width: 80.0,
            content_height: 120.0,
        });

        let mut tree = ElementTree::new();
        tree.set_root_id(parent_id);
        tree.insert(parent);
        tree.insert(child);

        apply_patches(&mut tree, vec![Patch::Remove { id: child_id }]).unwrap();

        assert!(tree.get(&child_id).is_none());

        let parent = tree.get(&parent_id).unwrap();
        assert_eq!(parent.children.len(), 1);
        let ghost_id = parent.children[0];
        assert_ne!(ghost_id, child_id);

        let ghost = tree.get(&ghost_id).expect("ghost root should exist");
        assert!(ghost.is_ghost_root());
        assert_eq!(ghost.spec.kind, ElementKind::Text);
        assert_eq!(ghost.layout.effective.on_click, None);
        assert_eq!(ghost.layout.effective.mouse_over, None);
        assert!(!ghost.runtime.mouse_over_active);
        assert_eq!(ghost.layout.effective.scrollbar_y, None);
        assert_eq!(ghost.layout.effective.ghost_scrollbar_y, Some(true));
        assert_eq!(ghost.layout.scroll_y, 12.0);
        assert_eq!(ghost.layout.effective.text_input_focused, None);
        assert!(ghost.lifecycle.ghost_exit_animation.is_some());
    }

    #[test]
    fn test_remove_with_animate_exit_preserves_ghost_subtree_topology() {
        let parent_id = NodeId::from_term_bytes(vec![120]);
        let removed_id = NodeId::from_term_bytes(vec![121]);
        let text_id = NodeId::from_term_bytes(vec![122]);

        let mut parent =
            Element::with_attrs(parent_id, ElementKind::El, Vec::new(), Attrs::default());
        parent.children = vec![removed_id];

        let removed_attrs = Attrs {
            animate_exit: Some(exit_alpha_spec()),
            ..Attrs::default()
        };
        let mut removed =
            Element::with_attrs(removed_id, ElementKind::Row, Vec::new(), removed_attrs);
        removed.children = vec![text_id];
        removed.paint_children = vec![text_id];

        let text = text_element(122, "ghost content");

        let mut tree = ElementTree::new();
        tree.set_root_id(parent_id);
        tree.insert(parent);
        tree.insert(removed);
        tree.insert(text);

        apply_patches(&mut tree, vec![Patch::Remove { id: removed_id }]).unwrap();

        let ghost_root_id = tree.child_ids(&parent_id)[0];
        let ghost_children = tree.child_ids(&ghost_root_id);
        assert_eq!(ghost_children.len(), 1);
        assert!(tree.get(&ghost_children[0]).unwrap().is_ghost());
        assert_eq!(tree.paint_child_ids_for(&ghost_root_id), ghost_children);
    }

    #[test]
    fn test_remove_with_animate_exit_keeps_rendering_but_drops_press_listener() {
        let root_id = NodeId::from_term_bytes(vec![30]);
        let child_id = NodeId::from_term_bytes(vec![31]);

        let mut root = Element::with_attrs(root_id, ElementKind::El, Vec::new(), Attrs::default());
        root.children = vec![child_id];
        root.layout.frame = Some(text_frame(0.0, 0.0, 120.0, 40.0));

        let mut child = text_element(31, "bye");
        child.layout.effective.on_click = Some(true);
        child.spec.declared.on_click = Some(true);
        child.layout.effective.animate_exit = Some(exit_alpha_spec());
        child.spec.declared.animate_exit = Some(exit_alpha_spec());
        child.layout.frame = Some(text_frame(8.0, 8.0, 48.0, 20.0));

        let mut tree = ElementTree::new();
        tree.set_root_id(root_id);
        tree.insert(root);
        tree.insert(child);

        apply_patches(&mut tree, vec![Patch::Remove { id: child_id }]).unwrap();

        let output = crate::tree::render::render_tree(&tree);
        let press_ids: Vec<_> = output
            .event_rebuild
            .base_registry
            .view()
            .iter_precedence()
            .filter(|listener| {
                listener.matcher.kind() == ListenerMatcherKind::CursorButtonLeftPressInside
            })
            .filter_map(|listener| listener.element_id)
            .collect();

        assert!(!output.scene.nodes.is_empty());
        assert!(press_ids.is_empty());
    }

    #[test]
    fn test_insert_subtree_preserves_ghost_anchor_against_live_order() {
        let parent_id = NodeId::from_term_bytes(vec![40]);
        let first_id = NodeId::from_term_bytes(vec![41]);
        let removed_id = NodeId::from_term_bytes(vec![42]);
        let third_id = NodeId::from_term_bytes(vec![43]);
        let new_id = NodeId::from_term_bytes(vec![44]);

        let mut parent =
            Element::with_attrs(parent_id, ElementKind::Row, Vec::new(), Attrs::default());
        parent.children = vec![first_id, removed_id, third_id];

        let first = text_element(41, "a");

        let mut removed = text_element(42, "b");
        removed.layout.effective.animate_exit = Some(exit_alpha_spec());
        removed.spec.declared.animate_exit = Some(exit_alpha_spec());

        let third = text_element(43, "c");

        let mut tree = ElementTree::new();
        tree.set_root_id(parent_id);
        tree.insert(parent);
        tree.insert(first);
        tree.insert(removed);
        tree.insert(third);

        let mut subtree = ElementTree::new();
        subtree.set_root_id(new_id);
        subtree.insert(text_element(44, "d"));

        apply_patches(
            &mut tree,
            vec![
                Patch::Remove { id: removed_id },
                Patch::InsertSubtree {
                    parent_id: Some(parent_id),
                    index: 1,
                    subtree,
                },
            ],
        )
        .unwrap();

        let parent = tree.get(&parent_id).unwrap();
        assert_eq!(parent.children.len(), 4);
        assert_eq!(parent.children[0], first_id);
        assert!(tree.get(&parent.children[1]).unwrap().is_ghost_root());
        assert_eq!(parent.children[2], new_id);
        assert_eq!(parent.children[3], third_id);
    }

    #[test]
    fn test_insert_nearby_subtree_keeps_ghost_before_new_live_nearby() {
        let host_id = NodeId::from_term_bytes(vec![50]);
        let old_nearby_id = NodeId::from_term_bytes(vec![51]);
        let new_nearby_id = NodeId::from_term_bytes(vec![52]);

        let mut host = Element::with_attrs(host_id, ElementKind::El, Vec::new(), Attrs::default());
        host.nearby.set(NearbySlot::OnRight, Some(old_nearby_id));

        let mut old_nearby = text_element(51, "old");
        old_nearby.layout.effective.animate_exit = Some(exit_alpha_spec());
        old_nearby.spec.declared.animate_exit = Some(exit_alpha_spec());

        let mut tree = ElementTree::new();
        tree.set_root_id(host_id);
        tree.insert(host);
        tree.insert(old_nearby);

        let mut subtree = ElementTree::new();
        subtree.set_root_id(new_nearby_id);
        subtree.insert(text_element(52, "new"));

        apply_patches(
            &mut tree,
            vec![
                Patch::Remove { id: old_nearby_id },
                Patch::InsertNearbySubtree {
                    host_id,
                    index: 0,
                    slot: NearbySlot::OnRight,
                    subtree,
                },
            ],
        )
        .unwrap();

        let host = tree.get(&host_id).unwrap();
        let slot_ids: Vec<_> = host.nearby.ids(NearbySlot::OnRight).cloned().collect();
        assert_eq!(slot_ids.len(), 2);
        assert!(tree.get(&slot_ids[0]).unwrap().is_ghost_root());
        assert_eq!(slot_ids[1], new_nearby_id);
    }

    #[test]
    fn test_insert_nearby_subtree_keeps_ghost_before_new_live_nearby_across_slots() {
        let host_id = NodeId::from_term_bytes(vec![53]);
        let old_nearby_id = NodeId::from_term_bytes(vec![54]);
        let new_nearby_id = NodeId::from_term_bytes(vec![55]);

        let mut host = Element::with_attrs(host_id, ElementKind::El, Vec::new(), Attrs::default());
        host.nearby.set(NearbySlot::InFront, Some(old_nearby_id));

        let mut old_nearby = text_element(54, "old");
        old_nearby.layout.effective.animate_exit = Some(exit_alpha_spec());
        old_nearby.spec.declared.animate_exit = Some(exit_alpha_spec());

        let mut tree = ElementTree::new();
        tree.set_root_id(host_id);
        tree.insert(host);
        tree.insert(old_nearby);

        let mut subtree = ElementTree::new();
        subtree.set_root_id(new_nearby_id);
        subtree.insert(text_element(55, "new"));

        apply_patches(
            &mut tree,
            vec![
                Patch::Remove { id: old_nearby_id },
                Patch::InsertNearbySubtree {
                    host_id,
                    index: 0,
                    slot: NearbySlot::Below,
                    subtree,
                },
            ],
        )
        .unwrap();

        let host = tree.get(&host_id).unwrap();
        assert_eq!(host.nearby.mounts.len(), 2);
        assert!(tree.get(&host.nearby.mounts[0].id).unwrap().is_ghost_root());
        assert_eq!(host.nearby.mounts[0].slot, NearbySlot::InFront);
        assert_eq!(host.nearby.mounts[1].id, new_nearby_id);
        assert_eq!(host.nearby.mounts[1].slot, NearbySlot::Below);
    }
}
