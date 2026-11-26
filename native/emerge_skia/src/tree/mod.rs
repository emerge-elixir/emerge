//! Wire IDs name declarations, keys name cache slots, and attachments name live input targets.
mod capacity;
mod changes;
mod declaration;
mod decoration;
mod images;
mod intrinsic;
mod layers;
mod paint;
mod paragraph;
mod pool;
mod queue;
mod rotation;
use rotation::Rotation;
mod scale;
mod shapes;
mod text;
mod wire;
pub(crate) use images::Ticket;
mod motion;
pub(crate) mod storage;
pub(crate) use motion::{Endpoints, Motion};
pub(crate) use wire::{
    media::Staged,
    publisher::{Draft, Publication, Publisher},
    stream::{CHUNK, Stream},
};

use layers::{Layers, Pass};
#[cfg(all(
    target_os = "linux",
    feature = "wayland",
    any(feature = "opengl", feature = "vulkan")
))]
pub(crate) use paint::GeometryPainter;
pub(crate) use paint::{Attachment, Raster, Scene};
use pool::{Budget, Pool, Span};
use slotmap::{SecondaryMap, SlotMap, new_key_type};
use std::{cell::Cell, collections::HashMap, ops::Range};
pub(crate) use text::BUNDLED_FONTS;
pub use text::{Family, TextMeasurer};
new_key_type! { pub(crate) struct Key; }
/// Root is at depth zero. Enforced during upload, before mounting.
const MAX_DEPTH: usize = 512;

use shapes::{FlowInput, Shape};

/// Endpoints of a monotone pixel clamp. Infinity is the internal open cap,
/// never an authored pixel value. Folding min/max preserves floor <= cap.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Bounds<T> {
    floor: T,
    cap: T,
}
impl<T> Bounds<T> {
    fn map<U>(self, f: impl Fn(T) -> U) -> Bounds<U> {
        Bounds {
            floor: f(self.floor),
            cap: f(self.cap),
        }
    }
}
impl Bounds<f64> {
    const OPEN: Self = Self {
        floor: 0.,
        cap: f64::INFINITY,
    };
    // Pointwise min/max of two clamps of the same input is another clamp.
    fn combine(self, other: Self, min: bool) -> Self {
        let choose = if min { f64::min } else { f64::max };
        Self {
            floor: choose(self.floor, other.floor),
            cap: choose(self.cap, other.cap),
        }
    }
    fn device(self, scale: f32) -> Bounds<f32> {
        self.map(|v| scale::pixels(v, scale))
    }
    fn resolve(self, basis: f32, scale: f32) -> f32 {
        let b = self.device(scale);
        basis.clamp(b.floor, b.cap)
    }
}
/// Only an authored floor replaces the automatic wrap minimum, even if it resolves to zero.
fn wrap_minimum(
    floor: Option<Bounds<f64>>,
    preferred: f32,
    automatic: f32,
    child_scale: impl FnOnce() -> f32,
) -> f32 {
    floor.map_or(automatic, |bounds| bounds.resolve(preferred, child_scale()))
}

