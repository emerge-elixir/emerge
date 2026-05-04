# Rendering Cache Engine Investigation

Last updated: 2026-04-28.

This document investigates retained renderer-cache approaches used by other
UI/rendering engines, then maps them onto Emerge's current renderer. Direct
non-cache draw-path work is covered by
`drawing-optimization-investigation.md`; this document owns renderer-thread
caches such as prepared primitive output, pictures, raster layers, dirty
regions, and tiles. It is not an active implementation plan. It is a reference
document for deciding the next renderer-performance slice.

For a focused post-implementation comparison between Emerge's current
clean-subtree cache and Flutter's Skia/Ganesh raster cache, see
`render-cache-flutter-comparison.md`. The implemented follow-up record from
that comparison is `active-render-cache-children-plan.md`.

## Why this exists

Recent `../emerge_demo` showcase/assets-page traces show that layout and refresh
are no longer the only interesting costs. Some frames are now dominated by
renderer work:

```text
render=8.384 ms draw=2.445 ms flush=5.939 ms gpu_flush=5.935 ms
draws={shadows=2 inset_shadows=1 images=0 texts=10}
draw detail: shadows=2.012 ms inset_shadows=0.046 ms texts=0.289 ms images=0.000 ms
present submit=5.933 ms
```

A later assets-page frame showed a different shape:

```text
render=4.482 ms draw=2.155 ms flush=2.327 ms gpu_flush=2.310 ms
draws={borders=17 texts=115 images=0}
draw detail: clips=0.174 ms borders=0.939 ms texts=0.973 ms
```

The first case is a large blurred shadow plus GPU/present synchronization. The
second case is many text and border draws under many clips. Emerge already has
retained layout and retained render-scene construction, but the render thread
still replays the final `RenderScene` to the Skia canvas each rendered frame.

## Current Emerge Baseline

Relevant files:

- `native/emerge_skia/src/render_scene.rs`
- `native/emerge_skia/src/renderer.rs`
- `native/emerge_skia/src/tree/render.rs`
- `native/emerge_skia/src/tree/element.rs`
- `native/emerge_skia/src/stats.rs`

Current rendering shape:

- `RenderScene` is a tree of `RenderNode::Clip`, `RelaxedClip`, `Transform`,
  `Alpha`, `ShadowPass`, and `Primitive`.
- Refresh can reuse clean retained render subtrees while constructing the scene.
  This avoids rebuilding `Vec<RenderNode>` for clean subtrees, but it does not
  skip drawing those nodes once the scene reaches the render thread.
- The renderer draws the full scene every frame, clears the target surface, then
  flushes/submits the Skia GPU context.
- Current resource caches include:
  - decoded raster assets
  - parsed SVG assets
  - rendered SVG variants keyed by asset id and rasterized size
  - fonts/typefaces
  - Skia's internal caches via `skia_safe::graphics::purge_all_caches()` reset
- Slow-frame diagnostics now split render total, draw, GPU flush, GPU submit,
  present submit, scene summary, draw categories, per-image detail, and
  per-shadow detail.

Important distinction:

- **Render subtree cache** currently caches render-scene construction output.
- **Picture/layer/raster caches** would cache renderer-thread drawing work.

The latter is what the latest traces are asking for.

## Companion Boundary

`drawing-optimization-investigation.md` owns the direct-renderer baseline:
primitive selection, path/clip reduction, `saveLayer` reduction, template tint,
opacity distribution, direct shadow alternatives, shader/pipeline warmup, and
backend/context options. The first active drawing plan has completed and its
landed/rejected decisions are folded into that investigation document.

This document starts where retained renderer output can add benefit beyond the
current direct renderer. A cache should only be selected for a hot category after
one of these is true:

- the matching direct draw-path optimization has landed and repeated cost
  remains
- the matching direct draw-path optimization has been benchmarked and rejected
- the matching direct draw-path optimization has been explicitly deferred because
  it is larger or riskier than the cache pilot

Cache work must compare against the current direct renderer, not against an
unoptimized path that a simpler primitive change would fix.

## Emerge Fit and Benefit Pass

The most useful transfer from the surveyed engines is not a single cache type.
It is the shared contract they apply before caching anything: clear ownership,
typed keys, explicit invalidation, bounded memory, visible stats, cheap clearing,
and cached-vs-direct correctness tests.

Emerge established those renderer-cache principles first, then selected the
clean-subtree raster/texture payload as the first broad pilot because the latest
post-drawing-pass traces were complex-subtree and GPU-flush shaped rather than
shadow-only shaped. Shadows, text blobs, retained pictures, dirty regions, and
tiles remain later cache types that should reuse the same contract instead of
creating one-off systems.

### First fit: renderer-cache contract

The first implementation work should define the renderer-cache boundary:

- layout caches continue to answer measurement and resolved-layout questions
- render-subtree caches continue to reuse `RenderScene` construction output
- renderer caches live under `SceneRenderer` and skip or simplify Skia drawing
- cache keys use typed render inputs and generation counters, not debug strings
- content identity and compositing state are kept separate where moving,
  clipping, opacity, or transforms can change independently
- every cache has hit, miss, store, eviction, byte, and current-size stats
- every cache has a hard count or byte budget and deterministic eviction
- context loss, backend reset, asset/font generation changes, and explicit
  global cache clearing have a single clearing path
- every output cache gets cached-vs-direct raster parity tests

This contract is the common part of Flutter raster cache heuristics, Slint item
cache generations, Iced explicit geometry caches, and WebRender picture/tile
budgets.

### Pilot candidates after the contract and direct baseline

The first concrete cache was chosen by measured benefit, by how well it
exercised the common model, and by whether the same cost still existed after the
direct-rendering pass. The completed first cache is the clean-subtree
raster/texture payload. It exercises content-vs-composite key separation,
retained element identity, layout-cache signals, admission, byte budgets,
cached-vs-direct parity, and GPU payload behavior.

Other useful candidates remain:

- **box-shadow textures:** compact key and strong pixel-test story if fresh
  traces again show repeated blurred-shadow cost
- **text blobs:** useful for code-heavy/static-label scenes, but font fallback
  and active text-input exclusions make the key subtler
- **Skia pictures for clean subtrees:** good fit for existing retained
  render-subtree identity, but less likely to reduce GPU blur/flush cost
- **simple raster layers:** broad benefit but higher stale-pixel and memory risk

Shadow caching is therefore a later primitive pilot, not the first implemented
pilot.

### Companion baseline: direct draw optimization

Border, text, clip, tint, alpha, shadow, warmup, and backend-option diagnostics
remain important, but the non-cache implementation belongs in
`drawing-optimization-investigation.md`.

Cache work should not absorb those direct fixes. If direct draw work removes a
cost center, skip the matching cache. If repeated cost remains, use this
document to decide whether retained output is worth the additional invalidation,
memory, and first-use complexity.

### Defer: backend-level redraw caches

Flutter-style raster layers, Slint-style dirty regions, and WebRender-style tile
caches are all relevant to Emerge, but dirty regions and tiles need broader
backend integration:

