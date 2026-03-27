//! Patch decoding and application for incremental tree updates.
//!
//! Patch binary format:
//! - Stream of operations, each starting with a tag byte:
//!   - 1: set_attrs - id_len(4) + id + attr_len(4) + attrs
//!   - 2: set_children - id_len(4) + id + count(2) + [child_id_len(4) + child_id]...
//!   - 3: insert_subtree - parent_len(4) + parent_id + index(2) + tree_len(4) + tree_bytes
//!   - 4: remove - id_len(4) + id
//!   - 5: insert_nearby_subtree - host_len(4) + host_id + slot(1) + tree_len(4) + tree_bytes

use super::animation::{scale_animation_spec, AnimationSpec};
use super::attrs::{
    decode_attrs, effective_scrollbar_x, effective_scrollbar_y, preserve_runtime_scroll_attrs,
    Attrs,
};
use super::deserialize::{decode_tree, DecodeError};
use super::element::{
    Element, ElementId, ElementKind, ElementTree, GhostAttachment, NearbyMounts, NearbySlot,
    NodeResidency, TextInputContentOrigin,
};
use std::collections::{HashMap, HashSet};

/// A single patch operation.
#[derive(Debug, Clone)]
pub enum Patch {
    /// Update attributes for an existing node.
    SetAttrs { id: ElementId, attrs_raw: Vec<u8> },

    /// Replace the children list for an existing node.
    SetChildren {
        id: ElementId,
        children: Vec<ElementId>,
    },

    /// Insert a new subtree.
    InsertSubtree {
        /// Parent ID (None if inserting as new root).
        parent_id: Option<ElementId>,
        /// Index in parent's children list.
        index: usize,
        /// The subtree to insert.
        subtree: ElementTree,
    },

    /// Insert a nearby-mounted subtree onto a host slot.
    InsertNearbySubtree {
        host_id: ElementId,
        slot: NearbySlot,
        subtree: ElementTree,
    },

    /// Remove a node and its descendants.
    Remove { id: ElementId },
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum AttachmentPoint {
    Root,
    Child {
        parent_id: ElementId,
        live_index: usize,
    },
    Nearby {
        host_id: ElementId,
        slot: NearbySlot,
    },
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
        3 => decode_insert_subtree(cursor),
        4 => decode_remove(cursor),
        5 => decode_insert_nearby_subtree(cursor),
        _ => Err(DecodeError::InvalidStructure(format!(
            "unknown patch tag: {}",
            tag
        ))),
    }
}

fn decode_set_attrs(cursor: &mut Cursor) -> Result<Patch, DecodeError> {
    let id_bytes = cursor.read_length_prefixed()?;
    let id = ElementId::from_term_bytes(id_bytes);

    let attrs_raw = cursor.read_length_prefixed()?;

    Ok(Patch::SetAttrs { id, attrs_raw })
}

fn decode_set_children(cursor: &mut Cursor) -> Result<Patch, DecodeError> {
    let id_bytes = cursor.read_length_prefixed()?;
    let id = ElementId::from_term_bytes(id_bytes);

    let count = cursor.read_u16_be()? as usize;
    let mut children = Vec::with_capacity(count);

    for _ in 0..count {
        let child_id_bytes = cursor.read_length_prefixed()?;
        children.push(ElementId::from_term_bytes(child_id_bytes));
    }

    Ok(Patch::SetChildren { id, children })
}

