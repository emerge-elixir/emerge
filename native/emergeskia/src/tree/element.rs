use slotmap::{SecondaryMap, SlotMap, SparseSecondaryMap, new_key_type};
use smallvec::SmallVec;
use std::collections::HashMap;

mod layout;
use layout::Layout;

new_key_type! {
   struct Key;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
struct Id(pub u64);

struct Tree {
    root: Option<Key>,

    elements: SlotMap<Key, Element>,
    by_element_id: HashMap<Id, Key>,

    layout: SecondaryMap<Key, Layout>,
}

struct Element {
    pub id: Id,

    pub parent: Option<Parent>,
    pub kind: Kind,
}

struct Parent {
    key: Key,
    relation: Relation,
}

enum Relation {
    Child,
    Nearby,
}

enum Kind {
    Text {
        content: String,
    },
    Container {
        kind: ContainerKind,
        children: SmallVec<[Key; 4]>,
        nearby: SmallVec<[NearbyRef; 2]>,
        attrs: Attrs,
    },
}

struct NearbyRef {
    key: Key,
    slot: NearbySlot,
}

enum NearbySlot {
    BehindContent,
    InFront,
    Above,
    Below,
    OnLeft,
    OnRight,
}

enum ContainerKind {
    El,
    Row,
}

struct Attrs {
    padding: f32,
    spacing: f32,
    font_size: Option<f32>,
}

enum CreateError {
    RootMustBeContainer,
}

impl Tree {
    fn new() -> Self {
        Self {
            root: None,
            elements: SlotMap::with_key(),
            by_element_id: HashMap::new(),
            layout: SecondaryMap::new(),
        }
    }

    fn with_root(id: Id, kind: Kind) -> Result<Self, CreateError> {
        if !matches!(kind, Kind::Container { .. }) {
            return Err(CreateError::RootMustBeContainer);
        }

        let mut elements = SlotMap::with_key();
        let root = elements.insert(Element {
            id,
            parent: None,
            kind,
        });

        Ok(Self {
            root: Some(root),
            elements,
            by_element_id: HashMap::from([(id, root)]),
            layout: SecondaryMap::new(),
        })
    }
}

#[cfg(test)]
mod test;
