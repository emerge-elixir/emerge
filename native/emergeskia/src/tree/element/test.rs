use super::ContainerKind::El;
use super::Kind::{Container, Text};
use super::Tree;
use super::*;

mod tree_builder;

use tree_builder::{build_tree, el, font_size, padding, row, spacing, text};

#[test]
fn new() {
    let tree = Tree::new();
    assert_eq!(tree.root, None)
}

#[test]
fn with_root() {
    let Ok(_) = Tree::with_root(Id(1), empty_el()) else {
        panic!("expected tree creation to succeed");
    };
}

#[test]
fn insert_node() {
    let mut tree = tree_with_root();
    let text = test_text();
    let Ok(_) = tree.insert_node(Id(2), text, tree.root.expect("should have root")) else {
        panic!("insert should succeed");
    };
}

#[test]
fn tree_buliding() {
    let tree = row_with_text_elements();
    assert_eq!(tree.elements.len(), 5)
}

// Subtree helpers
fn empty_el() -> Kind {
    Container {
        kind: El,
        children: SmallVec::new(),
        nearby: SmallVec::new(),
        attrs: empty_attrs(),
    }
}

fn empty_attrs() -> Attrs {
    Attrs {
        padding: 0.0,
        spacing: 0.0,
        font_size: None,
    }
}

fn test_text() -> Kind {
    Text {
        content: "Test".into(),
    }
}

fn row_with_text_elements() -> Tree {
    build_tree(row(
        [padding(10), spacing(10)],
        [
            el([font_size(14)], text("Foo")),
            el([font_size(10)], text("Bar")),
        ],
    ))
}

fn tree_with_root() -> Tree {
    Tree::with_root(Id(1), empty_el()).unwrap()
}