- raster layers need robust volatile-subtree exclusion and stale-pixel tests
- dirty regions depend on backend buffer preservation and surface damage
  semantics, especially for Wayland/EGL
- tile caches need scroll-container identity, content-space invalidation, clip
  chains, and fallback behavior when fragmentation is high

These should remain later stages after the renderer-cache contract, stats,
byte budgets, and context-loss clearing model are proven.

## Additional Engine-Derived Requirements

This pass looked for concrete mechanics that should become design requirements
before Emerge implements renderer caches.

### Frame lifecycle must be explicit

Flutter's raster cache is not a passive map lookup. It has a frame lifecycle:
begin frame, discover/mark cache candidates during preroll, evict unused
entries, prepare a limited number of new cache images, draw hits during paint,
then update metrics at end frame.

Emerge should use the same shape for renderer caches:

- `begin_frame` resets per-frame cache stats and store counters
- scene traversal marks cache candidates as encountered and visible/invisible
- entries can exist before they have a rendered payload, so repeated visibility
  can be counted before paying cache creation cost
- `prepare` builds at most a bounded number of expensive payloads per frame
- draw hits use cached output; misses always fall back to direct drawing
- `end_frame` records metrics and evicts or ages entries not encountered

This keeps cache creation from adding a new jank source and makes first-use
hitches separate from steady-state cache benefit.

### GPU payloads should be first-class

Flutter creates raster-cache payloads as GPU render targets when a GPU context is
available and falls back to CPU raster surfaces otherwise. That matters for
Emerge because Wayland, DRM, and macOS rendering are GPU-first: a CPU-backed
cache image can improve CPU draw time while moving the real cost into texture
upload or `gpu_flush`.

Emerge requirements:

- GPU render-target payloads should be the production default whenever
  `RenderFrame` has a `DirectContext`
- CPU raster payloads should remain as a raster/offscreen fallback and
  correctness-test harness
- payload stats must distinguish GPU and CPU preparation/hits so a cache win
  cannot hide upload or flush cost
- GPU payloads must clear on context reset, backend surface recreation, scale
  change, video/resource reset, and explicit cache clear
- GPU and CPU payload paths need separate parity and benchmark coverage
- store-frame benchmarks must include the full cost of render-target creation,
  snapshotting, draw, flush, submit, and present

### Prepare before direct fallback

Flutter's admitted raster-cache entries are prepared before paint and then drawn
from cache during paint if preparation succeeds. Emerge's first clean-subtree
payload implementation proved correctness but performs direct subtree rendering
and then separately rasterizes the same subtree into the cache on the admitted
store frame.

Emerge requirements:

- once a candidate is admitted and has budget, attempt payload preparation
  before direct fallback
- if preparation succeeds, draw the newly prepared payload in that same frame
  and skip direct subtree rendering
- if preparation fails, direct-render exactly as the old path would
- stats must show prepare success/failure and whether the admitted frame drew a
  new payload or fell back
- a parent payload prepared in the frame should suppress child payload
  preparation unless a child cache has an explicit independent reason to exist

### Route through an offscreen target, not the framebuffer

Emerge should not build GPU cache payloads by copying pixels after the subtree
has already been drawn to the window surface. That would cache final composited
output rather than transparent local subtree content, so background pixels,
siblings, clips, alpha, and draw order could leak into the cache. It also risks
turning a cache store into a GPU synchronization/readback problem, and the exact
cost and semantics vary by backend and swapchain/presentation surface.

The GPU-first store frame should instead route admitted subtree drawing through
the cache target:

- allocate a transparent offscreen GPU render target for the rounded local
  physical bounds
- draw the localized subtree into that target before the subtree is composited
  into the main scene
- snapshot/store the GPU-resident image
- draw that image to the main canvas in the same frame when preparation
  succeeds
- fall back to the existing direct subtree draw if preparation fails or exceeds
  budget

### Element alpha can reuse the same subtree payload

The offscreen-target model also makes a common Emerge animation case cacheable:
unchanged subtree content with changing opacity. In Emerge, alpha on an element
is composition state for that element's subtree. A transparent GPU payload for
that subtree can be drawn to the main canvas with a different alpha each frame,
so the payload key for that subtree does not need to include the element alpha.
If a descendant element's alpha changes inside a cached ancestor, the renderer
must either preserve that descendant as its own composited/cache boundary or
invalidate the ancestor payload; otherwise the ancestor image would contain
stale precomposited pixels.

Emerge requirements:

- model element alpha as subtree composition state in render-scene metadata
- keep an element's alpha out of the payload content key for caches rooted at
  that element, because alpha is applied while drawing the cached image
- include element alpha in the cache hit draw path, not the payload prepare path
- when descendant alpha changes under a cached ancestor, create/use a nested
  composited boundary or invalidate the ancestor payload
- require cached-vs-direct pixel parity for overlapping content, clips, shadows,
  text, and nested transparent children before production eligibility
- benchmark `../emerge_demo` app-selector menu fade and todo-entry
  translate+fade animations, because those should become cheap element-alpha
  cache hits when the subtree content is unchanged
- if benchmarks do not show full-frame improvement, keep the simpler direct
  renderer path and document that result near the benchmark

### Admission needs thresholds and throttles

Flutter delays raster caching until an item has been visible across multiple
frames and limits how many display-list caches can be generated per frame. It
also rejects volatile display lists through `will_change`, allows caller hints
through `is_complex`, skips singular/non-rasterizable transforms, and avoids
building child caches when an ancestor is already cached.

Emerge requirements:

- every cache kind needs an admission policy, even if the first version is
  simple
- candidates should track visible access count separately from cache hits
- cache creation must have a per-frame budget
- a cached parent should suppress child cache preparation unless the child cache
  has an independent reason to exist
- volatile content, active input, video, changing filters, and animation-heavy
  subtrees should be ineligible by default
- future Elixir hints should be advisory, not mandatory cache commands

### Keys must separate content, transform, and pixel alignment

Flutter's raster cache key combines content identity with the non-translation
parts of the transform, then rasterizes into rounded-out device bounds with
translation snapped for physical-pixel alignment. WebRender compares transform
dependencies explicitly instead of assuming a primitive is equivalent when its
descriptor is unchanged.

Emerge requirements:

- content keys and composite placement keys should be separate
- scale, skew, rotation, and raster-space choices belong in cache keys when they
  affect pixels
- pure translation should be a composite input where possible
- cached output should use rounded-out physical bounds
- cacheable content must have finite, non-empty device bounds and a per-axis
  surface cap
- fractional translation behavior needs one documented policy per cache kind:
  either include it in the key or snap consistently before direct and cached
  drawing can diverge

### Resource dependencies must be first-class

WebRender tile invalidation compares primitive descriptors plus dependency
streams for clips, transforms, image generations, opacity bindings, and color
bindings. Iced text cache keys include content, font, size, line height, bounds,
shaping, and alignment. Slint cache entries are invalidated through property
trackers and scale-factor checks.

Emerge requirements:

