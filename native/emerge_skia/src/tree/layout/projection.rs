//! Renderer-owned, layout-only endpoint projections. No animation clocks, render
//! traversal, asset loading, live rollback, or per-track tree copies live here.
//!
//! Callers bump `model_epoch` only for layout-model changes, provide immutable
//! projection/context inputs, and release the workspace when mixed tracks settle.
//! Runtime admission and boundary scheduling are separate from this evaluator.

use super::super::attrs::ImageSource;
use super::super::element::NodeRuntime;
use super::*;
use dimensions::{AxisFootprint, capture_axis, set_axis_sample};
use std::collections::HashSet;
use std::sync::Arc;

pub type ImageDimensions = HashMap<ImageSource, (u32, u32)>;
pub type Endpoint = (NodeId, Axis);

#[derive(Clone, Debug, Default, PartialEq)]
pub struct NodeProjection {
    /// Sparse declaration preimage for a native boundary witness. Replacement,
    /// unlike an overlay, also restores properties that were originally absent.
    pub model: Option<Arc<Attrs>>,
    pub attrs: Attrs,
    pub width: Option<AxisFootprint>,
    pub height: Option<AxisFootprint>,
    pub(crate) transport: Option<Arc<[Option<dimensions::TransportSource>; 2]>>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Projection {
    pub nodes: HashMap<NodeId, NodeProjection>,
}

/// Current mutable layout inputs, not a previous query's output. Capture only
/// known affected nodes; the evaluator never scans the live tree on warm queries.
#[derive(Clone, Debug, PartialEq)]
pub struct QuerySeed {
    pub runtime: NodeRuntime,
    pub scroll: [f32; 4],
}

impl QuerySeed {
    pub fn capture(element: &Element) -> Self {
        Self {
            runtime: element.runtime.clone(),
            scroll: [
                element.layout.scroll_x,
                element.layout.scroll_y,
                element.layout.scroll_x_max,
                element.layout.scroll_y_max,
            ],
        }
    }

