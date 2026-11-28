use super::{layers::Layers, text::Run, *};
mod bands;
mod scrollbars;
use bands::{Bands, Branch, Groups};
pub(crate) use scrollbars::ScrollHit;
mod navigation;
use interaction::Actions;
pub(crate) use navigation::{Navigation, Stop};
mod buffers;
mod pages;
mod recording;
use pages::Buffer;
use recording::Tables;
pub(super) use recording::{Sites, Window, charge as recording_charge};
mod clips;
mod coverage;
pub(super) mod fits;
use buffers::{Buffers, Packed};
use std::sync::Arc;
mod image;
mod inline;
mod opacity;
mod pose;
use pose::{Coords, Plane, Planes, Scope};
fn sk(r: Rect) -> skia_safe::Rect {
    skia_safe::Rect::from_xywh(r.x, r.y, r.width, r.height)
}
use clips::{Clips, Mask, OPEN, Rounded};
#[path = "paint/decoration.rs"]
mod styled;
use skia_safe::{
    AlphaType, Color, ColorType, ImageInfo, Paint, PixelGeometry, Point, SurfaceProps,
    SurfacePropsFlags, surfaces,
};
use styled::{Brush, MaterialIndex, Materials, Phase};
#[derive(Clone, Copy, Default, PartialEq)]
pub(super) struct PaintContext {
    pub content: Option<Key>, // nearest ordinary scroll edge; retained facts are unscrolled
    pub frame: Rect,
    pub boundary: Rect, // incoming clip, before this node's pose
    pub clip: Rect,
    pub child_clip: Rect,
    pub escape_boundary: Rect,
    pub escape_clip: Rect,
    pub content_scroll: [f32; 2],
    pub scrollport: bool,
    pub bars: bool,
    pub color: u32,
    pub gradient: Option<(Key, u64)>,
    pub rules: u8,
    pub offset: [f32; 2],
    pub scale: f32,
    pub transformed: bool,
    pub zoom: f32, // cumulative absolute uniform paint scale, independent of layout scale
    pub text_align: Alignment,
}
impl Branch {
    fn pose(
        &mut self,
        planes: &mut Planes,
        parent: Coords,
        local: u32,
        a: &Attrs,
        ctx: PaintContext,
        side: usize,
    ) -> Option<()> {
        if a.posed() {
            self.escape.plane = if self.escape == parent && ctx.boundary == ctx.escape_boundary {
                local
            } else {
                planes.branch(
                    self.escape.plane,
                    self.escape.mask,
                    a,
                    PaintContext {
                        boundary: ctx.escape_boundary,
                        ..ctx
                    },
                    side == 1,
                )?
            };
        }
        Some(())
    }
}
impl PaintContext {
    fn inner_guard(self, a: &Attrs, rounded: Option<Rounded>) -> Rounded {
        rounded.unwrap_or_else(|| Rounded {
            rect: a.decor.inner_rect(self.frame, self.scale),
            radii: [0.; 4],
        })
    }
    fn box_clip(self, shape: Shape) -> Rect {
        let clip = if shape == Shape::Image {
            self.child_clip
        } else {
            self.clip
        };
        clip.paint_clip(self.transformed)
    }
}
impl Rect {
    // A media draw may relax its nearest host's clip, not the containing
    // host's already-active clip. Its layout and hit rectangles do not bleed.
    pub(super) fn media_clip(self, parent: Option<PaintContext>) -> Self {
        if let Some(p) = parent
            && self.width > 0.
            && self.height > 0.
        {
            let pad = 0.5 / p.zoom;
            if !pad.is_finite() {
                return self;
            }
            let relaxed = Self {
                x: self.x - pad,
                y: self.y - pad,
                width: self.width + 2. * pad,
                height: self.height + 2. * pad,
            };
            p.clip.intersection(relaxed).unwrap_or_default()
        } else {
            self
        }
    }
    fn shifted(self, x: f32, y: f32) -> Self {
        Self {
            x: self.x + x,
            y: self.y + y,
            ..self
        }
    }
    /// Ordinary rectangular Skia clips use non-antialiased device edges.
    /// Hits keep the unrounded clip and the original element-local origin.
    pub(super) fn paint_clip(self, transformed: bool) -> Rect {
        // Rounding in a rotated/scaled local space is not device snapping.
        if transformed {
            return self;
        }
        let x = self.x.round();
        let y = self.y.round();
        Rect {
            x,
            y,
            width: (self.x + self.width).round() - x,
            height: (self.y + self.height).round() - y,
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct Attachment(pub(super) Key, pub(super) u64);
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum HitKind {
    Listener,
    Children,
    Content,
    Owner,
    Segment(Segment),
    Escape,
    Port,
    Bars,
}
#[derive(Clone, Copy, Debug, PartialEq)]
struct Hit {
    kind: HitKind,
    actions: Actions,
    wheel: u32,
    scope: Scope,
    key: Attachment,
    id: u64,
    rect: Rect,
    origin: [f32; 2],
    scale: f32,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) enum Outputs {
    Draw,
    Hit,
    #[default]
    Both,
}
impl std::ops::BitOrAssign for Outputs {
    fn bitor_assign(&mut self, rhs: Self) {
        if *self != rhs {
            *self = Self::Both;
        }
    }
}
impl Outputs {
    pub fn changed(draw: bool, hit: bool) -> Option<Self> {
        match (draw, hit) {
            (true, true) => Some(Self::Both),
            (true, false) => Some(Self::Draw),
            (false, true) => Some(Self::Hit),
            _ => None,
        }
    }
}
#[derive(Clone, Copy, Default, PartialEq)]
struct Port {
    revision: u64,
    nonempty: bool,
    children: usize,
    seen_at_parent: Option<(Option<Slot>, usize, u64)>,
    exports: bool,
    boundary: bool,
    guarded: bool,
}
#[derive(Clone, Copy, PartialEq)]
pub(super) struct GlyphPaint {
    pub pos: [f32; 2],
    pub color: u32,
    pub gradient: Option<(Key, u64)>,
    pub rules: u8,
    pub revision: u64,
    pub clip: Option<Rect>,
}
#[derive(Clone, Copy, Default, PartialEq)]
pub(super) struct OwnPaint {
    fill: Option<(Rect, u32)>,
    decorated: bool,
    pub text: Option<GlyphPaint>,
}
impl Rect {
    /// Finite local compile extent, never the current screen intersection.
    pub(super) const UNBOUNDED: Self = Self {
        x: -1e30,
        y: -1e30,
        width: 2e30,
        height: 2e30,
    };
}
impl OwnPaint {
    pub fn background(a: &Attrs, ctx: &PaintContext) -> Self {
        let clip = if a.image_src.len != 0 {
            ctx.box_clip(Shape::Image)
        } else {
            ctx.clip.paint_clip(ctx.transformed)
        };
        let fill = a
            .background
            .filter(|v| v & 255 != 0)
            .and_then(|v| ctx.frame.intersection(clip).map(|r| (r, v)));
        Self {
            fill,
            decorated: a.decor.active(),
            text: None,
        }
    }
    fn nonempty(self) -> bool {
        self.decorated || self.fill.is_some() || self.text.is_some()
    }
}
#[derive(Clone, Copy, Default)]
pub(super) struct Composition {
    reach: scroll::Reach,
    paint: OwnPaint,
    art: bool,
    hit: Option<Hit>,
    rounded: bool,
    bars: [bool; 2],
    posed: [bool; 2],
    opacity: Option<u8>,
    coupled: bool,
    independent_children: bool,
    pub(super) separate_body: bool,
    pub(super) own_epoch: [u64; 2],
    media_subtree: bool,
    port_subtree: bool,
    pub(super) binding_child: Option<Key>,
    pub(super) binding_sibling: Option<Key>,
    defer_clips: bool,
    ports: [Port; 2],
}
#[derive(Clone, Copy)]
pub(super) struct Projected {
    pub ctx: PaintContext,
    paint: OwnPaint,
    hit: Option<Hit>,
}
#[derive(Default)]
pub(super) struct ProjectionSlot {
    pub projection: Option<Projected>,
    pub(super) anchor_hidden: bool,
    saved: [recording::Local; 2],
}
impl ProjectionSlot {
    // Its contribution now lives in an ancestor's inseparable local program.
    // Drop the mutable cache, never a sealed reader's copy.
    pub(super) fn absorbed(&mut self, outputs: Outputs) {
        for (s, v) in self.saved.iter_mut().enumerate() {
            if outputs.includes(s) {
                *v = Default::default();
            }
        }
    }
    pub(super) fn retire(&mut self, outputs: Outputs, p: Composition) {
        for (s, v) in self.saved.iter_mut().enumerate() {
            if outputs.includes(s) {
                v.retire(p.own_epoch[s]);
            }
        }
    }
}
impl Outputs {
    pub(super) fn includes(self, s: usize) -> bool {
        self == Self::Both || (s == 0) == (self == Self::Draw)
    }
}
impl Projected {
    pub fn new(e: &Elements, k: Key, ctx: PaintContext, run: Option<&Run>) -> Self {
        Self {
            paint: e.slots[k].shape.paint(e, k, &ctx, run),
            hit: ctx.hit(e, k),
            ctx,
        }
    }
}
impl PaintContext {
    fn hit(self, e: &Elements, k: Key) -> Option<Hit> {
        let a = &e.attrs[k];
        let escapes = e.slots[k].slot.is_some_and(Slot::escapes);
        if !a.actions.any() && !escapes {
            return None;
        }
        let clip = if a.posed() && e.slots[k].shape == Shape::Image {
            self.child_clip
        } else {
            self.clip
        };
        Some(Hit {
            kind: if escapes {
                HitKind::Escape
            } else {
                HitKind::Listener
            },
            wheel: 0,
            scope: Scope::default(),
            key: Attachment(k, e.slots[k].attachment),
            actions: a.actions,
            id: if a.actions.any() { e.slots[k].id } else { 0 },
            rect: self.frame.intersection(clip)?,
            origin: [self.frame.x, self.frame.y],
            scale: self.scale,
        })
    }
}
impl Composition {
    pub(super) fn sides(self) -> [u64; 2] {
        self.ports.map(|p| p.revision)
    }
    pub(super) fn anchors(self) -> bool {
        self.ports.iter().any(|p| p.boundary)
    }
    pub(super) fn independent_children(self) -> bool {
        self.independent_children
    }
    pub(super) fn facts(self) -> scroll::Facts {
        scroll::Facts {
            reach: self.reach,
            active: self.ports.map(|p| p.nonempty),
            exports: self.ports.map(|p| p.exports),
            coupled: self.coupled,
            media: self.media_subtree,
            ports: self.port_subtree,
            defer_clips: self.defer_clips,
            opaque: self.paint.fill.is_some_and(|(_, c)| c & 255 == 255)
                && !self.paint.decorated
                && self.paint.text.is_none()
                && !self.art
                && self.ports[0].children == 0
                && self.opacity.is_none()
                && !self.posed[0],
        }
    }
    fn at(mut self, projected: Option<&Projected>) -> Self {
        if let Some(p) = projected {
            self.paint = p.paint;
            self.hit = p.hit;
        }
        self
    }
    /// Reordering two hit-only panels cannot dirty the drawing (or vice versa).
    /// An evicted generation can have contributed to the old stream; a cold
    /// attachment has not composed yet. Treat either as potentially visible.
    pub(super) fn mount_order_change(
        parts: &SecondaryMap<Key, Self>,
        host: Key,
        old: Edges<'_, Key>,
        new: Edges<'_, Key>,
    ) -> Option<Outputs> {
        let boundary = |o: Edges<'_, Key>| o.tags.iter().any(|s| s.escapes());
        let moved_boundary = boundary(old) != boundary(new);
        let changed = |side: usize| {
            let reordered = |escapes| {
                let contributes = |&(s, k): &(Slot, Key)| {
                    s.escapes() == escapes && parts.get(k).is_none_or(|p| p.ports[side].nonempty)
                };
                !old.nearby()
                    .filter(contributes)
                    .eq(new.nearby().filter(contributes))
            };
            (moved_boundary && parts.get(host).is_none_or(|p| p.ports[side].exports))
                || reordered(true)
                || reordered(false)
        };
        Outputs::changed(changed(0), changed(1))
    }
    pub(super) fn reattach(&mut self) {
        for p in &mut self.ports {
            p.seen_at_parent = None;
        }
    }
}
// Each child has one parent. Comparing (ordinal among nonempty children, value
// revision) and the count is exact for its ordered contribution, without a
// second child list. Children that changed to empty need no comparison by ID.
pub(super) fn compose(l: &mut Layers, e: &Elements, k: Key, requested: Outputs) -> Option<Outputs> {
    let ctx = &l.paint[k];
    let run = l.runs.get(k);
    let parts = &mut l.parts;
    let indexed = l.scroll.facts(e, k, parts);
    let old = parts.get(k).copied().unwrap_or_default();
    let mut next = old;
    let mut delta = [false; 2];
    let a = &e.attrs[k];
    for (side, changed) in delta.iter_mut().enumerate() {
        if (side == 0 && requested == Outputs::Hit) || (side == 1 && requested == Outputs::Draw) {
            continue;
        }
        let own = if side == 0 {
            // Don't snap the unscrolled box to today's pixel phase. A thin
            // fill (or glyph just outside its own box) can become paintable
            // after a fractional scroll. Exact clipping belongs to Project.
            let stable = PaintContext {
                transformed: ctx.transformed || ctx.content.is_some(),
                ..*ctx
            };
            let paint = e.slots[k].shape.paint(e, k, &stable, run);
            let unprojected = ctx.content.is_some()
                && (run.is_some()
                    || (paint.fill.is_none() && a.background.is_some_and(|v| v & 255 != 0)));
            let image = a.has_art();
            *changed = next.paint != paint
                || paint.decorated
                || image
                || next.art
                || next.opacity != a.opacity
                || unprojected;
            next.art = image;
            next.opacity = a.opacity;
            next.paint = paint;
            image || unprojected || paint.nonempty()
        } else {
            let hit = ctx.hit(e, k);
            let rounded = a.decor.radius != [0.; 4];
            *changed = hit != next.hit
                || rounded
                || next.rounded
                || (a.actions.any() && e.slots[k].shape == Shape::Image);
            next.rounded = rounded;
            next.hit = hit;
            let unprojected = ctx.content.is_some()
                && hit.is_none()
                && (a.actions.any() || e.slots[k].slot.is_some_and(Slot::escapes));
            *changed |= unprojected;
            hit.is_some() || unprojected
        };
        *changed |= ctx.bars || next.bars[side];
        next.bars[side] = ctx.bars;
        let own = own || ctx.bars;
        // A transform on an undecorated, listener-free ancestor still changes
        // its children's prepared commands and hit coordinates.
        *changed |= a.posed() || next.posed[side];
        next.posed[side] = a.posed();
        // Ordinals are local to the visit phase. Moving an escape past a behind
        // changes the wire but neither order, even when both contribute.
        let mut counts = [0usize; 3];
        let mut exports = false;
        let mut boundary = false;
        if let Some(facts) = indexed {
            // A changed child already sent its precise side up the workset. The
            // range root summarizes ordinary contributors, including hidden ones.
            counts[0] = usize::from(facts.active[side]);
            exports = facts.exports[side];
            *changed = true;
        }
        let order = e.order(k);
        let ordinary = if indexed.is_some() {
            &[]
        } else {
            order.children()
        };
        #[cfg(test)]
        {
            l.scroll.counts.compose_reads[side] += ordinary.len();
        }
        for (role, c) in ordinary
            .iter()
            .map(|&c| (None, c))
            .chain(order.nearby().map(|(s, c)| (Some(s), c)))
        {
            let escapes = role.is_some_and(Slot::escapes);
            boundary |= escapes;
            let port = &mut parts[c].ports[side];
            if port.nonempty {
                exports |= escapes || port.exports;
                let count = &mut counts[match role {
                    None => 0,
                    Some(Slot::Behind) => 1,
                    Some(_) => 2,
                }];
                let seen = Some((role, *count, port.revision));
                *changed |= seen != port.seen_at_parent;
                port.seen_at_parent = seen;
                *count += 1;
            }
        }
        // A paragraph also owns inline contributions, which need not have a
        // nonempty ordinary child frame. Only scheduled readers reach here.
        let inline = e.slots[k].shape == Shape::Paragraph;
        *changed |= inline;
        let port = &mut next.ports[side];
        let count = counts.into_iter().sum();
        *changed |=
            port.children != count || (boundary != port.boundary && (exports || port.exports));
        let guarded = (exports && a.clip_nearby) || (counts[0] != 0 && ctx.scrollport);
        *changed |= guarded || port.guarded;
        port.guarded = guarded;
        port.exports = exports;
        port.boundary = boundary;
        port.children = count;
        let nonempty = inline || own || count != 0;
        *changed |= port.nonempty != nonempty;
        port.nonempty = nonempty;
        if *changed {
            port.revision += 1
        }
    }
    if ctx.content.is_some() {
        let ordinary = indexed.map_or_else(
            || {
                e.children(k)
                    .iter()
                    .map(|&c| parts[c].reach)
                    .fold(scroll::Reach::default(), scroll::Reach::union)
            },
            |s| s.reach,
        );
        let mounts = e
            .nearby(k)
            .map(|(slot, c)| {
                if slot.escapes() {
                    scroll::Reach::anchor(ctx.frame, parts[c].ports.map(|p| p.nonempty))
                } else {
                    parts[c].reach
                }
            })
            .fold(scroll::Reach::default(), scroll::Reach::union);
        let overhang = run
            .filter(|_| e.inline_host(k).is_some())
            .map_or(0., |&run| l.text.inline_overhang(run));
        next.reach = scroll::Reach::compose(e, k, *ctx, overhang, ordinary, mounts);
        for (side, changed) in delta.iter_mut().enumerate() {
            if !*changed && next.reach.side_changed(old.reach, side) {
                next.ports[side].revision += 1;
                *changed = true;
            }
        }
    }
    if ctx.content.is_some() || ctx.scrollport {
        let children_coupled = indexed.map_or_else(
            || e.children(k).iter().any(|c| parts[*c].coupled),
            |f| f.coupled,
        );
        next.independent_children = !children_coupled;
        next.separate_body = !ctx.scrollport
            && indexed.map_or_else(
                || e.children(k).iter().all(|&c| parts[c].separate_body),
                |f| !f.ports,
            )
            && !e
                .nearby(k)
                .any(|(s, c)| s == Slot::Behind && !parts[c].separate_body);
        next.media_subtree = a.has_art()
            || indexed.map_or_else(
                || e.children(k).iter().any(|c| parts[*c].media_subtree),
                |f| f.media,
            );
        // A rounded ancestor clips primitives inside a local opacity group,
        // not the composited group a second time. Shadows choose their own strips.
        next.defer_clips = a.opacity.is_some()
            || a.decor.shadows.len != 0
            || indexed.map_or_else(
                || e.children(k).iter().any(|&c| parts[c].defer_clips),
                |f| f.defer_clips,
            )
            || e.nearby(k).any(|(_, c)| parts[c].defer_clips);
        next.coupled = children_coupled
            || ctx.scrollport
            || e.slots[k].stats.escapes != 0
            || e.nearby(k).any(|(_, c)| parts[c].coupled);
        next.port_subtree = ctx.scrollport || indexed.is_some_and(|f| f.ports);
        next.binding_child = None;
        let ordinary = if indexed.is_none() {
            e.children(k)
        } else {
            &[]
        };
        for c in ordinary.iter().copied().chain(e.nearby(k).map(|(_, c)| c)) {
            let child = &mut parts[c];
            next.port_subtree |= child.port_subtree;
            // Once this edge escapes, only another port can gate its rows.
            if child.port_subtree || (child.coupled && !e.slots[c].slot.is_some_and(Slot::escapes))
            {
                child.binding_sibling = next.binding_child;
                next.binding_child = Some(c);
            }
        }
    } else {
        next.binding_child = None;
        next.coupled = false;
        next.port_subtree = false;
    }
    if next.coupled != old.coupled || next.port_subtree != old.port_subtree {
        delta = [true; 2];
    }
    parts.insert(k, next);
    if ctx.content.is_some() {
        l.scroll.refit(e, k, parts, old.facts(), next.facts());
    }
    Outputs::changed(delta[0], delta[1])
}
#[derive(Clone, Copy, Debug, PartialEq)]
enum Call {
    Children,
    Window(Key),
    Owner(Key),
    Segment(Key, Segment),
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Segment {
    first: u32,
    end: u32,
    floating: Option<bool>,
}
#[derive(Clone, Debug, PartialEq)]
enum Command {
    Order {
        call: Call,
        clip: pose::Aperture,
        bounds: skia_safe::Rect,
        defer_clips: bool,
    },
    ScrollThumb(Key, Rect, Rect, usize),
    Fill(Rect, Brush),
    Thumb(Rect, Rect), // full pill and its stationary clip, not a layout child
    Image(image::Sample),
    Group {
        alpha: u8,
        bounds: opacity::Extent,
        end: usize,
    },
    Restore,
    Box {
        material: u32,
        rect: Rect,
        clip: Rect,
        phase: Phase,
    },
    Glyphs {
        pos: [f32; 2],
        clip: Option<Rect>,
        bounds: Rect,
        face: text::Face,
        size: f32,
        color: Brush,
        range: Range<usize>,
    },
}
impl Command {
    fn outer_shadow(&self, materials: &Materials) -> Option<(u32, Rect)> {
        match self {
            Self::Box {
                material,
                rect,
                phase,
                ..
            } if materials.shadow_pass(*material, *phase) => Some((*material, *rect)),
            _ => None,
        }
    }
}
#[derive(Default, PartialEq)]
pub(crate) struct Drawing {
    materials: Materials,
    commands: Buffer<(Scope, Command)>,
    clips: Clips,
    planes: Planes,
    shadow_guards: Vec<pose::ShadowGuard>,
    scope: Scope,
    ids: Buffer<u16>,
    positions: Buffer<Point>,
}
impl Drawing {
    fn push(&mut self, cmd: Command) {
        let scope = if matches!(cmd, Command::Group { .. } | Command::Restore) {
            Scope::default() // end_group sets a shared moving plane when there is one
        } else {
            self.scope
        };
        self.commands.push((scope, cmd));
    }
    fn rules(&mut self, rules: u8, brush: Brush, pos: [f32; 2], width: f32, size: f32, clip: Rect) {
        let h = (size * 0.06).max(1.);
        for (bit, offset) in [(1, 0.08), (2, -0.3)] {
            if rules & bit != 0
                && let Some(r) = (Rect {
                    x: pos[0],
                    y: pos[1] + size * offset - h / 2.,
                    width,
                    height: h,
                })
                .intersection(clip)
            {
                self.push(Command::Fill(r, brush));
            }
        }
    }
    fn clear(&mut self) {
        self.materials.clear();
        self.clips.clear();
        self.planes.0.clear();
        self.shadow_guards.clear();
        self.scope = Scope::default();
        self.commands.clear();
        self.ids.clear();
        self.positions.clear()
    }
}
// Mutable builder and last sealed inputs, not the live display installation.
// Drawing and hits can change independently; only the dispatcher installs.
#[derive(Default)]
pub(super) struct Output {
    pub(super) fits: fits::Fits,
    pub materials: usize,
    pub material_bytes: usize,
    pub rules: usize,
    pub groups: usize,
    pub poses: usize,
    pub clip_nearby: usize,
    pub scrolls: usize,
    bands: Bands,
    ancestry: Groups,
    index: MaterialIndex,
    stack: Vec<(Key, Visit, Scope)>,
    draw: Buffers<Drawing, 8>,
    hits: Buffers<Hits, 4>,
    navigation: Option<navigation::Bank>,
    local: recording::Cache,
    packing_local: bool,
    partition: Option<Key>,
    inline_base: Option<(inline::Span, PaintContext)>,
    marker: bool,
    installed: [Option<(Key, u64, u64)>; 2],
    pending: [Option<(Key, u64, u64)>; 2],
    dirty: [bool; 2],
    #[cfg(test)]
    pub(super) mounted_visits: [usize; 2],
    #[cfg(test)]
    pub(super) packed_children: [usize; 2],
}
#[derive(Clone, Copy)]
enum Visit {
    Node {
        rounded_parent: bool,
        branch: Branch,
    },
    Leaf {
        bleed: u8,
    },
    Inline(inline::Span, Branch),
    Ink(inline::Span, u32),
    Float {
        rounded_parent: bool,
        branch: Branch,
        release: usize,
    },
    EndFloat(usize, Option<bands::Chain>),
    Segment(Key, Segment, u32),
    ReleaseFloat(bands::Chain),
    Chrome,
    Contents {
        wheel: u32,
        local: bool,
    },
    Border,
    Opacity(usize),
}
impl Subtree {
    pub(super) fn extra_commands(self) -> Result<usize, String> {
        // Bound the expanded stream (also on 32-bit targets), before reserving.
        self.group_runs
            .checked_mul(2)
            .filter(|n| *n <= wire::MAX_BYTES / size_of::<(Scope, Command)>())
            .ok_or_else(|| "Nearby opacity groups exceed storage limit".into())
    }
}
pub(super) fn extra_clip_masks(explicit: usize, scrollports: usize) -> usize {
    // Explicit escape guards, ordinary scroll content and stationary chrome.
    explicit + 2 * scrollports
}
impl Output {
    pub fn reserve_bands(
        &mut self,
        escapes: usize,
        groups: usize,
        b: Budget,
    ) -> Result<(), String> {
        self.bands.reserve(escapes, b)?;
        if escapes != 0 {
            self.ancestry.reserve(groups, b)?;
        }
        Ok(())
    }
    pub fn reserve_poses(&mut self, count: usize, b: Budget) -> Result<(), String> {
        self.draw.reserve([0, 0, 0, 0, 0, 0, count, 0], b)?;
        self.hits.reserve([0, 0, count, 0], b)
    }
    pub fn reserve_materials(
        &mut self,
        keys: usize,
        count: usize,
        bytes: usize,
        words: usize,
        guards: usize,
        b: Budget,
    ) -> Result<(), String> {
        if count != 0 || guards != 0 {
            self.hits
                .reserve([0, 2 * count + 2 * words + guards, 0, 0], b)?;
            self.index.reserve(keys)?;
            self.draw
                .reserve([0, 0, 0, 2 * count + words + guards, count, bytes, 0, 0], b)?;
        }
        Ok(())
    }
    pub(in crate::tree) fn reserve_recordings(
        &mut self,
        sites: Sites,
        b: Budget,
    ) -> Result<(), String> {
        let d = self.draw.build.capacities();
        let readers = self.draw.readers();
        let h = self.hits.build.capacities();
        recording_charge(sites, d, h, readers, b)?;
        self.local.reserve(sites, d, h, readers, b)?;
        self.fits.reserve_versions(sites.images, readers + 2, b)?;
        self.index.versions.reserve(d[4], d[5], readers + 2, b)
    }
    pub fn reserve_for(
        &mut self,
        owners: usize,
        glyphs: usize,
        words: usize,
        extra_commands: usize,
        groups: usize,
        b: Budget,
    ) -> Result<(), String> {
        let stack = owners + words.min(owners) + groups;
        if stack > self.stack.capacity() {
            pool::reserve(&mut self.stack, b.capacity(stack))?
        }
        self.draw.reserve(
            [
                2 * owners + 5 * words + extra_commands,
                glyphs,
                glyphs,
                0,
                0,
                0,
                0,
                0,
            ],
            b,
        )?;
        self.hits.reserve([owners + 2 * words, 0, 0, 0], b)
    }
    // Only an *unsealed* stream can have outstanding bitmap jobs. Accepted
    // edits may supersede those jobs but must never alter a sealed drawing.
    pub(super) fn cancel_fits(&mut self, e: &mut Elements) {
        for k in self.fits.abandon() {
            if e.slots.get(k).is_some_and(|m| m.detached.is_none()) {
                e.slots[k].change.image_draft = true;
                e.touch(k);
            }
        }
        self.reset_fits();
    }
    fn reset_fits(&mut self) {
        if self.fits.unbound() {
            self.fits.reset();
            self.pending[0] = self.installed[0];
            self.draw.build.clear();
            self.dirty[0] = false;
        }
    }
    // A newly visible material may be keyed far beyond the previous window.
    // Make its index slot addressable on the declaration edit, not in packing.
    pub(super) fn prepare_material(&mut self, e: &Elements, k: Key) {
        self.fits.check_reader(k, e);
        self.index.prepare(e, k);
    }
    pub(in crate::tree) fn forget_material(&mut self, k: Key) {
        self.fits.forget_reader(k);
        self.index.forget(k);
    }
    #[cfg(test)]
    pub fn recorded(&self) -> [usize; 2] {
        self.local.recorded
    }
    #[cfg(test)]
    pub fn assembly(&self) -> (usize, [usize; 3]) {
        (self.local.ordinals, pages::meter::BYTES.get())
    }
    #[cfg(test)]
    pub fn reset_work(&mut self) {
        self.fits.additions = 0;
        pages::meter::BYTES.set([0; 3]);
        self.local.ordinals = 0;
        self.mounted_visits = [0; 2];
        self.packed_children = [0; 2];
        self.local.recorded = [0; 2];
    }
    pub(in crate::tree) fn prepare_window(
        &mut self,
        e: &Elements,
        l: &mut Layers,
        k: Key,
        host: Key,
        selection: (u64, bool),
        w: &mut Window,
    ) -> bool {
        let mut local = std::mem::take(&mut self.local);
        let structural = local.prepare(self, e, l, k, host, selection, w);
        self.local = local;
        structural
    }
    pub(in crate::tree) fn prepare_owner(&mut self, e: &Elements, l: &mut Layers, k: Key) {
        let mut local = std::mem::take(&mut self.local);
        local.standalone(self, e, l, k);
        self.local = local;
    }
    pub(in crate::tree) fn retire_local(&self, w: &mut Window, i: usize, k: Key, s: Outputs) {
        self.local.retire(w, i, k, s);
    }
    fn collect(&mut self, e: &Elements, root: Option<Key>, layers: &Layers) {
        self.fits.tracking = self.scrolls != 0 && !layers.scroll.exhaustive();
        let mut shell = false;
        for side in 0..2 {
            let stamp = root.map(|k| (k, layers.parts[k].ports[side].revision, layers.edge_epoch));
            if self.pending[side] == stamp {
                continue;
            }
            self.pending[side] = stamp;
            if side == 0 {
                self.fits.reset();
            }
            self.dirty[side] = stamp != self.installed[side];
            if !self.dirty[side] {
                continue;
            }
            self.local.maps.begin(side);
            if side == 0 {
                self.index.begin(false);
                self.draw.build.clear()
            } else {
                self.hits.build.clear()
            }
            self.pack(e, root, layers, side);
            self.local.maps.end(side);
            if side == 0 {
                shell = true;
            }
        }
        if shell || self.fits.new_drafts() {
            self.fits.finish_targets();
        }
        self.fits.bind(&mut self.draw.build);
    }
    // The ordinary body is one retained call. Its exported roots still enter
    // the common FIFO alongside other owners. Follow only exporting branches,
    // in submission (not float paint) order, carrying their establishing scopes.
    fn exports(&mut self, e: &Elements, l: &Layers, k: Key, side: usize, mut branch: Branch) {
        if !l.parts[k].ports[side].exports {
            return;
        }
        let ctx = l.painted(k);
        let a = &e.attrs[k];
        if side == 0 && a.opacity == Some(0) {
            return;
        }
        let (planes, clips) = if side == 0 {
            (&mut self.draw.build.planes, &mut self.draw.build.clips)
        } else {
            (&mut self.hits.build.planes, &mut self.hits.build.clips)
        };
        let Some(p) = planes.branch(
            branch.escape.plane,
            branch.escape.mask,
            a,
            PaintContext {
                boundary: ctx.escape_boundary,
                ..ctx
            },
            side == 1,
        ) else {
            return;
        };
        branch.escape.plane = p;
        if a.clip_nearby {
            let shape = ctx.inner_guard(a, a.decor.rounded_inner(ctx.frame, ctx.scale));
            branch.escape.mask = clips.add(branch.escape.mask, Some(shape));
        }
        if side == 0
            && let Some(alpha) = a.opacity
        {
            branch.group = self.ancestry.child(branch.group, alpha);
        }
        self.submit_escapes(e, k, l, side, &mut branch);
        for (slot, c) in e.nearby(k) {
            if slot == Slot::Behind {
                self.exports(e, l, c, side, branch);
            }
        }
        if !e
            .layout_parent(k)
            .is_some_and(|p| e.slots[p].shape == Shape::Paragraph && e.float_side(k).is_none())
        {
            for &c in e.children(k) {
                self.exports(e, l, c, side, branch);
            }
        }
    }
    fn reference(
        &mut self,
        e: &Elements,
        l: &Layers,
        k: Key,
        side: usize,
        scope: Scope,
        branch: Branch,
    ) -> bool {
        if self.packing_local || !l.band_owner(e, k) {
            return false;
        }
        let r = &l.projected[k].saved[side];
        if !r.valid(e, l, k, side) {
            return false;
        }
        let ctx = l.painted(k);
        let wheel = branch.wheel;
        self.exports(e, l, k, side, branch);
        if r.counts() == [0; 3] {
            return true;
        }
        self.local.maps.include_owner(r, side);
        let clip = ctx.boundary;
        if side == 0 {
            self.draw.build.scope = scope;
            self.draw.build.push(Command::Order {
                call: Call::Owner(k),
                bounds: r.placed_bounds(),
                clip: pose::Aperture {
                    body: clip,
                    media: if e.slots[k].shape == Shape::Image {
                        ctx.child_clip
                    } else {
                        clip
                    },
                },
                defer_clips: l.parts[k].defer_clips,
            });
        } else {
            self.hits.build.items.push(Hit {
                kind: HitKind::Owner,
                actions: Actions::default(),
                wheel,
                scope,
                key: r.key.unwrap(),
                id: e.slots[k].id,
                rect: clip,
                origin: [0.; 2],
                scale: ctx.scale,
            });
        }
        true
    }
    fn pack(&mut self, e: &Elements, root: Option<Key>, layers: &Layers, side: usize) {
        self.stack.extend(root.map(|k| {
            (
                k,
                self.inline_base.map_or(
                    Visit::Node {
                        rounded_parent: false,
                        branch: Branch::default(),
                    },
                    |(span, _)| Visit::Inline(span, Branch::default()),
                ),
                Scope::default(),
            )
        }));
        loop {
            let (k, visit, scope) = if let Some(local) = self.stack.pop() {
                local
            } else if let Some((k, branch, new_band)) = self.bands.next() {
                if side == 0 {
                    if new_band {
                        self.ancestry.switch(&mut self.draw.build, 0);
                    }
                    self.ancestry.switch(&mut self.draw.build, branch.group);
                }
                let Coords { mask, plane } = branch.escape;
                (
                    k,
                    Visit::Node {
                        rounded_parent: false,
                        branch,
                    },
                    Scope {
                        mask,
                        plane,
                        guard: branch.escape,
                        ..Scope::default()
                    },
                )
            } else {
                break;
            };
            let Scope {
                mask,
                plane: parent,
                ..
            } = scope;
            let (rounded_parent, mut branch) = match visit {
                Visit::Leaf { bleed } => {
                    self.leaf(e, k, layers, scope, bleed);
                    continue;
                }
                Visit::Chrome => {
                    self.chrome(e, k, layers, side, scope);
                    continue;
                }
                Visit::Inline(span, branch) => {
                    if !self.reference(e, layers, k, side, scope, branch) {
                        self.inline_owner(e, k, layers, side, scope, (span, branch));
                    }
                    continue;
                }
                Visit::Ink(span, wheel) => {
                    self.inline_ink(
                        e,
                        k,
                        layers,
                        side,
                        scope,
                        (span, wheel, inline::Part::Content),
                    );
                    continue;
                }
                Visit::Float {
                    rounded_parent,
                    branch,
                    release,
                } => {
                    let enclosing = self.bands.begin_float();
                    self.stack
                        .push((k, Visit::EndFloat(release, enclosing), scope));
                    (rounded_parent, branch)
                }
                Visit::EndFloat(release, enclosing) => {
                    self.stack[release].1 = Visit::ReleaseFloat(self.bands.end_float(enclosing));
                    continue;
                }
                Visit::ReleaseFloat(chain) => {
                    self.bands.release_float(chain);
                    continue;
                }
                Visit::Segment(port, segment, wheel) => {
                    self.local.maps.mark(side);
                    let clip = if port == k {
                        Rect::UNBOUNDED
                    } else {
                        layers.painted(k).child_clip
                    };
                    if side == 0 {
                        self.draw.build.scope = scope;
                        self.draw.build.push(Command::Order {
                            call: Call::Segment(port, segment),
                            bounds: sk(clip).with_outset((1., 1.)),
                            clip: if k == port {
                                pose::Aperture::fixed(clip)
                            } else {
                                pose::Aperture::children(layers.painted(k))
                            },
                            defer_clips: layers.parts[k].defer_clips,
                        });
                    } else {
                        self.hits.build.items.push(Hit {
                            kind: HitKind::Segment(segment),
                            wheel,
                            key: Attachment(port, e.slots[port].attachment),
                            scope,
                            actions: Actions::default(),
                            id: e.slots[port].id,
                            rect: clip,
                            origin: [0.; 2],
                            scale: layers.painted(k).scale,
                        });
                    }
                    continue;
                }
                Visit::Contents { wheel, local } => {
                    if !local {
                        self.local.maps.mark(side);
                    }
                    let ctx = layers.painted(k);
                    if side == 0 {
                        let draw = &mut self.draw.build;
                        draw.scope = scope;
                        draw.push(Command::Order {
                            call: if local {
                                Call::Children
                            } else {
                                Call::Window(k)
                            },
                            clip: pose::Aperture::children(ctx),
                            bounds: sk(pose::Aperture::children(ctx).media).with_outset((1., 1.)),
                            defer_clips: layers.parts[k].defer_clips,
                        });
                    } else {
                        self.hits.build.items.push(Hit {
                            kind: if local {
                                HitKind::Children
                            } else {
                                HitKind::Content
                            },
                            actions: Actions::default(),
                            wheel,
                            scope,
                            key: Attachment(k, e.slots[k].attachment),
                            id: e.slots[k].id,
                            rect: ctx.child_clip,
                            origin: [0.; 2],
                            scale: ctx.scale,
                        });
                    }
                    continue;
                }
                Visit::Border => {
                    let ctx = layers.painted(k);
                    let draw = &mut self.draw.build;
                    draw.scope = scope;
                    let material = self.index.get(draw, e, k);
                    draw.push(Command::Box {
                        material,
                        rect: ctx.frame,
                        clip: ctx.box_clip(e.slots[k].shape),
                        phase: Phase::Border,
                    });
                    continue;
                }
                Visit::Opacity(start) => {
                    self.draw.build.end_group(start);
                    continue;
                }
                Visit::Node {
                    rounded_parent,
                    branch,
                } => (rounded_parent, branch),
            };
            if self.reference(e, layers, k, side, scope, branch) {
                continue;
            }
            let p = layers.parts[k].at(layers.projection(k));
            if !p.ports[side].nonempty {
                continue;
            }
            #[cfg(test)]
            if e.slots[k].slot.is_some() {
                self.mounted_visits[side] += 1;
            }
            let ctx = layers.painted(k);
            let a = &e.attrs[k];
            let decor = a.decor;
            let (planes, clips) = if side == 0 {
                (&mut self.draw.build.planes, &self.draw.build.clips)
            } else {
                (&mut self.hits.build.planes, &self.hits.build.clips)
            };
            let stop = if e.slots[k].shape == Shape::Image
                && a.posed()
                && rounded_parent
                && !a.image_host_clip
            {
                clips.parent(mask)
            } else {
                mask
            };
            let Some(id) = planes.branch(parent, stop, a, ctx, side == 1) else {
                continue;
            };
            let scope = Scope { plane: id, ..scope };
            let plane = planes.get(id);
            let exports = p.ports[side].exports;
            if exports
                && branch
                    .pose(
                        planes,
                        Coords {
                            plane: parent,
                            mask: stop,
                        },
                        id,
                        a,
                        ctx,
                        side,
                    )
                    .is_none()
            {
                continue;
            }
            if side == 0 {
                let draw = &mut self.draw.build;
                draw.scope = scope;
                if let Some(alpha) = p.opacity {
                    if alpha == 0 {
                        continue;
                    }
                    if exports {
                        branch.group = self.ancestry.child(branch.group, alpha);
                    }
                    let start = draw.begin_group(alpha);
                    self.stack.push((k, Visit::Opacity(start), scope));
                }
                let material = p.paint.decorated.then(|| self.index.get(draw, e, k));
                let backdrop = p.art && e.slots[k].shape != Shape::Image;
                if backdrop {
                    if let Some(material) = material.filter(|_| decor.shadows.len != 0) {
                        draw.push(Command::Box {
                            material,
                            rect: ctx.frame,
                            clip: ctx.clip,
                            phase: Phase::OuterShadow,
                        });
                    }
                    if let Some(sample) = image::Sample::new(
                        e,
                        k,
                        ctx,
                        (plane, layers.paint[k].transformed),
                        0,
                        None,
                        &mut self.fits,
                    ) {
                        draw.scope.mask = draw
                            .clips
                            .add(mask, Rounded::new(ctx.frame, decor.radii(ctx.scale)));
                        draw.push(Command::Image(sample));
                        draw.scope.mask = mask;
                    }
                }
                if let Some(material) = material {
                    if e.attrs[k].background.is_some_and(|v| v & 255 != 0)
                        || decor.background_gradient.len != 0
                        || decor.shadows.len != 0
                    {
                        draw.push(Command::Box {
                            material,
                            rect: ctx.frame,
                            clip: ctx.box_clip(e.slots[k].shape),
                            phase: if backdrop { Phase::Inset } else { Phase::Under },
                        });
                    }
                    if decor.border != [0.; 4]
                        && (decor.border_color.is_some() || decor.border_gradient.len != 0)
                    {
                        self.stack.push((k, Visit::Border, scope));
                    }
                } else if let Some((r, c)) = p.paint.fill {
                    draw.push(Command::Fill(r, Brush::Solid(c)))
                }
            } else if let Some(mut hit) = p.hit {
                let hits = &mut self.hits.build;
                hit.scope = Scope {
                    mask: hits
                        .clips
                        .add(mask, Rounded::new(ctx.frame, decor.radii(ctx.scale))),
                    ..scope
                };
                hit.wheel = branch.wheel;
                hits.items.push(hit)
            }
            let mut wheel = branch.wheel;
            if ctx.bars
                && let Some(rect) = ctx.frame.intersection(ctx.clip)
            {
                let mut chrome = scope;
                let radius = Rounded::new(ctx.frame, decor.radii(ctx.scale));
                if side == 1 {
                    let hits = &mut self.hits.build;
                    chrome.mask = hits.clips.add(mask, radius);
                    wheel = hits.port(e, k, ctx, (chrome, rect), layers.scroll.view(k), wheel);
                } else {
                    chrome.mask = self.draw.build.clips.add(mask, radius);
                }
                self.stack.push((k, Visit::Chrome, chrome));
            }
            let inner = decor.rounded_inner(ctx.frame, ctx.scale);
            let clips = if side == 0 {
                &mut self.draw.build.clips
            } else {
                &mut self.hits.build.clips
            };
            let mask = clips.add(mask, inner);
            let inner_shape = || ctx.inner_guard(a, inner);
            if exports && a.clip_nearby {
                // This includes the host's own escaping slots.
                branch.escape.mask = clips.add(branch.escape.mask, Some(inner_shape()));
            }
            let mut content = Scope { mask, ..scope };
            if ctx.scrollport {
                if inner.is_none() {
                    content.mask = clips.add(mask, Some(inner_shape()));
                }
                if side == 0 {
                    let guards = &mut self.draw.build.shadow_guards;
                    guards.push(pose::ShadowGuard {
                        parent: scope.shadow,
                        plane: id,
                        inner: inner_shape(),
                        axes: a.scroll.enabled,
                    });
                    content.shadow = guards.len() as u32;
                }
            }
            self.submit_escapes(e, k, layers, side, &mut branch);
            if side == 0 && (p.paint.text.is_some() || e.slots[k].shape == Shape::Image) {
                let bleed = u8::from(rounded_parent) + u8::from(inner.is_some());
                let scope = if e.slots[k].shape == Shape::Image {
                    Scope { mask, ..scope }
                } else {
                    scope
                };
                if e.order(k).tags.contains(&Slot::Behind) {
                    self.stack.push((k, Visit::Leaf { bleed }, scope));
                } else {
                    self.leaf(e, k, layers, scope, bleed);
                }
            }
            let scope = Scope { mask, ..scope };
            let paragraph = e.slots[k].shape == Shape::Paragraph;
            let mut child_branch = Branch { wheel, ..branch };
            let local = self.partition == Some(k);
            let window = (!self.packing_local)
                .then(|| layers.scroll.recorded(k))
                .flatten();
            if let Some(w) = window.filter(|w| w.banded) {
                self.local
                    .maps
                    .include(w, side, scroll::delegated(e, k).is_none());
                let planes = if side == 0 {
                    &mut self.draw.build.planes
                } else {
                    &mut self.hits.build.planes
                };
                content.plane = planes.content(
                    content.plane,
                    content.mask,
                    pose::Aperture::children(ctx),
                    k,
                );
                if exports {
                    child_branch.escape.plane = planes.content(
                        child_branch.escape.plane,
                        child_branch.escape.mask,
                        pose::Aperture::fixed(ctx.escape_clip),
                        k,
                    );
                }
            } else if local || window.is_some() {
                self.marker |= local;
                if let Some(w) = window {
                    self.local.maps.include(w, side, true);
                }
                self.stack
                    .push((k, Visit::Contents { wheel, local }, content));
                self.behinds(e, k, layers, side, scope, branch);
                continue;
            }
            let rounded_parent = inner.is_some() && !ctx.scrollport;
            if !self.packing_local
                && let Some(port) = scroll::order_port(e, k)
                && let Some(w) = layers.scroll.recorded(port).filter(|w| w.banded)
            {
                if k != port {
                    self.local.maps.enable_rows(w, side);
                }
                self.band_children(
                    e,
                    layers,
                    (k, port),
                    side,
                    (content, child_branch),
                    rounded_parent,
                );
                self.behinds(e, k, layers, side, scope, branch);
                continue;
            }
            let mut ordinal = self.stack.len();
            let park_floats =
                paragraph && self.inline_visits(e, k, layers, side, content, child_branch);
            let mut coverage = if side == 0
                && !paragraph
                && e.children(k).len() > 1
                && !ctx.transformed
                && a.opacity.is_none()
                && e.slots[k].stats.mounts == 0
                && !window.is_some_and(|w| w.banded)
                && !layers.scroll.exhaustive()
            {
                coverage::Coverage::within(content, &self.draw.build.clips)
            } else {
                None
            };
            for &c in layers.scroll.children_rev(e, k) {
                #[cfg(test)]
                {
                    self.packed_children[side] += 1;
                }
                if !layers.parts[c].ports[side].nonempty
                    || (paragraph && e.float_side(c).is_none())
                    || coverage
                        .as_mut()
                        .is_some_and(|cover| cover.hides(e, layers, c))
                {
                    continue;
                }
                let visit = if park_floats && layers.parts[c].ports[side].exports {
                    // Both traversals use reverse declaration order. Advance one cursor
                    // through the inline visits; park the chain directly on its release.
                    while self.stack[ordinal].0 != c {
                        ordinal += 1;
                    }
                    let release = ordinal;
                    ordinal += 1;
                    Visit::Float {
                        rounded_parent,
                        branch: child_branch,
                        release,
                    }
                } else {
                    Visit::Node {
                        rounded_parent,
                        branch: child_branch,
                    }
                };
                self.stack.push((c, visit, content));
            }
            self.behinds(e, k, layers, side, scope, branch);
        }
        self.bands.finish();
        if side == 0 {
            self.ancestry.switch(&mut self.draw.build, 0);
            self.ancestry.finish();
            self.draw.build.scope = Scope::default();
        }
    }
    fn band_children(
        &mut self,
        e: &Elements,
        l: &Layers,
        (k, port): (Key, Key),
        side: usize,
        (scope, branch): (Scope, Branch),
        rounded_parent: bool,
    ) {
        let paragraph = e.slots[k].shape == Shape::Paragraph;
        let park = paragraph && l.parts[k].ports[side].exports;
        let mut ordinal = self.stack.len();
        // Two passes only for paragraphs: float paint precedes inline ink, but
        // float exports are released at their ordinary ordinal in the band's FIFO.
        let mut pass = |floating: Option<bool>| {
            l.scroll.bands_rev(e, k, port, &l.parts, |s| {
                let (owner, visit) = match s {
                    scroll::BandPart::Rows(r) => (
                        k,
                        Visit::Segment(
                            port,
                            Segment {
                                first: r.start as u32,
                                end: r.end as u32,
                                floating,
                            },
                            branch.wheel,
                        ),
                    ),
                    scroll::BandPart::Branch(c) => {
                        let float = e.float_side(c).is_some();
                        if floating == Some(false) {
                            if !float {
                                (c, Visit::Inline(inline::Span::at(l, k, c), branch))
                            } else if park && l.parts[c].ports[side].exports {
                                (c, Visit::ReleaseFloat(bands::Chain::default()))
                            } else {
                                return;
                            }
                        } else if (floating == Some(true) && !float)
                            || !l.parts[c].ports[side].nonempty
                        {
                            return;
                        } else if park && float && l.parts[c].ports[side].exports {
                            while self.stack[ordinal].0 != c {
                                ordinal += 1;
                            }
                            let release = ordinal;
                            ordinal += 1;
                            (
                                c,
                                Visit::Float {
                                    rounded_parent,
                                    branch,
                                    release,
                                },
                            )
                        } else {
                            (
                                c,
                                Visit::Node {
                                    rounded_parent,
                                    branch,
                                },
                            )
                        }
                    }
                };
                self.stack.push((owner, visit, scope));
            })
        };
        if paragraph {
            pass(Some(false));
            pass(Some(true));
        } else {
            pass(None);
        }
    }
    fn submit_escapes(
        &mut self,
        e: &Elements,
        k: Key,
        layers: &Layers,
        side: usize,
        branch: &mut Branch,
    ) {
        if !self.packing_local && layers.parts[k].ports[side].boundary {
            if layers.shows_anchor(e, k) {
                for (slot, child) in e.nearby(k) {
                    if slot.escapes() && layers.parts[child].ports[side].nonempty {
                        self.bands.push(child, *branch);
                    }
                }
            }
            branch.depth += 1;
        }
    }
    fn behinds(
        &mut self,
        e: &Elements,
        k: Key,
        layers: &Layers,
        side: usize,
        scope: Scope,
        branch: Branch,
    ) {
        self.stack.extend(
            e.nearby(k)
                .rev()
                .filter(|(s, c)| *s == Slot::Behind && layers.parts[*c].ports[side].nonempty)
                .map(|(_, c)| {
                    (
                        c,
                        Visit::Node {
                            rounded_parent: false,
                            branch,
                        },
                        scope,
                    )
                }),
        );
    }
    fn leaf(&mut self, e: &Elements, k: Key, layers: &Layers, scope: Scope, bleed: u8) {
        let ctx = layers.painted(k);
        let draw = &mut self.draw.build;
        draw.scope = scope;
        if let Some(glyphs) = layers.parts[k].at(layers.projection(k)).paint.text {
            let run = layers.runs[k];
            let start = draw.ids.len();
            for g in layers.text.glyphs.get(&run.span) {
                draw.ids.push(g.id);
                draw.positions.push(g.pos);
            }
            let brush = self.index.brush(draw, e, ctx, ctx.frame);
            draw.push(Command::Glyphs {
                pos: glyphs.pos,
                color: brush,
                clip: glyphs.clip,
                bounds: glyphs.clip.unwrap_or(Rect {
                    x: glyphs.pos[0] + run.metrics.ink.x,
                    y: glyphs.pos[1] + run.metrics.ink.y,
                    ..run.metrics.ink
                }),
                face: run.metrics.face,
                size: run.metrics.size,
                range: start..draw.ids.len(),
            });
            draw.rules(
                ctx.rules,
                brush,
                glyphs.pos,
                run.metrics.width,
                run.metrics.size,
                ctx.frame
                    .intersection(ctx.clip)
                    .unwrap_or_default()
                    .paint_clip(ctx.transformed),
            );
        } else if let Some(sample) = image::Sample::new(
            e,
            k,
            ctx,
            (draw.planes.get(scope.plane), layers.paint[k].transformed),
            bleed,
            (e.attrs[k].svg_gradient.len != 0).then(|| self.index.get(draw, e, k)),
            &mut self.fits,
        ) {
            draw.push(Command::Image(sample));
        }
    }
    fn drawing(&self) -> &Drawing {
        if self.dirty[0] {
            &self.draw.build
        } else {
            self.draw.current()
        }
    }
    fn install(&mut self) {
        if self.dirty[0] {
            self.draw.publish();
        }
        if self.dirty[1] {
            self.hits.publish();
        }
        self.installed = self.pending;
        self.local.maps.install();
        self.dirty = [false; 2];
    }
}
#[derive(Default)]
pub(crate) struct Raster {
    pub(crate) tree: Tree,
    viewport: Rect,
    pixels: Vec<u8>,
    #[cfg(test)]
    pub(super) draws: usize,
}
impl Raster {
    #[cfg(test)]
    pub(crate) fn with_tree(tree: Tree) -> Self {
        Self {
            tree,
            ..Default::default()
        }
    }
    #[cfg(test)]
    pub(crate) fn has_target(&self, key: Attachment) -> bool {
        self.tree.output.hit_frame().has_target(key)
    }
    #[cfg(test)]
    pub(crate) fn target_at(&self, x: f32, y: f32) -> Option<(Attachment, u64, f64, f64)> {
        self.tree.output.hit_frame().target_at(self.viewport, x, y)
    }
}
#[derive(Default)]
pub(crate) struct Hits {
    planes: Planes,
    clips: Clips,
    items: Buffer<Hit>,
    scrolls: HashMap<Attachment, ScrollHit>,
}
#[derive(Clone, Default)]
pub(crate) struct HitFrame {
    base: Arc<Hits>,
    maps: Option<Arc<Tables>>,
}
impl std::ops::Deref for HitFrame {
    type Target = Arc<Hits>;
    fn deref(&self) -> &Self::Target {
        &self.base
    }
}
impl Output {
    fn hit_frame(&self) -> HitFrame {
        HitFrame {
            base: self.hits.current().clone(),
            maps: self.local.maps.installed().cloned(),
        }
    }
}
impl HitFrame {
    fn tables(&self) -> &Tables {
        self.maps.as_deref().unwrap_or(&Tables::EMPTY)
    }
    #[cfg(test)]
    pub(super) fn ids(&self) -> impl Iterator<Item = u64> + '_ {
        self.items.iter().map(|h| h.id)
    }
    #[cfg(test)]
    pub(crate) fn has_target(&self, key: Attachment) -> bool {
        self.captured(key, 0., 0.)
            .is_some_and(|h| h.actions.has(Actions::CLICK))
    }
    #[cfg(test)]
    pub(crate) fn target_at(&self, v: Rect, x: f32, y: f32) -> Option<(Attachment, u64, f64, f64)> {
        self.listener_at(v, x, y)
            .filter(|h| h.actions.has(Actions::CLICK))
            .map(|h| (h.key, h.id, h.local[0], h.local[1]))
    }
    // Captured endpoints use the installed source's current plane and origin,
    // not a second point hit (the pointer may now be outside every clip).
    pub(crate) fn captured(&self, key: Attachment, x: f32, y: f32) -> Option<Listener> {
        self.items.iter().rev().find_map(|h| {
            if !matches!(
                h.kind,
                HitKind::Content | HitKind::Owner | HitKind::Segment(_)
            ) && (h.key != key || !h.actions.any())
            {
                return None;
            }
            let p = if let Some(plane) = self.planes.installed(h.scope.plane, self.tables(), 1) {
                if plane.bounds(sk(h.rect)).is_empty() {
                    return None;
                }
                plane.matrix.invert()?.map_point((x, y))
            } else {
                (x, y).into()
            };
            if !p.x.is_finite() || !p.y.is_finite() {
                return None;
            }
            let (px, py, shift) = self.point_basis(h, (x, y), (p.x, p.y));
            if matches!(
                h.kind,
                HitKind::Content | HitKind::Owner | HitKind::Segment(_)
            ) {
                let (h, x, y) = self.program_hit(h, px, py, Some(key), shift)?;
                Some(h.listener(x, y))
            } else {
                Some(h.shifted_listener(px, py, shift))
            }
        })
    }
    fn program_hit(
        &self,
        h: &Hit,
        x: f32,
        y: f32,
        capture: Option<Attachment>,
        shift: [f32; 2],
    ) -> Option<(Hit, f32, f32)> {
        if h.kind == HitKind::Content {
            self.tables().get(h.key.0, 1)?.hit(x, y, capture, shift)
        } else {
            let rect = if let Some(p) = self.planes.installed(h.scope.plane, self.tables(), 1) {
                p.visible(h.rect)?
            } else {
                h.rect
            };
            let rect = Rect {
                x: rect.x + shift[0],
                y: rect.y + shift[1],
                ..rect
            };
            if let HitKind::Segment(s) = h.kind {
                self.tables()
                    .get(h.key.0, 1)?
                    .segment(1, s)
                    .hit(x, y, rect, capture, shift)
            } else {
                self.tables()
                    .owner(h.key.0, 1)?
                    .hit(x, y, rect, capture, shift)
            }
        }
    }
    fn point_basis(&self, h: &Hit, screen: (f32, f32), local: (f32, f32)) -> (f32, f32, [f32; 2]) {
        self.planes
            .translated_point(h.scope.plane, screen, self.tables())
            .unwrap_or((local.0, local.1, [0.; 2]))
    }
    pub(crate) fn listener_at(&self, v: Rect, px: f32, py: f32) -> Option<Listener> {
        self.at(v, px, py, self.tables())
            .find_map(|(h, x, y)| match h.kind {
                HitKind::Port => None,
                HitKind::Content | HitKind::Owner | HitKind::Segment(_) => {
                    let (x, y, shift) = self.point_basis(h, (px, py), (x, y));
                    self.program_hit(h, x, y, None, shift)
                        .map(|(h, x, y)| h.actions.any().then(|| h.listener(x, y)))
                }
                HitKind::Bars => self.scroll_port(h.key)?.track(x, y).map(|_| None),
                _ => Some(h.actions.any().then(|| {
                    let (x, y, shift) = self.point_basis(h, (px, py), (x, y));
                    h.shifted_listener(x, y, shift)
                })),
            })
            .flatten()
    }
}

#[derive(Clone, Copy)]
pub(crate) struct Listener {
    pub key: Attachment,
    pub id: u64,
    pub actions: Actions,
    pub local: [f64; 2],
}
impl Hit {
    fn shifted_listener(mut self, x: f32, y: f32, shift: [f32; 2]) -> Listener {
        self.origin[0] += shift[0];
        self.origin[1] += shift[1];
        self.listener(x, y)
    }
    fn listener(&self, x: f32, y: f32) -> Listener {
        Listener {
            key: self.key,
            id: self.id,
            actions: self.actions,
            local: std::array::from_fn(|i| {
                (f64::from([x, y][i]) - f64::from(self.origin[i])) / f64::from(self.scale)
            }),
        }
    }
}

/// No occurrence table, pool span, writable pixel target or mutable device state escapes.
#[derive(Clone, Default)]
pub(crate) struct Scene {
    _workspace: Option<Arc<storage::Grant>>,
    pub viewport: Rect,
    pub drawing: Arc<Drawing>,
    pub hits: HitFrame,
    pub navigation: Option<Navigation>,
}
impl Scene {
    #[cfg(test)]
    pub(super) fn shares_order(&self, b: &Self, k: Key, side: usize) -> bool {
        self.hits
            .tables()
            .get(k, side)
            .zip(b.hits.tables().get(k, side))
            .is_some_and(|(a, b)| a.shares_order(b, side))
    }
    pub(crate) fn packed_counts(&self) -> [usize; 3] {
        [
            self.drawing.commands.len() + self.hits.tables().counts[0]
                - self.hits.tables().markers[0],
            self.drawing.ids.len() + self.hits.tables().counts[1],
            self.hits.items.len() + self.hits.tables().counts[2] - self.hits.tables().markers[1],
        ]
    }
    pub fn rasterize(&self) -> Result<Vec<u8>, String> {
        rasterize(self.viewport, &self.drawing, self.hits.tables())
    }
    pub fn same_drawing(&self, other: &Self) -> bool {
        self.viewport == other.viewport
            && self.hits.tables().same_picture(other.hits.tables())
            && (Arc::ptr_eq(&self.drawing, &other.drawing) || self.drawing == other.drawing)
    }
}
impl Tree {
    pub(crate) fn for_session(percent: u16) -> Self {
        let mut tree = Self::with_headroom(percent);
        tree.output.draw = Buffers::new(5);
        tree.output.hits = Buffers::new(5);
        tree
    }
    pub(crate) fn reclaim_frames(&mut self) {
        self.output.draw.reclaim();
        self.output.hits.reclaim();
        self.output.local.maps.reclaim();
    }
    pub(super) fn collect_output(&mut self) {
        let (e, l, o) = (&self.elements, &mut self.layers, &mut self.output);
        o.collect(e, self.root, l);
        // Growing a shared fitted bitmap changes the ink of thumbnails already
        // compiled from it. Reopen those recorded readers before issuing the
        // one worker target, including uses in an otherwise unchanged shell.
        // Newly emitted uses already participate. No layout inputs or hit
        // programs change; this is bounded resource preparation, not a solve.
        if o.fits
            .reopen_readers(|k| l.invalidate_composition(k, e.depth(k), Outputs::Draw))
        {
            for k in o.fits.abandon() {
                l.invalidate_composition(k, e.depth(k), Outputs::Draw);
            }
            o.reset_fits();
            l.drain(e, Pass::Compose, o);
            l.drain(e, Pass::Project, o);
            o.collect(e, self.root, l);
        }
    }
    /// Layout may advance with every reader pinned. Don't pack a side that
    /// cannot be sealed, and don't require a spare for an unchanged side.
    pub(crate) fn collect_if_available(&mut self) -> bool {
        let o = &mut self.output;
        let stamp = |side: usize| {
            self.root.map(|k| {
                (
                    k,
                    self.layers.parts[k].ports[side].revision,
                    self.layers.edge_epoch,
                )
            })
        };
        let focus_root = self
            .root
            .filter(|&k| self.elements.slots[k].stats.tab_stops != 0);
        if !o.local.maps.available()
            || (stamp(0) != o.installed[0] && !o.draw.available())
            || (stamp(1) != o.installed[1] && !o.hits.available())
            || (focus_root.is_some()
                && o.navigation
                    .as_ref()
                    .is_some_and(|b| !b.available(&self.layers)))
        {
            o.reset_fits();
            return false;
        }
        self.collect_output();
        let o = &mut self.output;
        if !o.fits.waiting()
            && let Some((k, bank)) = focus_root.zip(o.navigation.as_mut())
        {
            bank.prepare(&self.elements, &mut self.layers, k);
        }
        true
    }
    pub(crate) fn seal(&mut self) -> Option<Scene> {
        let o = &mut self.output;
        if o.fits.waiting()
            || !o.local.maps.available()
            || (o.dirty[0] && !o.draw.available())
            || (o.dirty[1] && !o.hits.available())
        {
            return None;
        }
        o.fits.bind(&mut o.draw.build);
        let navigation = if let Some(root) = self
            .root
            .filter(|&k| self.elements.slots[k].stats.tab_stops != 0)
            && let Some(bank) = &mut o.navigation
        {
            // Detached refresh can also be sealed directly, without a live
            // collect_if_available turn. Never change a pinned reader.
            if !bank.available(&self.layers) {
                return None;
            }
            bank.prepare(&self.elements, &mut self.layers, root);
            Some(bank.snapshot())
        } else {
            None
        };
        o.install();
        Some(Scene {
            navigation,
            _workspace: self.workspace.clone(),
            viewport: self.layers.viewport,
            drawing: o.draw.current().clone(),
            hits: o.hit_frame(),
        })
    }
}
impl Raster {
    #[cfg(test)]
    pub(super) fn hit_at(&self, x: f32, y: f32) -> Option<(u64, f64, f64)> {
        self.target_at(x, y).map(|(_, id, x, y)| (id, x, y))
    }
    #[cfg(test)]
    pub(super) fn pack_counts(&self) -> (usize, usize, usize, usize) {
        let out = &self.tree.output;
        let t = &out.local.maps.current;
        (
            out.drawing().commands.len() + t.counts[0] - t.markers[0],
            out.drawing().ids.len() + t.counts[1],
            out.fits.additions,
            out.hits.current().items.len() + t.counts[2] - t.markers[1],
        )
    }
    #[cfg(test)]
    pub(super) fn replay_drawing(&self) -> Vec<u8> {
        rasterize(
            self.tree.layers.viewport,
            self.tree.output.drawing(),
            &self.tree.output.local.maps.current,
        )
        .unwrap()
    }
    #[cfg(test)]
    pub fn pixels(&self) -> &[u8] {
        &self.pixels
    }
    pub fn into_pixels(self) -> Vec<u8> {
        self.pixels
    }
    pub fn draw(&mut self, w: f32, h: f32, scale: f32) -> Result<&[u8], String> {
        self.prepare(w, h, scale)?.submit(|_, _| Ok(()))?;
        Ok(&self.pixels)
    }
    /// Only the display worker settles or installs. Edits may reserve the
    /// candidate stores but never overwrite them during an in-flight submit.
    pub(crate) fn settle(&mut self, w: f32, h: f32, scale: f32) -> Result<bool, String> {
        self.tree.refresh(w, h, scale)?;
        let o = &self.tree.output;
        Ok(self.viewport != self.tree.layers.viewport
            || !o.local.maps.current.same_picture(
                o.local
                    .maps
                    .installed()
                    .map(AsRef::as_ref)
                    .unwrap_or(&Tables::EMPTY),
            )
            || (o.dirty[0] && **o.draw.current() != o.draw.build))
    }
    pub(crate) fn install(&mut self) {
        self.viewport = self.tree.layers.viewport;
        self.tree.output.install();
    }
    pub(crate) fn prepare(&mut self, w: f32, h: f32, scale: f32) -> Result<Prepared<'_>, String> {
        let changed = self.settle(w, h, scale)?;
        self.prepare_settled(changed)
    }
    pub(crate) fn prepare_settled(&mut self, changed: bool) -> Result<Prepared<'_>, String> {
        if self.tree.waiting_for_pixels() {
            return Err("fitted image inputs are not ready".into());
        }
        let o = &mut self.tree.output;
        o.fits.bind(&mut o.draw.build);
        let v = self.tree.layers.viewport;
        let next = (changed || self.pixels.is_empty())
            .then(|| {
                rasterize(
                    v,
                    self.tree.output.drawing(),
                    &self.tree.output.local.maps.current,
                )
            })
            .transpose()?;
        Ok(Prepared { raster: self, next })
    }
}
/// Borrows the session's only raster. Dropping a preparation or failing its
/// sink leaves pixels, viewport and hit attachments at their last installation.
/// `reuse` describes actual pixels, not whether this sink has submitted them.
#[must_use]
pub(crate) struct Prepared<'a> {
    raster: &'a mut Raster,
    next: Option<Vec<u8>>,
}
impl Prepared<'_> {
    #[cfg(test)]
    pub(super) fn accepted(&mut self) -> &mut Raster {
        self.raster
    }
    pub(crate) fn submit(
        self,
        sink: impl FnOnce(&[u8], bool) -> Result<(), String>,
    ) -> Result<(), String> {
        let r = self.raster;
        sink(
            self.next.as_deref().unwrap_or(&r.pixels),
            self.next.is_none(),
        )?;
        if let Some(next) = self.next {
            r.pixels = next;
            #[cfg(test)]
            {
                r.draws += 1;
            }
        }
        r.install();
        Ok(())
    }
}
fn rasterize(viewport: Rect, draw: &Drawing, tables: &Tables) -> Result<Vec<u8>, String> {
    let w = viewport.width.ceil() as usize;
    let h = viewport.height.ceil() as usize;
    let len = w
        .checked_mul(h)
        .and_then(|v| v.checked_mul(4))
        .filter(|&v| v <= 256 * 1024 * 1024)
        .ok_or("pixel buffer too large")?;
    if len == 0 {
        return Err("empty raster viewport".into());
    }
    let mut pixels = vec![0; len];
    {
        let info = ImageInfo::new(
            (w as i32, h as i32),
            ColorType::RGBA8888,
            AlphaType::Premul,
            None,
        );
        let props = SurfaceProps::new(SurfacePropsFlags::default(), PixelGeometry::RGBH);
        let mut surface = surfaces::wrap_pixels(&info, &mut pixels, w * 4, Some(&props))
            .ok_or("invalid raster surface")?;
        replay_inner(
            surface.canvas(),
            draw,
            recording::Replay::root(tables),
            &mut |canvas, run, paint| {
                canvas.draw_glyphs_at(
                    run.ids,
                    run.positions,
                    (run.origin[0], run.origin[1]),
                    &run.face.font(run.size),
                    paint,
                );
                Ok(())
            },
        )?;
    }
    Ok(pixels)
}

