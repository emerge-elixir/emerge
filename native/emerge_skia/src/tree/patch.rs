//! Patch decoding and application for incremental tree updates.
//!
//! Patch binary format:
//! - Stream of operations, each starting with a tag byte:
//!   - 1: set_attrs - id_len(4) + id + attr_len(4) + attrs
//!   - 2: set_children - id_len(4) + id + count(2) + [child_id_len(4) + child_id]...
//!   - 3: insert_subtree - parent_len(4) + parent_id + index(2) + tree_len(4) + tree_bytes
//!   - 4: remove - id_len(4) + id

use super::attrs::{decode_attrs, preserve_runtime_scroll_attrs, Attrs};
use super::deserialize::{decode_tree, DecodeError};
use super::element::{ElementId, ElementTree};

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

    /// Remove a node and its descendants.
    Remove { id: ElementId },
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
    for patch in patches {
        apply_patch(tree, patch)?;
    }
    Ok(())
}

/// Apply a single patch to the tree.
fn apply_patch(tree: &mut ElementTree, patch: Patch) -> Result<(), String> {
    match patch {
        Patch::SetAttrs { id, attrs_raw } => {
            let element = tree
                .get_mut(&id)
                .ok_or_else(|| "SetAttrs: node not found".to_string())?;
            element.attrs_raw = attrs_raw.clone();
            let decoded = decode_attrs(&attrs_raw).map_err(|e| e.to_string())?;
            element.base_attrs = decoded.clone();
            let mut merged = decoded;
            preserve_runtime_scroll_attrs(&element.attrs, &mut merged);
            element.attrs = merged;
        }

        Patch::SetChildren { id, children } => {
            let element = tree
                .get_mut(&id)
                .ok_or_else(|| "SetChildren: node not found".to_string())?;
            element.children = children;
        }

        Patch::InsertSubtree {
            parent_id,
            index,
            subtree,
        } => {
            // Get the root of the subtree
            let subtree_root_id = subtree
                .root
                .clone()
                .ok_or_else(|| "InsertSubtree: subtree has no root".to_string())?;

            // Insert all nodes from subtree into main tree
            for (id, element) in subtree.nodes {
                tree.nodes.insert(id, element);
            }

            // Update parent's children or set as tree root
            match parent_id {
                Some(pid) => {
                    let parent = tree
                        .get_mut(&pid)
                        .ok_or_else(|| "InsertSubtree: parent not found".to_string())?;

                    if !parent.children.contains(&subtree_root_id) {
                        // Insert at the specified index
                        let insert_idx = index.min(parent.children.len());
                        parent.children.insert(insert_idx, subtree_root_id);
                    }
                }
                None => {
                    // Inserting as new root
                    tree.root = Some(subtree_root_id);
                }
            }
        }

        Patch::Remove { id } => {
            // Remove the node and all its descendants
            remove_subtree(tree, &id);
        }
    }

    Ok(())
}

/// Recursively remove a node and all its descendants.
fn remove_subtree(tree: &mut ElementTree, id: &ElementId) {
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
    }
}

/// Collect a node and all its descendants.
fn collect_descendants(tree: &ElementTree, id: &ElementId, acc: &mut Vec<ElementId>) {
    acc.push(id.clone());

    if let Some(element) = tree.get(id) {
        for child_id in &element.children {
            collect_descendants(tree, child_id, acc);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tree::attrs::Attrs;
    use crate::tree::element::{Element, ElementId, ElementKind};

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

        apply_patch(&mut tree, patch).unwrap();

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

        apply_patch(&mut tree, patch).unwrap();

        let updated = tree.get(&id).unwrap();
        assert_eq!(updated.attrs.scrollbar_y, Some(true));
        assert_eq!(
            updated.attrs.scrollbar_hover_axis,
            Some(crate::tree::attrs::ScrollbarHoverAxis::Y)
        );
    }
}
