//! Wire IDs name declarations, keys name cache slots, and attachments name live input targets.
mod capacity;
mod changes;
mod intrinsic;
mod layers;
mod paint;
mod pool;
mod queue;
mod scale;
mod shapes;
mod text;
mod wire;

use layers::{Layers, Pass};
#[cfg(all(
    target_os = "linux",
    feature = "wayland",
    any(feature = "opengl", feature = "vulkan")
))]
pub(crate) use paint::GeometryPainter;
pub(crate) use paint::{Attachment, Raster};
use pool::{Budget, Pool, Span};
use slotmap::{SecondaryMap, SlotMap, new_key_type};
use std::{cell::Cell, collections::HashMap, ops::Range};
pub use text::{Family, TextMeasurer};
new_key_type! { pub(crate) struct Key; }
/// Root is at depth zero. Enforced during upload, before mounting.
const MAX_DEPTH: usize = 512;

use shapes::{FlowInput, Shape};
#[derive(Clone, Copy, Debug, Default, PartialEq)]
enum Length {
    #[default]
    Auto,
    Content,
    Px(f64),
    Fill,
    Weighted(f64),
    Capped {
        px: f64,
        weight: f64,
    },
    Floored {
        px: f64,
        weight: f64,
    },
}
impl Length {
    fn weight(self) -> Option<f64> {
        match self {
            Self::Fill => Some(1.),
            Self::Weighted(w)
            | Self::Capped { weight: w, .. }
            | Self::Floored { weight: w, .. } => Some(w),
            _ => None,
        }
    }
    // The splitter reads both leaves, but not operand order or the spelling of one share.
    fn split_policy(self) -> Option<Self> {
        match self {
            Self::Weighted(1.) => Some(Self::Fill),
            policy => policy.weight().map(|_| policy),
        }
    }
    fn without_weight(self) -> Self {
        match self {
            Self::Weighted(_) => Self::Fill,
            Self::Capped { px, .. } => Self::Capped { px, weight: 1. },
            Self::Floored { px, .. } => Self::Floored { px, weight: 1. },
            other => other,
        }
    }
    fn bound_px(self) -> Option<f64> {
        match self {
            Self::Capped { px, .. } | Self::Floored { px, .. } => Some(px),
            _ => None,
        }
    }
    fn authored_px(self) -> Option<f64> {
        if let Self::Px(px) = self {
            Some(px)
        } else {
            self.bound_px()
        }
    }
    fn definite(self) -> bool {
        !matches!(self, Self::Auto | Self::Content)
    }
    // The basis is already an outer extent in device pixels: intrinsic, a
    // parent's proportional share, or the viewport for a fill root. Only a
    // pixel operand needs to look up the declaring site's scale.
    fn resolve(self, basis: f32, scale: impl FnOnce() -> f32) -> f32 {
        match self {
            Self::Px(px) => scale::pixels(px, scale()),
            Self::Capped { px, .. } => basis.min(scale::pixels(px, scale())),
            Self::Floored { px, .. } => basis.max(scale::pixels(px, scale())),
            _ => basis,
        }
    }
}
#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct BoxStyle {
    width: Length,
    height: Length,
    padding: [f64; 4],
    spacing: [f64; 2],
}
impl BoxStyle {
    // Weights belong to the distributing parent, not to this intrinsic box.
    fn without_weights(self) -> Self {
        Self {
            width: self.width.without_weight(),
            height: self.height.without_weight(),
            ..self
        }
    }
}
#[derive(Clone, Debug, Default)]
struct Attrs {
    sizing: BoxStyle,
    family: Option<Family>,
    font_size: Option<f64>,
    layout_scale: Option<f64>,
    color: Option<u32>,
    background: Option<u32>,
    click: bool,
    // Relative to the pooled wire block. No retained String or second copy of UTF-8.
    text: Range<usize>,
    glyph_count: usize,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Subtree {
    count: usize,
    height: usize,
}
impl Default for Subtree {
    fn default() -> Self {
        Self {
            count: 1,
            height: 0,
        }
    }
}
impl Subtree {
    fn over(children: impl Iterator<Item = Self>) -> Self {
        children.fold(Self::default(), |s, c| Self {
            count: s.count + c.count,
            height: s.height.max(1 + c.height),
        })
    }
}
#[derive(Debug)]
struct Mount {
    id: u64,
    shape: Shape,
    parent: Option<Key>,
    depth: Cell<(u64, usize)>,
    stats: Subtree,
    // Resource inputs of the last accepted declaration, not installed geometry.
    admitted: scale::Inputs,
    index: usize,
    // Some only while this slot is in the detached cache.
    detached: Option<usize>,
    attachment: u64,
    change: changes::Change,
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
    touched: Vec<Key>,
    detached: Vec<Key>,
    free_indices: Vec<usize>,
    live_bytes: usize,
    topology: u64,
}
impl Elements {
    fn live(&self, id: u64) -> Option<Key> {
        self.ids
            .get(&id)
            .copied()
            .filter(|&k| self.slots[k].detached.is_none())
    }
    // Read final parent links, not the old depth of a moved kept branch. Only
    // paths read by admission or a scheduled pass get a current memo.
    fn depth(&self, k: Key) -> usize {
        let m = &self.slots[k];
        let (epoch, d) = m.depth.get();
        if epoch == self.topology {
            return d;
        }
        let d = m.parent.map_or(0, |p| 1 + self.depth(p));
        m.depth.set((self.topology, d));
        d
    }
    fn attached_count(&self) -> usize {
        self.slots.len() - self.detached.len()
    }
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
    pub fn frames(&self) -> impl Iterator<Item = (u64, Rect)> + '_ {
        self.elements
            .slots
            .iter()
            .filter(|(_, n)| n.detached.is_none())
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
        if self.layers.prepared_scale != scale {
            self.prepare_display(scale)?;
        }
        self.layers.invalidate_changes(&mut self.elements);
        self.layers
            .recompute(&self.elements, self.root, width, height);
        self.layers.bounds.context_dirty = false;
        self.output.collect(&self.elements, self.root, &self.layers);
        Ok(())
    }
}
#[cfg(test)]
mod tests;
