# Layout, Refresh, Render, and Cache Flow

This guide documents the runtime path from an Elixir tree update to rendered
pixels. It focuses on the Rust-side tree traversals, invalidation decisions, and
cache boundaries used by layout, refresh, render-scene construction, and the
renderer paint-layer cache.

## Entry Points

The public Elixir calls eventually send `TreeMsg` values to the Rust tree actor:

- `EmergeSkia.upload_tree/2` sends a complete EMRG tree.
- `EmergeSkia.patch_tree/3` sends incremental EMRG patch operations.
- Input, scroll, resize, focus, text edit, slider, image, and animation events
  also become `TreeMsg` values.

The high-level runtime flow is:

```text
Elixir tree upload or patch
  -> TreeMsg batch
  -> TreeUpdateEngine::process_messages
  -> optional frame attr preparation
  -> refresh decision
     -> Skip
     -> RegistryUpdate
     -> RefreshOnly
     -> Recompute layout + refresh
  -> LayoutOutput
  -> publish_layout_output
  -> RenderMsg::Scene
  -> backend render loop
  -> Skia surface flush/present
```

The tree actor owns mutation of `ElementTree`. The renderer never mutates the
tree. The renderer receives a `RenderScene`, renders it, and owns the GPU/CPU
payload cache for paint layers.

## Update Batching

`spawn_tree_actor_with_initial_tree` receives one `TreeMsg`, drains any queued
messages with `try_recv`, and sends the whole batch to
`TreeUpdateEngine::process_messages`.

`process_messages` first flattens nested `TreeMsg::Batch` values. An empty batch
returns `TreeUpdateEffect::Skip`.

The update engine then folds the batch into a single update plan:

- Tree uploads replace the whole tree after EMRG decode.
- Tree patches decode patch operations and apply them to the existing
  `ElementTree`.
- Resize updates width, height, and scale.
- Scroll, drag, hover, focus, text input, slider, and asset-state messages are
  collected and applied once per batch.
- Animation pulse metadata is kept so the animation runtime can sample the
  right frame time.

Each message contributes a `TreeInvalidation`. The invalidation enum is ordered
from cheapest to most expensive:

| Invalidation | Meaning | Minimum work |
|--------------|---------|--------------|
| `None` | No tree, layout, registry, or paint output changed. | Skip. |
| `Registry` | Event registry or hit-test metadata changed. | Registry refresh or cached registry reply. |
| `Paint` | Geometry stayed valid, but render output changed. | Render-scene refresh. |
| `Resolve` | Intrinsic sizes are valid, but placement changed. | Layout resolve + refresh. |
| `Measure` | Intrinsic or subtree size may have changed. | Measure + resolve + refresh. |
| `Structure` | Topology changed. | Rebuild topology-sensitive caches, measure + resolve + refresh. |

The final invalidation is a join of every message and every sampled animation
effect.

## Refresh Decision

After mutations and animation sampling, the engine calls `decide_refresh_action`.

The decision inputs are:

- final invalidation
- whether the event actor requested a registry rebuild
- whether `TreeUpdateEngine.cached_rebuild` exists
- whether the tree already has a root layout frame

The decision is intentionally small:

```text
requires recompute                -> Recompute
Paint or Registry with root frame -> RefreshOnly
Paint or Registry without frame   -> Recompute
registry requested with cache     -> UseCachedRebuild
otherwise                         -> Skip
```

`UseCachedRebuild` sends only `TreeUpdateEffect::RegistryUpdate`. It does not
publish a render scene.

`RefreshOnly` builds a new `RenderScene` and either reuses the cached event
registry or rebuilds it if registry damage exists.

`Recompute` runs layout passes, then does the same refresh work.

## Frame Attribute Preparation

Before layout or refresh, the tree has to expose the attributes for the current
frame in `element.layout.effective`.

The preparation path is:

```text
prepare_frame_attrs_for_update
or prepare_animation_frame_attrs_for_update
or prepare_dirty_frame_attrs_for_update
  -> prepare_frame_attrs
     -> ensure_topology
     -> set_current_scale
     -> sample_animation_overlays
     -> prepare_all_attrs_for_frame, active-node preparation, or dirty-id preparation
     -> mark_animation_refresh_effects_dirty
     -> apply_interaction_styles
```

The normal preparation traversal is a flat traversal over all nodes. It copies
declared frame attributes from `element.spec.declared`, overlays animation
samples, scales them into `layout.effective`, and normalizes extracted runtime
state.