- cache keys should include typed resource generations for assets, fonts, and
  backend texture/context generations
- picture/layer caches need dependency descriptors for clips, alpha, transforms,
  image ids/generations, font generations, and dynamic bindings
- text caches need explicit shaping/font/bounds inputs and must exclude active
  editor/caret/preedit paths initially
- cache invalidation should report the reason when possible: descriptor,
  geometry, transform, clip, image/font generation, opacity/color binding,
  volatile content, or resource loss

### Handles need generation guards

Slint keeps per-item `cache_index` handles but validates them against a renderer
cache generation. Clearing the backend cache increments generation so stale item
handles fail cheaply without walking every item.

Emerge requirements:

- if node-local handles are introduced, they must carry a renderer-cache
  generation
- clearing a cache should make stale handles invalid without a tree walk
- removing/destroying nodes should release only their owned handles when that is
  cheap; global generation invalidation remains the fallback for context loss

### Budgets must cover fragmentation, not only bytes

WebRender caps picture-cache slices and compositor surfaces, merging slices when
fragmentation gets too high. Slint dirty regions cap rectangle count and merge
when complexity grows. Iced damage grouping merges nearby rectangles by area
threshold.

Emerge requirements:

- every cache kind needs entry count and byte caps
- layer/tile-like caches also need fragmentation caps: max slices, max surfaces,
  max dirty rects, max tiles, or max cached subtrees per scene
- exceeding fragmentation caps should degrade to a simpler larger cache or full
  redraw, not unbounded subdivision
- stats should show both memory pressure and complexity pressure

### Storage reuse is a separate optimization

Iced preserves the previous cached value after `clear()` so backends can reuse
internal storage, and its cache groups let related caches share backend storage
or batching. This is separate from deciding whether content is valid.

Emerge requirements:

- cache validity and backing-storage reuse should be separate concepts
- a cleared entry may keep reusable allocation metadata if generation rules keep
  stale pixels unreachable
- future cache groups can group entries by scene, retained subtree, scroll
  container, or primitive family for batching and storage reuse

### Dirty-region and tile caches need dependency history

Slint computes dirty regions from old/new item bounds and rendering-property
trackers, with previous-buffer dirty regions included for swapped buffers.
WebRender stores per-tile dependency lists and adapts quadtree granularity based
on recent dirty history.

Emerge requirements for later backend-level caches:

- old and new conservative visual bounds must include shadows, transforms, clip
  changes, and alpha/layer effects
- clip or opacity changes must invalidate descendants unless a cache boundary
  proves otherwise
- backend dirty-region rendering needs buffer-age/history support
- tile caches need dependency comparison and recent-dirty history before they
  can be correct and profitable

## Engine Findings

## Flutter

Primary sources:

- Official `RepaintBoundary` docs:
  `https://api.flutter.dev/flutter/widgets/RepaintBoundary-class.html`
- Local framework source:
  `/workspace/tmp-layout-engines/flutter/packages/flutter/lib/src/rendering/object.dart`
  `/workspace/tmp-layout-engines/flutter/packages/flutter/lib/src/rendering/layer.dart`
  `/workspace/tmp-layout-engines/flutter/packages/flutter/lib/src/rendering/custom_paint.dart`
- Local Flutter engine raster cache source:
  `/workspace/tmp-layout-engines/flutter_engine/flow/raster_cache.cc`
  `/workspace/tmp-layout-engines/flutter_engine/flow/raster_cache.h`
  `/workspace/tmp-layout-engines/flutter_engine/flow/raster_cache_key.h`
  `/workspace/tmp-layout-engines/flutter_engine/flow/raster_cache_util.h`
  `/workspace/tmp-layout-engines/flutter_engine/flow/layers/display_list_raster_cache_item.cc`
  `/workspace/tmp-layout-engines/flutter_engine/flow/layers/layer_raster_cache_item.cc`
- Web reference for the same Flutter engine source:
  `https://chromium.googlesource.com/external/github.com/flutter/engine/+/refs/heads/flutter-3.16-candidate.5/flow/raster_cache.cc`

### Useful ideas

#### Repaint boundaries isolate paint invalidation

Flutter's `RepaintBoundary` creates a separate display list for a child. When a
render object needs paint, Flutter propagates paint dirtiness to the nearest
ancestor repaint boundary. During painting, `PaintingContext.paintChild` either
repaints a dirty boundary or reuses its retained `OffsetLayer`.

The transfer to Emerge is direct:

- Emerge already tracks render dirtiness by retained node.
- Emerge could introduce renderer-layer boundaries at selected retained nodes.
- A clean layer boundary should be composited by offset/transform without
  replaying its children.

This is different from current render-scene subtree reuse because the render
thread would skip draw traversal, not only scene construction.

#### Reused layers can move without repainting children

Flutter `OffsetLayer` is explicitly optimized for repaint boundaries. If a
boundary is clean, Flutter can mutate the layer offset and append the existing
layer instead of repainting the child subtree.

Transfer to Emerge:

- Moving clean overlays, drawers, scroll panels, and exit ghosts are good layer
  candidates.
- A cache key should separate child content identity from compositing state:
  content can be unchanged while offset/alpha/transform changes.
- Current Emerge render subtree keys include scene/render context and frame
  state. A layer cache may need two keys:
  - content key: primitive output inside local coordinates
  - composite key: final transform, clip, opacity, destination

#### Raster cache is heuristic and bounded

Flutter's engine `RasterCache` uses keys that include display-list/layer id and
matrix. It marks entries seen each frame, tracks accesses, rasterizes into a
surface, draws cached images when available, tracks memory metrics, and evicts
entries not encountered in a frame.

Transfer to Emerge:

- Do not raster-cache every subtree.
- Use an access threshold or explicit hint before paying offscreen rasterization
  cost.
- Include scale/matrix and physical size in the key.
- Track bytes, hits, misses, stores, and evictions in native stats.
- Evict entries that are not seen during a frame or when a byte budget is hit.
- Prefer GPU render-target payloads on GPU backends; CPU raster payloads are
  fallback and tests, not the main production path.
- Prepare admitted payloads before direct subtree rendering so the store frame
  can draw the payload instead of doing both direct draw and offscreen
  rasterization.

#### Emerge can be less heuristic than Flutter

Flutter must infer many decisions from retained layers, display-list complexity,
`isComplex`/`willChange` hints, platform-view constraints, and broad matrix
state. Emerge has a narrower and more explicit pipeline:

- retained element ids can become cache ids
- typed layout/render invalidation can reject volatile subtrees by semantic
  reason
- tree render can emit local clean-subtree candidates with stable content
  generation
- layout-cache hits can mark renderer-cache candidates as high-confidence lookup
  and admission targets
- render-subtree cache hits can reuse content generation instead of hashing
  arbitrary render-node lists again

This means Emerge should copy Flutter's frame lifecycle and GPU payload lesson,
not its broad layer/display-list heuristic surface. Renderer cache lookup still
needs resource generation, scale, payload backend, residency, and eviction
checks; a layout-cache hit is a strong signal, not a substitute for renderer key
validation.

