//! Deserialization of the EMRG binary format.
//!
//! EMRG binary format:
//! - Header: "EMRG" (4 bytes) + version (1 byte) + node_count (4 bytes BE)
//! - Per node:
//!   - id_len (4 bytes BE) + id_bytes (Erlang term binary)
//!   - type_tag (1 byte)
//!   - attr_len (4 bytes BE) + attr_bytes (typed attribute block)
//!   - child_count (2 bytes BE)
//!   - For each child: child_id_len (4 bytes BE) + child_id_bytes
//!
//! Attribute block format:
//!   - attr_count (2 bytes BE)
//!   - For each attr: tag (1 byte) + value (varies by tag)

use super::attrs::{Attrs, decode_attrs};
use super::element::{Element, ElementId, ElementKind, ElementTree};

const MAGIC: &[u8] = b"EMRG";
const VERSION: u8 = 2;

/// Error type for deserialization failures.
#[derive(Debug, Clone)]
pub enum DecodeError {
    InvalidMagic,
    UnsupportedVersion(u8),
    UnexpectedEof,
    InvalidTypeTag(u8),
    InvalidStructure(String),
}

impl std::fmt::Display for DecodeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidMagic => write!(f, "invalid magic bytes, expected 'EMRG'"),
            Self::UnsupportedVersion(v) => write!(f, "unsupported version: {}", v),
            Self::UnexpectedEof => write!(f, "unexpected end of input"),
            Self::InvalidTypeTag(t) => write!(f, "invalid type tag: {}", t),
            Self::InvalidStructure(msg) => write!(f, "invalid structure: {}", msg),
        }
    }
}

impl std::error::Error for DecodeError {}

