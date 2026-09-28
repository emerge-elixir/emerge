//! Native-only dimension samples. A used box is not its parent's allocation charge.
//!
//! Declared lengths remain on the element and on the wire. Layout reads this
//! sidecar through `DimensionInput`; installing/removing it invalidates the same
//! retained caches as an ordinary dimension edit.

use super::{
    AlignX, Attrs, Element, ElementKind, ElementTree, IntrinsicSize, LayoutInsets, Length, NodeId,
    image_size_from_axis, is_content_length, length_allows_content_expansion, length_requests_fill,
    resolve_outer_intrinsic_length,
};
use super::{Frame, TreeInvalidation};
use std::sync::Arc;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Axis {
    Width,
    Height,
}

impl Axis {
    pub(super) fn index(self) -> usize {
        match self {
            Self::Width => 0,
            Self::Height => 1,
        }
    }

    fn length(self, attrs: &Attrs) -> Option<&Length> {
        match self {
            Self::Width => attrs.width.as_ref(),
            Self::Height => attrs.height.as_ref(),
        }
    }

    fn extent(self, frame: Frame) -> f32 {
        match self {
            Self::Width => frame.width,
            Self::Height => frame.height,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum PlacementRole {
    Pool,
    Wrapped,
    Float,
}

/// A planner fact is meaningful only in this retained parent and role.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AllocationScope {
    parent: NodeId,
    mounted_at: u64,
    axis: Axis,
    role: PlacementRole,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct AxisPolicy {
    pub fill_request: f32,
    pub content: f32,
    pub expansion: f32,
    pub automatic: f32,
    pub fixed_image_axis: f32,
    pub children_fill: f32,
}

impl AxisPolicy {
    fn declared(length: Option<&Length>) -> Self {
        Self {
            fill_request: bool_weight(length_requests_fill(length)),
            content: bool_weight(is_content_length(length)),
            expansion: bool_weight(length_allows_content_expansion(length)),
            automatic: bool_weight(matches!(length, None | Some(Length::Content))),
            fixed_image_axis: bool_weight(matches!(length, Some(Length::Px(_)))),
            children_fill: 0.0,
        }
    }

    fn interpolate(self, to: Self, t: f32) -> Self {
        Self {
            fill_request: blend(self.fill_request, to.fill_request, t),
            content: blend(self.content, to.content, t),
            expansion: blend(self.expansion, to.expansion, t),
            automatic: blend(self.automatic, to.automatic, t),
            fixed_image_axis: blend(self.fixed_image_axis, to.fixed_image_axis, t),
            children_fill: blend(self.children_fill, to.children_fill, t),
        }
    }

    pub(crate) fn valid(self) -> bool {
        [
            self.fill_request,
            self.content,
            self.expansion,
            self.automatic,
            self.fixed_image_axis,
            self.children_fill,
        ]
        .into_iter()
        .all(|value| value.is_finite() && (0.0..=1.0).contains(&value))
    }
}

/// Values are logical units: boxes in node space, charges/placement in parent space.
/// `parent_extent` is independent of both the debit and final rotated frame.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AxisFootprint {
    pub axis: Axis,
    pub intrinsic: f32,
    pub initial: f32,
    pub visible: f32,
    pub charge: f32,
    /// Planner placement size, before final resolution/rotation changes the frame.
    pub parent_extent: f32,
    pub scope: Option<AllocationScope>,
    pub policy: AxisPolicy,
}

impl AxisFootprint {
    /// Candidate for proving that a retained sample is ordinary pixel sizing.
    /// This is not layout: callers must verify the candidate with native layout
    /// before using pixel equivalence to authorize a continuation.
    pub(crate) fn pixel_candidate(
        self,
        tree: &ElementTree,
        id: NodeId,
        pixels: f32,
    ) -> Option<Self> {
        if self.scope != allocation_scope(tree, &id, self.axis)
            || !matches!(self.policy.children_fill, 0.0 | 1.0)
        {
            return None;
        }
        let scale = tree.get(&id)?.layout.dimension_facts.scale;
        let charge = match self.scope {
            Some(scope) => pixels * scale / tree.get(&scope.parent)?.layout.dimension_facts.scale,
            None => 0.0,
        };
        let candidate = Self {
            axis: self.axis,
            intrinsic: pixels,
            initial: pixels,
            visible: pixels,
            charge,
            parent_extent: charge,
            scope: self.scope,
            policy: AxisPolicy {
                children_fill: self.policy.children_fill,
                ..AxisPolicy::declared(Some(&Length::Px(pixels as f64)))
            },
        };
        candidate.valid().then_some(candidate)
    }

    /// Release compares every layout channel in logical units, not just the box.
    pub(crate) fn release_matches(self, other: Self) -> bool {
        self.valid()
            && other.valid()
            && self.axis == other.axis
            && self.scope == other.scope
            && [
                (self.intrinsic, other.intrinsic),
                (self.initial, other.initial),
                (self.visible, other.visible),
                (self.charge, other.charge),
                (self.parent_extent, other.parent_extent),
            ]
            .into_iter()
            .all(|(a, b)| (a - b).abs() <= 0.002)
            && [
                (self.policy.fill_request, other.policy.fill_request),
                (self.policy.content, other.policy.content),
                (self.policy.expansion, other.policy.expansion),
                (self.policy.automatic, other.policy.automatic),
                (self.policy.fixed_image_axis, other.policy.fixed_image_axis),
                (self.policy.children_fill, other.policy.children_fill),
            ]
            .into_iter()
            .all(|(a, b)| (a - b).abs() <= 0.000002)
    }
    /// Re-anchor all layout channels, not only the visible pixel value.
    pub fn interpolate(self, to: Self, progress: f32) -> Result<Self, &'static str> {
        if !self.valid() || !to.valid() || !progress.is_finite() {
            return Err("invalid dimension footprint");
        }
        if self.axis != to.axis || self.scope != to.scope {
            return Err("dimension allocation scope changed");
        }
        let t = progress.clamp(0.0, 1.0);
        Ok(Self {
            axis: self.axis,
            intrinsic: blend(self.intrinsic, to.intrinsic, t),
            initial: blend(self.initial, to.initial, t),
            visible: blend(self.visible, to.visible, t),
            charge: blend(self.charge, to.charge, t),
            parent_extent: blend(self.parent_extent, to.parent_extent, t),
            scope: self.scope,
            policy: self.policy.interpolate(to.policy, t),
        })
    }

    pub(crate) fn valid(self) -> bool {
        [
            self.intrinsic,
            self.initial,
            self.visible,
            self.charge,
            self.parent_extent,
        ]
        .into_iter()
        .all(|value| value.is_finite() && value >= 0.0)
            && self.policy.valid()
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct DimensionSamples {
    axes: [Option<AxisFootprint>; 2],
    /// Private query only: planner must derive new role obligations, not reuse a debit.
    transported: [bool; 2],
}

impl DimensionSamples {
    pub(crate) fn get(&self, axis: Axis) -> Option<AxisFootprint> {
        self.axes[axis.index()]
    }
}

/// Only facts unavailable from existing frames/attrs need persistent storage.
#[derive(Clone, Copy, Debug)]
pub struct DimensionFacts {
    pub(crate) initial: [f32; 2],
    pub(super) charge: [f32; 2],
    pub(super) parent_extent: [f32; 2],
    pub(super) children_fill: [f32; 2],
    pub(crate) scale: f32,
    pub(super) imposed_width: Option<f32>,
}

impl Default for DimensionFacts {
    fn default() -> Self {
        Self {
            initial: [0.0; 2],
            charge: [0.0; 2],
            parent_extent: [0.0; 2],
            children_fill: [0.0; 2],
            scale: 1.0,
            imposed_width: None,
        }
    }
}

#[derive(Clone, Copy)]
pub(super) struct DimensionInput<'a> {
    length: Option<&'a Length>,
    sample: Option<AxisFootprint>,
    scale: f32,
}

impl<'a> DimensionInput<'a> {
    pub(super) fn new(
        attrs: &'a Attrs,
        samples: Option<&DimensionSamples>,
        axis: Axis,
        scale: f32,
    ) -> Self {
        Self {
            length: axis.length(attrs),
            sample: samples.and_then(|s| s.get(axis)),
            scale,
        }
    }

    pub(super) fn element(element: &'a Element, axis: Axis) -> Self {
        let imposed = axis == Axis::Width && element.layout.dimension_facts.imposed_width.is_some();
        let mut input = Self::new(
            &element.layout.effective,
            if imposed {
                None
            } else {
                element.layout.dimension_samples.as_deref()
            },
            axis,
            element.layout.dimension_facts.scale,
        );
        if imposed {
            input.length = Some(&Length::Px(0.0));
        } // policy only; size is a resolve input
        input
    }

    pub(super) fn sample(self) -> Option<AxisFootprint> {
        self.sample
    }

    pub(super) fn policy(self) -> AxisPolicy {
        self.sample
            .map_or_else(|| AxisPolicy::declared(self.length), |s| s.policy)
    }
    pub(super) fn intrinsic(self, normal: f32) -> f32 {
        self.sample.map_or(normal, |s| s.intrinsic * self.scale)
    }
    pub(super) fn initial(self, normal: f32) -> f32 {
        self.sample.map_or(normal, |s| s.initial * self.scale)
    }
    pub(super) fn visible(self, normal: f32) -> f32 {
        self.sample.map_or(normal, |s| s.visible * self.scale)
    }
    pub(super) fn children_fill(self, normal: f32) -> f32 {
        self.sample.map_or(normal, |s| s.policy.children_fill)
    }
}

pub(super) fn bool_weight(value: bool) -> f32 {
    if value { 1.0 } else { 0.0 }
}

pub(super) fn blend(from: f32, to: f32, t: f32) -> f32 {
    // Exact boundaries matter for cache keys, policy branches and handoff.
    if t == 0.0 {
        from
    } else if t == 1.0 {
        to
    } else {
        from + (to - from) * t
    }
}

pub(crate) fn declared_allocation_scope(
    tree: &ElementTree,
    id: &NodeId,
    axis: Axis,
) -> Option<AllocationScope> {
    allocation_scope_using(tree, id, axis, true)
}
fn allocation_scope(tree: &ElementTree, id: &NodeId, axis: Axis) -> Option<AllocationScope> {
    allocation_scope_using(tree, id, axis, false)
}
fn allocation_scope_using(
    tree: &ElementTree,
    id: &NodeId,
    axis: Axis,
    declared: bool,
) -> Option<AllocationScope> {
    use super::super::element::ParentLink;
    if tree.root_id() == Some(*id) {
        return None;
    }
    let ix = tree.ix_of(id)?;
    let ParentLink::Child { parent } = tree.parent_link_of(ix)? else {
        return None;
    };
    let parent = tree.get_ix(parent)?;
    let role = match (parent.spec.kind, axis) {
        (ElementKind::Row, Axis::Width) | (ElementKind::Column, Axis::Height) => {
            PlacementRole::Pool
        }
        (ElementKind::WrappedRow, Axis::Width) => PlacementRole::Wrapped,
        (ElementKind::Paragraph | ElementKind::TextColumn, _)
            if tree.get(id).is_some_and(|child| {
                matches!(
                    if declared {
                        child.spec.declared.align_x
                    } else {
                        child.layout.effective.align_x
                    },
                    Some(AlignX::Left | AlignX::Right)
                )
            }) =>
        {
            PlacementRole::Float
        }
        _ => return None,
    };
    Some(AllocationScope {
        parent: parent.id,
        mounted_at: parent.lifecycle.mounted_at_revision,
        axis,
        role,
    })
}

/// Capture after normal layout, before updates overwrite the presentation inputs.
pub(crate) fn capture_axis(tree: &ElementTree, id: &NodeId, axis: Axis) -> Option<AxisFootprint> {
    let element = tree.get(id)?;
    let input = DimensionInput::element(element, axis);
    let scale = element.layout.dimension_facts.scale;
    let frame = element.layout.render_frame.or(element.layout.frame)?;
    let measured = element
        .layout
        .measured_render_frame
        .or(element.layout.measured_frame)?;
    let scope = allocation_scope(tree, id, axis);
    let parent_scale = scope
        .and_then(|scope| tree.get(&scope.parent))
        .map_or(scale, |parent| parent.layout.dimension_facts.scale);
    let mut policy = input.policy();
    policy.children_fill = element.layout.dimension_facts.children_fill[axis.index()];
    Some(AxisFootprint {
        axis,
        intrinsic: axis.extent(measured) / scale,
        initial: element.layout.dimension_facts.initial[axis.index()] / scale,
        visible: axis.extent(frame) / scale,
        charge: if scope.is_some() {
            element.layout.dimension_facts.charge[axis.index()] / parent_scale
        } else {
            0.0
        },
        parent_extent: if scope.is_some() {
            element.layout.dimension_facts.parent_extent[axis.index()] / parent_scale
        } else {
            0.0
        },
        scope,
        policy,
    })
}

/// Ghost attrs are captured pixels, while live samples use their node/parent
/// logical spaces. Bake those units and remap only parents cloned into the ghost;
/// a surviving external allocation pool keeps its original charge units.
pub(crate) fn capture_ghost_samples(
    tree: &ElementTree,
    old: &Element,
    ids: &std::collections::HashMap<NodeId, NodeId>,
) -> Option<Arc<DimensionSamples>> {
    let samples = old.layout.dimension_samples.as_ref()?;
    let axes = [Axis::Width, Axis::Height].map(|axis| {
        samples.get(axis)?;
        let mut sample = capture_axis(tree, &old.id, axis)?;
        let own_scale = old.layout.dimension_facts.scale;
        sample.intrinsic *= own_scale;
        sample.initial *= own_scale;
        sample.visible *= own_scale;
        if let Some(scope) = sample.scope.as_mut()
            && let Some(parent) = ids.get(&scope.parent)
        {
            let parent_scale = tree.get(&scope.parent)?.layout.dimension_facts.scale;
            sample.charge *= parent_scale;
            sample.parent_extent *= parent_scale;
            scope.parent = *parent;
        }
        Some(sample)
    });
    Some(Arc::new(DimensionSamples {
        axes,
        ..Default::default()
    }))
}

/// Validate a staged group sample without mutating live layout or caches.
pub(crate) fn validate_prepared_axis_sample(
    tree: &ElementTree,
    id: &NodeId,
    axis: Axis,
    sample: Option<AxisFootprint>,
    prepared_role: bool,
) -> Result<(), &'static str> {
    if let Some(sample) = sample {
        if !sample.valid() || sample.axis != axis {
            return Err("invalid dimension footprint");
        }
        if sample.scope != allocation_scope(tree, id, axis)
            && !(prepared_role && sample.scope == declared_allocation_scope(tree, id, axis))
        {
            return Err("dimension allocation scope changed");
        }
    }
    tree.get(id).ok_or("dimension node not found")?;
    Ok(())
}

/// Ordinary full-footprint admission. This is not an encoded attr or a NIF API.
pub(crate) fn set_axis_sample(
    tree: &mut ElementTree,
    id: &NodeId,
    axis: Axis,
    sample: Option<AxisFootprint>,
) -> Result<(), &'static str> {
    set_prepared_axis_sample(tree, id, axis, sample, false)
}
pub(crate) fn set_prepared_axis_sample(
    tree: &mut ElementTree,
    id: &NodeId,
    axis: Axis,
    sample: Option<AxisFootprint>,
    prepared_role: bool,
) -> Result<(), &'static str> {
    validate_prepared_axis_sample(tree, id, axis, sample, prepared_role)?;
    let element = tree.get_mut(id).ok_or("dimension node not found")?;
    if element
        .layout
        .dimension_samples
        .as_deref()
        .and_then(|s| s.get(axis))
        == sample
        && !element
            .layout
            .dimension_samples
            .as_deref()
            .is_some_and(|s| s.transported[axis.index()])
    {
        return Ok(());
    }
    element.layout.intrinsic_measure_cache = None;
    let mut samples = element
        .layout
        .dimension_samples
        .as_deref()
        .cloned()
        .unwrap_or_default();
    samples.axes[axis.index()] = sample;
    samples.transported[axis.index()] = false;
    element.layout.dimension_samples = samples
        .axes
        .iter()
        .any(Option::is_some)
        .then(|| Arc::new(samples));
    // A same-pixel charge/policy change still changes layout. Include samples in
    // cache keys as well: dirty descendant reuse is allowed to compare keys.
    tree.mark_measure_dirty_for_invalidation(id, TreeInvalidation::Measure);
    Ok(())
}

