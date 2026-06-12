use super::Tree;
use super::geometry::*;
use super::shapes::{ElementSpec, el::ElSpec, text::TextSpec};
use super::*;

mod tree_builder;

use tree_builder::{build_tree, el, font_size, padding, spacing, text};

#[test]
fn with_root() {
    let Ok(_) = Tree::with_root(Id(1), empty_el()) else {
        panic!("expected tree creation to succeed");
    };
}

#[test]
fn insert_element() {
    let mut tree = tree_with_root();
    let text = test_text();
    let root = tree.root.expect("should have root");

    tree.insert_element(Id(2), text, root)
        .expect("insert_element should succeed");
}

#[test]
fn tree_buliding() {
    let tree = el_with_text();
    assert_eq!(tree.elements.len(), 2)
}

#[test]
fn layout_el() {
    let mut tree = el_with_text();
    tree.layout.layout(&tree.elements);
    dbg!(&tree);
    assert_eq!(tree.elements.len(), 2);
    let root_key = tree.root.expect("expect root");
    let root_frame = tree.layout.resolve[root_key].frame;
    assert_eq!(root_frame, Rect::new(0.0, 0.0, 44.0, 26.0))
}

// Subtree helpers
fn empty_el() -> ElementSpec {
    ElementSpec::El(ElSpec {
        attrs: empty_attrs(),
    })
}

fn empty_attrs() -> Attrs {
    Attrs {
        padding: 0.0,
        spacing: 0.0,
        font_size: None,
    }
}

fn test_text() -> ElementSpec {
    ElementSpec::Text(TextSpec {
        content: "Test".into(),
    })
}

fn el_with_text() -> Tree {
    build_tree(el([padding(10), spacing(10)], text("Foo")))
}

/*
fn row_with_text_elements() -> Tree {
    build_tree(row(
        [padding(10), spacing(10)],
        [
            el([font_size(14)], text("Foo")),
            el([font_size(10)], text("Bar")),
        ],
    ))
}
*/

fn tree_with_root() -> Tree {
    Tree::with_root(Id(1), empty_el()).unwrap()
}
