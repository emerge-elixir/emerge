# Render Cache Comparison: Emerge vs Flutter

Last updated: 2026-04-28.

Status: investigation/reference. This compares the current Emerge renderer
cache implementation with Flutter's Skia/Ganesh raster cache in the locally
cloned Flutter sources. It is not an implementation plan.

Primary local sources:

- Emerge:
  - `native/emerge_skia/src/renderer.rs`
  - `native/emerge_skia/src/render_scene.rs`
  - `native/emerge_skia/src/tree/render.rs`
  - `native/emerge_skia/src/stats.rs`
  - `native/emerge_skia/benches/renderer.rs`
- Flutter framework:
  - `/workspace/tmp-layout-engines/flutter/packages/flutter/lib/src/rendering/object.dart`
  - `/workspace/tmp-layout-engines/flutter/packages/flutter/lib/src/rendering/proxy_box.dart`
- Flutter engine:
  - `/workspace/tmp-layout-engines/flutter_engine/flow/raster_cache.h`
  - `/workspace/tmp-layout-engines/flutter_engine/flow/raster_cache.cc`
  - `/workspace/tmp-layout-engines/flutter_engine/flow/raster_cache_key.h`
  - `/workspace/tmp-layout-engines/flutter_engine/flow/raster_cache_util.h`
  - `/workspace/tmp-layout-engines/flutter_engine/flow/layers/display_list_raster_cache_item.cc`
  - `/workspace/tmp-layout-engines/flutter_engine/flow/layers/layer_raster_cache_item.cc`
  - `/workspace/tmp-layout-engines/flutter_engine/flow/layers/layer_tree.cc`

## Executive Summary

Emerge's current renderer cache is now recognizably Flutter-shaped:

- explicit per-frame lifecycle
- candidate discovery before drawing
- repeated-visibility admission
- bounded payload creation per frame
- GPU render-target payloads when a GPU context is available
- CPU raster fallback for raster/offscreen paths
- cached-image draw on hits
- direct-render fallback on misses, rejection, or prepare failure
- visible stats for hits, misses, stores, payload kinds, bytes, and rejections

The main difference is scope. Flutter has a mature raster cache for both layers
and display lists, with framework concepts such as repaint boundaries,
composited opacity layers, `isComplex`, and `willChange` feeding engine
heuristics. Emerge currently has one production cache kind: automatic
`CleanSubtree` candidates emitted from the retained render tree.

That narrower scope is reasonable for Emerge. The retained tree, typed
dirtiness, layout caches, and render-subtree caches give Emerge stronger
knowledge about clean local content than Flutter often has at the framework and
engine boundary. The current implementation uses that knowledge well for
integer translation, layout-reflow movement, and root element-alpha composition.

The main remaining differences compared to Flutter are:

- layer/display-list split, including a `cache children` mode
- transform policy beyond integer translation
- complexity scoring beyond node count and eligibility rules
- Flutter-style immediate unseen-entry eviction; Emerge now uses conservative
  stale-frame aging instead
- broader layer semantics for nested alpha/filter-like composition
- platform-view/RTree-style partial blit preservation

## Flutter Raster Cache Shape

Flutter's raster cache sits in the engine, below the Dart render tree and layer
tree.

Framework side:

- `RenderObject.isRepaintBoundary` gives a render object its own composited
  layer.
- `PaintingContext.paintChild` composites repaint-boundary children instead of
  painting them into the parent recording.
- `PaintingContext.repaintCompositedChild` repaints a dirty repaint boundary
  into its retained layer while reusing clean child layers.
- `RenderOpacity` and `RenderAnimatedOpacity` update an `OpacityLayer` alpha
  without repainting child content when possible.
- `RenderDecoratedBox` can set an `isComplex` hint when a decoration is complex.
- `RenderRepaintBoundary` exposes debug usefulness counters.

Engine side:

- `RasterCache` owns entries keyed by `RasterCacheKeyID` plus a normalized
  matrix.
- `RasterCacheItem` has `kNone`, `kCurrent`, and `kChildren` states.
- `DisplayListRasterCacheItem` handles display-list/picture payloads.
- `LayerRasterCacheItem` handles full-layer or layer-children payloads.
- `LayerTree::TryToRasterCache` prepares cache items before paint and suppresses
  child preparation when the parent was successfully cached.
