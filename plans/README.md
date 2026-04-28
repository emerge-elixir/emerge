# Plans

Last updated: 2026-04-29.

This directory tracks current implementation notes plus the background
investigations that led to the current native layout and renderer work. Files
with an `active-` prefix are reserved for currently open implementation slices.
`active-scroll-viewport-culling-plan.md` is the current active implementation
plan. `active-render-cache-children-plan.md` is implemented and retained as the
most recent completed active-plan record until the next cleanup pass.

## Files

### `active-scroll-viewport-culling-plan.md`

Current active plan for the scroll viewport traversal performance slice.

It now records the implemented benchmark-first shared viewport participation
gate for render traversal and event-registry traversal. The remaining active
work is focused exception auditing, direct regression tests for offscreen
pointer/focus/text-input behavior, and live-demo validation.

### `active-render-cache-children-plan.md`

The most recent completed renderer-cache implementation plan.

It records the implemented Flutter-inspired cache slice: parent/child cache
accounting, stale-entry lifecycle, and the benchmark-gated decision not to add a
new alpha-specific children-cache kind yet. It explicitly does not broaden
rotate, scale, fractional translation, active text input, video, placeholder,
shadow, text-blob, picture, or dirty-region cache scope.

### `layout-caching-roadmap.md`

The retained-layout implementation roadmap.

Use this when deciding what to build next. It reflects the current repo state:
initial identity/storage/invalidation/cache work, origin-agnostic scheduling,
targeted layout-affecting animation invalidation, text-flow resolve-cache
eligibility, the first relayout/dependency boundary, compact topology version
cache keys, refresh subtree skipping, and nearby relayout boundaries are done.
The next feature work is broader boundaries and viewport/repeater-aware caching.

### `layout-caching-engine-insights.md`

Cross-engine research notes.

This preserves the useful findings from Taffy, Yoga, Flutter, Slint, Iced, and
Servo. It is intentionally more detailed than the roadmap because it records why
certain design directions fit Emerge.

### `rendering-cache-engine-investigation.md`

Cross-engine rendering-cache research notes.

Use this when choosing renderer-thread performance work. It compares Flutter
repaint boundaries/raster cache, Slint dirty-region and shadow caches, Iced
geometry caches, Servo/WebRender display-list and tile caching, and Scenic
Driver Skia script replay against Emerge's current renderer. It covers retained
renderer output only; non-cache direct draw-path work lives in
`drawing-optimization-investigation.md`.

### `render-cache-flutter-comparison.md`

Focused comparison between Emerge's current clean-subtree renderer cache and
Flutter's Skia/Ganesh raster cache. Use this when deciding whether the next
cache slice should copy Flutter concepts such as stale-entry eviction,
layer-children caching, transform normalization, or complexity scoring.

### `caching-implementation-review.md`

Code-review style snapshot of all caching currently implemented in native
layout, refresh, registry, renderer payloads, and renderer resource caches. Use
this before adding new cache scope; it records current strengths, risks, and
the diagnostic work that should come before additional cache complexity.

### `frame-latency-implementation-notes.md`

Completed frame-latency implementation notes.

Use this when revisiting patch-to-visible-frame latency. The Wayland scheduler
now disables EGL swap-interval pacing when supported, allows at most one static
patch-derived late replacement while a frame callback is pending, and excludes
animation-active scenes from late replacement. Animation sampling is anchored to
Wayland frame callbacks rather than post-render swap completion. The file also
records DRM, macOS Metal, and raster/offscreen applicability for future
backend-specific work.

### `drawing-optimization-investigation.md`

Non-cache drawing optimization research notes.

Use this when investigating direct draw-path improvements without introducing
renderer caches: Skia primitive fast paths, shader/pipeline warmup, saveLayer
reduction, template tint, opacity distribution, shadow alternatives, backend
options, and benchmark gates for drawing optimizations.

### `native-tree-implementation-insights.md`

Implementation lessons from the completed node identity, `NodeIx` storage, and
native topology cleanup work.

This replaces the old separate node-identity / phase-4 / phase-5 plan files with
a single status-and-insights document.

### `performance-improvements-branch-review.md`

Branch review and revisit notes for `performance-improvements`.

