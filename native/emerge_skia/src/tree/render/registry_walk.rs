use crate::events::registry_builder::{
    RegistryRefreshCollector, RegistryTraversalContext, RegistryVisitResult,
};
use crate::tree::element::{Element, ElementTree, NearbySlot, NodeIx, RetainedPaintPhase};
use crate::tree::scene::{ResolvedNodeState, SceneContext, child_context as next_scene_context};

pub(super) trait RegistryTraversalSink {
    type Host: HostRegistryTraversalSink;

    /// Called when render culls or reuses a subtree but active registry rebuild
    /// still needs to account for that subtree. This is a no-op for clean
    /// registry reuse and for branches where the registry side already decided
    /// the subtree is irrelevant.
    fn collect_registry_for_render_skipped_subtree(&mut self, tree: &ElementTree, ix: NodeIx);

    fn visit_element(
        self,
        tree: &ElementTree,
        element: &Element,
        state: Option<&ResolvedNodeState>,
    ) -> Self::Host;
}

pub(super) trait HostRegistryTraversalSink {
    type Branch<'a>: RegistryTraversalSink
    where
        Self: 'a;

    fn local_nearby_traversal(
        &mut self,
        scene_state: Option<&ResolvedNodeState>,
    ) -> Self::Branch<'_>;

    fn child_context(&self, scene_ctx: SceneContext) -> Option<RegistryTraversalContext>;

    fn should_skip_child(
        &self,
        tree: &ElementTree,
        child_ix: NodeIx,
        scene_ctx: &SceneContext,
    ) -> bool;

    /// Collect registry metadata for a child whose render subtree is omitted
    /// by culling/cache reuse or by paragraph inline-event-only rendering.
    fn collect_child_subtree(
        &mut self,
        tree: &ElementTree,
        child_ix: NodeIx,
        context: Option<RegistryTraversalContext>,
    );

    fn child_traversal(
        &mut self,
        registry_skipped: bool,
        context: Option<RegistryTraversalContext>,
    ) -> Self::Branch<'_>;

    fn defer_escape_nearby_subtree(
        &mut self,
        tree: &ElementTree,
        mount_ix: NodeIx,
        slot: NearbySlot,
        scene_state: Option<&ResolvedNodeState>,
    );
}

pub(super) struct BuildRegistryTraversal<'a> {
    collector: &'a mut RegistryRefreshCollector,
    ctx: RegistryTraversalContext,
}

impl<'a> BuildRegistryTraversal<'a> {
    pub(super) fn root(collector: &'a mut RegistryRefreshCollector) -> Self {
        Self {
            collector,
            ctx: RegistryTraversalContext::root(),
        }
    }
}

pub(super) struct BuildHostRegistryTraversal<'a> {
    collector: &'a mut RegistryRefreshCollector,
    current_ctx: RegistryTraversalContext,
    visit: RegistryVisitResult,
}

// The collection branch intentionally stores the traversal state inline to avoid
// allocating on every child edge in the hot refresh walk. Keep this allowance
// local to the registry-walk adapter instead of spreading it through render.rs.
#[allow(clippy::large_enum_variant)]
pub(super) enum BuildRegistryBranch<'a> {
    Collect(BuildRegistryTraversal<'a>),
    SkipRegistry,
}

// See `BuildRegistryBranch`: this adapter makes "skip registry collection"
// explicit while render continues traversing the child.
#[allow(clippy::large_enum_variant)]
pub(super) enum BuildHostRegistryBranch<'a> {
    Collect(BuildHostRegistryTraversal<'a>),
    SkipRegistry,
}

#[derive(Clone, Copy)]
pub(super) struct ReuseCleanRegistryTraversal;

#[derive(Clone, Copy)]
pub(super) struct ReuseCleanHostRegistryTraversal;

impl<'a> RegistryTraversalSink for BuildRegistryTraversal<'a> {
    type Host = BuildHostRegistryTraversal<'a>;

    fn collect_registry_for_render_skipped_subtree(&mut self, tree: &ElementTree, ix: NodeIx) {
        self.collector.collect_subtree(tree, ix, self.ctx.clone());
    }