struct GlyphPaintRun<'a> {
    ids: &'a [u16],
    positions: &'a [Point],
    origin: [f32; 2],
    face: text::Face,
    size: f32,
}
fn replay_inner(
    canvas: &skia_safe::Canvas,
    draw: &Drawing,
    replay: recording::Replay<'_>,
    glyphs: &mut impl FnMut(&skia_safe::Canvas, GlyphPaintRun, &Paint) -> Result<(), String>,
) -> Result<(), String> {
    let _scope = skia_safe::AutoCanvasRestore::guard(canvas, false);
    let mut i = 0;
    while i < draw.commands.len() {
        let (scope, cmd) = &draw.commands[i];
        i += 1;
        match cmd {
            Command::Order {
                call,
                clip,
                defer_clips,
                ..
            } => {
                replay.enter(canvas, draw, *scope, *clip, *defer_clips, *call, glyphs)?;
                continue;
            }
            Command::Group { alpha, bounds, end } => {
                let bounds = bounds.resolved(
                    draw,
                    replay.children.map_or_else(Default::default, |c| c.bounds),
                );
                let bounds = draw
                    .planes
                    .installed(scope.plane, replay.tables, 0)
                    .map_or(bounds, |p| p.matrix.map_rect(bounds).0);
                if !opacity::open(canvas, bounds, *alpha) {
                    i = end + 1;
                }
                continue;
            }
            Command::Restore => {
                canvas.restore(); // layer
                canvas.restore(); // establishing matrix
                continue;
            }
            _ => (),
        }
        let plane = draw.planes.installed(scope.plane, replay.tables, 0);
        if let Some((material, rect)) = cmd.outer_shadow(&draw.materials) {
            // Ordinary host clips constrain the box, not its outer shadow.
            // Apply the same transform without installing those clips.
            let _enclosures = replay.apply(canvas, true);
            let _shadow_pose = (scope.shadow != 0
                || plane.is_some()
                || scope.guard != Coords::default())
            .then(|| {
                let restore = skia_safe::AutoCanvasRestore::guard(canvas, true);
                if scope.guard != Coords::default() {
                    scope.guard.clip(draw, canvas, replay.tables);
                }
                pose::ShadowGuard::apply(scope.shadow, draw, canvas, replay.tables);
                if let Some(p) = plane {
                    canvas.concat(&p.matrix);
                }
                restore
            });
            draw.materials.outer_shadow(canvas, material, rect);
            if matches!(
                cmd,
                Command::Box {
                    phase: Phase::OuterShadow,
                    ..
                }
            ) {
                continue;
            }
        }
        let _enclosures = replay.apply(canvas, false);
        let mut limit = replay.limit;
        let _posed = plane.map(|p| {
            let restore = skia_safe::AutoCanvasRestore::guard(canvas, true);
            limit = draw
                .planes
                .enter(p, canvas, &draw.clips, scope.before, replay.tables, limit);
            restore
        });
        let limit = limit.map(|v| match cmd {
            Command::Image(sample) => sample.limit(v),
            _ => v.body,
        });
        let _limit = limit
            .filter(|_| !matches!(cmd, Command::Fill(..)))
            .map(|r| {
                let restore = skia_safe::AutoCanvasRestore::guard(canvas, true);
                canvas.clip_rect(sk(r), None, false);
                restore
            });
        let stop = plane.map_or(OPEN, |p| p.stop);
        if let Command::Image(sample) = cmd {
            sample.draw(canvas, &draw.clips, scope.mask, plane, &draw.materials);
            continue;
        }
        draw.clips
            .within(scope.mask, stop, canvas, || -> Result<(), String> {
                match cmd {
                    Command::Image(_) | Command::Group { .. } | Command::Restore => unreachable!(),
                    Command::Box {
                        material,
                        rect,
                        clip,
                        phase,
                    } => {
                        draw.materials.draw(canvas, *material, *rect, *clip, *phase);
                    }
                    Command::Order { .. } => unreachable!(),
                    Command::Thumb(_, _) | Command::ScrollThumb(_, _, _, _) => {
                        let (r, clip) = match cmd {
                            Command::Thumb(r, clip) => (*r, *clip),
                            Command::ScrollThumb(k, frame, clip, axis) => {
                                let Some(bar) = scrollbars::Bar::new(
                                    *frame,
                                    *axis,
                                    replay.tables.get(*k, 0).expect("sealed chrome").view,
                                ) else {
                                    return Ok(());
                                };
                                (bar.thumb, *clip)
                            }
                            _ => unreachable!(),
                        };
                        let _restore = skia_safe::AutoCanvasRestore::guard(canvas, true);
                        canvas.clip_rect(sk(clip), None, false);
                        let radius = r.width.min(r.height) * 0.5;
                        canvas.draw_round_rect(
                            sk(r),
                            radius,
                            radius,
                            &draw.materials.paint(Brush::Solid(0xD0D5DC99)),
                        );
                    }
                    Command::Fill(r, brush) => {
                        let Some(r) = limit.map_or(Some(*r), |mut clip| {
                            let m = canvas.local_to_device_as_3x3();
                            if m.is_translate() {
                                let (tx, ty) = (m.translate_x(), m.translate_y());
                                clip = clip.shifted(tx, ty).paint_clip(false).shifted(-tx, -ty);
                            }
                            r.intersection(clip)
                        }) else {
                            return Ok(());
                        };
                        let paint = draw.materials.paint(*brush);
                        canvas.draw_rect(
                            skia_safe::Rect::from_xywh(r.x, r.y, r.width, r.height),
                            &paint,
                        );
                    }
                    Command::Glyphs {
                        pos,
                        face,
                        size,
                        range,
                        clip,
                        color,
                        ..
                    } => {
                        let paint = draw.materials.paint(*color);
                        if let Some(r) = clip {
                            canvas.save();
                            canvas.clip_rect(
                                skia_safe::Rect::from_xywh(r.x, r.y, r.width, r.height),
                                None,
                                false,
                            );
                        }
                        let result = (|| {
                            let mut r = range.clone();
                            while !r.is_empty() {
                                let ids = draw.ids.chunk(r.clone());
                                let positions = draw.positions.chunk(r.clone());
                                let n = ids.len().min(positions.len());
                                glyphs(
                                    canvas,
                                    GlyphPaintRun {
                                        ids: &ids[..n],
                                        positions: &positions[..n],
                                        origin: *pos,
                                        face: *face,
                                        size: *size,
                                    },
                                    &paint,
                                )?;
                                r.start += n;
                            }
                            Ok::<(), String>(())
                        })();
                        if clip.is_some() {
                            canvas.restore();
                        }
                        result?;
                    }
                }
                Ok(())
            })?;
    }
    Ok(())
}
#[cfg(all(
    target_os = "linux",
    feature = "wayland",
    any(feature = "opengl", feature = "vulkan")
))]
mod gpu;
#[cfg(all(
    target_os = "linux",
    feature = "wayland",
    any(feature = "opengl", feature = "vulkan")
))]
pub(crate) use gpu::GeometryPainter;