- `RasterCache::Rasterize` creates a transparent offscreen surface, using a GPU
  render target when `GrDirectContext` is available and a raster surface
  otherwise.
- `RasterCache::Draw` draws the cached `DlImage`, optionally preserving RTree
  regions instead of drawing one large image.

Flutter lifecycle:

1. During preroll, layers and display lists add cache items.
2. Preroll finalization marks entries as seen/visible and decides cache state.
3. Unseen entries are evicted.
4. A bounded number of eligible display-list entries can be prepared this frame.
5. Paint draws cached images where available, otherwise paints normally.
6. End frame updates timeline metrics for layer and picture counts/bytes.

Important Flutter policies:

- default raster-cache access threshold is 3
- default display-list cache creation limit is 3 per frame
- display lists are rejected when `willChange` is true or bounds are invalid
- display lists are cached when explicitly complex or complexity scoring says
  they are worth caching
- layer cache can choose current layer or children, which is especially useful
  when the layer's own effect changes but child content is stable
- keys ignore pure integer translation and include the remaining matrix shape
- scale+translation transforms can be snapped to integer translation for pixel
  alignment

## Emerge Renderer Cache Shape

Emerge currently emits `RenderNode::CacheCandidate(RenderCacheCandidate)` during
tree rendering. The implemented kind is `CleanSubtree`.

Candidate payload:

- `stable_id` from retained `NodeId`
- `content_generation` from the localized render-node subtree
- local `bounds`
- localized child `RenderNode`s for direct fallback and payload preparation

Content key:

- `stable_id`
- `content_generation`
- rounded local width and height
- `scale_bits`
- `resource_generation`

Composition state intentionally excluded from the content key:

- integer placement
- layout-reflow movement when local content and size are unchanged
- root element alpha for caches rooted at that element

Current eligibility:

- finite, non-empty bounds
- not a focused text input
- not inside scroll-offset scene context
- not a scroll container
- not direct `Text`, text input, image, video, `None`, or paragraph element root
- integer translation only
- no rotate/scale on the element
- no `ShadowPass`, `Transform`, `Alpha`, video, loading image, failed image, or
  nested cache candidate inside candidate children
- child primitives may include rects, rounded rects, borders, shadows, text with
  font, gradients, and ready images

Lifecycle:

1. `SceneRenderer` starts a renderer-cache frame.
2. Traversal sees clean-subtree candidates.
3. Candidate resource generations are computed from font and ready-image asset
   generations.
4. Candidate visibility is counted.
5. Warm hits draw the stored `Image`.
6. Misses direct-render until the repeated-visibility threshold is met.
7. Once admitted and budget is available, the renderer prepares a payload before
   direct fallback.
8. GPU frames prepare into an offscreen GPU render target; raster/offscreen
   frames prepare a CPU raster image.
9. If preparation succeeds, the new image is drawn in the same frame and stored.
10. If preparation fails, is over budget, or is ineligible, the subtree is
    direct-rendered.
11. Parent hits or parent prepares touch existing descendant cache entries as
    suppressed by parent, so child entries stay alive without preparing or
    drawing.
12. End frame evicts entries that have not been seen for the stale-frame window
    and reports current entries, bytes, payload counts, hit/draw timing,
    prepare timing, fallbacks, and rejection reasons.

Current defaults:

- new payloads per frame: 1
- minimum visible frames before store: 2
- max entries: 128
- max bytes: 32 MiB
- max entry bytes: 4 MiB
- bytes per pixel estimate: 4
- stale-frame window: 120 frames

## Direct Comparison

### Candidate Source

Flutter:

- candidates come from explicit framework/layer structure and engine display
  lists
- developers can influence candidate boundaries with `RepaintBoundary`
- display-list hints include `isComplex` and `willChange`

Emerge:

- candidates are automatic and generated from retained native tree/render state
- no public cache hint exists
- layout/render cleanliness can feed candidate confidence directly

Assessment:

Emerge's automatic boundary is simpler and less user-tunable, but it can be more
accurate for the common Emerge case because retained ids, layout dirtiness, and
render-subtree content generation are already known in the native pipeline.

### Payload Model

Flutter:

- stores `DlImage`
- uses GPU render target if `GrDirectContext` exists
- falls back to raster surface otherwise
- draws with optional paint, including opacity/filter composition paths
- can preserve RTree subrects for platform-view/overlay correctness

Emerge:

- stores `skia_safe::Image`
- uses GPU render target if the render frame exposes a `DirectContext`
- falls back to CPU raster only for raster/offscreen or GPU prepare failure
- draws the whole cached image at local placement
- excludes video and loading/failed image placeholders

Assessment:

The core payload model now matches Flutter's important GPU-first behavior. The
biggest Flutter feature Emerge does not yet mirror is RTree/subrect draw
preservation. That is probably not urgent until Emerge has platform-view or
overlay constraints similar to Flutter's.

### Key Model

Flutter:

- key is identity plus normalized matrix
- pure translation is removed from the matrix key
- rasterization uses rounded device bounds under an integral transform
- display-list identity is a unique id
- layer children can produce a composite key from child ids

Emerge:

- key is retained id plus content generation, local size, scale bits, and
  resource generation
- placement is separate and currently accepted only for integer translation
- render-subtree cache keys also include asset source status generation, so a
  pending placeholder cannot survive after asset resolution

Assessment:

Emerge's key is more content-explicit than Flutter's for the current cache kind.
That is a benefit of retained native data. Flutter's matrix handling is more
mature: it has a clear policy for scale+translation and pixel snapping. Emerge
should not copy that until parity tests cover fractional placement, scale, and
rotate, but Flutter gives a good design target.

### Admission and Budgeting

Flutter:

- access threshold defaults to 3
- display-list cache creation limit defaults to 3 per frame
- rejects `willChange`, invalid bounds, invisible/cull cases, platform views,
  and texture layers in relevant layer paths
- display-list complexity calculator decides if non-hinted content is worth
  caching
- unseen entries are evicted every frame

Emerge:

- visible threshold is 2
- new payload budget defaults to 1 per frame
- rejects unsupported primitives and transforms conservatively
- entry and total byte budgets are configurable through `EmergeSkia.start/1`
- eviction is LRU under entry/byte budget pressure
- entries that are not visible or parent-suppressed age out after a conservative
  stale-frame window

Assessment:

Emerge is stricter on creation rate and has explicit byte limits, which is good
for first-use jank and memory control. Flutter is stricter about stale entries:
anything not encountered in a frame is removed. Emerge should consider adding a
shorter stale policy later, but immediate unseen eviction is still likely too
aggressive for Emerge's automatic candidates during resize, interaction, and
parent-cache hit frames.

### Parent/Child Cache Interaction

Flutter:

- when a parent item prepares successfully, `LayerTree::TryToRasterCache`
  iterates over child cache items with `parent_cached=true`
- child items are touched/suppressed so they do not also prepare payloads under
  a parent cache
- `LayerRasterCacheItem` can switch between caching the current layer and
  caching children

Emerge:

- a parent clean-subtree hit or successful prepare returns before drawing child
  payloads
- existing descendant clean-subtree entries are touched as
  `suppressed_by_parent`
- child payloads are not prepared under a parent hit or same-frame parent store
- stale eviction keeps parent-suppressed child entries alive
- there is no explicit `cache children` state

Assessment:

Emerge now has the lifecycle semantics needed for stale aging without copying
Flutter's full layer cache model. The next jump to explicit parent-owned child
key lists or a `cache children` state should still be benchmark-gated; the
current descendant walk was neutral to better in the parent-hit benchmark.

### Alpha and Composition

Flutter:

- opacity is a layer property (`OpacityLayer`)
- animated opacity can update composited layer alpha without repainting child
  content
- raster cache can draw a cached child image while caller/layer applies opacity
- layer-children cache helps when child content is stable but the containing
  effect changes

Emerge:

- root element alpha is treated as composition state for a cached subtree
- root alpha stays out of the payload key and is applied while drawing the
  payload image
- descendant alpha inside a cached ancestor remains conservative and can
  invalidate/reject the candidate
