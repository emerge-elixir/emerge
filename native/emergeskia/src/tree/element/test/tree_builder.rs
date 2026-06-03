use super::super::{Attrs, ContainerKind, CreateError, Id, InsertError, Key, Kind, Tree};
use smallvec::SmallVec;

pub(super) struct NodeSpec {
    kind: SpecKind,
    children: Vec<NodeSpec>,
}

pub(super) enum SpecKind {
    Row(Attrs),
    El(Attrs),
    Text(String),
}

pub(super) enum AttrSpec {
    Padding(f32),
    Spacing(f32),
    FontSize(f32),
}

// Element helpers
pub(super) fn row(
    attrs: impl IntoIterator<Item = AttrSpec>,
    children: impl IntoIterator<Item = NodeSpec>,
) -> NodeSpec {
    NodeSpec {
        kind: SpecKind::Row(attrs_from(attrs)),
        children: children.into_iter().collect(),
    }
}

pub(super) fn el(attrs: impl IntoIterator<Item = AttrSpec>, child: NodeSpec) -> NodeSpec {
    NodeSpec {
        kind: SpecKind::El(attrs_from(attrs)),
        children: vec![child],
    }
}

pub(super) fn text(content: impl Into<String>) -> NodeSpec {
    NodeSpec {
        kind: SpecKind::Text(content.into()),
        children: Vec::new(),
    }
}

// Attribute helpers
pub(super) fn padding(px: i32) -> AttrSpec {
    AttrSpec::Padding(px as f32)
}

pub(super) fn spacing(px: i32) -> AttrSpec {
    AttrSpec::Spacing(px as f32)
}

pub(super) fn font_size(px: i32) -> AttrSpec {
    AttrSpec::FontSize(px as f32)
}

fn empty_attrs() -> Attrs {
    Attrs {
        padding: 0.0,
        spacing: 0.0,
        font_size: None,
    }
}

fn attrs_from(attrs: impl IntoIterator<Item = AttrSpec>) -> Attrs {
    let mut out = empty_attrs();

    for attr in attrs {
        match attr {
            AttrSpec::Padding(value) => out.padding = value,
            AttrSpec::Spacing(value) => out.spacing = value,
            AttrSpec::FontSize(value) => out.font_size = Some(value),
        }
    }

    out
}

fn kind_from_spec(kind: SpecKind) -> Kind {
    match kind {
        SpecKind::Text(content) => Kind::Text { content },

        SpecKind::Row(attrs) => Kind::Container {
            kind: ContainerKind::Row,
            children: SmallVec::new(),
            nearby: SmallVec::new(),
            attrs,
        },

        SpecKind::El(attrs) => Kind::Container {
            kind: ContainerKind::El,
            children: SmallVec::new(),
            nearby: SmallVec::new(),
            attrs,
        },
    }
}

struct IdGen(u64);

impl IdGen {
    fn next(&mut self) -> Id {
        let id = self.0;
        self.0 += 1;
        Id(id)
    }
}

pub(super) fn build_tree(root: NodeSpec) -> Tree {
    let mut ids = IdGen(1);

    let root_id = ids.next();
    let root_children = root.children;
    let root_kind = kind_from_spec(root.kind);

    let mut tree = Tree::with_root(root_id, root_kind).expect("tree should build");
    let root_key = tree.root.expect("Tree::with_root should create root");

    for child in root_children {
        insert_spec(&mut tree, root_key, child, &mut ids);
    }

    tree
}

fn insert_spec(tree: &mut Tree, parent: Key, spec: NodeSpec, ids: &mut IdGen) -> Key {
    let id = ids.next();
    let children = spec.children;
    let kind = kind_from_spec(spec.kind);

    let key = tree
        .insert_node(id, kind, parent)
        .expect("node should insert");

    for child in children {
        insert_spec(tree, key, child, ids);
    }

    key
}