#### Cache hints matter

Flutter exposes `CustomPaint.isComplex` and `willChange`. Those hints do not
force caching, but they inform compositor heuristics.

Transfer to Emerge:

- Automatic heuristics should be the default.
- A later public attribute such as `render_cache: :auto | :static | :volatile`
  could help app authors mark expensive static drawing or known volatile
  subtrees.
- Volatile flags should matter for cursors, active text inputs, video, and
  per-frame animations that change subtree content. Placement-only and
  element-alpha animations can stay cacheable when the cache boundary models
  them as composition state.

## Slint

Primary sources:

- `/workspace/tmp-layout-engines/slint/internal/core/partial_renderer.rs`
- `/workspace/tmp-layout-engines/slint/internal/core/item_rendering.rs`
- `/workspace/tmp-layout-engines/slint/internal/renderers/software/lib.rs`
- `/workspace/tmp-layout-engines/slint/internal/renderers/skia/lib.rs`
- `/workspace/tmp-layout-engines/slint/internal/core/graphics/boxshadowcache.rs`
- `/workspace/tmp-layout-engines/slint/internal/core/renderer.rs`

### Useful ideas

#### Dirty-region rendering is bounded and conservative

Slint's software renderer keeps a `DirtyRegion` with a small fixed max rectangle
count. When the region becomes too complex, it merges rectangles instead of
growing without bound. It supports:

- full redraw for new buffers
- dirty-only redraw for reused buffers
- previous-dirty-region inclusion for swapped/double-buffered buffers

Transfer to Emerge:

- Dirty regions are attractive for raster and DRM backends, and possibly
  Wayland/EGL if buffer age and surface damage are available.
- Use a small bounded region list first, not an unbounded region algebra.
- Include previous dirty regions for double/triple buffering.
- Fall back to full redraw when:
  - the dirty region grows too large,
  - a backend cannot preserve previous buffer contents,
  - a global clear/background change occurs,
  - GPU surface resize/reconfigure happens.

This would help text cursor, hover, scrollbar, and small animation updates more
than first-page-load rendering.

#### Per-item cached rendering data uses generation guards

Slint stores a small `CachedRenderingData` on each renderable item. The renderer
cache owns the actual cached payload and has a generation counter. If the cache
is cleared, stale item indexes stop matching.

Transfer to Emerge:

- Per-node cache handles should live in native node state.
- Renderer-owned caches should have generation/version guards.
- Cache clearing on backend/context loss should be cheap and should not require
  walking every node to scrub stale handles.

This fits Emerge's `NodeIx` and `NodeRefreshState` model.

#### Property tracking marks dirty geometry and rendering separately

Slint computes dirty regions by comparing old and new item geometry and by
checking rendering-property trackers. Clip and opacity changes force child
refresh because they affect descendants.

Transfer to Emerge:

- Emerge already has layout vs refresh dirtiness. Renderer caches should keep
  this split:
  - geometry changed: dirty old bounds and new bounds, invalidate raster layer
  - paint property changed: dirty current visual bounds
  - clip/alpha changed: dirty descendants or composite layer boundary
- Conservative visual bounds must include shadows, transforms, and alpha layers.

#### Box-shadow cache is a direct match

Slint has a `BoxShadowCache` keyed by physical width, height, color, blur, and
radius. It also combines a global cache with an item-level cache entry.

Transfer to Emerge:

- An earlier expensive first-visible assets-page draw was a CSS-like blurred box
  shadow.
- A physical-pixel shadow texture cache remains the most directly justified
  primitive cache if fresh traces show repeated shadow cost again.
- The cache key should include:
  - physical width and height of the shadow source
  - spread/size
  - blur/sigma
  - corner radius
  - color
  - scale factor
  - outer vs inset mode
- Store a byte budget and evict least-recently-used entries.

## Iced

Primary sources:

- `https://docs.rs/iced/latest/iced/widget/canvas/type.Cache.html`
- `/workspace/tmp-layout-engines/iced/runtime/src/user_interface.rs`
- `/workspace/tmp-layout-engines/iced/graphics/src/geometry/cache.rs`
- `/workspace/tmp-layout-engines/iced/graphics/src/cache.rs`
- `/workspace/tmp-layout-engines/iced/graphics/src/text/cache.rs`
- `/workspace/tmp-layout-engines/iced/graphics/src/damage.rs`
- `/workspace/tmp-layout-engines/iced/graphics/src/mesh.rs`

### Useful ideas

#### Keep UI state cache and draw cache separate

Iced's `UserInterface::build` accepts a previous `user_interface::Cache`, diffs
widget state, and lays out the tree. This is separate from canvas geometry
caches.

Transfer to Emerge:

- Emerge should keep retained tree/layout caches separate from renderer-thread
  caches.
- Renderer caches should not leak into layout correctness.
- Cache invalidation should flow from typed render damage, not from layout-cache
  hit/miss outcomes.

#### Explicit geometry cache is simple and predictable

Iced's canvas cache stores generated geometry. It redraws only when size
changes or the cache is explicitly cleared. The cache also keeps previous
geometry so backends can reuse internal storage.

Transfer to Emerge:

- For custom draw-like primitives or expensive static decorations, an explicit
  "geometry/picture cache" can be better than complex heuristics.
- Emerge's first implementation can be internal and automatic, but the model
  should stay simple:
  - same local bounds and same draw inputs -> reuse
  - changed size or inputs -> rebuild
  - explicit clear on resource loss

#### Cache groups support backend batching

Iced has cache groups so related caches can share backend storage or batching.

Transfer to Emerge:

- Later, Emerge could group cache entries by retained subtree or scroll
  container.
- For now, global LRU plus per-node handles is enough.

## Servo and WebRender

Primary sources:

- `/workspace/tmp-layout-engines/servo/components/layout/dom.rs`
- `/workspace/tmp-layout-engines/servo/components/layout/traversal.rs`
- `/workspace/tmp-layout-engines/servo/components/shared/layout/lib.rs`
- `/workspace/tmp-layout-engines/servo/components/layout/display_list/stacking_context.rs`
- `/workspace/tmp-layout-engines/webrender/webrender/src/tile_cache.rs`
- `/workspace/tmp-layout-engines/webrender/webrender/src/picture.rs`
- `https://docs.rs/webrender/latest/src/webrender/tile_cache.rs.html`

### Useful ideas

#### Damage boundaries prevent broad invalidation

Servo uses independent formatting contexts to isolate box-tree rebuild damage
and fragment-tree layout cache damage. When an ancestor has relayout damage,
crossing into an independent formatting context can preserve descendant fragment
layout caches.

Transfer to Emerge:

- The same idea applies to rendering:
  - layer/cache boundaries should be dependency boundaries,
  - dirty descendants should not force broad clean sibling redraw,
  - scroll containers and overlays are natural boundaries.

This complements existing layout relayout boundaries.

#### Display list rebuilding is separate from rendering

Servo explicitly tracks when layout needs a new display list. WebRender then
receives display lists and performs renderer-side caching.

Transfer to Emerge:

- Current `RenderScene` is Emerge's display list.
- Current render-subtree cache optimizes display-list construction.
- Picture/layer/raster caches should be implemented as a separate renderer
  cache layer under `SceneRenderer`, not folded into layout/refresh logic.

#### Picture/tile caching has slice limits

WebRender's tile cache builder creates picture-cache slices and merges when too
many slices would be created. It retains tile cache config for future frames and
keeps explicit profiler counts.

Transfer to Emerge:

- For large scrollable content, a tile cache is the long-term version of a layer
  cache.
- It should have hard caps:
  - max slices per scene/container
  - max tiles per layer
  - max bytes
  - fallback to one larger picture/layer when fragmentation is too high
- This is later work, not the first renderer-cache slice.

## Scenic Driver Skia

Primary sources:

- `/workspace/tmp-layout-engines/scenic_driver_skia/GUIDES.md`
- `/workspace/tmp-layout-engines/scenic_driver_skia/native/scenic_driver_skia/src/renderer.rs`

### Useful ideas

Scenic Driver Skia parses serialized scripts once into cached `ScriptOp` lists
and replays cached ops on redraw. Its guide lists a future per-script Skia
`Picture` cache for static scripts.

Transfer to Emerge:

- Emerge already avoids BEAM-to-native draw-command traffic per frame by keeping
  a native tree and render scene.
- The next similar step is to record clean render subtrees into Skia pictures or
  raster layers so the render thread avoids replaying unchanged primitive lists.

## Candidate Optimizations for Emerge

## 1. Renderer-cache contract and shared plumbing

This is not a visual optimization by itself. It is the groundwork that keeps the
first cache from becoming a one-off special case.

Work:

- define a small renderer-cache contract under `SceneRenderer`
- keep cache ownership out of layout and event reconciliation
- define typed key and generation rules for primitive, picture, and raster
  output
- split content keys from composite state where offset, transform, clip, alpha,
  or destination can change independently
- add shared byte-budget and deterministic LRU helpers
- add cache stats by kind:
  - hits
  - misses
  - stores
  - evictions
  - current entries
  - current bytes
  - rejected/ineligible draws where useful
- add a single clear path for renderer-owned caches on context loss, backend
  reset, asset/font generation change, scale change, and global cache clearing
- add cached-vs-direct raster parity helpers

Why first:

- the same model should support shadow textures, text blobs, pictures, and
  raster layers
- cache benefit must be measured together with draw, flush, submit, and present
- byte budgets and invalidation need tests before broad cache use

## 2. Keep improving cache instrumentation as needed

Current diagnostics are already useful. Before adding caches with correctness
and memory risk, add focused cache detail only where profiles ask for it. Direct
border/clip/layer breakdowns live in `drawing-optimization-investigation.md`
unless they are needed to decide cache eligibility.

Immediate additions worth considering:

- candidate and rejection counts by cache kind
- text-cache eligibility detail:
  - count static-label/code-like text draws by font/size/family, not full strings
  - count active-input and volatile text exclusions
- picture/layer eligibility detail:
  - clean retained subtree candidates by node count and bounds size
  - rejection reasons for video, active input, animation, huge bounds, or resource
    volatility
- cache stats:
  - shadow cache hits/misses/stores/evictions/bytes
  - text blob cache hits/misses/stores/evictions/bytes
  - layer/picture cache hits/misses/stores/evictions/bytes

Why this still matters:

- Logs first changed the diagnosis from image draw to shadows, and later
  post-drawing-pass traces moved the active priority from shadow-only work to
  clean complex subtrees.
- Caches can move cost between draw, flush, and present; stats need to show the
  whole frame, not only draw time.

## 3. Cache decision gate after direct drawing

This document should not carry direct draw fast-path implementation. For each
hot category, choose a cache only after the direct-rendering baseline is clear:

- border/clip-heavy frame: try the drawing investigation's border and clip
  simplifications first; consider picture or raster caches only if repeated
  stable subtrees remain expensive
- text-heavy frame: diagnose direct text cost first; consider retained text
  blobs only for static labels/code-like text after active-input exclusions are
  understood
- shadow-heavy frame: benchmark current blur shadows against direct Skia shadow
  alternatives first; consider a shadow texture cache only if repeated blurred
  shadows remain expensive
- image first-use frame: keep decode/upload/resource timing visible; do not hide
  first-use upload stalls behind a generic layer cache unless steady-state traces
  prove repeated benefit

Expected impact:

- prevents renderer caches from masking simple direct-renderer gaps
- makes cache benchmarks compare against the renderer users will actually run
- keeps cache complexity focused on repeated work, not one-time draw formulation
  mistakes

## 4. Clean-subtree GPU payload cache

Status: implemented. The completed active renderer-cache plan was folded back
into this investigation and `render-cache-flutter-comparison.md`.

### Why

- It matches the strongest post-direct-drawing signal: stable complex subtrees
  replayed across animation, layout-reflow, and interaction frames.
- It exercises the full shared renderer-cache model instead of optimizing one
  primitive in isolation.
- It takes advantage of Emerge-specific strengths: retained element identity,
  typed dirtiness, explicit clean-subtree candidates, render-subtree reuse, and
  layout-cache hits.
- It gives a single place to prove GPU render-target payloads,
  prepare-before-draw store frames, byte budgets, stats, and direct fallback.

### Implemented shape

- Candidate boundaries come from tree rendering with stable id, content
  generation, local bounds, and child render nodes.
- Payloads are GPU render-target images on GPU backends and CPU raster images
  only for raster/offscreen fallback and correctness harnesses.
- Stores route admitted subtree drawing through a transparent offscreen cache
  target before compositing to the main surface; they do not copy the already
  rendered framebuffer.
- Content keys exclude pure placement and element alpha for caches rooted at
  that element. Element alpha is applied while drawing the cached payload image.
- Descendant alpha changes inside a cached ancestor require nested composited
  boundaries or ancestor invalidation.
- Entries track `last_seen_frame`; parent cache hits and same-frame parent
  prepares touch existing descendant entries as `suppressed_by_parent`.
- Entries that are no longer visible or parent-suppressed age out after a
  conservative stale-frame window, while byte/count budgets remain the hard cap.

### Validation

- Benchmarks cover direct rendering, CPU payloads, GPU payloads, admitted
  prepare-before-draw frames, and warm hits.
- Demo-shaped cases include Nerves-style `move_x`, vertical/combined integer
  translation, showcase layout-reflow movement, app-selector alpha animation,
  todo-entry translate+alpha animation, warm assets, and border/text/clip-heavy
  mixed subtrees.
- If full-frame timings do not beat the direct renderer for a future cache
  expansion, keep the simpler direct path and document the rejected
  optimization.

Measured GPU Criterion checks from 2026-04-28:

- `gpu_cache_candidates_layout_reflow`: direct reflowed children `1.7046 ms`
  versus cached local content `177.71 us`
- `gpu_cache_candidates_translated/app_selector_menu_alpha`: direct
  `1.1213 ms` versus production cached candidate `396.02 us`