- a dedicated nested-alpha children-cache kind was benchmarked as a candidate
  design but not implemented because the GPU microbench did not beat direct
  drawing

Assessment:

Emerge already captured the most important Flutter lesson for app-selector and
todo fade/translate cases: alpha on a cache root should be composition state.
Flutter's next lesson is still `cache children`, but this pass showed that a
small alpha-only boundary is not automatically worth the extra renderer path.
Future composition-cache work needs a stronger workload before it lands.

### Transform Policy

Flutter:

- matrix participates in the key after translation normalization
- scale+translation can be snapped to integral translation for cache alignment
- non-trivial transforms are guarded by matrix/key behavior and tests

Emerge:

- current production eligibility is integer translation only
- rotate and scale stay direct-rendered
- fractional translation is rejected

Assessment:

Emerge is intentionally simpler and safer. The next transform expansion should
not be "support everything"; it should start with Flutter's scale+translation
snap policy and a benchmark/parity suite for fractional translation and scale.
Rotate should remain a later candidate because cached-image sampling and bounds
growth can easily hide quality regressions.

### Invalidation

Flutter:

- layer/display-list identity and matrix form the key
- changed display lists/layers naturally produce different ids or cache states
- unseen entries are removed
- texture/platform-view cases are rejected in layer paths

Emerge:

- retained `NodeId` plus content generation invalidates changed local pixels
- resource generation invalidates fonts and ready images
- render-subtree keys include asset status generation for pending/ready/failed
  source transitions
- video and loading/failed placeholders are excluded from renderer-cache
  payloads
- global cache clear handles context/resource reset paths

Assessment:

Emerge's typed invalidation is stronger for the current clean-subtree cache.
The asset status generation fix is a good example: the render-subtree cache now
knows that pending-vs-ready image status is part of render output, even though
the declared image source did not change.

### Stats and Observability

Flutter:

- timeline metrics report layer/picture count and MB
- repaint-boundary debug counters show usefulness in debug tooling
- engine tests inspect cache state and metrics

Emerge:

- renderer stats report candidates, visible candidates, admitted, hits, misses,
  suppressed-by-parent touches, stores, budget evictions, stale evictions,
  rejected, current entries/bytes, payload kind counts, prepare
  success/failure, rejection reasons, fallback-after-admission, and hit draw
  timing
- frame stats also split render, draw, GPU flush, submit, present, and full
  patch-to-frame-callback pipeline timing
- slow-frame logs include scene summary and draw-category detail

Assessment:

Emerge's runtime stats are more actionable for current performance work. They
make it easier to reject a cache that improves draw time but regresses GPU
flush, present, or full pipeline time.

## Where Emerge Is Already Better Aligned

- It does not need a broad user-facing `RepaintBoundary` equivalent yet because
  retained tree identity and layout/render dirtiness produce automatic clean
  subtree boundaries.
- Cache budgets are configurable through `EmergeSkia.start/1`.
- Rejection reasons and payload kinds are visible in normal renderer stats.
- Full-pipeline timing is part of the same investigation surface as cache hit
  timing.
- Root element alpha composition directly targets common Emerge animation
  patterns.
- Parent-cache hits now keep existing child entries alive while suppressing
  child preparation.
- Stale entries age out conservatively instead of living until budget pressure.
- The cache plan is benchmark-gated; rejected optimizations stay out.

## Where Flutter Is Ahead

- It has two mature cache families: display-list/picture and layer/children.
- It has a layer-children mode for stable children under changing effects.
- It has a more developed matrix and pixel-snapping policy.
- It has complexity scoring for display lists instead of only structural
  eligibility and node counts.
- It evicts unseen entries every frame.
- It preserves RTree information when drawing cached content above platform
  views.
- Framework debug tooling can report whether repaint boundaries are useful.

## Deeper Investigation For The Next Active Plan

The next Emerge plan should focus on Flutter's cache lifecycle and children
cache semantics, not on broad transform support or new primitive caches.

### Frame lifecycle and stale entries

