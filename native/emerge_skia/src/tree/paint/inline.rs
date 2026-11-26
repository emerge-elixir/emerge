//! A paragraph owns line breaks, but paint transforms and listeners belong
//! to the span. All its continuations share one pivot and one local origin.
use super::*;

#[derive(Clone, Copy)]
struct Space {
    plane: u32,
    clip: Rect,
    origin: [f32; 2], // translate paragraph-relative fragments into this space
}
impl Space {
    fn child(
        self,
        planes: &mut Planes,
        mask: Mask,
        a: &Attrs,
        mut ctx: PaintContext,
        host: PaintContext,
        hits: bool,
    ) -> Option<Self> {
        ctx.boundary = self.clip;
        let plane = planes.branch(self.plane, mask, a, ctx, hits)?;
        let clip = if a.posed() {
            a.pose.local_clip(ctx.frame, self.clip)
        } else {
            self.clip
        };
        Some(Self {
            plane,
            clip,
            origin: [
                host.frame.x + (ctx.offset[0] - host.offset[0]),
                host.frame.y + (ctx.offset[1] - host.offset[1]),
            ],
        })
    }
    /// A continuation's rounding stays in its owner's space even if the text
    /// moves or turns within it. `mask` and `base.mask` coincide for square sites.
    fn line_scope(self, owner: Self, base: Scope, mask: Mask) -> Scope {
        if self.plane == owner.plane || mask == base.mask {
            Scope {
                plane: self.plane,
                mask,
                ..base
            }
        } else {
            Scope {
                plane: self.plane,
                before: mask,
                ..base
            }
        }
    }
    fn rect(self, mut r: Rect) -> Rect {
        r.x += self.origin[0];
        r.y += self.origin[1];
        r
    }
}
fn lines(span: &[paragraph::Fragment]) -> impl Iterator<Item = (&[paragraph::Fragment], Rect)> {
    span.chunk_by(|a, b| a.bounds.y == b.bounds.y).map(|line| {
        (
            line,
            paragraph::union(line[0].bounds, line.last().unwrap().bounds),
        )
    })
}
impl Output {
    pub(super) fn inline(&mut self, e: &Elements, k: Key, l: &Layers, side: usize, scope: Scope) {
        let ctx = l.paint[k];
        let parent = Space {
            plane: scope.plane,
            clip: ctx.child_clip,
            origin: [ctx.frame.x, ctx.frame.y],
        };
        for span in l
            .paragraphs
            .fragments(k)
            .chunk_by(|a, b| a.owner == b.owner)
        {
            let (owner, source) = (span[0].owner, span[0].source);
            let planes = if side == 0 {
                &mut self.draw.build.planes
            } else {
                &mut self.hits.build.planes
            };
            let Some(outer) = parent.child(
                planes,
                scope.mask,
                &e.attrs[owner],
                l.paint[owner],
                ctx,
                side == 1,
            ) else {
                continue;
            };
            let ink = if source == owner {
                Some(outer)
            } else {
                outer.child(
                    planes,
                    scope.mask,
                    &e.attrs[source],
                    l.paint[source],
                    ctx,
                    side == 1,
                )
            };
            let corners = e.attrs[owner].decor.radii(l.paint[owner].scale);
            if side == 1 {
                let hits = &mut self.hits.build;
                if !(e.attrs[owner].click || e.attrs[source].click) {
                    continue;
                }
                for (_, local) in lines(span) {
                    let mask = hits
                        .clips
                        .add(scope.mask, Rounded::new(outer.rect(local), corners));
                    for (target, space) in [
                        (owner, Some(outer)),
                        (source, ink.filter(|_| source != owner)),
                    ] {
                        let Some(space) = space.filter(|_| e.attrs[target].click) else {
                            continue;
                        };
                        let rect = space.rect(local);
                        if let Some(area) = rect.intersection(space.clip) {
                            let frame = l.paint[target].frame;
                            hits.items.push(Hit {
                                scope: space.line_scope(outer, scope, mask),
                                key: Attachment(target, e.slots[target].attachment),
                                id: e.slots[target].id,
                                rect: area,
                                origin: [frame.x, frame.y],
                                scale: l.paint[target].scale,
                            });
                        }
                    }
                }
                continue;
            }
            let draw = &mut self.draw.build;
            let alpha = e.attrs[owner].opacity;
            if alpha == Some(0) {
                continue;
            }
            let group = alpha.map(|a| draw.begin_group(a));
            let material = (owner != source && e.attrs[owner].decor.active())
                .then(|| self.index.get(draw, e, owner));
            let decoration = |draw: &mut Drawing, rect, phase| {
                draw.scope = Scope {
                    plane: outer.plane,
                    ..scope
                };
                let rect = outer.rect(rect);
                let clip = outer.clip.paint_clip(outer.plane != 0);
                if let Some(material) = material {
                    draw.push(Command::Box {
                        material,
                        rect,
                        clip,
                        phase,
                    });
                } else if matches!(phase, Phase::Under)
                    && owner != source
                    && let Some(color) = e.attrs[owner].background
                    && let Some(area) = rect.intersection(clip)
                {
                    draw.push(Command::Fill(area, Brush::Solid(color)));
                }
            };
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
            let brush = self.index.brush(draw, e, source_ctx, bounds);
            let moved_source =
                source != owner && (e.attrs[source].posed() || e.attrs[source].offset != [0.; 2]);
            let glyphs = |draw: &mut Drawing, line: &[paragraph::Fragment], rect| {
                let Some(space) = ink.filter(|_| source_ctx.color & 255 != 0) else {
                    return;
                };
                let mask = if moved_source {
                    draw.clips
                        .add(scope.mask, Rounded::new(outer.rect(rect), corners))
                } else {
                    scope.mask
                };
                draw.scope = space.line_scope(outer, scope, mask);
                let run = l.runs[source];
                for f in line {
                    let start = draw.ids.len();
                    for (id, pos) in l.text.word_glyphs(run, f.first as usize..f.end as usize) {
                        draw.ids.push(id);
                        draw.positions.push(pos);
                    }
                    if start != draw.ids.len() {
                        let pos = [
                            f.baseline[0] + space.origin[0],
                            f.baseline[1] + space.origin[1],
                        ];
                        draw.push(Command::Glyphs {
                            pos,
                            clip: Some(space.clip.paint_clip(space.plane != 0)),
                            bounds: space.clip,
                            face: run.metrics.face,
                            size: run.metrics.size,
                            color: brush,
                            range: start..draw.ids.len(),
                        });
                        draw.rules(
                            source_ctx.rules,
                            brush,
                            pos,
                            f.width,
                            run.metrics.size,
                            space.clip.paint_clip(space.plane != 0),
                        );
                    }
                }
            };
            if owner != source
                && let Some(alpha) = e.attrs[source].opacity
            {
                for (_, r) in lines(span) {
                    decoration(draw, r, Phase::Under);
                }
                if alpha != 0 {
                    let inner = draw.begin_group(alpha);
                    for (line, r) in lines(span) {
                        glyphs(draw, line, r);
                    }
                    draw.end_group(inner, l.viewport);
                }
                for (_, r) in lines(span) {
                    decoration(draw, r, Phase::Border);
                }
            } else {
                for (line, r) in lines(span) {
                    decoration(draw, r, Phase::Under);
                    glyphs(draw, line, r);
                    decoration(draw, r, Phase::Border);
                }
            }
            if let Some(start) = group {
                draw.end_group(start, l.viewport);
            }
        }
    }
}
