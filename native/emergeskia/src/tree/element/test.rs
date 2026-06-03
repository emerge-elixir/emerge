use super::ContainerKind::El;
use super::Kind::Container;
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