Flutter's `RasterCache` entry has `encountered_this_frame`,
`visible_this_frame`, `accesses_since_visible`, and an optional image payload.
`MarkSeen` sets the frame flags and increments access count for visible entries.
`EvictUnusedCacheEntries` removes every entry that was not encountered before
the paint stage prepares new payloads, and `EndFrame` clears the encountered
flags after metrics are collected.

Local source points:

- `/workspace/tmp-layout-engines/flutter_engine/flow/raster_cache.h`
- `/workspace/tmp-layout-engines/flutter_engine/flow/raster_cache.cc`
- `/workspace/tmp-layout-engines/flutter_engine/flow/raster_cache_unittests.cc`

Emerge already has `visible_count`, `first_visible_frame`,
`last_visible_frame`, `last_used_frame`, byte budgets, and LRU eviction under
budget pressure. It does not yet have per-frame encountered state or stale-entry
eviction. Adding stale eviction is useful, but only after parent cache hits can
keep suppressed child entries alive. Otherwise a warm parent payload can prevent
child traversal and accidentally age out useful nested payloads.

### Parent-cached suppression

Flutter stores raster-cache items in preroll order. During preparation,
`LayerTree::TryToRasterCache` attempts a parent item first. If the parent
prepares successfully, Flutter walks the parent's child cache items and calls
`TryToPrepareRasterCache(..., parent_cached=true)`. The child items were already
seen during preroll, but `parent_cached=true` suppresses child payload creation
for that frame.

Local source points:

- `/workspace/tmp-layout-engines/flutter_engine/flow/layers/layer_tree.cc`
- `/workspace/tmp-layout-engines/flutter_engine/flow/layers/display_list_raster_cache_item.cc`
- `/workspace/tmp-layout-engines/flutter_engine/flow/layers/layer_raster_cache_item.cc`

Emerge currently returns immediately when a clean-subtree payload hits or is
prepared, so descendant render nodes are not traversed. That is fine without
stale eviction. Once stale eviction exists, a parent hit must either touch known
descendant cache entries or record that they were suppressed by a parent hit.

### `kChildren` layer state

Flutter's `RasterCacheItem::CacheState` has `kCurrent` and `kChildren`.
`LayerRasterCacheItem` initially caches children for filter/opacity-like layers
when `can_cache_children` is true, then can switch to caching the full current
layer after the layer itself proves stable for enough frames. The children key
is built from ordered child cache ids, not from the changing layer effect.

Local source points:

- `/workspace/tmp-layout-engines/flutter_engine/flow/raster_cache_item.h`
- `/workspace/tmp-layout-engines/flutter_engine/flow/layers/layer_raster_cache_item.cc`
- `/workspace/tmp-layout-engines/flutter_engine/flow/layers/opacity_layer_unittests.cc`
- `/workspace/tmp-layout-engines/flutter_engine/flow/layers/color_filter_layer_unittests.cc`
- `/workspace/tmp-layout-engines/flutter_engine/flow/layers/image_filter_layer_unittests.cc`

This maps directly to Emerge's remaining alpha gap. Emerge should not make
`Alpha` generally cacheable inside a clean-subtree payload. Alpha changes the
composition of a subtree and overlapping children require group-alpha semantics.
Instead, the tree/render pipeline should split cache boundaries at compositing
scopes:

- cache stable children as transparent local content
- draw that payload inside the existing alpha/composition scope
- keep ancestor clean-subtree candidates conservative when they contain nested
  compositing scopes

This is the Flutter `kChildren` lesson adapted to Emerge's simpler retained
tree.

### Transform discipline

Flutter's cache key removes pure translation from the matrix and preserves the
remaining matrix shape. For rasterization and drawing, it snaps translation to
integral device pixels only when the matrix is scale-and-translation only. It
avoids snapping for complex transforms because that can create artifacts.

Local source points:

- `/workspace/tmp-layout-engines/flutter_engine/flow/raster_cache_key.h`
- `/workspace/tmp-layout-engines/flutter_engine/flow/raster_cache_util.h`
- `/workspace/tmp-layout-engines/flutter_engine/flow/raster_cache_util.cc`

Emerge should keep rotate/scale/fractional placement out of the immediate
implementation plan. The right next step is benchmark and parity coverage for
fractional translation and scale+translation, then a separate transform plan if
those tests show a real win. This should not block the children-cache work.

