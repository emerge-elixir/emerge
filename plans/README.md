# Plans

Last updated: 2026-05-07.

This directory tracks active implementation notes and durable background
research for native layout and renderer work. Files with an `active-` prefix are
reserved for currently open implementation slices.

There are currently no active implementation plans. Completed implementation
records have been folded into this index or the durable reference notes below.

## Files

No `active-*.md` files are present right now.

## Folded Work

The old `completed/` directory and implementation-tied investigations were
removed after their useful state was folded into this index and the reference
documents below. Recently folded slices:

- layout-aware `Emerge.UI.scale/1` and `Emerge.UI.rotate/1`
- layout-aware transform animation
- mathematical `Emerge.UI.Size.min/2` and `max/2`
- performance branch merge-readiness fixes
- release-code bloat reductions for benchmark-only code, stats matrices, and
  renderer-cache admission helpers
- shared normal/profiled renderer draw traversal
- scroll viewport traversal culling
- renderer-cache parent/child lifecycle and stale-entry accounting
- direct renderer drawing optimizations and rejected benchmark-gated attempts
- frame-latency pacing and animation-cadence fixes
- renderer-cache engine investigation and Flutter comparison findings that have
  already landed
- macOS/Linux runtime convergence after the macOS hover refresh bug, including
  shared tree updates, event runtime driving, input normalization, present
  timing, cursor state, render timing stats, render-state scene installation,
  and macOS pipeline stats parity

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
- macOS and Linux now share retained-tree update semantics through the
  `TreeUpdateEngine`: `TreeMsg` application, animation sample timing,
  frame-attrs preparation, refresh/recompute decisions, cached-registry reuse,
  and asset-change refreshes all flow through the shared engine
- macOS still owns AppKit/window/protocol responsibilities, but its direct host
  runtime now shares the same event runtime driver behavior for input dispatch,
  timers, registry installs, cursor requests, present timing, and coalesced
  mouse-move bursts where synchronous AppKit text-input constraints allow it
- shared input helpers define pointer button labels/actions, scroll delta
  normalization, modifier packing, and text commit filtering so Wayland and
  macOS do not maintain divergent generic `InputEvent` construction
- Wayland, DRM, and macOS reuse the same plausible-frame-interval estimator for
  predicted next-present timing; macOS feeds present timing back into
  `HostEventRuntime`
- Wayland and macOS use a shared cursor icon reducer, while DRM keeps its
  hardware-cursor plane state backend-specific
- render timing stats recording uses a shared `RendererStatsCollector` helper
  for full `RenderTimings` values across Wayland, DRM, macOS Metal, and macOS
  raster
- renderer slow-frame diagnostics split render time into draw, GPU flush, GPU
  submit, and present-submit stages; profiled slow-frame logs now include scene
  summaries plus per-category draw timings, image details, and shadow details
- pipeline diagnostics split patch submission into tree actor, render queue,
  swap, and backend frame-callback/present timing so frame latency work can
  distinguish Emerge processing from backend/compositor pacing; macOS records
  the same split-pipeline stats from its direct dirty-frame draw path
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
- `RenderState::set_scene(...)` updates a scene and its derived
  `has_cache_candidates` flag together, so Wayland, DRM, and macOS cannot
  silently bypass renderer-cache traversal after installing a cache-candidate
  scene
- render-cache regression benchmarks compare cached and uncached refresh paths,
  including cold full layout+refresh after upload/switch; dirty/full rebuilds do
  not seed render caches, damaged refreshes with no existing caches use the
  uncached renderer, scroll-offset subtrees bypass render-cache lookup, and dirty
  scroll containers do not store large immediately-stale render caches
- event registry rebuilds have a conservative chunk-cache path with full-rebuild
  fallback for damaged/no-retained-cache and escape-nearby cases
- `animate_exit` removal keeps a cloned ghost subtree in active layout, with
  child, paint-child, and nearby topology remapped to ghost ids until pruning
- top-level `Emerge.UI.scale/1` and `Emerge.UI.rotate/1` are layout-aware attrs;
  `Emerge.UI.Transform.scale/1` and `Transform.rotate/1` remain paint-only
- layout-aware scale composes with global scale and ancestor layout scales while
  remaining unitless in animation keyframes
- layout-aware rotation reserves a transformed AABB, keeps authored
  `width`/`height` as the unrotated box, supports arbitrary angles, and has
  quarter-turn root fast paths
- transformed hit testing uses registry-built `HitGeometry` so pointer dispatch
  calls one `contains` path for rects, quads, local fallback geometry, and clips
- layout-aware scale and rotate are animatable and trigger layout-affecting
  invalidation; paint-only transform animations keep the refresh-only path
- `Emerge.UI.Size.min/2` and `max/2` are mathematical length combinators encoded
  as recursive length pairs, and row/column fill planning resolves nested fill
  leaves through a shared fill unit
- focused single-line text inputs suppress the follow-up Enter text commit when
  an Enter key-down binding is handled, so app-driven clears such as todo
  create remain authoritative
- the low-level macOS host frame/init protocol codec has Rust and Elixir fixture
  coverage; request/notify payload families remain mirrored across Rust and
  Elixir and should get fixtures before protocol expansion

## Next recommended implementation order

### 1. Fixture macOS request/notify protocol payloads

The low-level frame/init protocol now has parity fixtures. Before expanding the
macOS protocol, add request/notify-specific fixtures for start session, raw
input notify, element notify, asset config, and offscreen request payloads on
both the Rust host and Elixir sides.

### 2. Review render-cache children rollout with live traces

The parent/child lifecycle and stale-entry slice is implemented. The next cache
decision should start from fresh `../emerge_demo` stats: check stale eviction
churn, suppressed-by-parent counts, and whether current automatic candidates are
too cheap or too sparse before adding complexity scoring, transform expansion,
or a new composition-cache boundary.

### 3. Watch frame latency traces instead of adding scheduler policy

The Wayland frame-latency slice is implemented. Future work should start from
fresh split-pipeline traces before changing scheduler behavior. If repeated
`present submit`, `pipeline submit->swap`, or animation cadence issues return,
investigate compositor/driver behavior first and avoid fixed timing guesses.

### 4. Broaden other relayout/dependency boundaries

Nearby overlay topology no longer forces broad host/ancestor measurement or
resolve misses. The next layout-cache work should broaden boundaries for other
container/dependency shapes one at a time with focused correctness tests.

### 5. Revisit registry chunk seeding if profiles justify it

The guarded registry chunk infrastructure is in place. Leave damaged/no-cache
and escape-nearby cases on the full-rebuild fallback unless a future profile
shows registry rebuilds are the dominant cost and cheap seeding is proven safe.

### 6. Repeater/viewport-aware caching

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
