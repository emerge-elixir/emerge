//! To add a supported wire shape, implement its metadata and layout, choose
//! shared or custom paint, and register the module below. Shapes do not own a
//! second child order, allocate plans, schedule passes or install output.
use super::{
    layers::{Constraint, Demand, FlowCx, Measuring, PlaceCx, ReflowCx},
    paint::{OwnPaint, PaintContext},
    text::Run,
    *,
};
mod fill;
mod payload;
use fill::FillShares;
use payload::{Children, NoData, OnlyChild, Payload, Utf8};

trait ShapeMeta {
    const TAG: u8;
    type Payload: Payload;
    type Paint: PaintBehaviour<Self::Payload>;
}
type PayloadOf<S> = <S as ShapeMeta>::Payload;
type Data<'a, S> = <PayloadOf<S> as Payload>::Data<'a>;
type PaintOf<S> = <S as ShapeMeta>::Paint;

/// Read the shape's declaration only when a pass needs it. In particular,
/// painting text reads its settled glyph run, not its UTF-8 a second time.
struct Mounted<'a, P> {
    elements: &'a Elements,
    key: Key,
    payload: std::marker::PhantomData<P>,
}
impl<'a, P: Payload> Mounted<'a, P> {
    fn new(elements: &'a Elements, key: Key) -> Self {
        Self {
            elements,
            key,
            payload: std::marker::PhantomData,
        }
    }
    fn data(&self) -> P::Data<'a> {
        P::get(self.elements, self.key)
    }
    fn children(&self) -> &'a [Key] {
        self.elements.children(self.key)
    }
    fn attrs(&self) -> &'a Attrs {
        &self.elements.attrs[self.key]
    }
}

/// Inputs to child width offers that can change even if our outer size is equal.
#[derive(Clone, Copy, PartialEq)]
pub(super) enum FlowInput {
    ChildWidth(Length),
    ChildFill,  // changed whether the child requests fill
    ChildSplit, // changed the policy or scale of an existing fill
    HorizontalGap,
    Order,
}
trait LayoutBehaviour: ShapeMeta {
    /// Which main-axis length this shape distributes among its children.
    const ALLOTS_WIDTH: bool = false;
    const ALLOTS_HEIGHT: bool = false;
    /// Each fill child receives the full placed inner height without a split.
    const OFFERS_INNER_HEIGHT: bool = false;
    fn measure(data: Data<'_, Self>, cx: &mut Measuring<'_>) -> Demand;
    fn auto_fills(_node: Mounted<'_, Self::Payload>) -> bool {
        false
    }
    fn reads_flow(_width: Length, _input: FlowInput) -> bool {
        false
    }
    fn offer_widths(node: Mounted<'_, Self::Payload>, cx: &mut FlowCx<'_>) {
        if cx.offer_children {
            for &child in node.children() {
                cx.offer(child, Constraint::Available(cx.inner))
            }
        }
    }
    fn reflow(node: Mounted<'_, Self::Payload>, cx: &mut ReflowCx<'_>) -> f32;
    fn place_children(node: Mounted<'_, Self::Payload>, cx: &mut PlaceCx<'_>) {
        cx.translate_children(node.children());
    }
}
trait PaintBehaviour<P: Payload> {
    const CLIP_CHILDREN: bool = false;
    fn context_attrs_changed(_a: &Attrs, _b: &Attrs) -> bool {
        false
    }
    fn reads_context(a: &Attrs, old: Option<PaintContext>, ctx: PaintContext) -> bool {
        a.background.is_some() && old.is_none_or(|p| p.frame != ctx.frame || p.clip != ctx.clip)
    }
    fn attrs_changed(a: &Attrs, b: &Attrs) -> bool {
        a.background != b.background
    }
    fn paint(node: Mounted<'_, P>, ctx: &PaintContext, _run: Option<&Run>) -> OwnPaint {
        OwnPaint::background(node.attrs(), ctx)
    }
}
struct BoxPaint;
impl<P: Payload> PaintBehaviour<P> for BoxPaint {}

// Intersect the inherited paint and hit clip with the container's frame.
// An oversized child keeps its layout size without spilling into a sibling's gap.
struct ClippedBoxPaint;
impl PaintBehaviour<Children> for ClippedBoxPaint {
    const CLIP_CHILDREN: bool = true;
}

// Generate wire decoding and exhaustive static dispatch from the same list.
macro_rules! define_shapes {
    ($($module:ident::$shape:ident),+ $(,)?) => {
        $(mod $module;)+
        #[derive(Clone, Copy, Debug, PartialEq, Eq)]
        #[repr(u8)]
        pub(super) enum Shape { $($shape = $module::$shape::TAG),+ }
        impl Shape {
            pub fn from_tag(tag: u8) -> Result<Self, String> {
                match tag {
                    $($module::$shape::TAG => Ok(Self::$shape),)+
                    _ => Err(format!("unsupported element {tag} in first slice")),
                }
            }
        }
        macro_rules! with_shape {
            ($value:expr, |$ty:ident| $body:expr) => {
                match $value { $(Shape::$shape => { type $ty = $module::$shape; $body }),+ }
            };
        }
    };
}
define_shapes!(
    row::Row,
    wrapped_row::WrappedRow,
    column::Column,
    el::El,
    text_leaf::Text,
    empty::Empty
);

