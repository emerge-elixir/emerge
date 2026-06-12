use indexmap::IndexMap;
use slotmap::SecondaryMap;
use smallvec::SmallVec;

mod invalidation;

use super::geometry::{Point, Rect, Size};
use super::{ElementRef, Elements, Key};
use invalidation::Invalidation;

#[repr(usize)]
#[derive(Clone, Copy, PartialEq)]
enum Phase {
    Context = 0,
    Measure = 1,
    Resolve = 2,
}

impl Phase {
    const COUNT: usize = 3;

    const fn index(self) -> usize {
        self as usize
    }
}

#[derive(Default, Debug)]
pub(crate) struct Layout {
    pub context: SecondaryMap<Key, Context>,
    pub measure: SecondaryMap<Key, Measure>,
    pub resolve: SecondaryMap<Key, Resolve>,
    invalidation: Invalidation,
}

impl Layout {
    fn clear(&mut self) {
        self.context.clear();
        self.measure.clear();
        self.resolve.clear();
        self.invalidation.clear();
    }

    pub(crate) fn layout(&mut self, elements: &Elements) {
        self.invalidation.sort(Phase::Context);
        while let Some(key) = self.invalidation.pop(Phase::Context) {
            let element = elements.get(key).expect("element present for layout");
            if self.compute_context(&element) {
                self.dirty_measure(&element);
                for child_key in element.children {
                    let child = elements.get(*child_key).expect("child");
                    self.dirty_context(&child)
                }
            }
        }

        self.invalidation.sort(Phase::Measure);
        while let Some(key) = self.invalidation.pop(Phase::Measure) {
            let element = elements.get(key).expect("element present for layout");
            if self.compute_measure(&element) {
                self.dirty_resolve(&element);

                if let Some(parent_element) = element
                    .parent
                    .map(|parent| parent.key)
                    .and_then(|parent_key| elements.get(parent_key))
                {
                    self.dirty_measure(&parent_element);
                };
            }
        }

        self.invalidation.sort(Phase::Resolve);
        while let Some(key) = self.invalidation.pop(Phase::Resolve) {
            let element = elements.get(key).expect("element present for layout");
            if self.compute_resolve(&element) {
                for child_key in element.children {
                    let child = elements.get(*child_key).expect("child");
                    self.dirty_resolve(&child)
                }
            }
        }
    }

    fn dirty_context(&mut self, element: &ElementRef) {
        self.invalidation
            .dirty(Phase::Context, element.key, element.depth);
    }

    fn compute_context(&mut self, element: &ElementRef) -> bool {
        self.invalidation.clean(Phase::Context, element.key);

        let parent_context = self.parent_context(element);
        let new = element.shape.context(parent_context);
        let old = self.context.get(element.key);

        if old == Some(&new) {
            false
        } else {
            self.context.insert(element.key, new);
            self.dirty_measure(element);
            true
        }
    }

    fn context(&self, element: &ElementRef) -> Option<&Context> {
        self.context.get(element.key)
    }

    fn parent_context(&self, element: &ElementRef) -> &Context {
        element
            .parent
            .map(|parent| parent.key)
            .and_then(|parent_key| self.context.get(parent_key))
            .unwrap_or(&Context::DEFAULT)
    }

    fn dirty_measure(&mut self, element: &ElementRef) {
        self.invalidation
            .dirty(Phase::Measure, element.key, element.depth);
    }

    fn compute_measure(&mut self, element: &ElementRef) -> bool {
        self.invalidation.clean(Phase::Measure, element.key);

        let context = self.context(element).expect("context before measure");
        let new = {
            let child_measurements = self.child_measurements(element);
            element.shape.measure(context, &child_measurements)
        };

        let old = self.measure.get(element.key);
        if old == Some(&new) {
            false
        } else {
            self.measure.insert(element.key, new);
            self.dirty_resolve(element);
            true
        }
    }

    fn measure(&self, element: &ElementRef) -> Option<&Measure> {
        self.measure.get(element.key)
    }

    fn child_measurements(&self, element: &ElementRef) -> SmallVec<[ChildMeasure<'_>; 4]> {
        element
            .children
            .iter()
            .map(|child_key| ChildMeasure {
                key: *child_key,
                measure: self
                    .measure
                    .get(*child_key)
                    .expect("child measure before parent"),
            })
            .collect()
    }

    fn dirty_resolve(&mut self, element: &ElementRef) {
        self.invalidation
            .dirty(Phase::Resolve, element.key, element.depth);
    }

    fn compute_resolve(&mut self, element: &ElementRef) -> bool {
        self.invalidation.clean(Phase::Resolve, element.key);

        let context = self.context(element).expect("context before resolve");
        let measure = self.measure(element).expect("measure before resolve");

        let new = {
            let child_measurements = self.child_measurements(element);
            let placement = match element.parent {
                None => &Placement::DEFAULT,
                Some(_) => {
                    self
                        .parent_resolve(element)
                        .expect("parent resolve before child")
                        .children
                        .get(&element.key)
                        .expect("parent resolve to contain child")
                }
            };

            element
                .shape
                .resolve(context, measure, &child_measurements, placement)
        };

        let old = self.resolve.get(element.key);
        if old == Some(&new) {
            false
        } else {
            self.resolve.insert(element.key, new);
            self.dirty_resolve(element);
            true
        }
    }

    fn parent_resolve(&self, element: &ElementRef) -> Option<&Resolve> {
        element
            .parent
            .map(|parent| parent.key)
            .and_then(|parent_key| self.resolve.get(parent_key))
    }

    pub fn root_inserted(&mut self, elements: &Elements, key: Key) {
        self.invalidation.dirty(
            Phase::Context,
            key,
            elements.get(key).expect("element before layout").depth,
        );
    }

    pub fn element_inserted(&mut self, elements: &Elements, key: Key) {
        self.invalidation.dirty(
            Phase::Context,
            key,
            elements.get(key).expect("element before layout").depth,
        );
    }
}

pub trait LayoutBehaviour<Data> {
    fn context(data: Data, parent: &Context) -> Context;

    fn measure(context: &Context, children: &[ChildMeasure<'_>]) -> Measure;

    fn resolve(
        context: &Context,
        measure: &Measure,
        children: &[ChildMeasure<'_>],
        placement: &Placement,
    ) -> Resolve;
}

#[derive(Clone, Debug, PartialEq)]
pub struct Context {
    pub font_size: f32,
    pub padding: f32,
    pub spacing: f32,
    pub content_size: Option<Size>,
}

impl Context {
    pub const DEFAULT: Context = Context {
        font_size: 16.0,
        padding: 0.0,
        spacing: 0.0,
        content_size: None,
    };
}

impl Default for Context {
    fn default() -> Context {
        Context::DEFAULT
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Measure {
    pub(crate) intrinsic: Size,
}

#[derive(Clone, Copy, Debug)]
pub struct ChildMeasure<'a> {
    pub key: Key,
    pub measure: &'a Measure,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Resolve {
    // Actual frame inside parent
    pub frame: Rect,
    // Content inside parent after padding
    pub content: Rect,
    // Content intrinsic size
    pub content_size: Size,
    // Frame that constrains each child
    pub children: IndexMap<Key, Placement>,
}

#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct Placement {
    pub origin: Point,
    pub size: Option<Size>,
}

impl Placement {
    pub const DEFAULT: Placement = Placement {
        origin: Point::DEFAULT,
        size: None,
    };

    pub fn frame(self, intrinsic: Size) -> Rect {
        Rect {
            origin: self.origin,
            size: self.size.unwrap_or(intrinsic),
        }
    }
}
