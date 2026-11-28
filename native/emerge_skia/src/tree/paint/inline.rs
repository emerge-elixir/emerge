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
    fn rect(self, r: Rect) -> Rect {
        Rect {
            x: r.x + self.origin[0],
            y: r.y + self.origin[1],
            ..r
        }
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
#[derive(Clone, Copy)]
pub(super) struct Span {
    paragraph: Key,
    first: u32,
    end: u32,
}
impl Span {
    pub(super) fn at(l: &Layers, paragraph: Key, owner: Key) -> Self {
        let words = l.paragraphs.words_at(owner);
        Self {
            paragraph,
            first: words.start,
            end: words.end,
        }
    }
}
#[derive(Clone, Copy, PartialEq)]
pub(super) enum Part {
    Whole,
    Backdrop,
    Content,
}
impl Output {
    /// Match declarations to aligned fragments once, including empty owners.
    /// The stack pops them in source order, after ordinary float painting.
    pub(super) fn inline_visits(
        &mut self,
        e: &Elements,
        k: Key,
        l: &Layers,
        side: usize,
        scope: Scope,
        branch: Branch,
    ) -> bool {
        let park = l.parts[k].ports[side].exports;
        for &owner in l.scroll.children_rev(e, k) {
            #[cfg(test)]
            {
                self.packed_children[side] += 1;
            }
            if e.float_side(owner).is_some() {
                if park && l.parts[owner].ports[side].exports {
                    self.stack
                        .push((owner, Visit::ReleaseFloat(bands::Chain::default()), scope));
                }
            } else {
                let words = l.paragraphs.words_at(owner);
                if !words.is_empty() || !e.order(owner).tags.is_empty() {
                    let span = Span {
                        paragraph: k,
                        first: words.start,
                        end: words.end,
                    };
                    self.stack.push((owner, Visit::Inline(span, branch), scope));
                }
            }
        }
        park
    }
    pub(super) fn inline_owner(
        &mut self,
        e: &Elements,
        owner: Key,
        l: &Layers,
        side: usize,
        scope: Scope,
        (span, mut branch): (Span, Branch),
    ) {
        let a = &e.attrs[owner];
        if side == 0 && a.opacity == Some(0) {
            return;
        }
        let ctx = l.painted(owner);
        let (planes, clips) = if side == 0 {
            (&mut self.draw.build.planes, &mut self.draw.build.clips)
        } else {
            (&mut self.hits.build.planes, &mut self.hits.build.clips)
        };
        let Some(id) = planes.branch(scope.plane, scope.mask, a, ctx, side == 1) else {
            return;
        };
        let port = l.parts[owner].ports[side];
        let inner = a.decor.rounded_inner(ctx.frame, ctx.scale);
        if port.exports {
            if branch
                .pose(
                    planes,
                    Coords {
                        plane: scope.plane,
                        mask: scope.mask,
                    },
                    id,
                    a,
                    ctx,
                    side,
                )
                .is_none()
            {
                return;
            }
            if a.clip_nearby {
                branch.escape.mask = clips.add(branch.escape.mask, Some(ctx.inner_guard(a, inner)));
            }
        }
        let behind = e.order(owner).tags.contains(&Slot::Behind);
        let scope = Scope { plane: id, ..scope };
        let local = if behind {
            Scope {
                mask: clips.add(scope.mask, inner),
                ..scope
            }
        } else {
            scope
        };
        if side == 0
            && let Some(alpha) = a.opacity
        {
            if port.exports {
                branch.group = self.ancestry.child(branch.group, alpha);
            }
            let start = self.draw.build.begin_group(alpha);
            self.stack.push((owner, Visit::Opacity(start), scope));
        }
        self.submit_escapes(e, owner, l, side, &mut branch);
        self.inline_ink(
            e,
            owner,
            l,
            side,
            scope,
            (
                span,
                branch.wheel,
                if behind { Part::Backdrop } else { Part::Whole },
            ),
        );
        if behind {
            self.stack
                .push((owner, Visit::Ink(span, branch.wheel), scope));
            self.behinds(e, owner, l, side, local, branch);
        }
    }
    pub(super) fn inline_ink(
        &mut self,
        e: &Elements,
        owner: Key,
        l: &Layers,
        side: usize,
        scope: Scope,
        (range, wheel, part): (Span, u32, Part),
    ) {
        let k = range.paragraph;
        let span = &l.paragraphs.fragments(k)[range.first as usize..range.end as usize];
        let Some(first) = span.first() else { return };
        let source = first.source;
        let ctx = self
            .inline_base
            .filter(|(s, _)| s.paragraph == k)
            .map_or_else(|| l.painted(k), |(_, ctx)| ctx);
        let owner_ctx = l.painted(owner);
        let outer = Space {
            plane: scope.plane,
            clip: owner_ctx.clip,
            origin: [
                ctx.frame.x + (owner_ctx.offset[0] - ctx.offset[0]),
                ctx.frame.y + (owner_ctx.offset[1] - ctx.offset[1]),
            ],
        };
        let planes = if side == 0 {
            &mut self.draw.build.planes
        } else {
            &mut self.hits.build.planes
        };
        let ink = if part == Part::Backdrop {
            None
        } else if source == owner {
            Some(outer)
        } else {
            outer.child(
                planes,
                scope.mask,
                &e.attrs[source],
                l.painted(source),
                ctx,
                side == 1,
            )
        };
        // A separately layout-turned Text occupies one projected slot
        // inside its El. The El's hit, decoration and mask surround that
        // slot in the El's axes, not in the axes of the Text's glyphs.
        let owner_rect = |r| {
            if source != owner && e.attrs[source].rotation.mixes_axes() {
                owner_ctx.frame
            } else {
                outer.rect(r)
            }
        };
        let corners = e.attrs[owner].decor.radii(owner_ctx.scale);
        if side == 1 {
            let hits = &mut self.hits.build;
            if !((part != Part::Content && e.attrs[owner].actions.any())
                || (ink.is_some() && e.attrs[source].actions.any()))
            {
                return;
            }
            for (_, local) in lines(span) {
                let wrapper = owner_rect(local);
                let mask = hits.clips.add(scope.mask, Rounded::new(wrapper, corners));
                for (target, space) in [
                    (owner, (part != Part::Content).then_some(outer)),
                    (source, ink.filter(|_| source != owner)),
                ] {
                    let Some(space) = space.filter(|_| e.attrs[target].actions.any()) else {
                        continue;
                    };
                    let rect = if target == owner {
                        wrapper
                    } else {
                        space.rect(local)
                    };
                    if let Some(area) = rect.intersection(space.clip) {
                        let frame = l.painted(target).frame;
                        hits.items.push(Hit {
                            kind: HitKind::Listener,
                            actions: e.attrs[target].actions,
                            wheel,
                            scope: space.line_scope(outer, scope, mask),
                            key: Attachment(target, e.slots[target].attachment),
                            id: e.slots[target].id,
                            rect: area,
                            origin: [frame.x, frame.y],
                            scale: l.painted(target).scale,
                        });
                    }
                }
            }
            return;
        }
        let draw = &mut self.draw.build;
        let material = (owner != source && e.attrs[owner].decor.active())
            .then(|| self.index.get(draw, e, owner));
        let decoration = |draw: &mut Drawing, rect, phase| {
            draw.scope = Scope {
                plane: outer.plane,
                ..scope
            };
            let rect = owner_rect(rect);
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
        if part == Part::Backdrop {
            for (_, r) in lines(span) {
                decoration(draw, r, Phase::Under);
            }
            return;
        }
        let source_ctx = l.painted(source);
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
                    .add(scope.mask, Rounded::new(owner_rect(rect), corners))
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
            if part == Part::Whole {
                for (_, r) in lines(span) {
                    decoration(draw, r, Phase::Under);
                }
            }
            if alpha != 0 {
                let inner = draw.begin_group(alpha);
                for (line, r) in lines(span) {
                    glyphs(draw, line, r);
                }
                draw.end_group(inner);
            }
            for (_, r) in lines(span) {
                decoration(draw, r, Phase::Border);
            }
        } else {
            for (line, r) in lines(span) {
                if part == Part::Whole {
                    decoration(draw, r, Phase::Under);
                }
                glyphs(draw, line, r);
                decoration(draw, r, Phase::Border);
            }
        }
    }
}