/// Explicit native source conversion, never installed on the live tree. Own
/// channels retain physical units; destination layout derives charge/placement.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct TransportSource {
    pub footprint: AxisFootprint,
    pub scale: f32,
}
pub(crate) fn set_transport_sample(
    tree: &mut ElementTree,
    id: &NodeId,
    axis: Axis,
    source: TransportSource,
) -> Result<(), &'static str> {
    if !source.footprint.valid()
        || source.footprint.axis != axis
        || !source.scale.is_finite()
        || source.scale <= 0.0
    {
        return Err("invalid dimension transport source");
    }
    let scope = allocation_scope(tree, id, axis);
    let node = tree.get(id).ok_or("dimension node not found")?;
    let ratio = source.scale / node.layout.dimension_facts.scale;
    let fp = AxisFootprint {
        intrinsic: source.footprint.intrinsic * ratio,
        initial: source.footprint.initial * ratio,
        visible: source.footprint.visible * ratio,
        charge: 0.0,
        parent_extent: 0.0,
        scope,
        ..source.footprint
    };
    set_axis_sample(tree, id, axis, Some(fp))?;
    let node = tree.get_mut(id).ok_or("dimension node not found")?;
    let samples = Arc::make_mut(
        node.layout
            .dimension_samples
            .as_mut()
            .ok_or("missing dimension transport")?,
    );
    samples.transported[axis.index()] = true;
    Ok(())
}

