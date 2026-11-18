use super::{layers::Layers, text::Run, *};
use skia_safe::{
    AlphaType, Color, ColorType, ImageInfo, Paint, PixelGeometry, Point, SurfaceProps,
    SurfacePropsFlags, surfaces,
};
#[derive(Clone, Copy, PartialEq)]
pub(super) struct PaintContext {
    pub frame: Rect,
    pub clip: Rect,
    pub child_clip: Rect,
    pub color: u32,
    pub scale: f32,
    pub text_origin: [f32; 2],
}
impl PaintContext {
    /// Ordinary rectangular Skia host clips use non-antialiased device edges.
    /// Hits keep the unrounded clip and the original element-local origin.
    pub fn pixel_clip(&self) -> Rect {
        let c = self.clip;
        let x = c.x.round();
        let y = c.y.round();
        Rect {
            x,
            y,
            width: (c.x + c.width).round() - x,
            height: (c.y + c.height).round() - y,
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq)]
struct Hit {
    key: Key,
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
    seen_at_parent: (usize, u64),
}
#[derive(Clone, Copy, PartialEq)]
pub(super) struct GlyphPaint {
    pub pos: [f32; 2],
    pub color: u32,
    pub revision: u64,
    pub clip: Option<Rect>,
}
#[derive(Clone, Copy, Default, PartialEq)]
pub(super) struct OwnPaint {
    fill: Option<(Rect, u32)>,
    pub text: Option<GlyphPaint>,
}
impl OwnPaint {
    pub fn background(a: &Attrs, ctx: &PaintContext) -> Self {
        let fill = a
            .background
            .filter(|v| v & 255 != 0)
            .and_then(|v| ctx.frame.intersection(ctx.pixel_clip()).map(|r| (r, v)));
        Self { fill, text: None }
    }
    fn nonempty(self) -> bool {
        self.fill.is_some() || self.text.is_some()
    }
}
#[derive(Clone, Copy, Default)]
pub(super) struct Composition {
    paint: OwnPaint,
    hit: Option<Hit>,
    ports: [Port; 2],
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
            *changed = next.paint != paint;
            next.paint = paint;
            paint.nonempty()
        } else {
            let hit = ctx
                .frame
                .intersection(ctx.clip)
                .filter(|_| a.click)
                .map(|rect| Hit {
                    key: k,
                    id: e.slots[k].id,
                    rect,
                    origin: [ctx.frame.x, ctx.frame.y],
                    scale: ctx.scale,
                });
            *changed = hit != next.hit;
            next.hit = hit;
            hit.is_some()
        };
        let mut count = 0;
        for &c in e.children(k) {
            let port = &mut parts[c].ports[side];
            if port.nonempty {
                let seen = (count, port.revision);
                *changed |= seen != port.seen_at_parent;
                port.seen_at_parent = seen;
                count += 1;
            }
        }
        let port = &mut next.ports[side];
        *changed |= port.children != count;
        port.children = count;
        port.nonempty = own || count != 0;
        if *changed {
            port.revision += 1
        }
    }
    parts.insert(k, next);
    Outputs::changed(delta[0], delta[1])
}
#[derive(Clone, Debug, PartialEq)]
enum Command {
    Fill(Rect, u32),
    Glyphs {
        pos: [f32; 2],
        clip: Option<Rect>,
        family: Family,
        size: f32,
        color: u32,
        range: Range<usize>,
    },
}
#[derive(Default, PartialEq)]
struct Drawing {
    commands: Vec<Command>,
    ids: Vec<u16>,
    positions: Vec<Point>,
}
impl Drawing {
    fn reserve(&mut self, owners: usize, glyphs: usize, b: Budget) -> Result<(), String> {
        if 2 * owners > self.commands.capacity() {
            pool::reserve(&mut self.commands, b.capacity(2 * owners))?
        }
        if glyphs > self.ids.capacity() {
            pool::reserve(&mut self.ids, b.capacity(glyphs))?
        }
        if glyphs > self.positions.capacity() {
            pool::reserve(&mut self.positions, b.capacity(glyphs))?
        }
        Ok(())
    }
    fn clear(&mut self) {
        self.commands.clear();
        self.ids.clear();
        self.positions.clear()
    }
}
// Candidate and last successful execution inputs. Neither is addressable by a
// client. Pixel comparison never publishes; only the successful draw does.
#[derive(Default)]
pub(super) struct Output {
    stack: Vec<Key>,
    draw: [Drawing; 2],
    hits: [Vec<Hit>; 2],
    installed: [Option<(Key, u64)>; 2],
    pending: [Option<(Key, u64)>; 2],
    dirty: [bool; 2],
}
impl Output {
    pub fn reserve_for(&mut self, owners: usize, glyphs: usize, b: Budget) -> Result<(), String> {
        if owners > self.stack.capacity() {
            pool::reserve(&mut self.stack, b.capacity(owners))?
        }
        for v in &mut self.draw {
            v.reserve(owners, glyphs, b)?
        }
        for h in &mut self.hits {
            if owners > h.capacity() {
                pool::reserve(h, b.capacity(owners))?
            }
        }
        Ok(())
    }
    pub fn collect(&mut self, e: &Elements, root: Option<Key>, layers: &Layers) {
        for side in 0..2 {
            let stamp = root.map(|k| (k, layers.parts[k].ports[side].revision));
            if self.pending[side] == stamp {
                continue;
            }
            self.pending[side] = stamp;
            self.dirty[side] = stamp != self.installed[side];
            if !self.dirty[side] {
                continue;
            }
            if side == 0 {
                self.draw[1].clear()
            } else {
                self.hits[1].clear()
            }
            self.stack.extend(root);
            while let Some(k) = self.stack.pop() {
                let p = layers.parts[k];
                if !p.ports[side].nonempty {
                    continue;
                }
                if side == 0 {
                    let draw = &mut self.draw[1];
                    if let Some((r, c)) = p.paint.fill {
                        draw.commands.push(Command::Fill(r, c))
                    }
                    if let Some(glyphs) = p.paint.text {
                        let run = layers.runs[k];
                        let start = draw.ids.len();
                        for g in layers.text.glyphs.get(&run.span) {
                            draw.ids.push(g.id);
                            draw.positions.push(g.pos)
                        }
                        draw.commands.push(Command::Glyphs {
                            pos: glyphs.pos,
                            color: glyphs.color,
                            clip: glyphs.clip,
                            family: run.metrics.family,
                            size: run.metrics.size,
                            range: start..draw.ids.len(),
                        })
                    }
                } else if let Some(hit) = p.hit {
                    self.hits[1].push(hit)
                }
                self.stack.extend(
                    e.children(k)
                        .iter()
                        .rev()
                        .filter(|&&c| layers.parts[c].ports[side].nonempty),
                );
            }
        }
    }
    fn drawing(&self) -> &Drawing {
        &self.draw[usize::from(self.dirty[0])]
    }
    fn install(&mut self) {
        if self.dirty[0] {
            self.draw.swap(0, 1);
            self.draw[1].clear();
        }
        if self.dirty[1] {
            self.hits.swap(0, 1);
            self.hits[1].clear();
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
    pub(crate) fn with_tree(tree: Tree) -> Self {
        Self {
            tree,
            ..Default::default()
        }
    }
    pub(crate) fn has_target(&self, key: Key) -> bool {
        self.tree.output.hits[0].iter().any(|h| h.key == key)
    }
    pub(crate) fn target_at(&self, x: f32, y: f32) -> Option<(Key, u64, f64, f64)> {
        let v = self.viewport;
        if !(x >= 0. && x < v.width && y >= 0. && y < v.height) {
            return None;
        }
        self.tree.output.hits[0]
            .iter()
            .rev()
            .find(|h| {
                let r = h.rect;
                x >= r.x && x <= r.x + r.width && y >= r.y && y <= r.y + r.height
            })
            .map(|h| {
                (
                    h.key,
                    h.id,
                    (f64::from(x) - f64::from(h.origin[0])) / f64::from(h.scale),
                    (f64::from(y) - f64::from(h.origin[1])) / f64::from(h.scale),
                )
            })
    }
    #[cfg(test)]
    pub(super) fn hit_at(&self, x: f32, y: f32) -> Option<(u64, f64, f64)> {
        self.target_at(x, y).map(|(_, id, x, y)| (id, x, y))
    }
    pub fn pixels(&self) -> &[u8] {
        &self.pixels
    }
    pub fn into_pixels(self) -> Vec<u8> {
        self.pixels
    }
    pub fn draw(&mut self, w: f32, h: f32, scale: f32) -> Result<&[u8], String> {
        self.tree.refresh(w, h, scale)?;
        let v = self.tree.layers.viewport;
        let output = &mut self.tree.output;
        if self.viewport != v
            || self.pixels.is_empty()
            || (output.dirty[0] && output.draw[0] != output.draw[1])
        {
            self.pixels = rasterize(v, output.drawing())?;
            #[cfg(test)]
            {
                self.draws += 1;
            }
        }
        self.viewport = v;
        output.install();
        Ok(&self.pixels)
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
        let canvas = surface.canvas();
        let mut paint = Paint::default();
        paint.set_anti_alias(true);
        for cmd in &draw.commands {
            let [r, g, b, a] = match *cmd {
                Command::Fill(_, v) | Command::Glyphs { color: v, .. } => v.to_be_bytes(),
            };
            paint.set_color(Color::from_argb(a, r, g, b));
            match cmd {
                Command::Fill(r, _) => {
                    canvas.draw_rect(
                        skia_safe::Rect::from_xywh(r.x, r.y, r.width, r.height),
                        &paint,
                    );
                }
                Command::Glyphs {
                    pos,
                    family,
                    size,
                    range,
                    clip,
                    ..
                } => {
                    if let Some(r) = clip {
                        canvas.save();
                        canvas.clip_rect(
                            skia_safe::Rect::from_xywh(r.x, r.y, r.width, r.height),
                            None,
                            false,
                        );
                    }
                    canvas.draw_glyphs_at(
                        &draw.ids[range.clone()],
                        &draw.positions[range.clone()],
                        (pos[0], pos[1]),
                        &family.font(*size),
                        &paint,
                    );
                    if clip.is_some() {
                        canvas.restore();
                    }
                }
            }
        }
    }
    Ok(pixels)
}