The incremental animation preparation path is used for steady animation pulses
when the tree already has frames and no transient animation entries require a
full pass. It prepares only active animation nodes unless a layout scale root
forces subtree preparation.

Refresh-only runtime and `SetAttrs` updates can use the same incremental
preparation machinery. The update engine tracks touched frame-attr ids for
mouse-over/down/focus, text-input runtime/content, slider runtime values, and
`SetAttrs` patches. If the final invalidation is still `Paint` or `Registry`
and the patch list contains only `SetAttrs`, preparation touches only those ids
plus active animation ids. Scroll-only refresh with no active animations skips
frame-attr preparation entirely because scroll state already lives in layout
state; scroll refresh with active animations prepares the active animation ids
without walking unrelated frame attrs. Structure, nearby insertion/removal,
measure, resolve, and transient animation cases keep the full preparation path.

Preparation marks refresh damage separately from layout damage:

- Animation effects that only change paint mark render refresh dirty.
- Effects that alter layout mark layout dirty and also mark refresh dirty.
- Registry refresh is only marked when the changed subtree can affect the event
  registry.

## Layout Recompute

`run_layout_passes` is the layout recompute entry point. It performs the layout
tree traversals and refresh-affect bookkeeping:

```text
run_layout_passes
  -> mark_animation_layout_effects_dirty
  -> refresh_registry_subtree_affects_cache
  -> measure_element(root)
  -> resolve_element(root)
```

### Registry-Affects Prepass

`refresh_registry_subtree_affects_cache` computes whether each subtree can
affect the event registry. This lets registry refresh skip large neutral
subtrees later.

A subtree affects the registry when it contains interactive/event-visible state,
for example text inputs, sliders, focus/hover/down state, scrollbars, key
handlers, mouse handlers, virtual keys, or nearby mounts that can participate in
hit testing.

### Measure Traversal

The measure pass is bottom-up. It computes intrinsic sizes and measured frames
from pre-scaled effective attrs.

Important caches:

| Cache | Owner | Purpose |
|-------|-------|---------|
| `IntrinsicMeasureCache` | Node layout state | Reuse leaf/text/media intrinsic measurement when the intrinsic key and inherited font match. |
| `SubtreeMeasureCache` | Node layout state | Reuse measured subtree output when attrs, topology, constraints, and inherited font match. |

The measure pass uses dirty flags and cache keys so stable descendants do not
need full remeasurement when an unrelated sibling animates. For text, paragraph
fragments and measured text data are reused when the text/font key is unchanged.

When a cache hits, the pass restores the measured frames and associated
measurement outputs for the clean subtree. When a cache misses, it measures
children, computes content/fill sizing, records measured frames, and stores a
new cache entry.

### Resolve Traversal

The resolve pass is top-down. It takes measured sizes and resolves final
positions, scroll extents, render frames, nearby slots, and transformed geometry.

Important cache:

| Cache | Owner | Purpose |
|-------|-------|---------|
| `ResolveCache` | Node layout state | Reuse resolved frames, render frames, scroll maxima, and nearby placement when constraints, attrs, topology, and inherited font match. |

Resolve can reuse a clean parent while still visiting dirty descendants. When a
child changes size or placement, sibling and descendant frames can be shifted
instead of recomputing their full measurement.

The resolve pass also updates geometry that render and events need:

- `layout.frame`
- `layout.render_frame`
- `layout.scroll_x_max` and `layout.scroll_y_max`
- paragraph fragments
- nearby slot geometry
- transformed hit geometry

## Refresh

Refresh produces `LayoutOutput` from an already-laid-out tree:

```text
refresh or refresh_reusing_clean_registry
  -> render_tree_scene_with_scroll_layers
  -> build_registry_rebuild_cached if registry is dirty
  -> clear_refresh_dirty or clear_render_refresh_dirty
  -> LayoutOutput
```

`LayoutOutput` contains:

- `scene`: the render scene sent to the renderer
- `event_rebuild`: event registry payload when rebuilt
- `event_rebuild_changed`: whether to publish the registry payload
- IME state and cursor area
- `animations_active`

When `cached_rebuild` exists and the tree has no registry refresh damage,
`refresh_reusing_clean_registry` skips `build_registry_rebuild_cached`. It still
builds a fresh render scene, because paint and animation output may have changed.

When a registry rebuild does happen, the resulting rebuild is stored in
`TreeUpdateEngine.cached_rebuild` by `layout_effect`.

## Event Registry Traversal

The event registry describes hit-test and event-dispatch metadata for the event
actor. It is built from the laid-out tree after render-scene construction.