/// Only a mixed expression which cannot be reduced to a single pixel clamp
/// retains a program. The offsets address *this owner's* validated attr block.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Formula {
    start: u32,
    end: u32,
    natural: Bounds<f64>,
    weight: f64,
    reads_content: bool,
}
#[derive(Clone, Copy, Debug, Default, PartialEq)]
enum Length {
    #[default]
    Auto,
    Content,
    Px(f64),
    Fill,
    Weighted(f64),
    BoundedContent(Bounds<f64>),
    Formula(Formula),
    BoundedFill {
        bounds: Bounds<f64>,
        weight: f64,
    },
}
impl Length {
    fn weight(self) -> Option<f64> {
        match self {
            Self::Formula(f) => Some(f.weight),
            Self::Fill => Some(1.),
            Self::Weighted(w) | Self::BoundedFill { weight: w, .. } => Some(w),
            _ => None,
        }
    }
    // The splitter reads the folded interval, not the spelling of its operands.
    fn split_policy(self) -> Option<Self> {
        match self {
            Self::Weighted(1.) => Some(Self::Fill),
            policy => policy.weight().map(|_| policy),
        }
    }
    fn same_split(self, raw: &[u8], other: Self, other_raw: &[u8]) -> bool {
        match (self, other) {
            (Self::Formula(f), Self::Formula(g)) => f.bytes(raw) == g.bytes(other_raw),
            _ => self.split_policy() == other.split_policy(),
        }
    }
    fn reads_fill_content(self) -> bool {
        matches!(self, Self::Formula(f) if f.reads_content)
    }
    fn without_weight(self) -> Self {
        match self {
            // With no slot, every variable leaf reads the same intrinsic size.
            Self::Formula(f) => Self::BoundedFill {
                bounds: f.natural,
                weight: 1.,
            },
            Self::Weighted(_) => Self::Fill,
            Self::BoundedFill { bounds, .. } => Self::BoundedFill { bounds, weight: 1. },
            other => other,
        }
    }
    fn bounds(self) -> Option<Bounds<f64>> {
        match self {
            Self::BoundedContent(bounds) | Self::BoundedFill { bounds, .. } => Some(bounds),
            _ => None,
        }
    }
    fn natural_bounds(self) -> Bounds<f64> {
        match self {
            Self::Px(n) => Bounds { floor: n, cap: n },
            Self::Formula(f) => f.natural,
            _ => self.bounds().unwrap_or(Bounds::OPEN),
        }
    }
    fn definite(self) -> bool {
        !matches!(self, Self::Auto | Self::Content | Self::BoundedContent(_))
    }
    // An unconstrained column fits one group, even at fractional sizes.
    fn limits_column_height(self) -> bool {
        match self {
            Self::Auto | Self::Content => false,
            Self::BoundedContent(b) => b.cap.is_finite(),
            _ => true,
        }
    }
    fn independent_fill(self) -> bool {
        self.weight().is_some() && !self.reads_fill_content()
    }
    fn declared_height_slot(self) -> bool {
        matches!(self, Self::Px(_)) || self.independent_fill()
    }
    // Can wrapped content grow a floating paragraph beyond its intrinsic offer?
    // A min needs both branches to grow; a max needs just one. For a folded
    // content clamp the upper bound already answers this question.
    fn grows_with_content(self, raw: &[u8]) -> bool {
        match self {
            Self::Auto | Self::Content => true,
            Self::BoundedContent(b) => b.cap.is_infinite(),
            Self::Formula(f) => f.eval(
                raw,
                true,
                |_| false,
                |_| false,
                |min, a, b| if min { a && b } else { a || b },
            ),
            _ => false,
        }
    }
    // Unallotted fit at settled width, before an enclosing allocator assigns space.
    fn fits_content_height(self) -> bool {
        !matches!(self, Self::Px(_))
    }
    // The basis is already an outer extent in device pixels: intrinsic, a
    // parent's proportional share, or the viewport for a fill root. Only a
    // pixel operand needs to look up the declaring site's scale.
    fn resolve(self, basis: f32, scale: impl FnOnce() -> f32) -> f32 {
        match self {
            Self::Px(px) => scale::pixels(px, scale()),
            Self::Formula(Formula {
                natural: bounds, ..
            })
            | Self::BoundedContent(bounds)
            | Self::BoundedFill { bounds, .. } => bounds.resolve(basis, scale()),
            _ => basis,
        }
    }
    // For a fill policy on a cross axis (or at the root), every fill leaf
    // sees the same offer. Without a content leaf the folded clamp suffices.
    fn at_offer(self, e: &Elements, k: Key, basis: f32, offer: f32) -> f32 {
        if self.reads_fill_content() {
            self.allotted(e.raw(k), basis, e.slots[k].admitted.scale, |_| offer)
        } else {
            self.resolve(offer, || e.slots[k].admitted.scale)
        }
    }
    fn allotted(self, raw: &[u8], basis: f32, scale: f32, share: impl Fn(f64) -> f32) -> f32 {
        match self {
            Self::Formula(f) => f.eval(
                raw,
                basis,
                |p| scale::pixels(p, scale),
                share,
                |min, a, b| if min { a.min(b) } else { a.max(b) },
            ),
            _ => self.resolve(self.weight().map_or(basis, share), || scale),
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
    fn width_from_height(self) -> bool {
        !self.width.definite() && self.height.weight().is_some()
    }
    fn height_from_width(self) -> bool {
        !self.height.definite() && self.width.weight().is_some()
    }
    // Weights belong to the distributing parent, not to this intrinsic box.
    fn without_weights(self) -> Self {
        Self {
            width: self.width.without_weight(),
            height: self.height.without_weight(),
            ..self
        }
    }
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum Alignment {
    #[default]
    Start,
    Center,
    End,
}
impl Alignment {
    fn offset(self, slot: f32, extent: f32) -> f32 {
        match self {
            Self::Start => 0.,
            Self::Center => (slot - extent) * 0.5,
            Self::End => slot - extent,
        }
    }
}
#[derive(Clone, Copy, Debug, Default, PartialEq)]
enum ImageFit {
    #[default]
    Contain,
    Cover,
    Repeat,
    RepeatX,
    RepeatY,
}
impl ImageFit {
    fn tiled(self) -> bool {
        matches!(self, Self::Repeat | Self::RepeatX | Self::RepeatY)
    }
}
#[derive(Clone, Copy, Debug, PartialEq)]
struct Pose {
    degrees: f32,
    zoom: f32,
}
impl Default for Pose {
    fn default() -> Self {
        Self {
            degrees: 0.,
            zoom: 1.,
        }
    }
}
/// Aspect inference distinguishes literal dimensions from expressions which
/// happen to fold to the same allocation policy. Omission is bare content.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum ImageAxis {
    #[default]
    Content,
    Pixel,
    Bound,
    BoundContent,
    Fill,
    FillContent,
}
#[derive(Clone, Debug, Default)]
struct Attrs {
    sizing: BoxStyle,
    // Guaranteed floor of a fill width, as a clamp of preferred outer width.
    // Only a wrapped edge reads this. None keeps the automatic minimum.
    line_floor: Option<Bounds<f64>>,
    height_floor: Option<Bounds<f64>>,
    align: [Option<Alignment>; 2],
    space_evenly: bool,
    // Admission also checks operands masked by a tighter bound. This is not a layout input.
    largest_length_px: f64,
    family: Option<Family>,
    weight: Option<text::Weight>,
    italic: Option<bool>,
    text_align: Option<Alignment>,
    font_size: Option<f64>,
    layout_scale: Option<f64>,
    color: Option<u32>,
    background: Option<u32>,
    image_src: decoration::Bytes,
    svg_expected: bool,
    svg_color: Option<u32>,
    svg_gradient: decoration::Bytes,
    image_fit: ImageFit,
    image_axes: [ImageAxis; 2],
    pose: Pose,
    pose_declared: bool, // reserve a scope even at an authored identity endpoint
    rotation: Rotation,
    image_host_clip: bool,
    opacity: Option<u8>,
    decor: decoration::Decor,
    font_gradient: decoration::Bytes,
    underline: Option<bool>,
    strike: Option<bool>,
    offset: [f64; 2],
    click: bool,
    // Relative to the pooled wire block. No retained String or second copy of UTF-8.
    text: Range<usize>,
    glyph_count: usize,
    words: usize,
    multiline: bool,
}
#[derive(Clone, Copy, PartialEq)]
enum MainSpacing {
    Fixed(f64),
    Even,
}
impl MainSpacing {
    // Only the share budget. Intrinsic sizing and wrap breaks use authored gaps.
    fn charge(self) -> f64 {
        match self {
            Self::Fixed(gap) => gap,
            Self::Even => 0.,
        }
    }
}
impl Attrs {
    fn has_art(&self) -> bool {
        self.image_src.len != 0
    }
    fn posed(&self) -> bool {
        self.pose != Pose::default()
    }
    fn clips_media(&self) -> bool {
        self.decor.radius != [0.; 4] || self.decor.border != [0.; 4]
    }
    fn main_spacing(&self, axis: usize) -> MainSpacing {
        let length = [self.sizing.width, self.sizing.height][axis];
        if self.space_evenly && length.definite() {
            MainSpacing::Even
        } else {
            MainSpacing::Fixed(self.sizing.spacing[axis])
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Subtree {
    count: usize,
    height: usize,
    // Cache the children's union separately: an attrs-only edit never needs
    // to reopen a kept order to prove its own layout dependencies.
    own: rotation::Dependencies,
    children: rotation::Dependencies,
}
impl Default for Subtree {
    fn default() -> Self {
        Self {
            count: 1,
            height: 0,
            own: Default::default(),
            children: Default::default(),
        }
    }
}
impl Subtree {
    fn over(children: impl Iterator<Item = Self>) -> Self {
        children.fold(Self::default(), |s, c| Self {
            count: s.count + c.count,
            height: s.height.max(1 + c.height),
            children: s.children.union(c.own),
            own: Default::default(),
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
    ink_revision: u64,
    change: changes::Change,
    children: Span<Key>,
    bytes: Span<u8>,
}
#[derive(Default)]
struct Elements {
    slots: declaration::Mounts,
    ids: HashMap<u64, Key>,
    attrs: declaration::Attributes,
    child_pool: Pool<Key>,
    bytes: Pool<u8>,
    touched: Vec<Key>,
    images: images::Images,
    media: wire::media::Declarations,
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
    fn parent_shape(&self, k: Key) -> Option<Shape> {
        self.slots[k].parent.map(|p| self.slots[p].shape)
    }
    fn float_side(&self, k: Key) -> Option<Alignment> {
        self.attrs[k].align[0].filter(|a| *a != Alignment::Center)
    }
    /// The paragraph owns a direct non-float and, for an El, its text source.
    fn is_prose_float(&self, k: Key) -> bool {
        self.parent_shape(k).is_some_and(Shape::has_prose_sites) && self.float_side(k).is_some()
    }
    fn inline_host(&self, k: Key) -> Option<Key> {
        let p = self.slots[k].parent?;
        if self.slots[p].shape == Shape::Paragraph {
            return self.float_side(k).is_none().then_some(p);
        }
        (self.slots[k].shape == Shape::Text
            && self.slots[p].shape == Shape::El
            && self.float_side(p).is_none()
            && self.parent_shape(p) == Some(Shape::Paragraph))
        .then(|| self.slots[p].parent.unwrap())
    }
    fn alignment(&self, k: Key, axis: usize) -> Alignment {
        self.attrs[k].align[axis]
            .or_else(|| {
                self.slots[k]
                    .parent
                    .filter(|&p| self.slots[p].shape == Shape::El)
                    .and_then(|p| self.attrs[p].align[axis])
            })
            .unwrap_or_default()
    }
    fn children(&self, key: Key) -> &[Key] {
        if let Some(v) = &self.attrs.version {
            &v.get(key).expect("declared owner").children
        } else {
            self.child_pool.get(&self.slots[key].children)
        }
    }
    fn raw(&self, key: Key) -> &[u8] {
        if let Some(v) = &self.attrs.version {
            &v.get(key).expect("declared owner").block.raw
        } else {
            self.bytes.get(&self.slots[key].bytes)
        }
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
    fn contains(self, x: f32, y: f32) -> bool {
        self.width > 0.
            && self.height > 0.
            && x >= self.x
            && x <= self.x + self.width
            && y >= self.y
            && y <= self.y + self.height
    }
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
    workspace: Option<std::sync::Arc<storage::Grant>>,
    elements: Elements,
    root: Option<Key>,
    layers: Layers,
    wire: wire::Scratch,
    input_budget: crate::raster_assets::InputBudget,
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
        self.layout(width, height, scale)?;
        self.output.collect(&self.elements, self.root, &self.layers);
        Ok(())
    }
    pub(crate) fn layout(&mut self, width: f32, height: f32, scale: f32) -> Result<(), String> {
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
        self.layers
            .invalidate_changes(&mut self.elements, self.root);
        self.layers
            .recompute(&self.elements, self.root, width, height);
        self.layers.bounds.context_dirty = false;
        Ok(())
    }
}
#[cfg(test)]
mod tests;
#[cfg(test)]
pub(crate) use tests::image_fixture;