pub(super) fn sample_allocation(tree: &ElementTree, id: &NodeId, axis: Axis) -> Option<(f32, f32)> {
    let element = tree.get(id)?;
    let samples = element.layout.dimension_samples.as_deref()?;
    if samples.transported[axis.index()] {
        return None;
    }
    let sample = samples.get(axis)?;
    let scope = sample.scope?;
    if scope.role != PlacementRole::Pool || Some(scope) != allocation_scope(tree, id, axis) {
        return None;
    }
    let scale = tree.get(&scope.parent)?.layout.dimension_facts.scale;
    Some((sample.charge * scale, sample.parent_extent * scale))
}

pub type DimensionCacheKey = Option<(Arc<DimensionSamples>, f32)>;

pub(super) fn cache_key(element: &Element) -> DimensionCacheKey {
    element
        .layout
        .dimension_samples
        .as_ref()
        .map(|samples| (Arc::clone(samples), element.layout.dimension_facts.scale))
}

pub(super) fn prepare_scale(element: &mut Element, scale: f32) {
    element.layout.dimension_facts.imposed_width = None;
    if element.layout.dimension_samples.is_some() && element.layout.dimension_facts.scale != scale {
        element.layout.intrinsic_measure_cache = None;
    }
    element.layout.dimension_facts.scale = scale;
}

