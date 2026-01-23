//! Element types for Emerge UI trees.

use std::collections::HashMap;
use super::attrs::Attrs;

/// Unique identifier for an element, derived from Erlang term.
/// Stored as the raw bytes of the serialized Erlang term for exact matching.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct ElementId(pub Vec<u8>);

impl ElementId {
    pub fn from_term_bytes(bytes: Vec<u8>) -> Self {
        Self(bytes)
    }
}

/// The type/kind of an element.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ElementKind {
    Row,
    WrappedRow,
    Column,
    El,
    Text,
    None,
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
            _ => None,
        }
    }

}

/// Frame representing the computed layout bounds.
#[derive(Clone, Copy, Debug, Default)]
pub struct Frame {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
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

    /// Decoded attributes.
    pub attrs: Attrs,

    /// Child element IDs (order matters).
    pub children: Vec<ElementId>,

    /// Computed layout frame (populated after layout pass).
    pub frame: Option<Frame>,
}

impl Element {
    /// Create an element with decoded attributes.
    pub fn with_attrs(id: ElementId, kind: ElementKind, attrs_raw: Vec<u8>, attrs: Attrs) -> Self {
        Self {
            id,
            kind,
            attrs_raw,
            attrs,
            children: Vec::new(),
            frame: None,
        }
    }
}

/// The complete element tree with indexed access.
#[derive(Clone, Debug, Default)]
pub struct ElementTree {
    /// Root element ID (if tree is non-empty).
    pub root: Option<ElementId>,

    /// All elements indexed by ID for O(1) lookup.
    pub nodes: HashMap<ElementId, Element>,
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
        self.root = None;
        self.nodes.clear();
    }

}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_element_kind_from_tag() {
        assert_eq!(ElementKind::from_tag(1), Some(ElementKind::Row));
        assert_eq!(ElementKind::from_tag(2), Some(ElementKind::WrappedRow));
        assert_eq!(ElementKind::from_tag(3), Some(ElementKind::Column));
        assert_eq!(ElementKind::from_tag(4), Some(ElementKind::El));
        assert_eq!(ElementKind::from_tag(5), Some(ElementKind::Text));
        assert_eq!(ElementKind::from_tag(6), Some(ElementKind::None));
        assert_eq!(ElementKind::from_tag(7), None);
    }
}