### Complexity scoring

Flutter accepts explicit `isComplex` and `willChange` hints from framework
rendering code and uses `DisplayListComplexityCalculator` for display-list
payload decisions. Emerge has no public render-cache hints, and that is still
the right default. Emerge can use stronger native information instead:
render-node count, primitive mix, previous direct-render cost for a stable id,
and layout/render-subtree cache confidence.

Local source points:

- `/workspace/tmp-layout-engines/flutter/packages/flutter/lib/src/rendering/custom_paint.dart`
- `/workspace/tmp-layout-engines/flutter/packages/flutter/lib/src/rendering/proxy_box.dart`
- `/workspace/tmp-layout-engines/flutter_engine/flow/layers/display_list_raster_cache_item.cc`

This is a later tuning step. The current simple repeated-visibility admission
has not yet been shown to admit too many cheap entries or reject useful ones.

## Recommendations

The lifecycle and parent/child recommendations below were implemented in
`active-render-cache-children-plan.md`. The alpha children-cache expansion was
measured and rejected for this slice.

### 1. Do not broaden eligibility without a fresh plan

The current clean-subtree cache is in a good narrow state. New eligibility for
scale, rotate, fractional placement, descendant alpha, active input, or video
should start with a fresh plan document and benchmark/parity gates.

### 2. Keep stale-entry policy tied to parent-child semantics

Flutter evicts entries that are not encountered in the current frame. Emerge now
uses conservative stale aging because automatic candidates can temporarily leave
the visible traversal during interaction and resize.

Implemented Emerge design:

- `last_seen_frame` aging with a `max_stale_frames` window
- parent payload hits and same-frame stores touch descendant entries as
  suppressed-by-parent
- byte/entry budget eviction remains the hard cap
- stale evictions and suppressed-by-parent counts are visible in renderer-cache
  stats

### 3. Revisit `cache children` only with a stronger benchmark

Flutter's `kChildren` layer state is the best model for Emerge's next alpha
work. Instead of allowing any descendant alpha inside an ancestor payload, a
future nested composited/cache boundary should:

- cache stable children as local transparent content
- apply the changing alpha/filter/effect during composition
- keep ancestor payload invalidation conservative until this boundary exists

The first nested-alpha microbench was not enough to land a new production cache
kind. Keep the current root-alpha cache behavior and require a target workload
that beats direct drawing before adding the extra path.

### 4. Copy Flutter's transform discipline, not broad transform support

For transforms, the useful Flutter lesson is pixel alignment:

- normalize placement out of the content key
- snap scale+translation only when it is visually stable
- keep complex transforms direct-rendered until parity proves otherwise

Emerge should next test fractional translation and scale+translation. Rotate is
lower priority because image sampling, expanded bounds, and antialiasing can
make cached-vs-direct parity subtle.

### 5. Replace node-count heuristics with measured complexity

Flutter uses display-list hints and complexity scoring. Emerge can do better by
combining:

- render-node count
- primitive mix
- text/image/shadow counts
- previous direct-render draw cost for a stable id
- layout-cache/render-subtree hit confidence

Do this only after live stats show that the current simple threshold admits too
many cheap entries or rejects useful ones.

### 6. Keep RTree/platform-view preservation deferred

Flutter's RTree preservation matters for platform views and overlay
composition. Emerge currently excludes video and does not have the same platform
view overlay model in this cache path. Do not add RTree-like complexity until a
concrete backend feature needs it.

## Bottom Line

Emerge's current render cache is a deliberately smaller Flutter raster cache:
GPU-first payloads, repeated-use admission, prepare-before-draw, direct fallback,
and stats are in place. The narrower clean-subtree model is a strength while the
system is young because it exploits Emerge's retained native tree and keeps
cache invalidation understandable.

The Flutter lessons now adopted are stale-entry lifecycle and explicit
parent/child cache accounting. A layer-children model for changing composition
effects remains a useful design target, but the first alpha-only benchmark did
not justify implementing it. Transform expansion, complexity scoring, and
RTree-like partial blit preservation should wait until stronger benchmarks call
for them.
