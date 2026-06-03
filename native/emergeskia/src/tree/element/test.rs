use super::ContainerKind::El;
use super::Kind::{Container, Text};
use super::Tree;
use super::*;

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


fn tree_with_root() -> Tree {
    Tree::with_root(Id(1), empty_el()).unwrap()
}
