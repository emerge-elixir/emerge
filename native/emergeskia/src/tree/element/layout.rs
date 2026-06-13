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
    const ALL: [Phase; Self::COUNT] = [Self::Context, Self::Measure, Self::Resolve];
    const fn index(self) -> usize {
        self as usize
    }
}

#[derive(Default, Debug)]
pub(crate) struct Layout {
    pub context: SecondaryMap<Key, Context>,
    pub measure: SecondaryMap<Key, Measure>,
    pub resolve: SecondaryMap<Key, Resolve>,

    placement: SecondaryMap<Key, Placement>,
    invalidation: Invalidation,
}

impl Layout {
    fn clear(&mut self) {
        self.context.clear();
        self.measure.clear();
        self.resolve.clear();
        self.placement.clear();
        self.invalidation.clear();
    }

    pub(crate) fn layout_queued(&mut self, elements: &Elements) {
        self.sort_invalidation();
        self.layout(elements);
    }

    pub(crate) fn layout(&mut self, elements: &Elements) {
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

        while let Some(key) = self.invalidation.pop(Phase::Resolve) {
            let element = elements.get(key).expect("element present for layout");
            let (_changed, dirty_children) = self.compute_resolve(&element);
            for child_key in dirty_children {
                let child = elements.get(child_key).expect("child");
                self.dirty_resolve(&child)
            }
        }
    }

    fn queue_context(&mut self, element: &ElementRef) {
        self.invalidation
            .queue(Phase::Context, element.key, element.depth);
    }

    fn dirty_context(&mut self, element: &ElementRef) {
        self.invalidation
            .dirty(Phase::Context, element.key, element.depth);
    }

    fn compute_context(&mut self, element: &ElementRef) -> bool {
        let parent_context = self.parent_context(element);
        let new = element.shape.context(parent_context);
        let old = self.context.get(element.key);

        if old == Some(&new) {
            false
        } else {
            self.context.insert(element.key, new);
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

    fn queue_measure(&mut self, element: &ElementRef) {
        self.invalidation
            .queue(Phase::Measure, element.key, element.depth);
    }

    fn dirty_measure(&mut self, element: &ElementRef) {
        self.invalidation
            .dirty(Phase::Measure, element.key, element.depth);
    }

    fn compute_measure(&mut self, element: &ElementRef) -> bool {
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

    fn queue_resolve(&mut self, element: &ElementRef) {
        self.invalidation
            .queue(Phase::Resolve, element.key, element.depth);
    }

    fn dirty_resolve(&mut self, element: &ElementRef) {
        self.invalidation
            .dirty(Phase::Resolve, element.key, element.depth);
    }

    fn compute_resolve(&mut self, element: &ElementRef) -> (bool, SmallVec<[Key; 4]>) {
        let context = self.context(element).expect("context before resolve");
        let measure = self.measure(element).expect("measure before resolve");
        let placement = self.placement(element);

        let result = {
            let child_measurements = self.child_measurements(element);

            element
                .shape
                .resolve(context, measure, &child_measurements, placement)
        };

        let resolve_changed = self.resolve.get(element.key) != Some(&result.resolve);
        if resolve_changed {
            self.resolve.insert(element.key, result.resolve);
        }

        let child_placement_changes = result
            .child_placements
            .iter()
            .filter_map(|(child, placement)| {
                if self.placement.get(*child) == Some(placement) {
                    None
                } else {
                    self.placement.insert(*child, *placement);
                    Some(*child)
                }
            })
            .collect();

        (resolve_changed, child_placement_changes)
    }

    fn placement(&self, element: &ElementRef) -> &Placement {
        match element.parent {
            None => &Placement::DEFAULT,
            Some(_) => self
                .placement
                .get(element.key)
                .expect("placement before child resolve"),
        }
    }

    pub fn root_inserted(&mut self, element: &ElementRef) {
        self.queue_context(element);
    }

    pub fn element_inserted(&mut self, element: &ElementRef, parent: &ElementRef) {
        self.queue_context(element);
        self.queue_resolve(parent);
    }

    // Required after subtree manipulations
    pub fn sort_invalidation(&mut self) {
        self.invalidation.sort_all();
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
    ) -> ResolveResult;
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
pub(crate) struct ResolveResult {
    pub resolve: Resolve,
    pub child_placements: SmallVec<[(Key, Placement); 4]>,
}

impl ResolveResult {
    pub(crate) fn new(
        frame: Rect,
        content: Rect,
        content_size: Size,
        child_placements: SmallVec<[(Key, Placement); 4]>,
    ) -> Self {
        let children = child_placements
            .iter()
            .map(|(key, _placement)| *key)
            .collect();

        Self {
            resolve: Resolve {
                frame,
                content,
                content_size,
                children,
            },
            child_placements,
        }
    }

    pub(crate) fn new_leaf(frame: Rect, content: Rect, content_size: Size) -> Self {
        Self::new(frame, content, content_size, SmallVec::new())
    }
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
    pub children: SmallVec<[Key; 4]>,
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
