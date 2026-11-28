//! Ordinarily the enclosing clip is entered once at a child reference. An
//! opacity group needs those clips on its primitives, not its composite. An
//! outer shadow keeps only active-axis strips and escape guards. Keep enclosing
//! scopes on the call stack, in their establishing coordinate systems. No
//! commands or geometry are assembled during replay.
use super::*;
use skia_safe::{Canvas, M44};
#[derive(Clone, Copy)]
pub(in crate::tree::paint) struct Replay<'a> {
    pub tables: &'a Tables,
    pub children: Option<Children<'a>>,
    outer: Option<&'a Boundary<'a>>,
    pub limit: Option<pose::Aperture>,
}
struct Boundary<'a> {
    parent: Option<&'a Boundary<'a>>,
    draw: &'a Drawing,
    tables: &'a Tables,
    scope: Scope,
    limit: Option<pose::Aperture>,
    matrix: M44,
}
impl Boundary<'_> {
    fn apply(&self, canvas: &Canvas, shadow: bool) {
        if let Some(parent) = self.parent {
            parent.apply(canvas, shadow);
        }
        canvas.set_matrix(&self.matrix);
        let scope = self.scope;
        let d = self.draw;
        if shadow {
            scope.guard.clip(d, canvas, self.tables);
            pose::ShadowGuard::apply(scope.shadow, d, canvas, self.tables);
        } else {
            let p = d.planes.get(scope.plane);
            if let Some(p) = p {
                d.planes
                    .enter(p, canvas, &d.clips, scope.before, self.tables, self.limit);
            }
            d.clips
                .apply(scope.mask, p.map_or(OPEN, |p| p.stop), canvas, 0, (0., 0.));
        }
    }
}
impl<'a> Replay<'a> {
    pub fn root(tables: &'a Tables) -> Self {
        Self {
            tables,
            children: None,
            outer: None,
            limit: None,
        }
    }
    pub(super) fn translated(self, delta: [f32; 2]) -> Self {
        Self {
            limit: self.limit.map(|r| r.shifted(-delta[0], -delta[1])),
            ..self
        }
    }
    pub(super) fn with_children(self, children: Children<'a>) -> Self {
        Self {
            children: Some(children),
            ..self
        }
    }
    pub(in crate::tree::paint) fn apply<'c>(
        &self,
        canvas: &'c Canvas,
        shadow: bool,
    ) -> Option<skia_safe::canvas::AutoRestoredCanvas<'c>> {
        self.outer.map(|b| {
            let save = skia_safe::AutoCanvasRestore::guard(canvas, true);
            let m = canvas.local_to_device();
            b.apply(canvas, shadow);
            canvas.set_matrix(&m);
            save
        })
    }
    #[allow(clippy::too_many_arguments)]
    pub(in crate::tree::paint) fn enter(
        self,
        canvas: &Canvas,
        d: &Drawing,
        scope: Scope,
        clip: pose::Aperture,
        defer_clips: bool,
        call: Call,
        glyphs: &mut impl FnMut(&Canvas, GlyphPaintRun, &Paint) -> Result<(), String>,
    ) -> Result<(), String> {
        let _save = skia_safe::AutoCanvasRestore::guard(canvas, true);
        let boundary = Boundary {
            parent: self.outer,
            scope,
            draw: d,
            tables: self.tables,
            limit: self.limit,
            matrix: canvas.local_to_device(),
        };
        let outer = if defer_clips {
            Some(&boundary)
        } else {
            boundary.apply(canvas, false);
            canvas.set_matrix(&boundary.matrix);
            None
        };
        if let Some(p) = d.planes.installed(scope.plane, self.tables, 0) {
            canvas.concat(&p.matrix);
        }
        let pass = matches!(call, Call::Owner(_))
            || matches!(call, Call::Segment(..)) && clip.body == Rect::UNBOUNDED;
        let limit = clip.intersect(d.planes.logical(scope.plane, self.limit, self.tables), pass);
        let replay = Replay {
            outer,
            limit: Some(limit),
            ..self
        };
        match call {
            Call::Segment(k, s) => self
                .tables
                .get(k, 0)
                .expect("sealed band order")
                .segment(0, s)
                .draw(canvas, glyphs, replay),
            Call::Window(k) => self
                .tables
                .get(k, 0)
                .expect("sealed content")
                .draw(canvas, glyphs, replay),
            Call::Children => self
                .children
                .expect("sealed child order")
                .draw(canvas, glyphs, replay),
            Call::Owner(k) => self
                .tables
                .owner(k, 0)
                .expect("sealed band participant")
                .draw(canvas, glyphs, replay),
        }
    }
}