/// Measurement's bare-pixel image policy differs from resolution's fill policy.
/// Blend candidates at their original stages; never synthesize an image Size attr.
pub(super) fn measure_image(
    element: &Element,
    source_size: Option<(f64, f64)>,
    insets: LayoutInsets,
    normal: IntrinsicSize,
) -> IntrinsicSize {
    if element.spec.kind != ElementKind::Image || element.layout.dimension_samples.is_none() {
        return normal;
    }
    let attrs = &element.layout.effective;
    let width = DimensionInput::element(element, Axis::Width);
    let height = DimensionInput::element(element, Axis::Height);
    let (iw, ih) = source_size.unwrap_or(if attrs.image_src.is_some() {
        (64.0, 64.0)
    } else {
        (0.0, 0.0)
    });
    let base = IntrinsicSize {
        width: resolve_outer_intrinsic_length(attrs.width.as_ref(), iw as f32, insets.horizontal()),
        height: resolve_outer_intrinsic_length(attrs.height.as_ref(), ih as f32, insets.vertical()),
    };
    let inferred_height = image_size_from_axis(
        source_size,
        insets,
        Some(width.initial(base.width) as f64),
        None,
    );
    let inferred_width = image_size_from_axis(
        source_size,
        insets,
        None,
        Some(height.initial(base.height) as f64),
    );
    IntrinsicSize {
        width: inferred_width.map_or(base.width, |size| {
            blend(
                base.width,
                size.width,
                height.policy().fixed_image_axis * width.policy().automatic,
            )
        }),
        height: inferred_height.map_or(base.height, |size| {
            blend(
                base.height,
                size.height,
                width.policy().fixed_image_axis * height.policy().automatic,
            )
        }),
    }
}

pub(super) fn sample_placement(
    tree: &ElementTree,
    id: &NodeId,
    axis: Axis,
    role: PlacementRole,
) -> Option<f32> {
    let samples = tree.get(id)?.layout.dimension_samples.as_deref()?;
    if samples.transported[axis.index()] {
        return None;
    }
    let sample = samples.get(axis)?;
    let scope = sample.scope?;
    if scope.role != role || Some(scope) != allocation_scope(tree, id, axis) {
        return None;
    }
    Some(sample.parent_extent * tree.get(&scope.parent)?.layout.dimension_facts.scale)
}