    fn visit_element(
        self,
        tree: &ElementTree,
        element: &Element,
        state: Option<&ResolvedNodeState>,
    ) -> Self::Host {
        let visit = self
            .collector
            .visit_element(tree, element, state, &self.ctx);
        BuildHostRegistryTraversal {
            collector: self.collector,
            current_ctx: self.ctx,
            visit,
        }
    }
}

impl<'a> RegistryTraversalSink for BuildRegistryBranch<'a> {
    type Host = BuildHostRegistryBranch<'a>;

    fn collect_registry_for_render_skipped_subtree(&mut self, tree: &ElementTree, ix: NodeIx) {
        if let Self::Collect(registry) = self {
            registry.collect_registry_for_render_skipped_subtree(tree, ix);
        }
    }

    fn visit_element(
        self,
        tree: &ElementTree,
        element: &Element,
        state: Option<&ResolvedNodeState>,
    ) -> Self::Host {
        match self {
            Self::Collect(registry) => {
                BuildHostRegistryBranch::Collect(registry.visit_element(tree, element, state))
            }
            Self::SkipRegistry => BuildHostRegistryBranch::SkipRegistry,
        }
    }
}

impl RegistryTraversalSink for ReuseCleanRegistryTraversal {
    type Host = ReuseCleanHostRegistryTraversal;

    #[inline(always)]
    fn collect_registry_for_render_skipped_subtree(&mut self, _tree: &ElementTree, _ix: NodeIx) {}

    #[inline(always)]
    fn visit_element(
        self,
        _tree: &ElementTree,
        _element: &Element,
        _state: Option<&ResolvedNodeState>,
    ) -> Self::Host {
        ReuseCleanHostRegistryTraversal
    }
}

impl<'a> HostRegistryTraversalSink for BuildHostRegistryTraversal<'a> {
    type Branch<'b>
        = BuildRegistryBranch<'b>
    where
        Self: 'b;

    fn local_nearby_traversal(
        &mut self,
        scene_state: Option<&ResolvedNodeState>,
    ) -> Self::Branch<'_> {
        BuildRegistryBranch::Collect(BuildRegistryTraversal {
            collector: self.collector,
            ctx: RegistryTraversalContext {
                scroll_contexts: self.current_ctx.scroll_contexts.clone(),
                hover_stack: self.current_ctx.hover_stack.clone(),
                scene_ctx: scene_context_for_phase(scene_state, RetainedPaintPhase::BehindContent),
            },
        })
    }

    fn child_context(&self, scene_ctx: SceneContext) -> Option<RegistryTraversalContext> {
        Some(RegistryTraversalContext {
            scroll_contexts: self.visit.next_scroll_contexts.clone(),
            hover_stack: self.visit.next_hover_stack.clone(),
            scene_ctx,
        })
    }

    fn should_skip_child(
        &self,
        tree: &ElementTree,
        child_ix: NodeIx,
        scene_ctx: &SceneContext,
    ) -> bool {
        self.collector.should_skip_child(tree, child_ix, scene_ctx)
    }

    fn collect_child_subtree(
        &mut self,
        tree: &ElementTree,
        child_ix: NodeIx,
        context: Option<RegistryTraversalContext>,
    ) {
        if let Some(context) = context {
            self.collector.collect_subtree(tree, child_ix, context);
        }
    }

    fn child_traversal(
        &mut self,
        registry_skipped: bool,
        context: Option<RegistryTraversalContext>,
    ) -> Self::Branch<'_> {
        if registry_skipped {
            return BuildRegistryBranch::SkipRegistry;
        }

        context
            .map(|ctx| {
                BuildRegistryBranch::Collect(BuildRegistryTraversal {
                    collector: self.collector,
                    ctx,
                })
            })
            .unwrap_or(BuildRegistryBranch::SkipRegistry)
    }

    fn defer_escape_nearby_subtree(
        &mut self,
        tree: &ElementTree,
        mount_ix: NodeIx,
        slot: NearbySlot,
        scene_state: Option<&ResolvedNodeState>,
    ) {
        let Some(mount_id) = tree.id_of(mount_ix) else {
            return;
        };

        self.collector.defer_subtree(
            mount_id,
            RegistryTraversalContext {
                scroll_contexts: self.current_ctx.scroll_contexts.clone(),
                hover_stack: self.current_ctx.hover_stack.clone(),
                scene_ctx: scene_context_for_phase(scene_state, RetainedPaintPhase::Overlay(slot)),
            },
        );
    }
}