/// A cursor for reading binary data.
struct Cursor<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> Cursor<'a> {
    fn new(data: &'a [u8]) -> Self {
        Self { data, pos: 0 }
    }

    fn remaining(&self) -> usize {
        self.data.len().saturating_sub(self.pos)
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

/// Intermediate node representation during parsing.
struct RawNode {
    id: ElementId,
    kind: ElementKind,
    attrs_raw: Vec<u8>,
    attrs: Attrs,
    child_ids: Vec<ElementId>,
}

/// Decode a full tree from the EMRG binary format.
pub fn decode_tree(data: &[u8]) -> Result<ElementTree, DecodeError> {
    let mut cursor = Cursor::new(data);

    // Read and validate header
    let magic = cursor.read_bytes(4)?;
    if magic != MAGIC {
        return Err(DecodeError::InvalidMagic);
    }

    let version = cursor.read_u8()?;
    if version != VERSION {
        return Err(DecodeError::UnsupportedVersion(version));
    }

    let node_count = cursor.read_u32_be()? as usize;

    if node_count == 0 {
        return Ok(ElementTree::new());
    }

    // Parse all nodes
    let mut raw_nodes = Vec::with_capacity(node_count);
    for _ in 0..node_count {
        let node = decode_node(&mut cursor)?;
        raw_nodes.push(node);
    }

    // Verify we consumed all data
    if cursor.remaining() > 0 {
        return Err(DecodeError::InvalidStructure(format!(
            "{} bytes remaining after parsing",
            cursor.remaining()
        )));
    }

    // Build the tree
    let mut tree = ElementTree::new();

    // First node is the root
    if let Some(first) = raw_nodes.first() {
        tree.root = Some(first.id.clone());
    }

    // Insert all nodes
    for raw in raw_nodes {
        let mut element = Element::with_attrs(raw.id, raw.kind, raw.attrs_raw, raw.attrs);
        element.children = raw.child_ids;
        tree.insert(element);
    }

    Ok(tree)
}

/// Decode a single node from the cursor.
fn decode_node(cursor: &mut Cursor) -> Result<RawNode, DecodeError> {
    // Read ID
    let id_bytes = cursor.read_length_prefixed()?;
    let id = ElementId::from_term_bytes(id_bytes);

    // Read type tag
    let type_tag = cursor.read_u8()?;
    let kind = ElementKind::from_tag(type_tag)
        .ok_or(DecodeError::InvalidTypeTag(type_tag))?;

    // Read attributes
    let attrs_raw = cursor.read_length_prefixed()?;
    let attrs = decode_attrs(&attrs_raw)?;

    // Read children
    let child_count = cursor.read_u16_be()? as usize;
    let mut child_ids = Vec::with_capacity(child_count);
    for _ in 0..child_count {
        let child_id_bytes = cursor.read_length_prefixed()?;
        child_ids.push(ElementId::from_term_bytes(child_id_bytes));
    }

    Ok(RawNode {
        id,
        kind,
        attrs_raw,
        attrs,
        child_ids,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_header(node_count: u32) -> Vec<u8> {
        let mut buf = Vec::new();
        buf.extend_from_slice(MAGIC);
        buf.push(VERSION);
        buf.extend_from_slice(&node_count.to_be_bytes());
        buf
    }

    #[test]
    fn test_decode_empty_tree() {
        let data = make_header(0);
        let tree = decode_tree(&data).unwrap();
        assert!(tree.is_empty());
    }

    #[test]
    fn test_invalid_magic() {
        let data = b"XXXX\x02\x00\x00\x00\x00";
        let err = decode_tree(data).unwrap_err();
        assert!(matches!(err, DecodeError::InvalidMagic));
    }

    #[test]
    fn test_unsupported_version() {
        let mut data = Vec::new();
        data.extend_from_slice(MAGIC);
        data.push(99); // unsupported version
        data.extend_from_slice(&0u32.to_be_bytes());

        let err = decode_tree(&data).unwrap_err();
        assert!(matches!(err, DecodeError::UnsupportedVersion(99)));
    }

    #[test]
    fn test_decode_single_node() {
        let mut data = make_header(1);

        let fake_id = vec![0x01, 0x02, 0x03]; // fake term bytes

        // attrs: empty block (0 attrs)
        let attrs_block: Vec<u8> = vec![0, 0]; // attr_count = 0

        // id_len + id
        data.extend_from_slice(&(fake_id.len() as u32).to_be_bytes());
        data.extend_from_slice(&fake_id);

        // type tag
        data.push(4); // el

        // attr_len + attrs
        data.extend_from_slice(&(attrs_block.len() as u32).to_be_bytes());
        data.extend_from_slice(&attrs_block);

        // child_count = 0
        data.extend_from_slice(&0u16.to_be_bytes());

        let tree = decode_tree(&data).unwrap();
        assert_eq!(tree.len(), 1);

        let root_id = tree.root.as_ref().unwrap();
        let root = tree.get(root_id).unwrap();
        assert_eq!(root.kind, ElementKind::El);
        assert!(root.children.is_empty());
    }

    #[test]
    fn test_decode_with_attrs() {
        let mut data = make_header(1);

        let fake_id = vec![0x01, 0x02, 0x03];

        // attrs: 1 attr, tag=4 (spacing), f64=10.0
        let mut attrs_block = vec![0, 1]; // attr_count = 1
        attrs_block.push(4); // tag = spacing
        attrs_block.extend_from_slice(&10.0_f64.to_be_bytes());

        // id_len + id
        data.extend_from_slice(&(fake_id.len() as u32).to_be_bytes());
        data.extend_from_slice(&fake_id);

        // type tag
        data.push(3); // column

        // attr_len + attrs
        data.extend_from_slice(&(attrs_block.len() as u32).to_be_bytes());
        data.extend_from_slice(&attrs_block);

        // child_count = 0
        data.extend_from_slice(&0u16.to_be_bytes());

        let tree = decode_tree(&data).unwrap();
        let root_id = tree.root.as_ref().unwrap();
        let root = tree.get(root_id).unwrap();

        assert_eq!(root.kind, ElementKind::Column);
        assert_eq!(root.attrs.spacing, Some(10.0));
    }
}