- `gpu_cache_candidates_translated/todo_entry_translate_alpha`: direct
  `929.48 us` versus production cached candidate `389.62 us`
- `gpu_surfaceless/mixed_ui_scene`: no statistically significant change
  (`3.8186 ms`, `+0.5121%`, `p = 0.28`)

Measured child-cache follow-up from 2026-04-28:

- parent-hit nested clean-subtree accounting was neutral to better, so
  descendant suppression accounting is retained for stale-entry correctness
- a dedicated nested-alpha children-cache kind did not beat direct GPU drawing
  in the small overlapping-alpha microbench, so no new production cache kind was
  added
- raster no-candidate cold mixed frames improved in the short guard run, so the
  metadata path did not show a direct-renderer regression

Remaining conservative exclusions:

- descendant alpha under a cached ancestor
- fractional placement, rotate, and scale
- active text input/caret/preedit visuals
- active video
- image loading/failure placeholder payloads

Asset source status generation is part of render-subtree keys, so a subtree
cached while an image is pending is invalidated when the asset becomes ready or
failed. Placeholder drawing remains direct-rendered.

## 5. Box-shadow texture cache

This is a later primitive-cache pilot if fresh traces again show repeated shadow
cost after the clean-subtree GPU payload path and direct drawing baseline are
proven.

### Why

- The assets-page slow frame had no images and only a small text count.
- Shadow draw alone took about 2 ms, followed by about 6 ms in GPU flush and
  present synchronization.
- Slint has a purpose-built box-shadow cache with almost the same key shape.
- Direct `Canvas::draw_shadow` was benchmarked and rejected in
  `drawing-optimization-investigation.md`; this cache follows only if repeated
  shadow cost remains in fresh traces.

### Proposed shape

Add a renderer-owned `ShadowTextureCache`:

```text
ShadowTextureKey {
  kind: outer | inset,
  physical_width,
  physical_height,
  offset_x,
  offset_y,
  blur,
  spread,
  radius,
  color,
  scale_bucket,
}
```

Cache value:

```text
ShadowTexture {
  image: skia_safe::Image,
  logical_draw_rect,
  bytes,
  last_used,
}
```

Eligibility:

- cache only shadows above a measured cost or above a blur/area threshold
- skip tiny shadows where direct draw is cheaper
- skip shadows larger than a per-entry byte cap
- cap total bytes and entries

Implementation options:

- **Full texture cache:** render the whole shadow into an offscreen surface and
  draw the image later. This is simplest and likely enough for current demo
  card shadows.
- **Nine-patch shadow cache:** cache corners/edges and stretch them. This saves
  memory for large cards but is much more complex. Defer until full texture
  cache proves memory pressure.
- **CPU-raster shadow cache:** creates a CPU image and draws it later. It may
  still pay first GPU upload. Useful for raster backend and correctness tests.
- **GPU render-target shadow cache:** best for Wayland/DRM steady-state draw,
  but must live on the render thread and be cleared on context loss.

Risks:

- first use still pays cache creation unless warmed on the render thread
- cached blur must match current Skia output closely
- clip-difference semantics must keep the element center transparent
- scale and fractional geometry can produce fuzzy cache misses or visual drift

Validation:

- raster pixel parity tests for outer/inset rounded shadows
- renderer unit tests for cache keying and LRU eviction
- demo stats before/after on assets/showcase shadow frames
- slow-frame logs should show shadow cache hit and lower shadow draw/gpu flush

## 6. Text blob cache

Text was about 1 ms in a frame with 115 text draws. Current rendering uses
`canvas.draw_str` per text primitive.

Candidate:

- cache shaped text blobs/runs keyed by:
  - text content
  - font family
  - font size
  - weight
  - italic
  - scale/font smoothing relevant inputs
- draw cached blobs at changing positions with new paint color
- keep fill color out of the blob key if Skia allows paint color at draw time

Eligibility:

- static labels, repeated code samples, menu labels
- skip active text input cursor/preedit paths initially
- cap by total bytes and text length

Risks:

- Unicode shaping, fallback fonts, font asset changes, and synthetic font
  fallback make invalidation subtle
- current `draw_str` may already use internal Skia caches for simple strings,
  so measure before implementing

Validation:

- add text cache hit/miss stats before enabling broadly
- compare code-heavy showcase pages
- keep text rendering pixel tests for decoration/caret/preedit

## 7. Retained Skia picture cache for clean subtrees

This caches recorded draw commands, not pixels.

### Why

It avoids CPU traversal and repeated creation of paints, paths, clips, borders,
and text draw calls for clean subtrees. It will not remove GPU rasterization
cost of expensive effects, but it can help frames dominated by many borders,
texts, and clip scopes.

### Proposed shape

Attach a renderer-side picture cache to retained render subtree identity:

```text
PictureCacheKey {
  render_subtree_key_hash,
  scale,
  local_bounds,
  asset_generation,
  font_generation,
}
```

The render thread records a clean subtree into `skia_safe::Picture` and later
draws that picture when the key matches.

Good first targets:

- small clean subtrees already accepted by `RenderSubtreeCache`
- no video
- no active text input
- no volatile animation
- no huge scroll-offset content

Risks:

- picture recording must preserve current clip/transform/alpha semantics
- assets that resolve asynchronously need generation invalidation
- picture cache may reduce CPU draw time but not GPU flush time
- too many small pictures can increase overhead

Validation:

- render-scene parity tests by comparing cached vs uncached raster output
- stats for picture cache hit/miss/store
- hard caps matching current render-subtree store budget and node-count cap

## 8. Raster layer cache for expensive stable subtrees

This caches pixels/images, similar in spirit to Flutter's raster cache.

### Why

It can turn repeated expensive shadow/text/border subtree draws into one image
draw. It is the cache type most likely to reduce repeated GPU work after the
first cached frame.

### Proposed shape

Use a renderer-owned `LayerRasterCache`:

```text
LayerRasterKey {
  retained_node_id_or_cache_id,
  content_generation,
  physical_size,
  matrix_or_scale_bucket,
  clip_shape_hash,
}
```

Use heuristics:

- mark seen every frame
- count visible accesses
- rasterize after N accesses or when explicitly hinted
- skip volatile subtrees
- evict entries not seen or when memory budget is exceeded

Natural Emerge boundaries:

- scroll panel contents
- app selector cards
- code preview panels
- static image/text cards with expensive decorations
- exit-animation ghosts after snapshotting, if semantics allow

Risks:

- memory pressure
- stale pixels if render damage classification misses a dependency
- first-use hitch
- image/video/text-input content should be excluded initially
- alpha and transform composition must match direct rendering

Validation:

- cached-vs-uncached image output tests
- cache generation tests for attrs, runtime state, assets, fonts, scale, and
  layout frame changes
- memory budget tests
- slow-frame stats on repeated animations/scrolls

## 9. Dirty-region redraw

This caches the previous frame's pixels in the back buffer and redraws only
changed regions.

### Backend fit