impl Shape {
    pub fn check_children(self, count: usize) -> Result<(), String> {
        match with_shape!(self, |S| PayloadOf::<S>::MAX_CHILDREN) {
            max if count <= max => Ok(()),
            0 => Err("leaf has children".into()),
            1 => Err(format!("{self:?} supports at most one child")),
            max => Err(format!("{self:?} supports at most {max} children")),
        }
    }
    pub fn decode_attr(
        self,
        tag: u8,
        r: &mut wire::Reader<'_>,
        raw: &[u8],
        a: &mut Attrs,
    ) -> Result<(), String> {
        with_shape!(self, |S| PayloadOf::<S>::decode(tag, r, raw, a))
    }
    pub fn allots_width(self) -> bool {
        with_shape!(self, |S| S::ALLOTS_WIDTH)
    }
    pub fn offers_inner_height(self) -> bool {
        with_shape!(self, |S| S::OFFERS_INNER_HEIGHT)
    }
    pub fn allots_height(self) -> bool {
        with_shape!(self, |S| S::ALLOTS_HEIGHT)
    }
    pub fn uses_glyphs(self) -> bool {
        with_shape!(self, |S| PayloadOf::<S>::USES_GLYPHS)
    }
    pub fn measure(self, e: &Elements, k: Key, cx: &mut Measuring<'_>) -> Demand {
        with_shape!(self, |S| S::measure(PayloadOf::<S>::get(e, k), cx))
    }
    pub fn uses_offer(self, e: &Elements, k: Key) -> bool {
        match e.attrs[k].sizing.width {
            Length::Auto => with_shape!(self, |S| S::auto_fills(Mounted::new(e, k))),
            length => length.weight().is_some(),
        }
    }
    pub fn reads_flow(self, width: Length, input: FlowInput) -> bool {
        with_shape!(self, |S| S::reads_flow(width, input))
    }
    pub fn offer_widths(self, e: &Elements, k: Key, cx: &mut FlowCx<'_>) {
        with_shape!(self, |S| S::offer_widths(Mounted::new(e, k), cx))
    }
    pub fn reflow(self, e: &Elements, k: Key, cx: &mut ReflowCx<'_>) -> f32 {
        with_shape!(self, |S| S::reflow(Mounted::new(e, k), cx))
    }
    pub fn place_children(self, e: &Elements, k: Key, cx: &mut PlaceCx<'_>) {
        with_shape!(self, |S| S::place_children(Mounted::new(e, k), cx))
    }
    pub fn child_clip(self, inherited: Rect, frame: Rect) -> Rect {
        if with_shape!(self, |S| {
            <PaintOf<S> as PaintBehaviour<PayloadOf<S>>>::CLIP_CHILDREN
        }) {
            inherited.intersection(frame).unwrap_or_default()
        } else {
            inherited
        }
    }
    pub fn paint_context_attrs_changed(self, a: &Attrs, b: &Attrs) -> bool {
        with_shape!(self, |S| {
            <PaintOf<S> as PaintBehaviour<PayloadOf<S>>>::context_attrs_changed(a, b)
        })
    }
    pub fn reads_paint_context(
        self,
        a: &Attrs,
        old: Option<PaintContext>,
        ctx: PaintContext,
    ) -> bool {
        with_shape!(self, |S| {
            <PaintOf<S> as PaintBehaviour<PayloadOf<S>>>::reads_context(a, old, ctx)
        })
    }
    pub fn paint_attrs_changed(self, a: &Attrs, b: &Attrs) -> bool {
        with_shape!(self, |S| {
            <PaintOf<S> as PaintBehaviour<PayloadOf<S>>>::attrs_changed(a, b)
        })
    }
    pub fn paint(self, e: &Elements, k: Key, ctx: &PaintContext, run: Option<&Run>) -> OwnPaint {
        with_shape!(self, |S| PaintOf::<S>::paint(
            Mounted::<PayloadOf<S>>::new(e, k),
            ctx,
            run
        ))
    }
}

// A single row fits the tallest unallotted child. El uses the same fit;
// WrappedRow also uses it as the cross-axis basis before stacking lines.
fn line_height(children: &[Key], cx: &ReflowCx<'_>) -> f32 {
    if cx.fixed_basis {
        return cx.measured_height;
    }
    cx.box_height(children.iter().map(|&c| cx.child(c).h).fold(0., f32::max))
}

fn measure_line(children: &[Key], cx: &Measuring<'_>, wraps: bool) -> Demand {
    let content = children
        .iter()
        .map(|&c| cx.child(c))
        .fold(Demand::default(), |s, c| Demand {
            w: s.w + c.w,
            h: s.h.max(c.h),
            min_w: if wraps {
                s.min_w.max(c.min_w)
            } else {
                s.min_w + c.min_w
            },
        });
    let gaps = cx.gap[0] * children.len().saturating_sub(1) as f32;
    Demand {
        w: content.w + gaps,
        min_w: if wraps {
            content.min_w
        } else {
            content.min_w + gaps
        },
        ..content
    }
}