`build_registry_rebuild_cached` traverses registry-relevant subtrees. Each node
can hold a `RegistrySubtreeCache`:

| Field | Meaning |
|-------|---------|
| `RegistrySubtreeKey.kind` | Element kind at the subtree root. |
| `attrs_hash` | Effective attrs that affect registry output. |
| `runtime_hash` | Runtime state that affects registry output. |
| `frame_hash` | Frame and hit geometry input. |
| `hover_stack_hash` | Hover ancestry input. |
| `scene_context_hash` | Scroll/transform/clip context input. |
| `scroll_contexts_hash` | Active scroll context stack. |
| `topology` | Topology dependency key. |

Registry traversal uses three pruning layers:

1. If a subtree is known to not affect the registry, it is skipped.
2. If a clean cached subtree key matches, the cached registry chunk is merged.
3. If a clean eligible subtree has no cache, a local chunk is built and stored.

Nearby mounts can defer traversal because they render and hit-test in a
different scene context from their host.

Scroll refresh is intentionally conservative for registry chunks. Scroll changes
shift hit-test geometry, so scroll damage bypasses subtree chunk reuse rather
than churning scroll-dependent cache keys. The full scroll registry traversal
still uses the retained `registry_subtree_affects` prepass for clean subtrees,
so neutral branches are skipped without recursively rediscovering whether they
contain event-visible state.

## Render-Scene Construction

Render-scene construction is a tree traversal over laid-out elements:

```text
render_tree_scene_with_scroll_layers
  -> build_element_subtree(root)
  -> wrap root paint layer when eligible
```

`build_element_subtree` reads:

- `layout.effective`
- `layout.frame`
- `layout.render_frame`
- scroll state
- runtime hover/focus/down state
- refresh dirty flags

For each element it:

1. Resolves the current scene state from scroll, transforms, alpha, and clips.
2. Culls the subtree if viewport culling proves it cannot be visible.
3. Attempts to reuse a retained stable paint-layer scene fragment.
4. Builds own visual nodes: shadows, background, host content, border, text,
   images, videos, gradients, scrollbars, and nearby content.
5. Recursively builds children.
6. Wraps content with clips, transforms, alpha, and paint-layer boundaries.

The result is a `RenderScene` made of `RenderNode` values. The renderer consumes
this scene; it does not traverse the `ElementTree`.

## Paint-Layer Boundaries

Paint layers are semantic render boundaries. A frame is rendered as a
composition of paint layers and direct render nodes.

Current paint-layer reasons:

| Reason | Typical boundary | Policy |
|--------|------------------|--------|
| `Root` | Whole scene when root is clean and cacheable. | `Cacheable` |
| `ScrollContainer` | The content boundary of a scroll container. | `DynamicRedraw` |
| `StableSubtree` | Stable child content inside a scroll context. | `Cacheable` |
| `Animation` | Dirty animated content that needs redraw. | `DynamicRedraw` |
| `Nearby` | Nearby overlay content mounted from another element. | `Cacheable` |

Placement is separate from contents:

| Placement | Meaning |
|-----------|---------|
| `Fixed` | Layer bounds are already in scene coordinates. |
| `ScrollMoving` | Layer payload is stable; current placement can move during composition. |

This separation is the key compositing invariant. If an element's contents did
not change, but its position changes inside a parent or scroll container, the
payload can remain valid and the renderer only changes the transform used to
compose it.

### Own Nodes and Child References

`RenderPaintLayer` splits layer content into:

- `own_nodes`: primitives and wrappers owned by this layer payload
- `child_refs`: nested paint layers that must stay independently composited

This is a correctness requirement. If a parent paint layer is invalidated and
redrawn, child layer references must survive so stable children do not lose
their own cache identity. Parent payload preparation should redraw only the
parent's own content, then compose child layers separately.

## Render-Layer Scene Cache

Each element has `refresh.render_layer_cache`. This is not the GPU payload
cache. It is a retained render-scene fragment used during render-scene
construction.

The key is:

| Field | Meaning |
|-------|---------|
| `paint_generation` | Per-element generation incremented when render output changes. |
| `topology` | Topology dependency key. |
| `bounds` | Layer bounds that define payload size. |

If a stable moving layer is clean and the key matches, render-scene construction
can return the retained `RenderPaintLayer` without descending through its
children. The layer is wrapped in the current placement transform, so scrolling
or layout movement does not invalidate stable content.

When render damage is marked on an element, `paint_generation` increments and
the retained layer cache is cleared.

## Renderer Paint-Layer Cache