impl<'a> HostRegistryTraversalSink for BuildHostRegistryBranch<'a> {
    type Branch<'b>
        = BuildRegistryBranch<'b>
    where
        Self: 'b;

    fn local_nearby_traversal(
        &mut self,
        scene_state: Option<&ResolvedNodeState>,
    ) -> Self::Branch<'_> {
        match self {
            Self::Collect(registry) => registry.local_nearby_traversal(scene_state),
            Self::SkipRegistry => BuildRegistryBranch::SkipRegistry,
        }
    }

    fn child_context(&self, scene_ctx: SceneContext) -> Option<RegistryTraversalContext> {
        match self {
            Self::Collect(registry) => registry.child_context(scene_ctx),
            Self::SkipRegistry => None,
        }
    }

    fn should_skip_child(
        &self,
        tree: &ElementTree,
        child_ix: NodeIx,
        scene_ctx: &SceneContext,
    ) -> bool {
        match self {
            Self::Collect(registry) => registry.should_skip_child(tree, child_ix, scene_ctx),
            Self::SkipRegistry => true,
        }
    }

    fn collect_child_subtree(
        &mut self,
        tree: &ElementTree,
        child_ix: NodeIx,
        context: Option<RegistryTraversalContext>,
    ) {
        if let Self::Collect(registry) = self {
            registry.collect_child_subtree(tree, child_ix, context);
        }
    }

    fn child_traversal(
        &mut self,
        registry_skipped: bool,
        context: Option<RegistryTraversalContext>,
    ) -> Self::Branch<'_> {
        match self {
            Self::Collect(registry) => registry.child_traversal(registry_skipped, context),
            Self::SkipRegistry => BuildRegistryBranch::SkipRegistry,
        }
    }

    fn defer_escape_nearby_subtree(
        &mut self,
        tree: &ElementTree,
        mount_ix: NodeIx,
        slot: NearbySlot,
        scene_state: Option<&ResolvedNodeState>,
    ) {
        if let Self::Collect(registry) = self {
            registry.defer_escape_nearby_subtree(tree, mount_ix, slot, scene_state);
        }
    }
}

impl HostRegistryTraversalSink for ReuseCleanHostRegistryTraversal {
    type Branch<'a> = ReuseCleanRegistryTraversal;

    #[inline(always)]
    fn local_nearby_traversal(
        &mut self,
        _scene_state: Option<&ResolvedNodeState>,
    ) -> Self::Branch<'_> {
        ReuseCleanRegistryTraversal
    }

    #[inline(always)]
    fn child_context(&self, _scene_ctx: SceneContext) -> Option<RegistryTraversalContext> {
        None
    }

    #[inline(always)]
    fn should_skip_child(
        &self,
        _tree: &ElementTree,
        _child_ix: NodeIx,
        _scene_ctx: &SceneContext,
    ) -> bool {
        true
    }

    #[inline(always)]
    fn collect_child_subtree(
        &mut self,
        _tree: &ElementTree,
        _child_ix: NodeIx,
        _context: Option<RegistryTraversalContext>,
    ) {
    }

    #[inline(always)]
    fn child_traversal(
        &mut self,
        _registry_skipped: bool,
        _context: Option<RegistryTraversalContext>,
    ) -> Self::Branch<'_> {
        ReuseCleanRegistryTraversal
    }

    #[inline(always)]
    fn defer_escape_nearby_subtree(
        &mut self,
        _tree: &ElementTree,
        _mount_ix: NodeIx,
        _slot: NearbySlot,
        _scene_state: Option<&ResolvedNodeState>,
    ) {
    }
}

pub(super) fn scene_context_for_phase(
    scene_state: Option<&ResolvedNodeState>,
    phase: RetainedPaintPhase,
) -> SceneContext {
    scene_state
        .cloned()
        .map(|state| next_scene_context(state, phase))
        .unwrap_or_default()
}

pub(super) fn children_scene_context(scene_state: Option<&ResolvedNodeState>) -> SceneContext {
    scene_context_for_phase(scene_state, RetainedPaintPhase::Children)
}

pub(super) fn nearby_scene_context(
    scene_state: Option<&ResolvedNodeState>,
    slot: NearbySlot,
) -> SceneContext {
    scene_context_for_phase(scene_state, slot.spec().phase)
}