- Raster backend: best first target. The buffer is controlled and testable.
- DRM backend: useful if buffer age or explicit previous-buffer handling is
  tracked.
- Wayland/EGL: possible, but depends on preserving buffer contents and
  communicating damage to the compositor. Needs careful backend work.

### Proposed shape

- Track old and new visual bounds for render-dirty nodes.
- Keep a bounded dirty region list, probably 3 rectangles initially like Slint.
- Include previous dirty regions for swapped/double-buffered buffers.
- Clip all rendering to dirty region during redraw.
- Fall back to full redraw on resize, scale change, clear color change, context
  loss, too many dirty rects, or unsupported backend.

Expected wins:

- text cursor blinking
- hover changes
- scrollbar hover/drag
- small localized animations
- lightweight input feedback

Expected non-wins:

- first page draw
- full-scene animations
- large scroll movement unless combined with scroll/tile reuse

## 9. Tile cache for scrollable content

This is the long-term WebRender-like path.

Candidate:

- define tile caches per scroll container or large retained layer
- cache content-space tiles
- on scroll, composite existing tiles and render newly exposed strips
- merge/fallback when too many tiles/slices are created

Why later:

- Emerge still needs primitive caches and simple layer caches first
- tile invalidation must integrate with scroll extents, clip chains, transforms,
  shadows, nearby overlays, and event hit testing

## Recommended Order

### Slice 1: renderer benchmark and direct-baseline gate

Goal:

- prevent renderer-cache work from landing with hidden draw-path regressions or
  from replacing simpler direct-renderer fixes

Work:

- benchmark direct raster rendering before adding cache code
- cover text-heavy, border/clip-heavy, shadow-heavy, and mixed renderer scenes
- save a Criterion baseline before implementation
- compare cache changes against that baseline before landing
- check `drawing-optimization-investigation.md` for the matching hot category
  and either use its completed direct-renderer decision or create a fresh direct
  drawing benchmark before choosing a cache pilot

Required command:

```bash
cargo bench --manifest-path native/emerge_skia/Cargo.toml --bench renderer -- --save-baseline render_cache_before
```

Landing rule:

- cache-enabled steady state must not regress the direct-renderer baseline
- cache-enabled steady state must beat the current direct renderer for the
  targeted repeated cost
- cache miss and cache warming paths must not add unacceptable first-use stalls
- cache eligibility must narrow or the cache must stay disabled when one
  scenario improves by regressing another

### Slice 2: renderer-cache contract and shared plumbing

Goal:

- establish the common renderer-cache rules before optimizing one draw category

Work:

- define renderer-cache ownership under `SceneRenderer`
- define the frame lifecycle: begin, mark candidates, admit, bounded prepare,
  draw hit/direct fallback, end stats, and eviction
- define typed key, dependency, and generation requirements for primitive,
  picture, text, and raster caches
- add shared stats shape for candidates, visible candidates, hits, misses,
  stores, evictions, rejected/ineligible draws, bytes, and current size by cache
  kind
- add byte, entry, per-frame creation, and fragmentation budgets
- add LRU or seen-this-frame helpers with deterministic eviction
- add clear hooks for context loss, backend reset, asset/font generation change,
  and global cache clearing
- add cached-vs-direct raster parity test helpers

Why first:

- it prevents a one-off shadow cache from setting the wrong abstraction
- it lets shadow, text, picture, and raster caches report comparable stats
- it makes cache correctness, admission, memory behavior, and cache-warming cost
  testable before broad use

### Slice 3: clean-subtree CPU-backed pilot

Goal:

- validate the shared renderer-cache model on explicit clean-subtree candidates
  before moving the production payload to GPU render targets

Work:

- emit renderer-facing clean-subtree candidates with stable ids, content
  generations, local bounds, and child render nodes
- keep content identity separate from integer placement
- add CPU-backed raster payloads as correctness scaffold and raster/offscreen
  fallback
- prove stats, budget, admission, eviction, clearing, direct fallback, and
  cached-vs-direct parity
- benchmark integer translation and layout-reflow placement reuse

Status:

- completed during the first clean-subtree cache slice

### Slice 4: GPU-first payload and prepare-before-draw

Goal:

- replace CPU-backed production payloads on GPU backends with GPU-resident
  render-target images and remove admitted-frame double work

Work:

- prepare admitted candidates before direct fallback
- route the admitted subtree into a transparent offscreen GPU cache target before
  compositing to the main surface
- snapshot/store the GPU image and draw it in the same frame when preparation
  succeeds
- keep CPU raster payloads as fallback/test harness
- add payload-kind, prepare, hit-draw, current-bytes, and rejection stats
- benchmark direct vs CPU payload vs GPU payload, including store frame, warm
  hit frame, GPU flush, submit, present, and pipeline timing

Status:

- implemented; GPU frames prepare offscreen GPU render-target payloads,
  raster/offscreen frames keep CPU raster payloads, admitted candidates prepare
  before drawing, and stats now split payload kind, prepare result, direct
  fallback, and rejection reasons

### Slice 5: element-alpha composition eligibility

Goal:

- reuse a cached subtree payload while element alpha changes, because alpha on
  an Emerge element is composition state for that element's subtree

Work:

- root element alpha is implemented as cache composition state for caches rooted
  at that element
- root element alpha stays out of the payload content key when applied while
  drawing the cached image
- descendant alpha remains conservative: nested `Alpha` under a candidate still
  rejects the candidate until nested composited/cache boundaries have parity
  coverage and benchmark evidence
- app-selector menu fades and todo-entry translate+fade animations are covered
  by GPU renderer benchmarks
- cached-vs-direct pixel parity is covered for root-alpha payload reuse; broader
  overlapping/nested alpha cases now have a benchmark guard, and the first
  alpha-only cache-boundary expansion stayed out because it did not prove a GPU
  win

### Slice 6: later primitive cache pilot

Goal:

- exercise the shared cache contract on a bounded primitive cache only if fresh
  profiles show repeated primitive cost after the clean-subtree GPU path

Work:

- choose the primitive after the direct draw baseline gate and fresh traces
- start with shadow textures only if repeated blurred-shadow cost remains
- keep cache creation behind admission thresholds and per-frame creation budget
- require cache stats, byte caps, clear hooks, direct fallback, and raster parity
  tests before enabling by default

### Slice 7: text blob cache if profiles still show text cost

Goal:

- reduce repeated static label/code text overhead beyond the direct text baseline

Work:

- instrument cache eligibility by font/size/count
- cache text blobs for static single-line text
- exclude active text input paths initially

### Slice 8: retained Skia picture cache

Goal:

- skip CPU primitive replay for clean static subtrees

Work:

- prototype on subtrees already eligible for `RenderSubtreeCache`
- keep strict caps
- compare cached and uncached raster output

### Slice 9: broader raster layer cache

Goal:

- skip repeated expensive rasterization for stable complex layers

Work:

- access-thresholded layer cache
- explicit bytes budget
- seen/evict lifecycle
- avoid volatile subtrees

### Slice 10: dirty-region redraw and later tile cache

Goal:

