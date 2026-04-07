//! Serialization of the EMRG binary format.
//!
//! Produces EMRG v5 from an ElementTree.

use super::element::{Element, ElementId, ElementKind, ElementTree, NearbySlot};

const MAGIC: &[u8] = b"EMRG";
const VERSION: u8 = 5;

pub fn encode_tree(tree: &ElementTree) -> Vec<u8> {
    let Some(root_id) = tree.root.as_ref() else {
        return encode_header(0);
    };

    let nodes = collect_nodes(tree, root_id);
    let mut out = encode_header(nodes.len() as u32);

    for element in nodes {
        encode_node(&mut out, tree, element);
    }

    out
}

fn encode_header(node_count: u32) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(MAGIC);
    out.push(VERSION);
    out.extend_from_slice(&node_count.to_be_bytes());
    out
}

fn encode_node(out: &mut Vec<u8>, tree: &ElementTree, element: &Element) {
    encode_id(out, &element.id);

    let tag = kind_tag(element.kind);
    out.push(tag);

    encode_attrs(out, &element.attrs_raw);
    let live_children: Vec<ElementId> = element
        .children
        .iter()
        .filter(|child_id| tree.get(child_id).is_some_and(Element::is_live))
        .cloned()
        .collect();
    encode_children(out, &live_children);
    encode_nearby(out, tree, element);
}

fn encode_id(out: &mut Vec<u8>, id: &ElementId) {
    out.extend_from_slice(&(id.0.len() as u32).to_be_bytes());
    out.extend_from_slice(&id.0);
}

fn encode_attrs(out: &mut Vec<u8>, attrs_raw: &[u8]) {
    out.extend_from_slice(&(attrs_raw.len() as u32).to_be_bytes());
    out.extend_from_slice(attrs_raw);
}

fn encode_children(out: &mut Vec<u8>, children: &[ElementId]) {
    out.extend_from_slice(&(children.len() as u16).to_be_bytes());
    for child_id in children {
        encode_id(out, child_id);
    }
}

fn encode_nearby(out: &mut Vec<u8>, tree: &ElementTree, element: &Element) {
    let live_mounts: Vec<_> = element
        .nearby
        .iter()
        .filter(|mount| tree.get(&mount.id).is_some_and(Element::is_live))
        .collect();

    out.extend_from_slice(&(live_mounts.len() as u16).to_be_bytes());
    for mount in live_mounts {
        out.push(NearbySlot::tag(mount.slot));
        encode_id(out, &mount.id);
    }
}

fn kind_tag(kind: ElementKind) -> u8 {
    match kind {
        ElementKind::Row => 1,
        ElementKind::WrappedRow => 2,
        ElementKind::Column => 3,
        ElementKind::El => 4,
        ElementKind::Text => 5,
        ElementKind::None => 6,
        ElementKind::Paragraph => 7,
        ElementKind::TextColumn => 8,
        ElementKind::Image => 9,
        ElementKind::TextInput => 10,
        ElementKind::Video => 11,
    }
}

fn collect_nodes<'a>(tree: &'a ElementTree, root: &ElementId) -> Vec<&'a Element> {
    let mut out = Vec::new();
    collect_nodes_inner(tree, root, &mut out);
    out
}

fn collect_nodes_inner<'a>(tree: &'a ElementTree, id: &ElementId, out: &mut Vec<&'a Element>) {
    let Some(element) = tree.get(id) else {
        return;
    };

    if element.is_ghost() {
        return;
    }

    out.push(element);

    for child_id in &element.children {
        if tree.get(child_id).is_some_and(Element::is_live) {
            collect_nodes_inner(tree, child_id, out);
        }
    }

    for mount in element.nearby.iter() {
        if tree.get(&mount.id).is_some_and(Element::is_live) {
            collect_nodes_inner(tree, &mount.id, out);
        }
    }
}
