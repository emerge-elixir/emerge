use super::{layers::Layers, text::Run, *};
mod buffers;
mod clips;
pub(super) mod fits;
use buffers::Buffers;
use std::sync::Arc;
mod image;
mod opacity;
mod pose;
use pose::{Plane, Planes, Scope};
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
#[derive(Clone, Copy, PartialEq)]
pub(super) struct PaintContext {
    pub frame: Rect,
    pub clip: Rect,
    pub child_clip: Rect,
    pub color: u32,
    pub gradient: Option<(Key, u64)>,
    pub rules: u8,
    pub offset: [f32; 2],
    pub scale: f32,
    pub text_align: Alignment,
}
impl PaintContext {
    fn box_clip(self, shape: Shape) -> Rect {
        if shape == Shape::Image {
            self.child_clip.pixel_clip()
        } else {
            self.clip.pixel_clip()
        }
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
            let relaxed = Self {
                x: self.x - 0.5,
                y: self.y - 0.5,
                width: self.width + 1.,
                height: self.height + 1.,
            };
            p.clip.intersection(relaxed).unwrap_or_default()
        } else {
            self
        }
    }
    /// Ordinary rectangular Skia clips use non-antialiased device edges.
    /// Hits keep the unrounded clip and the original element-local origin.
    pub(super) fn pixel_clip(self) -> Rect {
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
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Attachment(Key, u64);
#[derive(Clone, Copy, Debug, PartialEq)]
struct Hit {
    scope: Scope,
    key: Attachment,
    id: u64,
    rect: Rect,
    origin: [f32; 2],
    scale: f32,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Outputs {
    Draw,
    Hit,
    Both,
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
    seen_at_parent: Option<(usize, u64)>,
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
impl OwnPaint {
    pub fn background(a: &Attrs, ctx: &PaintContext) -> Self {
        let clip = if a.image_src.len != 0 {
            ctx.box_clip(Shape::Image)
        } else {
            ctx.clip.pixel_clip()
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
    paint: OwnPaint,
    art: bool,
    hit: Option<Hit>,
    rounded: bool,
    opacity: Option<u8>,
    ports: [Port; 2],
}
impl Composition {
    pub(super) fn reattach(&mut self) {
        for p in &mut self.ports {
            p.seen_at_parent = None;
        }
    }
}
// Each child has one parent. Comparing (ordinal among nonempty children, value
// revision) and the count is exact for its ordered contribution, without a
// second child list. Children that changed to empty need no comparison by ID.
pub(super) fn compose(
    e: &Elements,
    k: Key,
    ctx: &PaintContext,
    run: Option<&Run>,
    parts: &mut SecondaryMap<Key, Composition>,
    requested: Outputs,
) -> Option<Outputs> {
    let mut next = parts.get(k).copied().unwrap_or_default();
    let mut delta = [false; 2];
    let a = &e.attrs[k];
    for (side, changed) in delta.iter_mut().enumerate() {
        if (side == 0 && requested == Outputs::Hit) || (side == 1 && requested == Outputs::Draw) {
            continue;
        }
        let own = if side == 0 {
            let paint = e.slots[k].shape.paint(e, k, ctx, run);
            let image = a.has_art();
            *changed = next.paint != paint
                || paint.decorated
                || image
                || next.art
                || next.opacity != a.opacity;
            next.art = image;
            next.opacity = a.opacity;
            next.paint = paint;
            image || paint.nonempty()
        } else {
            let hit = ctx
                .frame
                .intersection(if a.posed() { ctx.child_clip } else { ctx.clip })
                .filter(|_| a.click)
                .map(|rect| Hit {
                    scope: Scope::default(),
                    key: Attachment(k, e.slots[k].attachment),
                    id: e.slots[k].id,
                    rect,
                    origin: [ctx.frame.x, ctx.frame.y],
                    scale: ctx.scale,
                });
            let rounded = a.decor.radius != [0.; 4];
            *changed = hit != next.hit
                || rounded
                || next.rounded
                || (a.click && e.slots[k].shape == Shape::Image);
            next.rounded = rounded;
            next.hit = hit;
            hit.is_some()
        };
        let mut count = 0;
        for &c in e.children(k) {
            let port = &mut parts[c].ports[side];
            if port.nonempty {
                let seen = Some((count, port.revision));
                *changed |= seen != port.seen_at_parent;
                port.seen_at_parent = seen;
                count += 1;
            }
        }
        // A paragraph also owns inline contributions, which need not have a
        // nonempty ordinary child frame. Only scheduled readers reach here.
        let inline = e.slots[k].shape == Shape::Paragraph;
        *changed |= inline;
        let port = &mut next.ports[side];
        *changed |= port.children != count;
        port.children = count;
        port.nonempty = inline || own || count != 0;
        if *changed {
            port.revision += 1
        }
    }
    parts.insert(k, next);
    Outputs::changed(delta[0], delta[1])
}
#[derive(Clone, Debug, PartialEq)]
enum Command {
    Fill(Rect, Brush),
    Image(image::Sample),
    Group {
        alpha: u8,
        bounds: skia_safe::Rect,
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
#[derive(Default, PartialEq)]
pub(crate) struct Drawing {
    materials: Materials,
    commands: Vec<(Scope, Command)>,
    clips: Clips,
    planes: Planes,
    plane: u32,
    mask: Mask,
    ids: Vec<u16>,
    positions: Vec<Point>,
}
impl Drawing {
    fn push(&mut self, cmd: Command) {
        let scope = if matches!(cmd, Command::Group { .. } | Command::Restore) {
            Scope::default() // opacity bounds are already in viewport coordinates
        } else {
            Scope {
                mask: self.mask,
                plane: self.plane,
            }
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
        self.plane = 0;
        self.mask = OPEN;
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
    index: MaterialIndex,
    stack: Vec<(Key, Visit, Mask)>,
    draw: Buffers<Drawing, 7>,
    hits: Buffers<Hits, 3>,
    installed: [Option<(Key, u64)>; 2],
    pending: [Option<(Key, u64)>; 2],
    dirty: [bool; 2],
}
#[derive(Clone, Copy)]
enum Visit {
    Node { rounded_parent: bool },
    Inline,
    Border(u32),
    Opacity(usize),
}
impl Output {
    pub fn reserve_images(&mut self, count: usize, b: Budget) -> Result<(), String> {
        self.fits.reserve(count, b)?;
        self.draw.reserve([0, 0, 0, 0, 0, 0, count], b)?;
        self.hits.reserve([0, 0, count], b)
    }
    pub fn reserve_materials(
        &mut self,
        keys: usize,
        count: usize,
        bytes: usize,
        words: usize,
        b: Budget,
    ) -> Result<(), String> {
        if count != 0 {
            self.hits.reserve([0, 2 * count + words, 0], b)?;
            self.index.reserve(keys)?;
            self.draw
                .reserve([0, 0, 0, 2 * count, count, bytes, 0], b)?;
        }
        Ok(())
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
            ],
            b,
        )?;
        self.hits.reserve([owners + 2 * words, 0, 0], b)
    }
    // Only an *unsealed* stream can have outstanding bitmap jobs. Accepted
    // edits may supersede those jobs but must never alter a sealed drawing.
    pub(super) fn cancel_fits(&mut self) {
        if self.fits.unbound() {
            self.fits.reset();
            self.pending[0] = self.installed[0];
            self.draw.build.clear();
            self.dirty[0] = false;
        }
    }
    pub fn collect(&mut self, e: &Elements, root: Option<Key>, layers: &Layers) {
        for side in 0..2 {
            let stamp = root.map(|k| (k, layers.parts[k].ports[side].revision));
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
            if side == 0 {
                self.index.begin();
                self.draw.build.clear()
            } else {
                self.hits.build.clips.clear();
                self.hits.build.planes.0.clear();
                self.hits.build.items.clear()
            }
            self.stack.extend(root.map(|k| {
                (
                    k,
                    Visit::Node {
                        rounded_parent: false,
                    },
                    OPEN,
                )
            }));
            while let Some((k, visit, mask)) = self.stack.pop() {
                self.draw.build.plane = 0;
                let rounded_parent = match visit {
                    Visit::Inline => {
                        self.inline(e, k, layers, side, mask);
                        continue;
                    }
                    Visit::Border(plane) => {
                        let ctx = layers.paint[k];
                        let draw = &mut self.draw.build;
                        draw.plane = plane;
                        draw.mask = mask;
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
                        self.draw.build.end_group(start, layers.viewport);
                        continue;
                    }
                    Visit::Node { rounded_parent } => rounded_parent,
                };
                let p = layers.parts[k];
                if !p.ports[side].nonempty {
                    continue;
                }
                let ctx = layers.paint[k];
                let a = &e.attrs[k];
                let decor = a.decor;
                if a.pose.zoom == 0. {
                    continue;
                } // collapsed leaf has neither ink nor a target
                let plane = a.posed().then(|| {
                    let stop = if rounded_parent && !a.image_host_clip {
                        (if side == 0 {
                            &self.draw.build.clips
                        } else {
                            &self.hits.build.clips
                        })
                        .parent(mask)
                    } else {
                        mask
                    };
                    Plane::new(a, ctx, stop)
                });
                if plane.is_some_and(|p| {
                    !p.matrix.is_finite() || !p.matrix.map_rect(sk(ctx.frame)).0.is_finite()
                }) {
                    continue;
                }
                if side == 0 {
                    let draw = &mut self.draw.build;
                    draw.mask = mask;
                    draw.plane = draw.planes.add(plane);
                    if let Some(alpha) = p.opacity {
                        if alpha == 0 {
                            continue;
                        }
                        let start = draw.begin_group(alpha);
                        self.stack.push((k, Visit::Opacity(start), mask));
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
                        if let Some(sample) =
                            image::Sample::new(e, k, ctx, None, 0, None, &mut self.fits)
                        {
                            draw.mask = draw
                                .clips
                                .add(mask, Rounded::new(ctx.frame, decor.radii(ctx.scale)));
                            draw.push(Command::Image(sample));
                            draw.mask = mask;
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
                            self.stack.push((k, Visit::Border(draw.plane), mask));
                        }
                    } else if let Some((r, c)) = p.paint.fill {
                        draw.push(Command::Fill(r, Brush::Solid(c)))
                    }
                    if let Some(glyphs) = p.paint.text {
                        let run = layers.runs[k];
                        let start = draw.ids.len();
                        for g in layers.text.glyphs.get(&run.span) {
                            draw.ids.push(g.id);
                            draw.positions.push(g.pos)
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
                                .pixel_clip(),
                        );
                    }
                } else if let Some(mut hit) = p.hit {
                    let hits = &mut self.hits.build;
                    hit.scope.mask = hits
                        .clips
                        .add(mask, Rounded::new(ctx.frame, decor.radii(ctx.scale)));
                    if let Some(plane) = plane {
                        let Some(inverse) = plane.inverse() else {
                            continue;
                        };
                        hit.scope.plane = hits.planes.add(Some(inverse));
                    }
                    hits.items.push(hit)
                }
                let inner = decor.rounded_inner(ctx.frame, ctx.scale);
                let mask = if side == 0 {
                    self.draw.build.clips.add(mask, inner)
                } else {
                    self.hits.build.clips.add(mask, inner)
                };
                if side == 0
                    && e.slots[k].shape == Shape::Image
                    && let Some(sample) = image::Sample::new(
                        e,
                        k,
                        ctx,
                        plane,
                        u8::from(rounded_parent) + u8::from(inner.is_some()),
                        (e.attrs[k].svg_gradient.len != 0)
                            .then(|| self.index.get(&mut self.draw.build, e, k)),
                        &mut self.fits,
                    )
                {
                    let draw = &mut self.draw.build;
                    draw.mask = mask;
                    draw.push(Command::Image(sample));
                }
                let paragraph = e.slots[k].shape == Shape::Paragraph;
                if paragraph && !layers.paragraphs.fragments(k).is_empty() {
                    self.stack.push((k, Visit::Inline, mask));
                }
                self.stack.extend(
                    e.children(k)
                        .iter()
                        .rev()
                        .filter(|&&c| {
                            layers.parts[c].ports[side].nonempty
                                && (!paragraph || e.float_side(c).is_some())
                        })
                        .map(|&c| {
                            (
                                c,
                                Visit::Node {
                                    rounded_parent: inner.is_some(),
                                },
                                mask,
                            )
                        }),
                );
            }
            if side == 0 {
                self.fits.finish_targets();
            }
        }
        self.fits.bind(&mut self.draw.build);
    }
    fn inline(&mut self, e: &Elements, k: Key, l: &Layers, side: usize, mask: Mask) {
        let ctx = l.paint[k];
        let clip = ctx.child_clip;
        for line in l
            .paragraphs
            .fragments(k)
            .chunk_by(|a, b| a.owner == b.owner && a.bounds.y == b.bounds.y)
        {
            let first = line[0];
            let owner = first.owner;
            let source = first.source;
            let mut rect = paragraph::union(first.bounds, line.last().unwrap().bounds);
            rect.x += ctx.frame.x;
            rect.y += ctx.frame.y;
            if side == 1 {
                if let Some(area) = rect.intersection(clip) {
                    let mask = self.hits.build.clips.add(
                        mask,
                        Rounded::new(rect, e.attrs[owner].decor.radii(l.paint[owner].scale)),
                    );
                    for target in [Some(owner), (source != owner).then_some(source)]
                        .into_iter()
                        .flatten()
                        .filter(|&c| e.attrs[c].click)
                    {
                        self.hits.build.items.push(Hit {
                            scope: Scope { mask, plane: 0 },
                            key: Attachment(target, e.slots[target].attachment),
                            id: e.slots[target].id,
                            rect: area,
                            // The mask is per continuation, but a listener has
                            // one outer origin across every line of its span.
                            origin: [
                                ctx.frame.x + l.paragraphs.site(target).x,
                                ctx.frame.y + l.paragraphs.site(target).y,
                            ],
                            scale: e.slots[target].admitted.scale,
                        });
                    }
                }
                continue;
            }
            let draw = &mut self.draw.build;
            draw.mask = mask;
            let alpha = e.attrs[owner].opacity;
            if alpha == Some(0) {
                continue;
            }
            let start = alpha.map(|a| draw.begin_group(a));
            let material = (owner != source && e.attrs[owner].decor.active())
                .then(|| self.index.get(draw, e, owner));
            if let Some(material) = material {
                draw.push(Command::Box {
                    material,
                    rect,
                    clip: clip.pixel_clip(),
                    phase: Phase::Under,
                });
            } else if owner != source
                && let Some(color) = e.attrs[owner].background
                && let Some(area) = rect.intersection(clip.pixel_clip())
            {
                draw.push(Command::Fill(area, Brush::Solid(color)));
            }
            let run = l.runs[source];
            let source_ctx = l.paint[source];
            let [t, r, b, left] = e.attrs[k]
                .sizing
                .padding
                .map(|v| scale::pixels(v, ctx.scale));
            let bounds = Rect {
                x: ctx.frame.x + left,
                y: ctx.frame.y + t,
                width: (ctx.frame.width - left - r).max(0.),
                height: (ctx.frame.height - t - b).max(0.),
            };
            let color = self.index.brush(draw, e, source_ctx, bounds);
            for f in line.iter().filter(|_| source_ctx.color & 255 != 0) {
                let start = draw.ids.len();
                for (id, pos) in l.text.word_glyphs(run, f.first as usize..f.end as usize) {
                    draw.ids.push(id);
                    draw.positions.push(pos);
                }
                if start != draw.ids.len() {
                    draw.push(Command::Glyphs {
                        pos: [f.baseline[0] + ctx.frame.x, f.baseline[1] + ctx.frame.y],
                        clip: Some(clip.pixel_clip()),
                        bounds: clip,
                        face: run.metrics.face,
                        size: run.metrics.size,
                        color,
                        range: start..draw.ids.len(),
                    });
                    draw.rules(
                        source_ctx.rules,
                        color,
                        [f.baseline[0] + ctx.frame.x, f.baseline[1] + ctx.frame.y],
                        f.width,
                        run.metrics.size,
                        clip.pixel_clip(),
                    );
                }
            }
            if let Some(material) = material {
                draw.push(Command::Box {
                    material,
                    rect,
                    clip: clip.pixel_clip(),
                    phase: Phase::Border,
                });
            }
            if let Some(start) = start {
                draw.end_group(start, l.viewport);
            }
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
        self.tree.output.hits.current().has_target(key)
    }
    #[cfg(test)]
    pub(crate) fn target_at(&self, x: f32, y: f32) -> Option<(Attachment, u64, f64, f64)> {
        self.tree
            .output
            .hits
            .current()
            .target_at(self.viewport, x, y)
    }
}
#[derive(Default)]
pub(crate) struct Hits {
    planes: Planes,
    clips: Clips,
    items: Vec<Hit>,
}
impl Hits {
    pub(crate) fn has_target(&self, key: Attachment) -> bool {
        self.items.iter().any(|h| h.key == key)
    }
    pub(crate) fn target_at(&self, v: Rect, x: f32, y: f32) -> Option<(Attachment, u64, f64, f64)> {
        if !(x >= 0. && x < v.width && y >= 0. && y < v.height) {
            return None;
        }
        let inside =
            |r: Rect, x, y| x >= r.x && x <= r.x + r.width && y >= r.y && y <= r.y + r.height;
        self.items.iter().rev().find_map(|h| {
            let (x, y, stop) = if let Some(p) = self.planes.get(h.scope.plane) {
                if !inside(p.outer, x, y) || !self.clips.contains(p.stop, OPEN, x, y) {
                    return None;
                }
                let local = p.matrix.map_point((x, y));
                (local.x, local.y, p.stop)
            } else {
                (x, y, OPEN)
            };
            (inside(h.rect, x, y) && self.clips.contains(h.scope.mask, stop, x, y)).then(|| {
                (
                    h.key,
                    h.id,
                    (f64::from(x) - f64::from(h.origin[0])) / f64::from(h.scale),
                    (f64::from(y) - f64::from(h.origin[1])) / f64::from(h.scale),
                )
            })
        })
    }
}

/// No occurrence table, pool span, writable pixel target or mutable device state escapes.
#[derive(Clone, Default)]
pub(crate) struct Scene {
    _workspace: Option<Arc<storage::Grant>>,
    pub viewport: Rect,
    pub drawing: Arc<Drawing>,
    pub hits: Arc<Hits>,
}
impl Scene {
    pub fn rasterize(&self) -> Result<Vec<u8>, String> {
        rasterize(self.viewport, &self.drawing)
    }
    pub fn same_drawing(&self, other: &Self) -> bool {
        self.viewport == other.viewport
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
    }
    /// Layout may advance with every reader pinned. Don't pack a side that
    /// cannot be sealed, and don't require a spare for an unchanged side.
    pub(crate) fn collect_if_available(&mut self) -> bool {
        let o = &mut self.output;
        let stamp = |side: usize| {
            self.root
                .map(|k| (k, self.layers.parts[k].ports[side].revision))
        };
        if (stamp(0) != o.installed[0] && !o.draw.available())
            || (stamp(1) != o.installed[1] && !o.hits.available())
        {
            o.cancel_fits();
            return false;
        }
        o.collect(&self.elements, self.root, &self.layers);
        true
    }
    pub(crate) fn seal(&mut self) -> Option<Scene> {
        let o = &mut self.output;
        if o.fits.waiting()
            || (o.dirty[0] && !o.draw.available())
            || (o.dirty[1] && !o.hits.available())
        {
            return None;
        }
        o.install();
        Some(Scene {
            _workspace: self.workspace.clone(),
            viewport: self.layers.viewport,
            drawing: o.draw.current().clone(),
            hits: o.hits.current().clone(),
        })
    }
}
impl Raster {
    #[cfg(test)]
    pub(super) fn hit_at(&self, x: f32, y: f32) -> Option<(u64, f64, f64)> {
        self.target_at(x, y).map(|(_, id, x, y)| (id, x, y))
    }
    #[cfg(test)]
    pub(super) fn replay_drawing(&self) -> Vec<u8> {
        rasterize(self.tree.layers.viewport, self.tree.output.drawing()).unwrap()
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
        let v = self.tree.layers.viewport;
        let next = (changed || self.pixels.is_empty())
            .then(|| rasterize(v, self.tree.output.drawing()))
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
fn rasterize(viewport: Rect, draw: &Drawing) -> Result<Vec<u8>, String> {
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
        replay(surface.canvas(), draw, |canvas, run, paint| {
            canvas.draw_glyphs_at(
                run.ids,
                run.positions,
                (run.origin[0], run.origin[1]),
                &run.face.font(run.size),
                paint,
            );
            Ok(())
        })?;
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
fn replay(
    canvas: &skia_safe::Canvas,
    draw: &Drawing,
    mut glyphs: impl FnMut(&skia_safe::Canvas, GlyphPaintRun, &Paint) -> Result<(), String>,
) -> Result<(), String> {
    let _scope = skia_safe::AutoCanvasRestore::guard(canvas, false);
    let mut i = 0;
    while i < draw.commands.len() {
        let (scope, cmd) = &draw.commands[i];
        i += 1;
        match cmd {
            Command::Group { alpha, bounds, end } => {
                if let Some(local) = opacity::visible(canvas, *bounds) {
                    canvas.save_layer_alpha(local, u32::from(*alpha));
                    #[cfg(test)]
                    opacity::record(local);
                } else {
                    i = end + 1;
                }
                continue;
            }
            Command::Restore => {
                canvas.restore();
                continue;
            }
            _ => (),
        }
        let plane = draw.planes.get(scope.plane);
        let _posed = plane.map(|p| {
            let restore = skia_safe::AutoCanvasRestore::guard(canvas, true);
            p.enter(canvas, &draw.clips);
            restore
        });
        let stop = plane.map_or(OPEN, |p| p.stop);
        if let Command::Image(sample) = cmd {
            sample.draw(canvas, &draw.clips, scope.mask, plane, &draw.materials);
            continue;
        }
        if let Command::Box {
            material,
            rect,
            phase: phase @ (Phase::Under | Phase::OuterShadow),
            ..
        } = cmd
        {
            draw.materials.outer_shadow(canvas, *material, *rect);
            if *phase == Phase::OuterShadow {
                continue;
            }
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
                    Command::Fill(r, brush) => {
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
                        let result = glyphs(
                            canvas,
                            GlyphPaintRun {
                                ids: &draw.ids[range.clone()],
                                positions: &draw.positions[range.clone()],
                                origin: *pos,
                                face: *face,
                                size: *size,
                            },
                            &paint,
                        );
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
