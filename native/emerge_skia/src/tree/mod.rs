//! The native producer. Wire IDs name declarations; a Key names one mount.
mod capacity;
mod layers;
mod paint;
mod pool;
mod queue;
mod text;
mod wire;

use layers::{Layers, Pass};
pub(crate) use paint::Raster;
use pool::{Budget, Pool, Span};
use slotmap::{SecondaryMap, SlotMap, new_key_type};
use std::{collections::HashMap, ops::Range};
pub use text::{Family, TextMeasurer};
new_key_type! { pub(crate) struct Key; }

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
enum Kind {
    Wrap = 2,
    Column = 3,
    El = 4,
    Text = 5,
    Empty = 6,
}
#[derive(Clone, Copy, Debug, Default, PartialEq)]
enum Length {
    #[default]
    Auto,
    Content,
    Px(f32),
    Fill,
}
impl Length {
    fn intrinsic(self, demand: f32, scale: f32) -> f32 {
        if let Self::Px(px) = self {
            px * scale
        } else {
            demand
        }
    }
}
#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct BoxStyle {
    width: Length,
    height: Length,
    padding: [f32; 4],
    spacing: [f32; 2],
}
#[derive(Clone, Debug, Default)]
struct Attrs {
    sizing: BoxStyle,
    family: Option<Family>,
    font_size: Option<f32>,
    color: Option<u32>,
    background: Option<u32>,
    click: bool,
    // Relative to the pooled wire block. No retained String or second copy of UTF-8.
    text: Range<usize>,
    glyph_count: usize,
}
#[derive(Debug)]
struct Mount {
    id: u64,
    kind: Kind,
    parent: Option<Key>,
    depth: usize,
    index: usize,
    children: Span<Key>,
    bytes: Span<u8>,
}
#[derive(Default)]
struct Elements {
    slots: SlotMap<Key, Mount>,
    ids: HashMap<u64, Key>,
    attrs: SecondaryMap<Key, Attrs>,
    child_pool: Pool<Key>,
    bytes: Pool<u8>,
}
impl Elements {
    fn children(&self, key: Key) -> &[Key] {
        self.child_pool.get(&self.slots[key].children)
    }
    fn raw(&self, key: Key) -> &[u8] {
        self.bytes.get(&self.slots[key].bytes)
    }
    fn text(&self, key: Key) -> &str {
        std::str::from_utf8(&self.raw(key)[self.attrs[key].text.clone()]).expect("validated UTF-8")
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}
impl Rect {
    fn intersection(self, b: Self) -> Option<Self> {
        let (x, y) = (self.x.max(b.x), self.y.max(b.y));
        let (w, h) = (
            (self.x + self.width).min(b.x + b.width) - x,
            (self.y + self.height).min(b.y + b.height) - y,
        );
        (w > 0. && h > 0.).then_some(Self {
            x,
            y,
            width: w,
            height: h,
        })
    }
}
#[derive(Default)]
pub struct Tree {
    elements: Elements,
    root: Option<Key>,
    layers: Layers,
    wire: wire::Scratch,
    output: paint::Output,
    budget: Budget,
}
impl Tree {
    pub fn with_headroom(percent: u16) -> Self {
        Self {
            budget: Budget(usize::from(percent)),
            ..Self::default()
        }
    }
    fn clear(&mut self) {
        self.elements.slots.clear(); // retain generations, invalidate every old key
        self.elements.ids.clear();
        self.elements.attrs.clear();
        self.elements.child_pool.clear();
        self.elements.bytes.clear();
        self.root = None;
    }
    pub fn frames(&self) -> impl Iterator<Item = (u64, Rect)> + '_ {
        self.elements
            .slots
            .iter()
            .filter_map(|(k, n)| Some((n.id, *self.layers.frames.get(k)?)))
    }
    pub(crate) fn refresh(&mut self, width: f32, height: f32, scale: f32) -> Result<(), String> {
        if ![width, height, scale].into_iter().all(f32::is_finite)
            || width < 0.
            || height < 0.
            || scale <= 0.
        {
            return Err("invalid layout viewport or scale".into());
        }
        if self.layers.prepared_scale != scale && self.layers.text_count != 0 {
            self.layers.text.ensure(
                self.layers.bounds.context_peak(),
                self.layers.text_count,
                self.budget,
            )?;
            self.layers
                .text
                .provision(self.layers.text.sizes, scale, true)?;
        }
        self.layers.prepared_scale = scale;
        self.layers
            .recompute(&self.elements, self.root, width, height, scale);
        self.layers.bounds.context_dirty = false;
        self.output.collect(&self.elements, self.root, &self.layers);
        Ok(())
    }
}
#[cfg(test)]
mod tests;
