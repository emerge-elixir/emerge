use slotmap::{SlotMap, new_key_type};
use smallvec::SmallVec;
use std::collections::HashMap;

mod geometry;
mod layout;
mod serde;
mod shapes;

use layout::Layout;
use shapes::{ElementSpec, Shape};

new_key_type! {
   pub(crate) struct Key;
   pub(crate) struct TextKey;
   pub(crate) struct ContainerKey;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
struct Id(pub u64);

#[derive(Debug)]
struct Tree {
    root: Option<Key>,
    elements: Elements,
    layout: Layout,
}

impl Default for Tree {
    fn default() -> Tree {
        Tree {
            root: None,
            elements: Elements::default(),
            layout: Layout::default(),
        }
    }
}

#[derive(Debug)]
struct Elements {
    storage: SlotMap<Key, Element>,
    by_element_id: HashMap<Id, Key>,

    texts: SlotMap<TextKey, TextData>,
    containers: SlotMap<ContainerKey, ContainerData>,
}

impl Default for Elements {
    fn default() -> Self {
        Self {
            storage: SlotMap::with_key(),
            by_element_id: HashMap::new(),
            texts: SlotMap::with_key(),
            containers: SlotMap::with_key(),
        }
    }
}

#[derive(Debug)]
struct Element {
    pub id: Id,
    pub parent: Option<Parent>,
    depth: usize,
    shape: Shape,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct ElementRef<'a> {
    pub(crate) key: Key,
    pub(crate) parent: Option<Parent>,
    pub(crate) depth: usize,
    pub(crate) shape: shapes::ShapeRef<'a>,
    pub(crate) children: &'a [Key],
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct Parent {
    key: Key,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct TextData {
    pub(crate) content: String,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct ContainerData {
    pub(crate) attrs: Attrs,
    pub(crate) children: SmallVec<[Key; 4]>,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Attrs {
    padding: f32,
    spacing: f32,
    font_size: Option<f32>,
}

#[derive(Debug)]
pub(crate) enum CreateError {
    RootMustBeContainer,
}

#[derive(Debug)]
pub(crate) enum InsertError {
    DuplicateId,
    MissingParent,
    ParentCannotHaveChildren,
    ElAlreadyHasChild,
}

impl Tree {
    fn with_root(id: Id, element: ElementSpec) -> Result<Self, CreateError> {
        let mut tree = Self::default();
        let root = tree.elements.insert_root(id, element)?;
        tree.root = Some(root);
        let element = &tree.elements.get(root).unwrap();
        tree.layout.root_inserted(element);
        Ok(tree)
    }

    fn insert_element(
        &mut self,
        id: Id,
        element: ElementSpec,
        parent: Key,
    ) -> Result<Key, InsertError> {
        let key = self.elements.insert_element(id, element, parent)?;
        let element = &self.elements.get(key).unwrap();
        let parent_element = &self.elements.get(parent).unwrap();
        self.layout.element_inserted(element, parent_element);
        Ok(key)
    }
}

impl Elements {
    fn insert_root(&mut self, id: Id, spec: ElementSpec) -> Result<Key, CreateError> {
        if !spec.can_be_root() {
            return Err(CreateError::RootMustBeContainer);
        }

        let shape = spec.insert(self);

        let key = self.storage.insert(Element {
            id,
            parent: None,
            depth: 0,
            shape,
        });

        self.by_element_id.insert(id, key);

        Ok(key)
    }

    fn insert_element(
        &mut self,
        id: Id,
        spec: ElementSpec,
        parent: Key,
    ) -> Result<Key, InsertError> {
        if self.by_element_id.contains_key(&id) {
            return Err(InsertError::DuplicateId);
        }

        let parent_element = self.get(parent).ok_or(InsertError::MissingParent)?;
        let parent_container_key = parent_element.shape.valid_as_parent()?;

        let depth = parent_element.depth + 1;
        let shape = spec.insert(self);

        let key = self.storage.insert(Element {
            id,
            depth,
            parent: Some(Parent { key: parent }),
            shape,
        });

        self.by_element_id.insert(id, key);
        self.containers[parent_container_key].children.push(key);

        Ok(key)
    }

    fn get(&self, key: Key) -> Option<ElementRef<'_>> {
        let element = self.storage.get(key)?;
        let shape = element.shape.bind(self);

        Some(ElementRef {
            key,
            parent: element.parent,
            depth: element.depth,
            shape,
            children: shape.source_children(),
        })
    }

    fn get_mut(&mut self, key: Key) -> Option<&mut Element> {
        self.storage.get_mut(key)
    }

    fn contains_key(&self, key: Key) -> bool {
        self.storage.contains_key(key)
    }

    fn len(&self) -> usize {
        self.storage.len()
    }

    fn is_empty(&self) -> bool {
        self.storage.is_empty()
    }
}

#[cfg(test)]
mod test;
