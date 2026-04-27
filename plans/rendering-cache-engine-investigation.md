# Rendering Cache Engine Investigation

Last updated: 2026-04-27.

This document investigates rendering optimization and caching approaches used by
other UI/rendering engines, then maps them onto Emerge's current renderer. It is
not an active implementation plan. It is a reference document for deciding the
next renderer-performance slice.

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

#### Cache hints matter

Flutter exposes `CustomPaint.isComplex` and `willChange`. Those hints do not
force caching, but they inform compositor heuristics.

Transfer to Emerge:

- Automatic heuristics should be the default.
- A later public attribute such as `render_cache: :auto | :static | :volatile`
  could help app authors mark expensive static drawing or known volatile
  subtrees.
- Volatile flags should matter for cursors, active text inputs, video, and
  per-frame animations.

## Slint

Primary sources:

- `/workspace/tmp-layout-engines/slint/internal/core/partial_renderer.rs`
- `/workspace/tmp-layout-engines/slint/internal/renderers/software/lib.rs`
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

- Emerge's current expensive first-frame draw is a CSS-like blurred box shadow.
- A physical-pixel shadow texture cache is the most directly justified
  primitive cache.
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

## 1. Keep improving instrumentation first

Current diagnostics are already useful. Before adding caches with correctness
and memory risk, add focused detail only where profiles ask for it.

Immediate additions worth considering:

- per-border detail for slow frames:
  - count solid vs dashed vs dotted
  - count uniform vs asymmetric widths
  - include max border draw time and dimensions
- text detail:
  - top slow text draw count by font/size/family, not full strings
  - count text blob/cache eligibility once implemented
- cache stats:
  - shadow cache hits/misses/stores/evictions/bytes
  - text blob cache hits/misses/stores/evictions/bytes
  - layer/picture cache hits/misses/stores/evictions/bytes

Why first:

- The latest logs already changed the diagnosis from image draw to shadows.
- Caches can move cost between draw, flush, and present; stats need to show the
  whole frame, not only draw time.

## 2. Low-risk draw fast paths

These are not full renderer caches, but they reduce the cost shown in the later
assets-page frame.

### Solid border fast paths

Current `draw_border` builds a band path and draws it even for simple solid
borders. For common cases, use cheaper Skia operations:

- uniform rectangular solid border:
  - draw four filled rects, or stroke rect when pixel parity is acceptable
- uniform rounded solid border:
  - draw `RRect` stroke clipped to border band only when needed
- asymmetric non-rounded borders:
  - draw edge rects directly
- keep current path-based fallback for:
  - asymmetric rounded joins,
  - dashed/dotted,
  - fractional cases that need existing hairline behavior.

Expected impact:

- targets the observed `borders=0.939 ms` frame
- low memory risk
- correctness can be guarded with existing border pixel tests

### Clip stack reduction

Recent scene summaries show many clip scopes and clip shapes. Cheap reductions:

- avoid emitting empty clip nodes
- combine adjacent identical clip shapes
- avoid reapplying the same inherited clip around separate decorative/content
  groups when paint order allows
- preserve the existing shadow-pass clip escape semantics

Expected impact:

- reduces CPU draw overhead and may lower Skia clip-stack work
- must be guarded by shadow/nearby/scroll clipping tests

## 3. Box-shadow texture cache

This is the highest-priority cache candidate from the latest logs.

### Why

- The assets-page slow frame had no images and only a small text count.
- Shadow draw alone took about 2 ms, followed by about 6 ms in GPU flush and
  present synchronization.
- Slint has a purpose-built box-shadow cache with almost the same key shape.

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

## 4. Text blob cache

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

## 5. Retained Skia picture cache for clean subtrees

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

## 6. Raster layer cache for expensive stable subtrees

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

## 7. Dirty-region redraw

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

## 8. Tile cache for scrollable content

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

### Slice 1: diagnostics plus border fast path

Goal:

- make the next assets-page trace explain border/text/clip cost as clearly as
  image/shadow cost
- reduce known simple border overhead without new cache memory

Work:

- add border detail to slow-frame logs
- implement solid simple border fast paths
- keep path fallback for complex borders

### Slice 2: shadow texture cache

Goal:

- reduce repeated large blurred shadow draw cost and GPU flush pressure

Work:

- implement bounded shadow cache
- add shadow cache stats
- guard with pixel tests and LRU tests
- validate on showcase/assets and todo card shadows

### Slice 3: text blob cache if profiles still show text cost

Goal:

- reduce repeated label/code text draw overhead

Work:

- instrument text draws by font/size/count
- cache text blobs for static single-line text
- exclude active text input paths initially

### Slice 4: retained Skia picture cache

Goal:

- skip CPU primitive replay for clean static subtrees

Work:

- prototype on subtrees already eligible for `RenderSubtreeCache`
- keep strict caps
- compare cached and uncached raster output

### Slice 5: raster layer cache

Goal:

- skip repeated expensive rasterization for stable complex layers

Work:

- access-thresholded layer cache
- explicit bytes budget
- seen/evict lifecycle
- avoid volatile subtrees

### Slice 6: dirty-region redraw and later tile cache

Goal:

- avoid full-surface redraw for localized changes
- later, reuse scrolled content at tile granularity

Work:

- start with raster backend
- then DRM/Wayland only if backend buffer semantics are solid

## Design Rules for Emerge

- Keep layout cache, render-scene cache, and renderer-thread caches separate.
- Cache keys must use typed generations where possible, not debug strings.
- Every renderer cache needs hit/miss/store/eviction/byte stats.
- Every cache must have a hard byte or count budget.
- Cache clearing on backend/context loss must be cheap and correct.
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
- Should shadow cache creation happen lazily on first draw, or should the render
  thread have an idle/prewarm phase after scene upload?
- Should layer cache eligibility be purely automatic, or should Emerge expose an
  Elixir-side cache hint once the internal implementation is proven?
- Is Skia `Picture` recording enough to lower the border/text frame, or are
  direct draw fast paths and text blobs better first?
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
- Slint local source:
  `/workspace/tmp-layout-engines/slint/internal/core/partial_renderer.rs`
  `/workspace/tmp-layout-engines/slint/internal/renderers/software/lib.rs`
  `/workspace/tmp-layout-engines/slint/internal/core/graphics/boxshadowcache.rs`
- Iced local source:
  `/workspace/tmp-layout-engines/iced/runtime/src/user_interface.rs`
  `/workspace/tmp-layout-engines/iced/graphics/src/geometry/cache.rs`
  `/workspace/tmp-layout-engines/iced/graphics/src/cache.rs`
- Servo local source:
  `/workspace/tmp-layout-engines/servo/components/layout/dom.rs`
  `/workspace/tmp-layout-engines/servo/components/layout/traversal.rs`
  `/workspace/tmp-layout-engines/servo/components/shared/layout/lib.rs`
  `/workspace/tmp-layout-engines/servo/components/layout/display_list/stacking_context.rs`
  `/workspace/tmp-layout-engines/webrender/webrender/src/tile_cache.rs`
- Scenic Driver Skia local source:
  `/workspace/tmp-layout-engines/scenic_driver_skia/GUIDES.md`
  `/workspace/tmp-layout-engines/scenic_driver_skia/native/scenic_driver_skia/src/renderer.rs`