    fn restore(&self, tree: &mut ElementTree, id: &NodeId) -> bool {
        let Some(element) = tree.get_mut(id) else {
            return false;
        };
        let runtime_changed = element.runtime != self.runtime;
        if runtime_changed {
            element.runtime = self.runtime.clone();
        }
        let scroll_changed = restore_scroll(element, self.scroll);
        if runtime_changed || scroll_changed {
            tree.mark_layout_dirty_for_invalidation(
                id,
                if runtime_changed {
                    TreeInvalidation::Measure
                } else {
                    TreeInvalidation::Resolve
                },
            );
        }
        runtime_changed
    }
}

/// Immutable inputs make identity-based last-query caching safe: retained Arcs
/// prevent in-place mutation. Font/metric providers must be stable within an epoch.
#[derive(Clone, Debug, PartialEq)]
pub struct QueryContext {
    pub constraint: Constraint,
    pub scale: f32,
    pub inherited: FontContext,
    pub metrics_epoch: u64,
    pub fonts: Option<crate::renderer::FontSnapshot>,
    pub images: Arc<ImageDimensions>,
    /// Sparse replacements over the model snapshot's runtime seeds, not deltas.
    pub seeds: HashMap<NodeId, QuerySeed>,
}

impl QueryContext {
    /// A viewport event changes units/constraints, not the model, metric provider,
    /// media facts or interaction seeds. Model identity is checked by the caller.
    #[cfg(test)]
    pub(crate) fn viewport_change_from(&self, old: &Self) -> bool {
        (self.constraint != old.constraint || self.scale != old.scale) && self.same_environment(old)
    }
    /// Historical evaluation is possible for recorded inputs, not an opaque
    /// custom metric-version change. Native fonts carry their actual snapshot.
    /// Equal seed membership prevents falling back to new live runtime values
    /// for a node whose previous state was never captured.
    pub(crate) fn replayable_change_from(&self, old: &Self) -> bool {
        self != old
            && self.seeds.len() == old.seeds.len()
            && self.seeds.keys().all(|id| old.seeds.contains_key(id))
            && (self.metrics_epoch == old.metrics_epoch && self.fonts == old.fonts
                || self.fonts.is_some() && old.fonts.is_some())
    }
    /// Unlike replay against a new workspace baseline, an opaque native receipt
    /// already records the old result including seed absence/membership. This is
    /// only for receipt-backed evidence, not permission to replay unknown seeds.
    pub(crate) fn recorded_change_from(&self, old: &Self) -> bool {
        self != old
            && (self.metrics_epoch == old.metrics_epoch && self.fonts == old.fonts
                || self.fonts.is_some() && old.fonts.is_some())
    }
    pub(crate) fn same_seed_membership(&self, old: &Self) -> bool {
        self.seeds.len() == old.seeds.len()
            && self.seeds.keys().all(|id| old.seeds.contains_key(id))
    }
    pub(crate) fn same_environment(&self, old: &Self) -> bool {
        self.inherited == old.inherited
            && self.metrics_epoch == old.metrics_epoch
            && self.fonts == old.fonts
            && self.images == old.images
            && self.seeds == old.seeds
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ProjectionStats {
    pub model_copies: u64,
    pub copied_nodes: u64,
    pub source_slots: u64,
    pub layout_queries: u64,
    pub cache_hits: u64,
    pub seed_reset_visits: u64,
    pub metric_invalidations: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ProjectionError {
    #[cfg(test)]
    InjectedQuery,
    UnknownNode(NodeId),
    InvalidContext,
    InvalidOwnership(NodeId),
    UnsupportedLength(NodeId),
    InvalidTiming(NodeId),
    ConflictingDimension(NodeId, Axis),
    InvalidSample(&'static str),
    MissingLayout(NodeId, Axis),
    ReleaseMismatch(NodeId, Axis),
    StalePreparation,
    GenerationExhausted,
}

/// Opaque native evaluation evidence. Created only after a successful resolver
/// query, never from a retained animation target. Values contain no clocks/history.
#[derive(Debug)]
pub(crate) struct NativeGoalReceipt {
    model: u64,
    root: Option<(NodeId, u64)>,
    projection: Arc<Projection>,
    context: Arc<QueryContext>,
    mounts: HashMap<NodeId, u64>,
    values: HashMap<Endpoint, AxisFootprint>,
}
impl std::ops::Deref for NativeGoalReceipt {
    type Target = HashMap<Endpoint, AxisFootprint>;
    fn deref(&self) -> &Self::Target {
        &self.values
    }
}
impl NativeGoalReceipt {
    pub(crate) fn native(
        &self,
        model: u64,
        projection: &Arc<Projection>,
        context: &Arc<QueryContext>,
        key: Endpoint,
        mount: u64,
    ) -> Result<AxisFootprint, ProjectionError> {
        if self.model != model
            || self.root.is_none()
            || !Arc::ptr_eq(&self.projection, projection)
            || !Arc::ptr_eq(&self.context, context)
            || self.mounts.get(&key.0) != Some(&mount)
        {
            return Err(ProjectionError::StalePreparation);
        }
        self.values
            .get(&key)
            .copied()
            .ok_or(ProjectionError::MissingLayout(key.0, key.1))
    }
}

struct Workspace {
    tree: ElementTree,
    model_epoch: u64,
    root_identity: Option<(NodeId, u64)>,
    baseline_seeds: HashMap<NodeId, QuerySeed>,
    temporary_seeds: HashMap<NodeId, QuerySeed>,
    previous: Arc<Projection>,
    context: Option<Arc<QueryContext>>,
    cache: Option<(Arc<Projection>, Arc<QueryContext>)>,
}

/// One workspace per owning runtime, never shared across renderers. This layer
/// caches the last immutable request; running tracks own their endpoint caches.
#[derive(Default)]
pub struct EndpointResolver {
    workspace: Option<Workspace>,
    stats: ProjectionStats,
}

impl EndpointResolver {
    #[cfg(feature = "bench-diagnostics")]
    pub(crate) fn retained_projections(&self) -> impl Iterator<Item = &Arc<Projection>> {
        self.workspace.iter().flat_map(|workspace| {
            std::iter::once(&workspace.previous)
                .chain(workspace.cache.iter().map(|(projection, _)| projection))
        })
    }

    pub fn stats(&self) -> ProjectionStats {
        self.stats
    }

    pub fn release(&mut self) {
        self.workspace = None;
    }

    pub(crate) fn resolve_certified<M: TextMeasurer>(
        &mut self,
        source: &ElementTree,
        model: u64,
        projection: Arc<Projection>,
        context: Arc<QueryContext>,
        endpoints: &[Endpoint],
        measurer: &M,
    ) -> Result<Arc<NativeGoalReceipt>, ProjectionError> {
        let values = self.resolve(
            source,
            model,
            Arc::clone(&projection),
            Arc::clone(&context),
            endpoints,
            measurer,
        )?;
        Ok(Arc::new(NativeGoalReceipt {
            model,
            root: source.root_id().and_then(|id| {
                source
                    .get(&id)
                    .map(|node| (id, node.lifecycle.mounted_at_revision))
            }),
            mounts: endpoints
                .iter()
                .filter_map(|(id, _)| {
                    source
                        .get(id)
                        .map(|node| (*id, node.lifecycle.mounted_at_revision))
                })
                .collect(),
            projection,
            context,
            values,
        }))
    }

    pub fn resolve<M: TextMeasurer>(
        &mut self,
        source: &ElementTree,
        model_epoch: u64,
        projection: Arc<Projection>,
        context: Arc<QueryContext>,
        endpoints: &[Endpoint],
        measurer: &M,
    ) -> Result<HashMap<Endpoint, AxisFootprint>, ProjectionError> {
        validate_context(&context)?;
        let _fonts = context.fonts.as_ref().map(|fonts| fonts.enter());
        let root_identity = source.root_id().and_then(|id| {
            source
                .get(&id)
                .map(|node| (id, node.lifecycle.mounted_at_revision))
        });
        let rebuilt = self
            .workspace
            .as_ref()
            .is_none_or(|w| w.model_epoch != model_epoch || w.root_identity != root_identity);
        if rebuilt {
            let tree = source.layout_query_snapshot();
            let baseline_seeds = tree
                .iter_nodes()
                .filter(|node| seed_candidate(node))
                .map(|node| (node.id, QuerySeed::capture(node)))
                .collect();
            self.stats.model_copies += 1;
            self.stats.source_slots += source.nodes.len() as u64;
            self.stats.copied_nodes += tree.id_to_ix.len() as u64;
            self.workspace = Some(Workspace {
                tree,
                model_epoch,
                root_identity,
                baseline_seeds,
                temporary_seeds: HashMap::new(),
                previous: Arc::new(Projection::default()),
                context: None,
                cache: None,
            });
        }
        let workspace = self
            .workspace
            .as_mut()
            .ok_or(ProjectionError::InvalidContext)?;
        validate_nodes(&workspace.tree, &projection, &context, endpoints)?;
        if workspace
            .cache
            .as_ref()
            .is_some_and(|(p, c)| Arc::ptr_eq(p, &projection) && Arc::ptr_eq(c, &context))
        {
            self.stats.cache_hits += 1;
            return collect_endpoints(&workspace.tree, endpoints);
        }
        workspace.cache = None;
        let metrics_changed = workspace.context.as_ref().is_some_and(|old| {
            old.metrics_epoch != context.metrics_epoch
                || old.fonts != context.fonts
                || !Arc::ptr_eq(&old.images, &context.images)
        });
        let full_prepare = rebuilt
            || workspace
                .context
                .as_ref()
                .is_none_or(|old| old.scale != context.scale);
        let tree = &mut workspace.tree;
        let mut prepare_ids: HashSet<NodeId> = workspace
            .previous
            .nodes
            .keys()
            .chain(projection.nodes.keys())
            .copied()
            .collect();
        let scale_roots: HashSet<NodeId> = workspace
            .previous
            .nodes
            .iter()
            .chain(projection.nodes.iter())
            .filter_map(|(id, node)| {
                (node.attrs.layout_scale.is_some() || node.model.is_some()).then_some(*id)
            })
            .collect();

        // Drop the previous projection, including full foreign dimension samples.
        for id in workspace.previous.nodes.keys() {
            set_axis_sample(tree, id, Axis::Width, None).map_err(ProjectionError::InvalidSample)?;
            set_axis_sample(tree, id, Axis::Height, None)
                .map_err(ProjectionError::InvalidSample)?;
        }
        // The one workspace always returns to the current model before applying
        // sparse historical declarations. No rollback tree is retained.
        for (id, node) in &workspace.previous.nodes {
            if node.model.is_some() {
                let attrs = source
                    .get(id)
                    .ok_or(ProjectionError::UnknownNode(*id))?
                    .spec
                    .declared
                    .clone();
                tree.get_mut(id)
                    .ok_or(ProjectionError::UnknownNode(*id))?
                    .spec
                    .declared = attrs;
            }
        }
        for (id, node) in &projection.nodes {
            if let Some(attrs) = &node.model {
                tree.get_mut(id)
                    .ok_or(ProjectionError::UnknownNode(*id))?
                    .spec
                    .declared = (**attrs).clone();
            }
        }
        for (id, seed) in workspace.temporary_seeds.drain().chain(
            workspace
                .baseline_seeds
                .iter()
                .map(|(id, seed)| (*id, seed.clone())),
        ) {
            self.stats.seed_reset_visits += 1;
            if seed.restore(tree, &id) {
                prepare_ids.insert(id);
            }
        }
        workspace.temporary_seeds = projection
            .nodes
            .keys()
            .chain(context.seeds.keys())
            .copied()
            .filter(|id| !workspace.baseline_seeds.contains_key(id))
            .filter_map(|id| tree.get(&id).map(|node| (id, QuerySeed::capture(node))))
            .collect();
        for (id, seed) in &context.seeds {
            if seed.restore(tree, id) {
                prepare_ids.insert(*id);
            }
        }
        // Normal attr composition can reapply a declared scroll position. Frozen
        // runtime scroll inputs take precedence in a query, independent of which
        // subset of attrs happens to need preparation this time.
        let scroll_inputs: Vec<_> = workspace
            .baseline_seeds
            .keys()
            .chain(workspace.temporary_seeds.keys())
            .filter_map(|id| {
                tree.get(id)
                    .map(|node| (*id, QuerySeed::capture(node).scroll))
            })
            .collect();
        let overlays: HashMap<_, _> = projection
            .nodes
            .iter()
            .map(|(id, node)| {
                (
                    *id,
                    super::super::animation::AnimationSample {
                        attrs: node.attrs.clone(),
                        active: false,
                    },
                )
            })
            .collect();
        for id in &prepare_ids {
            tree.mark_layout_dirty_for_invalidation(id, TreeInvalidation::Measure);
        }
        if full_prepare || metrics_changed {
            tree.mark_all_measure_dirty();
        }
        if metrics_changed {
            self.stats.metric_invalidations += 1;
            tree.iter_nodes_mut()
                .for_each(|node| node.layout.intrinsic_measure_cache = None);
        }
        tree.set_current_scale(context.scale);
        tree.reset_scroll_cache_context_for_layout();
        prepare_projection_attrs(
            tree,
            context.scale,
            &overlays,
            &prepare_ids,
            &scale_roots,
            full_prepare,
        );
        for (id, scroll) in scroll_inputs {
            if tree
                .get_mut(&id)
                .is_some_and(|node| restore_scroll(node, scroll))
            {
                tree.mark_layout_dirty_for_invalidation(&id, TreeInvalidation::Resolve);
            }
        }
        // Remember touched inputs even if scope admission or collection fails.
        // The next request will restore them; failures never mark a cache hit.
        workspace.previous = Arc::clone(&projection);
        workspace.context = Some(Arc::clone(&context));
        for (id, node) in &projection.nodes {
            set_axis_sample(tree, id, Axis::Width, node.width)
                .map_err(ProjectionError::InvalidSample)?;
            set_axis_sample(tree, id, Axis::Height, node.height)
                .map_err(ProjectionError::InvalidSample)?;
            for (axis, source) in [Axis::Width, Axis::Height]
                .into_iter()
                .zip(node.transport.as_deref().copied().unwrap_or_default())
            {
                if let Some(source) = source {
                    dimensions::set_transport_sample(tree, id, axis, source)
                        .map_err(ProjectionError::InvalidSample)?;
                }
            }
        }
        self.stats.layout_queries += 1;
        if let Some(root) = tree.root_id() {
            let metrics = FrameMeasurer {
                text: measurer,
                images: Some(&context.images),
            };
            run_measure_resolve(
                tree,
                &root,
                context.constraint,
                &metrics,
                &context.inherited,
            );
        }
        let result = collect_endpoints(tree, endpoints)?;
        workspace.cache = Some((projection, context));
        Ok(result)
    }
}

fn validate_context(context: &QueryContext) -> Result<(), ProjectionError> {
    let valid_space = |space| match space {
        AvailableSpace::Definite(value) => value.is_finite() && value >= 0.0,
        AvailableSpace::MinContent | AvailableSpace::MaxContent => true,
    };
    if !context.scale.is_finite()
        || context.scale <= 0.0
        || !valid_space(context.constraint.width)
        || !valid_space(context.constraint.height)
        || context.seeds.values().any(|seed| {
            seed.scroll
                .iter()
                .any(|value| !value.is_finite() || *value < 0.0)
        })
    {
        Err(ProjectionError::InvalidContext)
    } else {
        Ok(())
    }
}

fn validate_nodes(
    tree: &ElementTree,
    projection: &Projection,
    context: &QueryContext,
    endpoints: &[Endpoint],
) -> Result<(), ProjectionError> {
    for id in projection
        .nodes
        .keys()
        .chain(context.seeds.keys())
        .chain(endpoints.iter().map(|(id, _)| id))
    {
        if tree.get(id).is_none() {
            return Err(ProjectionError::UnknownNode(*id));
        }
    }
    for (id, node) in &projection.nodes {
        for (axis, sample, declared) in [
            (Axis::Width, node.width, node.attrs.width.is_some()),
            (Axis::Height, node.height, node.attrs.height.is_some()),
        ] {
            if let Some(source) = node
                .transport
                .as_deref()
                .and_then(|sources| sources[if axis == Axis::Width { 0 } else { 1 }])
            {
                if sample.is_some() || declared {
                    return Err(ProjectionError::ConflictingDimension(*id, axis));
                }
                if !source.footprint.valid()
                    || source.footprint.axis != axis
                    || !source.scale.is_finite()
                    || source.scale <= 0.0
                {
                    return Err(ProjectionError::InvalidSample(
                        "invalid dimension transport source",
                    ));
                }
            }
            if let Some(sample) = sample {
                if sample.axis != axis || !sample.valid() {
                    return Err(ProjectionError::InvalidSample(
                        "invalid dimension footprint",
                    ));
                }
                if declared {
                    return Err(ProjectionError::ConflictingDimension(*id, axis));
                }
            }
        }
    }
    Ok(())
}

fn collect_endpoints(
    tree: &ElementTree,
    endpoints: &[Endpoint],
) -> Result<HashMap<Endpoint, AxisFootprint>, ProjectionError> {
    endpoints
        .iter()
        .map(|&(id, axis)| {
            capture_axis(tree, &id, axis)
                .filter(|sample| sample.valid())
                .map(|sample| ((id, axis), sample))
                .ok_or(ProjectionError::MissingLayout(id, axis))
        })
        .collect()
}

pub(crate) fn seed_candidate(node: &Element) -> bool {
    node.spec.kind.is_text_input_family()
        || node.spec.kind == ElementKind::Slider
        || effective_scrollbar_x(&node.spec.declared)
        || effective_scrollbar_y(&node.spec.declared)
        || node.layout.scroll_x != 0.0
        || node.layout.scroll_y != 0.0
        || node.layout.scroll_x_max != 0.0
        || node.layout.scroll_y_max != 0.0
        || node.spec.declared.mouse_over.is_some()
        || node.spec.declared.mouse_down.is_some()
        || node.spec.declared.focused.is_some()
        || node.runtime.mouse_over_active
        || node.runtime.mouse_down_active
        || node.runtime.focused_active
}

fn restore_scroll(node: &mut Element, scroll: [f32; 4]) -> bool {
    let changed = [
        node.layout.scroll_x,
        node.layout.scroll_y,
        node.layout.scroll_x_max,
        node.layout.scroll_y_max,
    ] != scroll;
    [
        node.layout.scroll_x,
        node.layout.scroll_y,
        node.layout.scroll_x_max,
        node.layout.scroll_y_max,
    ] = scroll;
    changed
}

fn prepare_projection_attrs(
    tree: &mut ElementTree,
    scale: f32,
    overlays: &HashMap<NodeId, super::super::animation::AnimationSample>,
    ids: &HashSet<NodeId>,
    scale_roots: &HashSet<NodeId>,
    full: bool,
) {
    if full {
        prepare_all_attrs_for_frame(tree, scale, overlays);
        apply_interaction_styles(tree);
        return;
    }
    // Only outermost changed scale roots need traversal, including scale removal.
    let under_scale_root = |tree: &ElementTree, id: &NodeId| {
        let mut ix = tree.ix_of(id);
        while let Some(parent) =
            ix.and_then(|ix| super::super::element::parent_ix_from_link(tree.parent_link_of(ix)))
        {
            if tree
                .id_of(parent)
                .is_some_and(|id| scale_roots.contains(&id))
            {
                return true;
            }
            ix = Some(parent);
        }
        false
    };
    let roots: Vec<_> = scale_roots
        .iter()
        .filter(|id| !under_scale_root(tree, id))
        .copied()
        .collect();
    for id in &roots {
        tree.mark_layout_scale_dirty_for_animation(id);
        let inherited = inherited_layout_scale_for_node(tree, id, scale, overlays);
        prepare_attrs_for_subtree(tree, *id, inherited, overlays);
    }
    apply_interaction_styles_for_subtrees(tree, &roots);
    for id in ids {
        if scale_roots.contains(id) || under_scale_root(tree, id) {
            continue;
        }
        let factor = effective_layout_scale_for_node_with_samples(tree, id, scale, overlays);
        prepare_attrs_for_single_node(tree, id, factor, overlays);
        apply_interaction_styles_for_ids(tree, &[*id]);
    }
}

pub(super) struct FrameMeasurer<'a, M> {
    pub(super) text: &'a M,
    pub(super) images: Option<&'a ImageDimensions>,
}

impl<M: TextMeasurer> TextMeasurer for FrameMeasurer<'_, M> {
    fn metrics_epoch(&self) -> u64 {
        self.text.metrics_epoch()
    }
    fn font_snapshot(&self) -> Option<crate::renderer::FontSnapshot> {
        self.text.font_snapshot()
    }
    fn image_dimensions(&self, source: &ImageSource, _load: bool) -> Option<(u32, u32)> {
        self.images.map_or_else(
            || self.text.image_dimensions(source, _load),
            |images| images.get(source).copied(),
        )
    }
    fn measure_with_font(
        &self,
        text: &str,
        size: f32,
        family: &str,
        weight: u16,
        italic: bool,
    ) -> (f32, f32) {
        self.text
            .measure_with_font(text, size, family, weight, italic)
    }
    fn measure_visual_width_with_font(
        &self,
        text: &str,
        size: f32,
        family: &str,
        weight: u16,
        italic: bool,
    ) -> f32 {
        self.text
            .measure_visual_width_with_font(text, size, family, weight, italic)
    }
    fn measure_text_layout_with_font(
        &self,
        text: &str,
        size: f32,
        family: &str,
        weight: u16,
        italic: bool,
    ) -> (f32, f32) {
        self.text
            .measure_text_layout_with_font(text, size, family, weight, italic)
    }
    fn font_metrics(&self, size: f32, family: &str, weight: u16, italic: bool) -> (f32, f32) {
        self.text.font_metrics(size, family, weight, italic)
    }
}

#[cfg(test)]
mod tests;