- avoid full-surface redraw for localized changes
- later, reuse scrolled content at tile granularity

Work:

- start with raster backend
- then DRM/Wayland only if backend buffer semantics are solid

## Design Rules for Emerge

- Keep layout cache, render-scene cache, and renderer-thread caches separate.
- Use `drawing-optimization-investigation.md` to establish or explicitly defer
  the direct-renderer fix for the same cost center before selecting a cache.
- Implement and baseline renderer benchmarks before adding renderer caches.
- Introducing a renderer cache is not allowed to degrade the direct-renderer
  benchmark suite.
- Do not use renderer caches to hide a simple direct drawing fast path that can
  be implemented and tested with lower memory and invalidation risk.
- Cache keys must use typed generations where possible, not debug strings.
- Every renderer cache needs candidate, hit, miss, store, eviction, rejection,
  current-entry, and byte stats where applicable.
- Every cache must have hard byte/count budgets and, for layer/tile caches,
  fragmentation budgets.
- Hard cache budgets should have conservative defaults and be configurable from
  `EmergeSkia.start/1` options so memory-constrained deployments can tune or
  disable stores without recompiling.
- GPU render-target payloads should be the default for GPU backends; CPU raster
  payloads are fallback/test harnesses unless benchmarks prove otherwise.
- GPU cache stores should be built by routing admitted subtree drawing into an
  offscreen cache target before compositing to the main surface, not by copying
  already-composited framebuffer pixels.
- Admitted cache entries should prepare before direct fallback so store frames do
  not duplicate subtree drawing and payload creation.
- Cache admission must be separate from lookup: visible access thresholds and
  per-frame creation budgets prevent cache creation from becoming the next
  slow-frame source.
- Content identity must be separated from composite placement when translation,
  alpha, clips, or transforms can change independently.
- Element alpha should be treated as cache-hit draw state for caches rooted at
  that element, not payload content, when the cached subtree was rendered into a
  transparent target and alpha is applied while drawing the payload image.
- Parent cache hits must keep descendant cache entries seen/suppressed before
  stale eviction is applied; otherwise useful child payloads can age out only
  because a parent payload was warm.
- Layout-cache and render-subtree hits should feed renderer-cache
  lookup/admission as high-confidence signals, but renderer-cache keys still
  validate paint/resource/scale/backend/residency state.
- Cacheable output must have finite, non-empty, rounded physical bounds and a
  documented fractional-translation policy.
- Cache clearing on backend/context loss must be cheap and correct.
- Node-local handles need renderer-cache generation guards.
- Resource generations for assets, fonts, backend textures, and dynamic
  bindings must participate in picture/layer/text cache invalidation.
- Favor explicit eligibility and conservative fallback over broad speculative
  caching.
- Do not cache active video, active text input editing visuals, or volatile
  animation until those cases have targeted tests.
- Use cached-vs-uncached raster parity tests for every draw-output cache.
- Treat first-use hitch separately from steady-state cost. Raster caches help
  repeated frames; prewarming is a separate feature.

## Open Questions

- Can Wayland/EGL reliably preserve back-buffer contents in the current backend,
  or should dirty-region rendering be raster/DRM-first?
- Should GPU clean-subtree payload preparation happen only inside the render
  frame budget, or should the render thread later gain an idle/prewarm phase
  after scene upload?
- For descendant alpha changes under a cached ancestor, should nested composited
  boundaries be automatic during tree render, or should the first implementation
  invalidate the ancestor until nested boundaries are proven?
- Should layer cache eligibility be purely automatic, or should Emerge expose an
  Elixir-side cache hint once the internal implementation is proven?
- After the direct drawing pass, is Skia `Picture` recording enough to lower
  repeated border/text-heavy subtree cost, or is a narrower retained text cache
  still justified?
- Can the current render subtree key be reused for picture/layer caches, or does
  it need to be split into content and composite keys?

## Source Index

- Flutter repaint boundaries:
  `https://api.flutter.dev/flutter/widgets/RepaintBoundary-class.html`
- Flutter raster cache:
  `https://chromium.googlesource.com/external/github.com/flutter/engine/+/refs/heads/flutter-3.16-candidate.5/flow/raster_cache.cc`
- WebRender tile cache source:
  `https://docs.rs/webrender/latest/src/webrender/tile_cache.rs.html`
- Iced canvas cache docs:
  `https://docs.rs/iced/latest/iced/widget/canvas/type.Cache.html`
- Flutter local source:
  `/workspace/tmp-layout-engines/flutter/packages/flutter/lib/src/rendering/object.dart`
  `/workspace/tmp-layout-engines/flutter/packages/flutter/lib/src/rendering/layer.dart`
  `/workspace/tmp-layout-engines/flutter/packages/flutter/lib/src/rendering/custom_paint.dart`
  `/workspace/tmp-layout-engines/flutter_engine/flow/raster_cache.cc`
  `/workspace/tmp-layout-engines/flutter_engine/flow/raster_cache.h`
  `/workspace/tmp-layout-engines/flutter_engine/flow/raster_cache_key.h`
  `/workspace/tmp-layout-engines/flutter_engine/flow/raster_cache_util.h`
  `/workspace/tmp-layout-engines/flutter_engine/flow/layers/display_list_raster_cache_item.cc`
  `/workspace/tmp-layout-engines/flutter_engine/flow/layers/layer_raster_cache_item.cc`
- Slint local source:
  `/workspace/tmp-layout-engines/slint/internal/core/item_rendering.rs`
  `/workspace/tmp-layout-engines/slint/internal/core/partial_renderer.rs`
  `/workspace/tmp-layout-engines/slint/internal/renderers/software/lib.rs`
  `/workspace/tmp-layout-engines/slint/internal/renderers/skia/lib.rs`
  `/workspace/tmp-layout-engines/slint/internal/core/graphics/boxshadowcache.rs`
- Iced local source:
  `/workspace/tmp-layout-engines/iced/runtime/src/user_interface.rs`
  `/workspace/tmp-layout-engines/iced/graphics/src/geometry/cache.rs`
  `/workspace/tmp-layout-engines/iced/graphics/src/cache.rs`
  `/workspace/tmp-layout-engines/iced/graphics/src/text/cache.rs`
  `/workspace/tmp-layout-engines/iced/graphics/src/damage.rs`
- Servo local source:
  `/workspace/tmp-layout-engines/servo/components/layout/dom.rs`
  `/workspace/tmp-layout-engines/servo/components/layout/traversal.rs`
  `/workspace/tmp-layout-engines/servo/components/shared/layout/lib.rs`
  `/workspace/tmp-layout-engines/servo/components/layout/display_list/stacking_context.rs`
  `/workspace/tmp-layout-engines/webrender/webrender/src/tile_cache.rs`
  `/workspace/tmp-layout-engines/webrender/webrender/src/picture.rs`
- Scenic Driver Skia local source:
  `/workspace/tmp-layout-engines/scenic_driver_skia/GUIDES.md`
  `/workspace/tmp-layout-engines/scenic_driver_skia/native/scenic_driver_skia/src/renderer.rs`