Use this when checking merge readiness. It records the original blocker, the
resolved fixes, and the completed merge-readiness checklist.
The completed merge-readiness plan and one-off fix plan were folded into this
review and removed to keep `plans/` focused on active or durable reference
documents.

## Current repo state

The native layout-caching foundation is in place:

- shared runtime identity is `NodeId`
- native traversal/storage identity is `NodeIx`
- `ElementTree` is dense/index-backed with `id_to_ix`
- production topology is `NodeIx`-based with parent/host links
- nodes are split into `NodeSpec`, `NodeRuntime`, `NodeLayoutState`, and
  `NodeLifecycle`
- `TreeInvalidation` distinguishes registry, paint, resolve, measure, and
  structure invalidation
- the tree actor combines external invalidation with sampled/effective dynamic
  invalidation before choosing skip, cached registry rebuild, refresh, or layout
- layout caches exist for:
  - intrinsic leaf/media/text measurement
  - subtree measurement
  - coordinate-invariant resolved layout
- layout-affecting animation samples are converted into ordinary dirty paths so
  unrelated clean subtrees can still use caches
- measure/resolve dirtiness propagates upward through parent links
- measure dirtiness can stop at the first fixed-size `El`/`None` boundary while
  traversal dirtiness keeps dirty descendants reachable
- nearby topology changes mark nearby traversal/refresh work without forcing
  host/ancestor measurement dirtiness when host size is independent of the
  nearby overlay
- recently removed small nearby subtrees can restore detached layout state when
  the same animation-free structural signature is reinserted with the same
  attachment context, avoiding repeated cold code-block layout on hover toggles
- detached nearby layout cache restore is scoped by host id, slot, host frame,
  subtree signature, and scale so changed-host or changed-slot reinserts
  relayout instead of reusing stale absolute frames
- non-registry nearby remove/restored-show changes classify as paint/render
  damage so warmed code-preview hover toggles can use refresh-only scheduling
  and cached registry reuse
- subtree-measure cache keys use compact child topology dependency versions and
  intentionally ignore nearby topology; resolve/cache-render keys still include
  nearby topology where output can depend on ordering/placement
- native stats collection is gated/default-off and exposed through one unified
  stats path:
  - `stats: true` enables collection without periodic logs
  - `renderer_stats_log: true` enables collection and periodic logs
  - `renderer_animation_log: true` enables separate Wayland animation cadence
    trace logs without coupling them to renderer stats logs
  - `Native.stats/2` and `EmergeSkia.stats/2` expose peek/take/reset snapshots
- renderer slow-frame diagnostics split render time into draw, GPU flush, GPU
  submit, and present-submit stages; profiled slow-frame logs now include scene
  summaries plus per-category draw timings, image details, and shadow details
- pipeline diagnostics split patch submission into tree actor, render queue,
  swap, and Wayland frame-callback wait so frame latency work can distinguish
  Emerge processing from backend/compositor pacing
- profiled renderer slow-frame logs also include clip, border, and layer detail
  for direct drawing optimization work
- direct renderer drawing benchmarks cover focused border, tint, alpha, shadow,
  clip, gradient, image, cold-frame, GPU-surface, and mixed-scene cases;
  `drawing_opt_before` is the baseline for non-cache drawing optimizations
- proven non-cache drawing optimizations have landed for unclipped solid
  borders, template-image tint without `saveLayer`, and narrow single-primitive
  alpha distribution; clipped border fast paths, clip combining, direct Skia
  shadows, and warmup behavior stayed out of renderer code because benchmarks did
  not prove a win
- renderer-cache work has a saved `render_cache_before` Criterion baseline,
  fresh demo trace gate, shared `SceneRenderer` cache lifecycle, generation
  clear, per-frame payload budget, stats path, configurable
  `EmergeSkia.start/1` cache limits, GPU render-target payloads for GPU frames,
  CPU raster fallback for raster/offscreen frames, prepare-before-draw
  admission, layout-reflow placement reuse, and root element-alpha composition
  reuse
- renderer-cache lifecycle now tracks seen/visible/used state separately,
  touches existing descendant entries as `suppressed_by_parent` when a parent
  payload hits or prepares, and ages out entries that have not been seen for the
  stale-frame window; stats expose suppressed counts, stale evictions, and stale
  bytes