The renderer owns `RendererCacheManager`. It caches prepared paint-layer
payloads as GPU images on GPU-backed backends and CPU images on raster paths.

Each render frame:

```text
RendererCacheManager::begin_frame
  -> render_nodes_with_cache_tracking
     -> visit RenderNode tree
     -> for each paint layer:
        -> compute device-space visibility and payload key
        -> hit: draw cached image
        -> miss: render own_nodes into payload and store
        -> compose child_refs independently
  -> RendererCacheManager::end_frame
  -> frame.flush
```

The payload key includes:

- layer stable id
- content generation
- payload width and height in pixels
- scale bits
- resource generation for fonts/images when applicable

The cache records:

- candidates and visible candidates
- low-value bypasses
- hits, misses, stores, and evictions
- rejected stores: ineligible, oversized, payload budget, fractional placement,
  unsupported transform
- payload residency: entries, bytes, GPU/CPU payload counts
- composition pixels: cached image draws, payload pixels, visible pixels, and
  waste ratio
- prepare time and cached-image hit draw time

### Admission and Bypass

Paint-layer scene construction says what may be cached. Renderer admission says
whether a concrete payload should be stored.

The renderer can bypass low-value payloads such as tiny cheap layers or large
simple layers where texture composition is likely more expensive than direct
drawing. It also rejects payloads that exceed entry or total byte budgets.

Bypassing a payload does not change scene correctness. It only means the layer's
own nodes are drawn directly for that frame while child layer references still
compose independently.

### GPU and Raster Behavior

The cache model is backend-neutral:

- GPU-backed backends store GPU render-target payloads and compose them as Skia
  images.
- Raster backend stores CPU raster payloads.
- Backends without cache tracking render the scene directly.

Correctness must not depend on Wayland-specific behavior. Wayland, DRM, macOS,
and raster paths should see the same scene semantics.

## Dirty Flags and Cache Invalidations

`NodeRefreshState` tracks render and registry damage:

```text
render_dirty
render_descendant_dirty
registry_dirty
registry_descendant_dirty
registry_cache
render_layer_cache
registry_subtree_affects
paint_generation
```

Marking render dirty:

- increments `paint_generation` once for the dirty cycle
- clears the node's retained `render_layer_cache`
- sets `render_dirty` on the origin
- bubbles `render_descendant_dirty` to ancestors

Marking registry dirty:

- clears the node's `registry_cache`
- sets `registry_dirty` on the origin
- bubbles `registry_descendant_dirty` to ancestors

`clear_refresh_dirty` clears render and registry damage after a full refresh.
`clear_render_refresh_dirty` clears only render damage when the registry was
reused.

## Common Update Scenarios

### Registry-Only Request

If the event actor asks for the registry and `cached_rebuild` is clean,
`UseCachedRebuild` sends `TreeUpdateEffect::RegistryUpdate`. There is no layout
and no render scene.

### Paint-Only Hover or Focus

Hover/focus state can change attrs. Preparation applies interaction styles and
marks paint and possibly registry damage. If geometry is unchanged, the engine
uses `RefreshOnly`.

Expected traversals:

- frame attr preparation for affected state
- render-scene construction
- registry traversal only if registry-affecting state changed
- render-node traversal in the renderer

Expected cache behavior:

- unchanged paint layers hit
- changed paint layers miss or bypass
- child paint layers survive parent redraws

### Layout Animation in a Scroll Container

For a layout-affecting animated row inside a scroll container, the row's size
and follower placement may change. Stable sibling sections and stable rows can
remain cached paint layers. Their contents do not need redraw just because their
position changes.

Expected traversals:

- incremental animation preparation when possible
- measure/resolve for dirty layout region
- render-scene construction with stable moving layer reuse
- renderer composition using cached payloads for stable sections

### Scroll

Scroll changes update scroll offsets and visible scene context. Layout frames
are usually still valid, so scroll can refresh without measure/resolve.

Expected cache behavior:

- stable scroll-moving layers keep their payloads
- layers that leave the viewport can remain resident within cache budgets
- layers that re-enter the viewport can hit if their payload key still matches

### Tree Patch

A patch can change attrs, text, structure, or nearby topology. Patch application
marks the weakest invalidation that preserves correctness.

Expected behavior:

- paint-only attr patch refreshes without layout
- geometry attr patch recomputes resolve or measure as needed
- structural patch invalidates topology-sensitive caches
- event-affecting patch invalidates registry cache for that subtree

## Traversal Inventory