fn decode_insert_subtree(cursor: &mut Cursor) -> Result<Patch, DecodeError> {
    let parent_id_bytes = cursor.read_length_prefixed()?;

    // Check if parent_id is nil (Erlang atom nil serializes to specific bytes)
    // Erlang :nil atom serializes as <<131, 100, 0, 3, 110, 105, 108>> (ETF format)
    // or <<131, 119, 3, 110, 105, 108>> (newer atom format)
    let parent_id = if is_nil_term(&parent_id_bytes) {
        None
    } else {
        Some(ElementId::from_term_bytes(parent_id_bytes))
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
    let id_bytes = cursor.read_length_prefixed()?;
    let id = ElementId::from_term_bytes(id_bytes);

    Ok(Patch::Remove { id })
}

fn decode_insert_nearby_subtree(cursor: &mut Cursor) -> Result<Patch, DecodeError> {
    let host_id_bytes = cursor.read_length_prefixed()?;
    let host_id = ElementId::from_term_bytes(host_id_bytes);
    let slot_tag = cursor.read_u8()?;
    let slot = NearbySlot::from_tag(slot_tag).ok_or_else(|| {
        DecodeError::InvalidStructure(format!("unknown nearby slot tag: {}", slot_tag))
    })?;

    let tree_len = cursor.read_u32_be()? as usize;
    let tree_bytes = cursor.read_bytes(tree_len)?;
    let subtree = decode_tree(tree_bytes)?;

    Ok(Patch::InsertNearbySubtree {
        host_id,
        slot,
        subtree,
    })
}

/// Check if the term bytes represent Erlang nil atom.
fn is_nil_term(bytes: &[u8]) -> bool {
    // Erlang External Term Format for :nil atom
    // Old format: <<131, 100, 0, 3, "nil">> = <<131, 100, 0, 3, 110, 105, 108>>
    // New format: <<131, 119, 3, "nil">> = <<131, 119, 3, 110, 105, 108>>
    const NIL_OLD: &[u8] = &[131, 100, 0, 3, 110, 105, 108];
    const NIL_NEW: &[u8] = &[131, 119, 3, 110, 105, 108];

    bytes == NIL_OLD || bytes == NIL_NEW
}

/// Apply a list of patches to an element tree.
pub fn apply_patches(tree: &mut ElementTree, patches: Vec<Patch>) -> Result<(), String> {
    if patches.is_empty() {
        return Ok(());
    }

    let patches = filter_descendant_remove_patches(tree, patches);

    let batch_revision = tree.bump_revision();

    for patch in patches {
        apply_patch(tree, patch, batch_revision)?;
    }
    Ok(())
}

/// Apply a single patch to the tree.
fn apply_patch(tree: &mut ElementTree, patch: Patch, batch_revision: u64) -> Result<(), String> {
    match patch {
        Patch::SetAttrs { id, attrs_raw } => {
            let element = tree
                .get_mut(&id)
                .ok_or_else(|| "SetAttrs: node not found".to_string())?;
            element.attrs_raw = attrs_raw.clone();
            let decoded = decode_attrs(&attrs_raw).map_err(|e| e.to_string())?;
            let content_is_from_patch =
                element.kind == ElementKind::TextInput && decoded.content.is_some();
            element.base_attrs = decoded.clone();
            let mut merged = decoded;
            preserve_runtime_scroll_attrs(&element.attrs, &mut merged);
            element.attrs = merged;
            if content_is_from_patch {
                element.text_input_content_origin = TextInputContentOrigin::TreePatch;
            }
        }

        Patch::SetChildren { id, children } => {
            let merged_children = tree.merge_live_children_with_ghosts(&id, children);
            let element = tree
                .get_mut(&id)
                .ok_or_else(|| "SetChildren: node not found".to_string())?;
            element.children = merged_children;
        }

        Patch::InsertSubtree {
            parent_id,
            index,
            mut subtree,
        } => {
            // Get the root of the subtree
            let subtree_root_id = subtree
                .root
                .clone()
                .ok_or_else(|| "InsertSubtree: subtree has no root".to_string())?;

            subtree.stamp_all_mounted_at_revision(batch_revision);

            // Insert all nodes from subtree into main tree
            for (id, element) in subtree.nodes {
                tree.nodes.insert(id, element);
            }

            // Update parent's children or set as tree root
            match parent_id {
                Some(pid) => {
                    let mut live_children = tree.live_child_ids(&pid);
                    if !live_children.contains(&subtree_root_id) {
                        let insert_idx = index.min(live_children.len());
                        live_children.insert(insert_idx, subtree_root_id.clone());
                        let merged_children =
                            tree.merge_live_children_with_ghosts(&pid, live_children);
                        let parent = tree
                            .get_mut(&pid)
                            .ok_or_else(|| "InsertSubtree: parent not found".to_string())?;
                        parent.children = merged_children;
                    }
                }
                None => {
                    // Inserting as new root
                    tree.root = Some(subtree_root_id);
                }
            }
        }

        Patch::InsertNearbySubtree {
            host_id,
            slot,
            mut subtree,
        } => {
            let subtree_root_id = subtree
                .root
                .clone()
                .ok_or_else(|| "InsertNearbySubtree: subtree has no root".to_string())?;

            subtree.stamp_all_mounted_at_revision(batch_revision);

            for (id, element) in subtree.nodes {
                tree.nodes.insert(id, element);
            }

            let merged_ids =
                tree.merge_nearby_slot_with_ghosts(&host_id, slot, Some(subtree_root_id));
            let host = tree
                .get_mut(&host_id)
                .ok_or_else(|| "InsertNearbySubtree: host not found".to_string())?;
            let slot_ids = host.nearby.ids_mut(slot);
            slot_ids.clear();
            slot_ids.extend(merged_ids);
        }

        Patch::Remove { id } => {
            if let Some(ghost_root_id) = maybe_capture_exit_ghost(tree, &id)? {
                attach_ghost_root(tree, &ghost_root_id)?;
            }

            remove_subtree(tree, &id);
        }
    }

    Ok(())
}

fn filter_descendant_remove_patches(tree: &ElementTree, patches: Vec<Patch>) -> Vec<Patch> {
    let remove_ids: Vec<ElementId> = patches
        .iter()
        .filter_map(|patch| match patch {
            Patch::Remove { id } if tree.get(id).is_some_and(Element::is_live) => Some(id.clone()),
            _ => None,
        })
        .collect();

    let remove_set: HashSet<ElementId> = remove_ids.iter().cloned().collect();

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

fn has_removed_live_ancestor(
    tree: &ElementTree,
    id: &ElementId,
    remove_set: &HashSet<ElementId>,
) -> bool {
    let mut current = id.clone();

    while let Some(parent_id) = find_parent_id(tree, &current) {
        if remove_set.contains(&parent_id) {
            return true;
        }
        current = parent_id;
    }

    false
}

fn maybe_capture_exit_ghost(
    tree: &mut ElementTree,
    id: &ElementId,
) -> Result<Option<ElementId>, String> {
    let Some(element) = tree.get(id) else {
        return Ok(None);
    };

    if element.is_ghost() {
        return Ok(None);
    }

    let Some(spec) = captured_exit_spec(element, tree.current_scale()) else {
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

    let capture_scale = tree.current_scale();
    let mut to_clone = Vec::new();
    collect_descendants(tree, id, &mut to_clone);

    let mut id_map = HashMap::new();
    for old_id in &to_clone {
        id_map.insert(old_id.clone(), tree.mint_ghost_id());
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
        AttachmentPoint::Nearby { host_id, slot } => GhostAttachment::Nearby {
            host_id,
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

    for ghost in ghost_nodes {
        tree.insert(ghost);
    }

    Ok(Some(ghost_root_id))
}

fn attach_ghost_root(tree: &mut ElementTree, ghost_root_id: &ElementId) -> Result<(), String> {
    let attachment = tree
        .get(ghost_root_id)
        .and_then(|ghost| ghost.ghost_attachment.clone())
        .ok_or_else(|| "attach_ghost_root: ghost root missing attachment".to_string())?;

    match attachment {
        GhostAttachment::Child { parent_id, .. } => {
            if let Some(parent) = tree.get_mut(&parent_id) {
                parent.children.push(ghost_root_id.clone());
            }

            let live_children = tree.live_child_ids(&parent_id);
            let merged = tree.merge_live_children_with_ghosts(&parent_id, live_children);
            if let Some(parent) = tree.get_mut(&parent_id) {
                parent.children = merged;
            }
        }
        GhostAttachment::Nearby { host_id, slot, .. } => {
            if let Some(host) = tree.get_mut(&host_id) {
                host.nearby.push(slot, ghost_root_id.clone());
            }

            let live_id = tree.live_nearby_id(&host_id, slot);
            let merged = tree.merge_nearby_slot_with_ghosts(&host_id, slot, live_id);
            if let Some(host) = tree.get_mut(&host_id) {
                let slot_ids = host.nearby.ids_mut(slot);
                slot_ids.clear();
                slot_ids.extend(merged);
            }
        }
    }

    Ok(())
}

fn captured_exit_spec(element: &Element, capture_scale: f32) -> Option<AnimationSpec> {
    element.attrs.animate_exit.clone().or_else(|| {
        element
            .base_attrs
            .animate_exit
            .as_ref()
            .map(|spec| scale_animation_spec(spec, capture_scale as f64))
    })
}

fn locate_attachment(tree: &ElementTree, id: &ElementId) -> Option<AttachmentPoint> {
    if tree.root.as_ref() == Some(id) {
        return Some(AttachmentPoint::Root);
    }

    tree.nodes.values().find_map(|element| {
        let live_index = element
            .children
            .iter()
            .scan(0usize, |live_index, child_id| {
                let current_live_index = *live_index;
                let is_live = tree.get(child_id).is_some_and(Element::is_live);
                let result = (child_id.clone(), current_live_index, is_live);
                if is_live {
                    *live_index += 1;
                }
                Some(result)
            })
            .find_map(|(child_id, child_live_index, _is_live)| {
                (child_id == *id).then_some(child_live_index)
            });

        if let Some(live_index) = live_index {
            return Some(AttachmentPoint::Child {
                parent_id: element.id.clone(),
                live_index,
            });
        }

        NearbySlot::PAINT_ORDER.into_iter().find_map(|slot| {
            element
                .nearby
                .ids(slot)
                .iter()
                .any(|nearby_id| nearby_id == id)
                .then_some(AttachmentPoint::Nearby {
                    host_id: element.id.clone(),
                    slot,
                })
        })
    })
}

fn find_parent_id(tree: &ElementTree, id: &ElementId) -> Option<ElementId> {
    tree.nodes.values().find_map(|element| {
        element
            .children
            .iter()
            .chain(
                NearbySlot::PAINT_ORDER
                    .into_iter()
                    .flat_map(|slot| element.nearby.ids(slot).iter()),
            )
            .any(|candidate_id| candidate_id == id)
            .then_some(element.id.clone())
    })
}

fn clone_as_ghost(
    old: &Element,
    id_map: &HashMap<ElementId, ElementId>,
    capture_scale: f32,
    ghost_root_id: &ElementId,
    ghost_attachment: &GhostAttachment,
    exit_spec: &AnimationSpec,
) -> Element {
    let new_id = id_map
        .get(&old.id)
        .cloned()
        .expect("ghost id should exist for every cloned node");
    let (kind, attrs) = sanitize_ghost_visual(old.kind, &old.attrs);

    let mut cloned = Element {
        id: new_id.clone(),
        kind,
        attrs_raw: Vec::new(),
        base_attrs: attrs.clone(),
        attrs,
        text_input_content_origin: old.text_input_content_origin,
        children: old
            .children
            .iter()
            .filter_map(|child_id| id_map.get(child_id).cloned())
            .collect(),
        nearby: remap_nearby_mounts(&old.nearby, id_map),
        frame: old.frame,
        measured_frame: old.measured_frame,
        mounted_at_revision: old.mounted_at_revision,
        residency: NodeResidency::Ghost,
        ghost_attachment: None,
        ghost_capture_scale: Some(capture_scale),
        ghost_exit_animation: None,
    };

    if new_id == *ghost_root_id {
        cloned.ghost_attachment = Some(ghost_attachment.clone());
        cloned.ghost_exit_animation = Some(exit_spec.clone());
    }

    cloned
}

fn remap_nearby_mounts(
    nearby: &NearbyMounts,
    id_map: &HashMap<ElementId, ElementId>,
) -> NearbyMounts {
    let mut remapped = NearbyMounts::default();

    for slot in NearbySlot::PAINT_ORDER {
        remapped.ids_mut(slot).extend(
            nearby
                .ids(slot)
                .iter()
                .filter_map(|nearby_id| id_map.get(nearby_id).cloned()),
        );
    }

    remapped
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
    attrs.mouse_over_active = None;
    attrs.mouse_down_active = None;
    attrs.focused_active = None;

    attrs.scrollbar_hover_axis = None;
    attrs.ghost_scrollbar_x = effective_scrollbar_x(&attrs).then_some(true);
    attrs.ghost_scrollbar_y = effective_scrollbar_y(&attrs).then_some(true);
    attrs.scrollbar_x = None;
    attrs.scrollbar_y = None;

    attrs.animate = None;
    attrs.animate_enter = None;
    attrs.animate_exit = None;

    attrs.text_input_focused = None;
    attrs.text_input_cursor = None;
    attrs.text_input_selection_anchor = None;
    attrs.text_input_preedit = None;
    attrs.text_input_preedit_cursor = None;

    let ghost_kind = if kind == ElementKind::TextInput {
        ElementKind::Text
    } else {
        kind
    };

    (ghost_kind, attrs)
}

/// Recursively remove a node and all its descendants.
pub(crate) fn remove_subtree(tree: &mut ElementTree, id: &ElementId) {
    // First collect all descendant IDs
    let mut to_remove = Vec::new();
    collect_descendants(tree, id, &mut to_remove);

    // Remove all collected nodes
    for remove_id in to_remove {
        tree.nodes.remove(&remove_id);
    }

    // If this was the root, clear it
    if tree.root.as_ref() == Some(id) {
        tree.root = None;
    }

    // Remove from any parent's children list
    // (This is O(n) but patches are typically small)
    for element in tree.nodes.values_mut() {
        element.children.retain(|child_id| child_id != id);
        for slot in NearbySlot::PAINT_ORDER {
            element.nearby.remove(slot, id);
        }
    }
}

/// Collect a node and all its descendants.
fn collect_descendants(tree: &ElementTree, id: &ElementId, acc: &mut Vec<ElementId>) {
    acc.push(id.clone());

    if let Some(element) = tree.get(id) {
        for child_id in &element.children {
            collect_descendants(tree, child_id, acc);
        }

        for slot in NearbySlot::PAINT_ORDER {
            for nearby_id in element.nearby.ids(slot) {
                collect_descendants(tree, nearby_id, acc);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::events::registry_builder::ListenerMatcherKind;
    use crate::tree::animation::{AnimationCurve, AnimationRepeat, AnimationSpec};
    use crate::tree::attrs::Attrs;
    use crate::tree::element::{
        Element, ElementId, ElementKind, Frame, NearbySlot, TextInputContentOrigin,
    };

    fn exit_alpha_spec() -> AnimationSpec {
        let mut from = Attrs::default();
        from.alpha = Some(1.0);

        let mut to = Attrs::default();
        to.alpha = Some(0.0);

        AnimationSpec {
            keyframes: vec![from, to],
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
        let mut attrs = Attrs::default();
        attrs.content = Some(content.to_string());
        let mut element = Element::with_attrs(
            ElementId::from_term_bytes(vec![id]),
            ElementKind::Text,
            Vec::new(),
            attrs,
        );
        element.frame = Some(text_frame(0.0, 0.0, 64.0, 24.0));
        element
    }

    #[test]
    fn test_is_nil_term() {
        // Old atom format
        let nil_old = vec![131, 100, 0, 3, 110, 105, 108];
        assert!(is_nil_term(&nil_old));

        // New atom format
        let nil_new = vec![131, 119, 3, 110, 105, 108];
        assert!(is_nil_term(&nil_new));

        // Not nil
        let not_nil = vec![131, 100, 0, 4, 116, 101, 115, 116]; // :test
        assert!(!is_nil_term(&not_nil));
    }

    #[test]
    fn test_preserve_runtime_attrs_on_patch() {
        let id = ElementId::from_term_bytes(vec![1]);
        let mut attrs = Attrs::default();
        attrs.scroll_x = Some(12.0);
        attrs.scroll_y = Some(34.0);
        attrs.scroll_x_max = Some(50.0);
        attrs.scroll_y_max = Some(60.0);
        attrs.scrollbar_y = Some(true);
        attrs.scrollbar_hover_axis = Some(crate::tree::attrs::ScrollbarHoverAxis::Y);

        let element = Element::with_attrs(id.clone(), ElementKind::El, Vec::new(), attrs);
        let mut tree = ElementTree::new();
        tree.root = Some(id.clone());
        tree.nodes.insert(id.clone(), element);

        let patch = Patch::SetAttrs {
            id: id.clone(),
            attrs_raw: Vec::new(),
        };

        apply_patch(&mut tree, patch, 1).unwrap();

        let updated = tree.get(&id).unwrap();
        assert_eq!(updated.attrs.scroll_x, Some(12.0));
        assert_eq!(updated.attrs.scroll_y, Some(34.0));
        assert_eq!(updated.attrs.scroll_x_max, Some(50.0));
        assert_eq!(updated.attrs.scroll_y_max, Some(60.0));
        assert_eq!(updated.attrs.scrollbar_hover_axis, None);
    }

    #[test]
    fn test_preserve_runtime_attrs_on_patch_when_axis_present() {
        let id = ElementId::from_term_bytes(vec![1]);
        let mut attrs = Attrs::default();
        attrs.scroll_x = Some(12.0);
        attrs.scroll_y = Some(34.0);
        attrs.scroll_x_max = Some(50.0);
        attrs.scroll_y_max = Some(60.0);
        attrs.scrollbar_y = Some(true);
        attrs.scrollbar_hover_axis = Some(crate::tree::attrs::ScrollbarHoverAxis::Y);

        let element = Element::with_attrs(id.clone(), ElementKind::El, Vec::new(), attrs);
        let mut tree = ElementTree::new();
        tree.root = Some(id.clone());
        tree.nodes.insert(id.clone(), element);

        let patch = Patch::SetAttrs {
            id: id.clone(),
            attrs_raw: vec![0, 1, 7, 1],
        };

        apply_patch(&mut tree, patch, 1).unwrap();

        let updated = tree.get(&id).unwrap();
        assert_eq!(updated.attrs.scrollbar_y, Some(true));
        assert_eq!(
            updated.attrs.scrollbar_hover_axis,
            Some(crate::tree::attrs::ScrollbarHoverAxis::Y)
        );
    }

    #[test]
    fn test_patch_clears_mouse_over_active_when_mouse_over_removed() {
        let id = ElementId::from_term_bytes(vec![1]);
        let mut attrs = Attrs::default();
        attrs.mouse_over = Some(crate::tree::attrs::MouseOverAttrs::default());
        attrs.mouse_over_active = Some(true);

        let element = Element::with_attrs(id.clone(), ElementKind::El, Vec::new(), attrs);
        let mut tree = ElementTree::new();
        tree.root = Some(id.clone());
        tree.nodes.insert(id.clone(), element);

        let patch = Patch::SetAttrs {
            id: id.clone(),
            attrs_raw: Vec::new(),
        };

        apply_patch(&mut tree, patch, 1).unwrap();

        let updated = tree.get(&id).unwrap();
        assert_eq!(updated.attrs.mouse_over, None);
        assert_eq!(updated.attrs.mouse_over_active, None);
    }

    #[test]
    fn test_apply_patches_advances_revision_once_per_batch() {
        let id = ElementId::from_term_bytes(vec![1]);
        let element =
            Element::with_attrs(id.clone(), ElementKind::El, Vec::new(), Attrs::default());
        let mut tree = ElementTree::new();
        tree.root = Some(id.clone());
        tree.nodes.insert(id.clone(), element);

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
    fn test_insert_subtree_stamps_inserted_nodes_with_batch_revision() {
        let parent_id = ElementId::from_term_bytes(vec![1]);
        let child_id = ElementId::from_term_bytes(vec![2]);

        let parent = Element::with_attrs(
            parent_id.clone(),
            ElementKind::El,
            Vec::new(),
            Attrs::default(),
        );

        let child = Element::with_attrs(
            child_id.clone(),
            ElementKind::El,
            Vec::new(),
            Attrs::default(),
        );

        let mut subtree = ElementTree::new();
        subtree.root = Some(child_id.clone());
        subtree.insert(child);

        let mut tree = ElementTree::new();
        tree.root = Some(parent_id.clone());
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
        assert_eq!(inserted.mounted_at_revision, tree.revision());
    }

    #[test]
    fn test_set_attrs_preserves_existing_mount_revision() {
        let id = ElementId::from_term_bytes(vec![7]);
        let mut element =
            Element::with_attrs(id.clone(), ElementKind::El, Vec::new(), Attrs::default());
        element.mounted_at_revision = 4;

        let mut tree = ElementTree::new();
        tree.root = Some(id.clone());
        tree.insert(element);

        apply_patches(
            &mut tree,
            vec![Patch::SetAttrs {
                id: id.clone(),
                attrs_raw: Vec::new(),
            }],
        )
        .unwrap();

        assert_eq!(tree.get(&id).unwrap().mounted_at_revision, 4);
    }

    #[test]
    fn test_set_attrs_marks_text_input_content_as_tree_patch_when_content_present() {
        let id = ElementId::from_term_bytes(vec![17]);
        let mut attrs = Attrs::default();
        attrs.content = Some("before".to_string());
        let mut element =
            Element::with_attrs(id.clone(), ElementKind::TextInput, Vec::new(), attrs);
        element.text_input_content_origin = TextInputContentOrigin::Event;

        let mut tree = ElementTree::new();
        tree.root = Some(id.clone());
        tree.insert(element);

        apply_patches(
            &mut tree,
            vec![Patch::SetAttrs {
                id: id.clone(),
                attrs_raw: vec![0, 1, 21, 0, 5, b'a', b'f', b't', b'e', b'r'],
            }],
        )
        .unwrap();

        let updated = tree.get(&id).unwrap();
        assert_eq!(updated.base_attrs.content.as_deref(), Some("after"));
        assert_eq!(
            updated.text_input_content_origin,
            TextInputContentOrigin::TreePatch
        );
    }

    #[test]
    fn test_set_attrs_preserves_text_input_content_origin_when_content_absent() {
        let id = ElementId::from_term_bytes(vec![18]);
        let mut attrs = Attrs::default();
        attrs.content = Some("before".to_string());
        let mut element =
            Element::with_attrs(id.clone(), ElementKind::TextInput, Vec::new(), attrs);
        element.text_input_content_origin = TextInputContentOrigin::Event;

        let mut tree = ElementTree::new();
        tree.root = Some(id.clone());
        tree.insert(element);

        apply_patches(
            &mut tree,
            vec![Patch::SetAttrs {
                id: id.clone(),
                attrs_raw: Vec::new(),
            }],
        )
        .unwrap();

        assert_eq!(
            tree.get(&id).unwrap().text_input_content_origin,
            TextInputContentOrigin::Event
        );
    }

    #[test]
    fn test_set_children_preserves_existing_mount_revisions() {
        let parent_id = ElementId::from_term_bytes(vec![8]);
        let first_id = ElementId::from_term_bytes(vec![9]);
        let second_id = ElementId::from_term_bytes(vec![10]);

        let mut parent = Element::with_attrs(
            parent_id.clone(),
            ElementKind::El,
            Vec::new(),
            Attrs::default(),
        );
        parent.children = vec![first_id.clone(), second_id.clone()];
        parent.mounted_at_revision = 2;

        let mut first = Element::with_attrs(
            first_id.clone(),
            ElementKind::El,
            Vec::new(),
            Attrs::default(),
        );
        first.mounted_at_revision = 2;

        let mut second = Element::with_attrs(
            second_id.clone(),
            ElementKind::El,
            Vec::new(),
            Attrs::default(),
        );
        second.mounted_at_revision = 3;

        let mut tree = ElementTree::new();
        tree.root = Some(parent_id.clone());
        tree.insert(parent);
        tree.insert(first);
        tree.insert(second);

        apply_patches(
            &mut tree,
            vec![Patch::SetChildren {
                id: parent_id,
                children: vec![second_id.clone(), first_id.clone()],
            }],
        )
        .unwrap();

        assert_eq!(tree.get(&first_id).unwrap().mounted_at_revision, 2);
        assert_eq!(tree.get(&second_id).unwrap().mounted_at_revision, 3);
    }

    #[test]
    fn test_remove_then_reinsert_stamps_new_mount_revision() {
        let parent_id = ElementId::from_term_bytes(vec![11]);
        let child_id = ElementId::from_term_bytes(vec![12]);

        let mut parent = Element::with_attrs(
            parent_id.clone(),
            ElementKind::El,
            Vec::new(),
            Attrs::default(),
        );
        parent.children = vec![child_id.clone()];

        let mut child = Element::with_attrs(
            child_id.clone(),
            ElementKind::El,
            Vec::new(),
            Attrs::default(),
        );
        child.mounted_at_revision = 1;

        let mut tree = ElementTree::new();
        tree.root = Some(parent_id.clone());
        tree.insert(parent);
        tree.insert(child);
        tree.set_revision(1);

        apply_patches(
            &mut tree,
            vec![Patch::Remove {
                id: child_id.clone(),
            }],
        )
        .unwrap();
        let removed_revision = tree.revision();

        let mut subtree = ElementTree::new();
        subtree.root = Some(child_id.clone());
        subtree.insert(Element::with_attrs(
            child_id.clone(),
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

        assert!(tree.get(&child_id).unwrap().mounted_at_revision > removed_revision);
    }

    #[test]
    fn test_remove_with_animate_exit_creates_sanitized_child_ghost() {
        let parent_id = ElementId::from_term_bytes(vec![20]);
        let child_id = ElementId::from_term_bytes(vec![21]);

        let mut parent = Element::with_attrs(
            parent_id.clone(),
            ElementKind::El,
            Vec::new(),
            Attrs::default(),
        );
        parent.children = vec![child_id.clone()];

        let mut child_attrs = Attrs::default();
        child_attrs.content = Some("ghosted".to_string());
        child_attrs.on_click = Some(true);
        child_attrs.mouse_over = Some(crate::tree::attrs::MouseOverAttrs::default());
        child_attrs.mouse_over_active = Some(true);
        child_attrs.scrollbar_y = Some(true);
        child_attrs.scroll_y = Some(12.0);
        child_attrs.scroll_y_max = Some(40.0);
        child_attrs.text_input_focused = Some(true);
        child_attrs.text_input_cursor = Some(2);
        child_attrs.animate_exit = Some(exit_alpha_spec());

        let mut child = Element::with_attrs(
            child_id.clone(),
            ElementKind::TextInput,
            Vec::new(),
            child_attrs,
        );
        child.frame = Some(Frame {
            x: 0.0,
            y: 0.0,
            width: 80.0,
            height: 24.0,
            content_width: 80.0,
            content_height: 120.0,
        });

        let mut tree = ElementTree::new();
        tree.root = Some(parent_id.clone());
        tree.insert(parent);
        tree.insert(child);

        apply_patches(
            &mut tree,
            vec![Patch::Remove {
                id: child_id.clone(),
            }],
        )
        .unwrap();

        assert!(tree.get(&child_id).is_none());

        let parent = tree.get(&parent_id).unwrap();
        assert_eq!(parent.children.len(), 1);
        let ghost_id = parent.children[0].clone();
        assert_ne!(ghost_id, child_id);

        let ghost = tree.get(&ghost_id).expect("ghost root should exist");
        assert!(ghost.is_ghost_root());
        assert_eq!(ghost.kind, ElementKind::Text);
        assert_eq!(ghost.attrs.on_click, None);
        assert_eq!(ghost.attrs.mouse_over, None);
        assert_eq!(ghost.attrs.mouse_over_active, None);
        assert_eq!(ghost.attrs.scrollbar_y, None);
        assert_eq!(ghost.attrs.ghost_scrollbar_y, Some(true));
        assert_eq!(ghost.attrs.scroll_y, Some(12.0));
        assert_eq!(ghost.attrs.text_input_focused, None);
        assert!(ghost.ghost_exit_animation.is_some());
    }

    #[test]
    fn test_remove_with_animate_exit_keeps_rendering_but_drops_press_listener() {
        let root_id = ElementId::from_term_bytes(vec![30]);
        let child_id = ElementId::from_term_bytes(vec![31]);

        let mut root = Element::with_attrs(
            root_id.clone(),
            ElementKind::El,
            Vec::new(),
            Attrs::default(),
        );
        root.children = vec![child_id.clone()];
        root.frame = Some(text_frame(0.0, 0.0, 120.0, 40.0));

        let mut child = text_element(31, "bye");
        child.attrs.on_click = Some(true);
        child.base_attrs.on_click = Some(true);
        child.attrs.animate_exit = Some(exit_alpha_spec());
        child.base_attrs.animate_exit = Some(exit_alpha_spec());
        child.frame = Some(text_frame(8.0, 8.0, 48.0, 20.0));

        let mut tree = ElementTree::new();
        tree.root = Some(root_id.clone());
        tree.insert(root);
        tree.insert(child);

        apply_patches(
            &mut tree,
            vec![Patch::Remove {
                id: child_id.clone(),
            }],
        )
        .unwrap();

        let output = crate::tree::render::render_tree(&tree);
        let press_ids: Vec<_> = output
            .event_rebuild
            .base_registry
            .view()
            .iter_precedence()
            .filter(|listener| {
                listener.matcher.kind() == ListenerMatcherKind::CursorButtonLeftPressInside
            })
            .filter_map(|listener| listener.element_id.clone())
            .collect();

        assert!(!output.scene.nodes.is_empty());
        assert!(press_ids.is_empty());
    }

    #[test]
    fn test_insert_subtree_preserves_ghost_anchor_against_live_order() {
        let parent_id = ElementId::from_term_bytes(vec![40]);
        let first_id = ElementId::from_term_bytes(vec![41]);
        let removed_id = ElementId::from_term_bytes(vec![42]);
        let third_id = ElementId::from_term_bytes(vec![43]);
        let new_id = ElementId::from_term_bytes(vec![44]);

        let mut parent = Element::with_attrs(
            parent_id.clone(),
            ElementKind::Row,
            Vec::new(),
            Attrs::default(),
        );
        parent.children = vec![first_id.clone(), removed_id.clone(), third_id.clone()];

        let first = text_element(41, "a");

        let mut removed = text_element(42, "b");
        removed.attrs.animate_exit = Some(exit_alpha_spec());
        removed.base_attrs.animate_exit = Some(exit_alpha_spec());

        let third = text_element(43, "c");

        let mut tree = ElementTree::new();
        tree.root = Some(parent_id.clone());
        tree.insert(parent);
        tree.insert(first);
        tree.insert(removed);
        tree.insert(third);

        let mut subtree = ElementTree::new();
        subtree.root = Some(new_id.clone());
        subtree.insert(text_element(44, "d"));

        apply_patches(
            &mut tree,
            vec![
                Patch::Remove {
                    id: removed_id.clone(),
                },
                Patch::InsertSubtree {
                    parent_id: Some(parent_id.clone()),
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
        let host_id = ElementId::from_term_bytes(vec![50]);
        let old_nearby_id = ElementId::from_term_bytes(vec![51]);
        let new_nearby_id = ElementId::from_term_bytes(vec![52]);

        let mut host = Element::with_attrs(
            host_id.clone(),
            ElementKind::El,
            Vec::new(),
            Attrs::default(),
        );
        host.nearby
            .set(NearbySlot::OnRight, Some(old_nearby_id.clone()));

        let mut old_nearby = text_element(51, "old");
        old_nearby.attrs.animate_exit = Some(exit_alpha_spec());
        old_nearby.base_attrs.animate_exit = Some(exit_alpha_spec());

        let mut tree = ElementTree::new();
        tree.root = Some(host_id.clone());
        tree.insert(host);
        tree.insert(old_nearby);

        let mut subtree = ElementTree::new();
        subtree.root = Some(new_nearby_id.clone());
        subtree.insert(text_element(52, "new"));

        apply_patches(
            &mut tree,
            vec![
                Patch::Remove {
                    id: old_nearby_id.clone(),
                },
                Patch::InsertNearbySubtree {
                    host_id: host_id.clone(),
                    slot: NearbySlot::OnRight,
                    subtree,
                },
            ],
        )
        .unwrap();

        let host = tree.get(&host_id).unwrap();
        let slot_ids = host.nearby.ids(NearbySlot::OnRight);
        assert_eq!(slot_ids.len(), 2);
        assert!(tree.get(&slot_ids[0]).unwrap().is_ghost_root());
        assert_eq!(slot_ids[1], new_nearby_id);
    }
}