- a separate nested-alpha children-cache kind was benchmarked and left out
  because the measured GPU microbench did not beat direct drawing; root
  clean-subtree alpha composition remains the production alpha cache path
- render-subtree cache keys include asset source status generation, so a subtree
  cached while an image is pending is invalidated when that asset becomes ready
  or failed; image loading/failure placeholders now use light neutral/soft error
  visuals
- Wayland frame latency uses callback-paced rendering with nonblocking EGL swap
  when supported, one-shot static late replacement, animation-active replacement
  exclusion, and callback-anchored animation sample timing
- raster image assets are decoded eagerly when inserted into the renderer asset
  cache so deferred PNG/JPEG decode is not paid during the first draw
- retained-layout benchmarks print grep-friendly layout-cache counters
- refresh-specific dirty state tracks render vs registry damage separately from
  layout-cache outcomes
- animation-only refresh frames can update effective attrs for active animation
  nodes without re-preparing every node once root geometry exists
- cached-registry refresh avoids cloning the full registry payload when the
  registry did not change
- render refresh culls clipped/offscreen subtrees using conservative visual
  bounds that account for shadows and transforms
- refresh-only frames can reuse the cached full event registry when registry
  damage is clean
- refresh scene rendering can reuse clean retained render subtrees
- render-cache regression benchmarks compare cached and uncached refresh paths,
  including cold full layout+refresh after upload/switch; dirty/full rebuilds do
  not seed render caches, damaged refreshes with no existing caches use the
  uncached renderer, scroll-offset subtrees bypass render-cache lookup, and dirty
  scroll containers do not store large immediately-stale render caches
- event registry rebuilds have a conservative chunk-cache path with full-rebuild
  fallback for damaged/no-retained-cache and escape-nearby cases
- `animate_exit` removal keeps a cloned ghost subtree in active layout, with
  child, paint-child, and nearby topology remapped to ghost ids until pruning
- focused single-line text inputs suppress the follow-up Enter text commit when
  an Enter key-down binding is handled, so app-driven clears such as todo
  create remain authoritative

## Next recommended implementation order

### 1. Review render-cache children rollout with live traces

The parent/child lifecycle and stale-entry slice is implemented. The next cache
decision should start from fresh `../emerge_demo` stats: check stale eviction
churn, suppressed-by-parent counts, and whether current automatic candidates are
too cheap or too sparse before adding complexity scoring, transform expansion,
or a new composition-cache boundary.

### 2. Watch frame latency traces instead of adding scheduler policy

The Wayland frame-latency slice is implemented. Future work should start from
fresh split-pipeline traces before changing scheduler behavior. If repeated
`present submit`, `pipeline submit->swap`, or animation cadence issues return,
investigate compositor/driver behavior first and avoid fixed timing guesses.

### 3. Broaden other relayout/dependency boundaries

Nearby overlay topology no longer forces broad host/ancestor measurement or
resolve misses. The next layout-cache work should broaden boundaries for other
container/dependency shapes one at a time with focused correctness tests.

### 4. Revisit registry chunk seeding if profiles justify it

The guarded registry chunk infrastructure is in place. Leave damaged/no-cache
and escape-nearby cases on the full-rebuild fallback unless a future profile
shows registry rebuilds are the dominant cost and cheap seeding is proven safe.

### 5. Repeater/viewport-aware caching

Later large-list work should preserve cache identity across dynamic list edits
and viewport movement.

## Validation expectations

For implementation work, run at least:

```bash
cargo test --manifest-path native/emerge_skia/Cargo.toml
mix test
```

For focused layout-cache work, also run a small benchmark smoke such as:

```bash
EMERGE_BENCH_SCENARIOS=list_text \
EMERGE_BENCH_SIZES=50 \
EMERGE_BENCH_MUTATIONS=layout_attr \
EMERGE_BENCH_WARMUP=0.1 \
EMERGE_BENCH_TIME=0.1 \
EMERGE_BENCH_MEMORY_TIME=0 \
mix bench.native.retained_layout
```

Use the printed `layout_cache_stats` lines to choose the next optimization.