| Traversal | Entry point | Direction | Runs when | Primary cache |
|-----------|-------------|-----------|-----------|---------------|
| Message flattening | `push_tree_message_flat` | Message tree | Every batch | None |
| Patch application | `apply_patches` | Patch list plus affected nodes | Patch messages | Tree topology and dirty flags |
| Frame attr preparation | `prepare_frame_attrs_for_update` | Flat tree or active nodes | Dirty frame or animation sample | Animation samples |
| Registry-affects prepass | `refresh_registry_subtree_affects_cache` | Tree | Layout recompute | `registry_subtree_affects` |
| Measure | `measure_element` | Bottom-up | Recompute requiring measure | Intrinsic and subtree measure caches |
| Resolve | `resolve_element` | Top-down | Recompute requiring resolve | Resolve cache |
| Render scene | `build_element_subtree` | Top-down with child recursion | Every refresh | Retained render-layer scene cache |
| Event registry | `build_registry_rebuild_cached` | Top-down with deferred nearby | Registry damage | Registry subtree cache and whole cached rebuild |
| Renderer scene draw | `render_nodes_with_cache_tracking` | Render-node tree | Every rendered scene | Renderer paint-layer payload cache |
| Visible hash/resource generation | `hash_visible_render_nodes` and resource-generation helpers | Render-node tree | Cache admission/keying | Font/image generation |
| Dirty cleanup | `clear_refresh_dirty` or `clear_render_refresh_dirty` | Tree | After refresh | Dirty flags |

## Cache Inventory

| Cache | Stored on | Key inputs | Invalidated by | Reuses |
|-------|-----------|------------|----------------|--------|
| Intrinsic measure | Element layout state | Kind, text/media/font/input attrs | Measure/structure damage or key change | Leaf intrinsic size |
| Subtree measure | Element layout state | Attr subset, inherited font, constraints, topology | Measure/structure damage or key change | Measured subtree frames |
| Resolve | Element layout state | Resolve attrs, available space, constraints, topology | Resolve/measure/structure damage or key change | Final frames and scroll/nearby geometry |
| Registry subtree | Element refresh state | Registry attrs/runtime/frame/hover/scene/scroll/topology | Registry damage or key change | Event registry chunks |
| Whole registry rebuild | `TreeUpdateEngine` | Last clean rebuild | Any registry rebuild replacement | Registry-only response and clean refresh |
| Render-layer scene | Element refresh state | Paint generation, topology, bounds | Render damage or key change | `RenderPaintLayer` scene fragment |
| Renderer paint payload | Renderer cache manager | Stable id, content generation, pixel size, scale, resource generation | Store budget, stale frames, resource/content change | GPU/CPU image payload |

## Correctness Invariants

- Layout and render read `layout.effective`, not the unscaled declared attrs.
- Patches write unscaled declared attrs; preparation applies scale each frame.
- A paint-layer payload is content plus independent placement. Position and
  alpha can change at composition time without changing cached content.
- A parent layer owns only `own_nodes`; nested layers are `child_refs`.
- Child layer references must survive parent invalidation and redraw.
- Registry cache reuse is legal only when no registry damage exists and the
  subtree key matches the current scene and scroll context.
- Renderer cache hits are an optimization only. Bypass or rejection must fall
  back to direct drawing without changing output.
- Cached layer bounds must include the visual content that belongs to the layer,
  but should not include unrelated parent clips except as composition clips.

## Source Map

Primary files:

- `native/emerge_skia/src/runtime/tree_actor.rs`: tree actor loop and output
  publication.
- `native/emerge_skia/src/runtime/tree_update.rs`: message batching,
  invalidation, refresh decision, and animation sync.
- `native/emerge_skia/src/tree/invalidation.rs`: invalidation ordering and
  refresh decision.
- `native/emerge_skia/src/tree/element.rs`: tree state, dirty flags, layout
  caches, registry caches, and retained render-layer scene cache.
- `native/emerge_skia/src/tree/patch.rs`: patch application and patch
  invalidation.
- `native/emerge_skia/src/tree/layout.rs`: frame attr preparation, measure,
  resolve, refresh, and benchmark profiling entry points.
- `native/emerge_skia/src/events/registry_builder.rs`: event registry traversal
  and registry subtree cache.
- `native/emerge_skia/src/tree/render.rs`: tree-to-render-scene traversal and
  paint-layer boundary construction.
- `native/emerge_skia/src/render_scene.rs`: render scene nodes, paint layer
  model, own-node/child-ref split, and layer metrics.
- `native/emerge_skia/src/renderer.rs`: render-node traversal, paint-layer
  payload cache, cache stats, and Skia drawing.
