# Plans

Last updated: 2026-07-30.

This directory tracks active implementation notes and durable background
research for native layout, renderer, and input/runtime work.

Active implementation plans:

- `active-drm-gles2-compatibility.md`
- `active-low-resource-animation-smoothness.md`
- `active-combined-refresh-traversal.md`
- `active-combined-tree-walk-cleanup.md`

Files with an `active-` prefix are reserved for open implementation slices.
When a slice completes, fold the useful details into this index or one of the
durable reference notes below, then remove the completed implementation log.

## Files

### `active-drm-gles2-compatibility.md`

Implementation record for restoring OpenGL ES 2 as the explicit DRM baseline while
capability-gating timer profiling, PRIME video, core VAOs, and core sync paths.
Automated validation and base GLES2-only Macaw rendering are confirmed; extended
PRIME degradation and newer-device checks remain open.

### `active-low-resource-animation-smoothness.md`

Open plan for improving sidepane animation smoothness on constrained hardware by
instrumenting tree/refresh costs, reducing transform-only animation cache churn,
and avoiding expensive recompute paths where safe.

### `active-combined-refresh-traversal.md`

Implementation notes for the unified combined render/registry refresh traversal.

### `active-combined-tree-walk-cleanup.md`

Planning-only cleanup slice for making the combined tree walk easier to maintain,
including guardrails for the newly reported Showcase Borders/Interaction visual
clipping regressions.

### `active-enter-animation-completion.md`

Completed/current-context note for transient `animate_enter` completion so nearby
sidepane enter animations reach their final settled frame on both Wayland and
DRM before animation pulses stop.

### `active-drm-framerate-animation.md`

Open plan for correcting DRM display framerate reporting and animation pulse
prediction so fixed-mode displays report their physical refresh cadence while
animation remains page-flip backpressured.

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

### `elixir-reconciliation-optimization.md`

Completed Elixir runtime update optimization notes, including the assigned-tree
free viewport patch path, incremental event-registry reconciliation, benchmark
methodology, and measured speedups against the previous public update path.

### `biaxial-drag-scroll.md`

Completed input-runtime note for two-axis drag scrolling after threshold
activation, including the retained primary axis for inertia and split active
scroll dispatch for scroll containers that can move in both axes.

### `skia-ddl-paint-layer-note.md`

Durable renderer note about future Skia Deferred Display List / picture
recording options for paint-layer preparation. It records that paint layers
could eventually be recorded in parallel, while GPU raster/upload/composition
must stay serialized through the Skia GPU context.

### `platform-runtime-architecture-differences.md`

Explanatory note comparing Linux/Wayland actor-backed runtime orchestration with
macOS host-session orchestration. Use it when investigating platform parity,
event/tree registry synchronization, or convergence work.

## Folded Work

The old `completed/` directory and implementation-tied investigations were
removed after their useful state was folded into this index and the reference
documents below. Recently folded slices:

- Wayland suspend/resume and shutdown hardening, including render-log-gated
  Wayland/event-runtime diagnostics, direct watchdog evidence that suspend
  freezes were caused by the viewport heartbeat watchdog issuing a false stop,
  liveness checks that poll `renderer.running?/1` before stopping, synchronous
  `EmergeSkia.stop/1` native thread teardown, supervisor shutdown waiting for
  viewport renderer cleanup, immediate Wayland surface unmap on shutdown, and
  macOS stop-session timeout alignment
- first-class `Emerge.UI.Input.slider/2`, including `Slider.config/1`, standard
  Emerge element slots for track, filled track, and thumb, typed float change
  events, text-input-style controlled reconciliation, rotated hit testing,
  endpoint thumb reservation, and shadow bleed fixes
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
- renderer-diff parent-shell and clean-subtree scroll cache active plans; their
  benchmark evidence remains useful, but the separate algorithms were removed
  from active planning because they encode the wrong mental model for Emerge's
  tree-derived cache boundaries
- paint-layer cache boundary simplification, including removal of stale
  parent-shell / clean-subtree / scroll-item cache-family code, one
  tree-derived `RenderNode::PaintLayer` model, one shared paint-layer payload
  cache, paint-layer stats naming, benchmark deduplication, and final cleanup
  validation
- macOS/Linux runtime convergence after the macOS hover refresh bug, including
  shared tree updates, event runtime driving, input normalization, present
  timing, cursor state, render timing stats, render-state scene installation,
  and macOS pipeline stats parity
- cleanup of runtime-convergence scaffolding, including shared pipeline timing
  helpers and reduced macOS host wrapper-only tests
- renderer refactor cleanup after paint-layer cache work, including deletion of
  unused cache-boundary facts, consolidation of direct/cache/fixed paint-layer
  traversal, shared paint-layer hashing, shared cache stat accounting helpers,
  focused paint-layer cache proof benchmarks for scrolling and animation, and
  rich Borders showcase Criterion coverage; full `./ci-tests.sh` passed on
  2026-05-13
- renderer-cache audit fixes, including clipped non-cacheable parent layers
  still rendering child paint layers, `max_stale_frames` honoring stale eviction
  policy/stats, `min_visible_before_store` avoiding one-frame admission churn,
  and cleanup of pending screenshot/debug artifacts; validation passed on
  2026-05-15
- offscreen paint-layer cache-hit fixes, including payload-clip-aware fixed
  static hashing/resource generation, visible fixed-segment preparation,
  offscreen dynamic redraw skipping, scroll-away/scroll-back moving payload
  retention, and Criterion proof for the layout-animation scroll viewport case;
  full `./ci-tests.sh` passed on 2026-05-13
- layout/refresh optimization after composited paint-layer rendering,
  including exact emerge_demo showcase fixtures, retained nearby render
  fragments, shared registry listener storage, offscreen virtual-key culling,
  Scaled Press registry rebuild fixes, slider glow/thumb regression tests, and
  the final benchmark gate recheck; full `./ci-tests.sh all` passed on
  2026-05-15
- code-bloat reduction after retained layout/refresh and paint-layer cache work,
  including removal of dead renderer cache stats/API plumbing, Hex package
  native-test exclusion, stale layout benchmark wrapper consolidation,
  canonical `RenderPaintLayer` content cleanup, shared paint-layer/fingerprint
  hash helpers, retained cache-layer overlap audit, and final validation; full
  `./ci-tests.sh all` passed on 2026-05-18
- review-finding cleanup after code-bloat reduction, including stats schema
  version 15, pixel-level dirty-descendant paint refresh coverage, ordered
  paint-layer content splitting after child paint-layer boundaries, and removal
  of stale duplicate `uncached` layout benchmark labels; full
  `./ci-tests.sh all` passed on 2026-05-18
- Elixir reconciliation/runtime update optimization, including the runtime
  binary patch path that skips assigned-tree construction, reusable
  event-registry extraction, incremental per-vnode event registry updates,
  update-path benchmarks, and parity coverage for event-heavy mutations;
  validation passed on 2026-05-20
- biaxial drag scrolling for oversized two-axis scroll containers, preserving a
  primary gesture axis for inertia while active drag movement dispatches X and Y
  scroll components independently, plus Wayland stale-registry fixes for no-op
  scroll responses, reliable registry delivery, and listener-free overlay nearby
  blockers; validation passed on 2026-05-20

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
- behind-content non-registry nearby remove/restored-show changes classify as
  paint/render damage so warmed decorative toggles can use refresh-only
  scheduling and cached registry reuse; overlay nearby slots (`above`,
  `below`, `on_left`, `on_right`, `in_front`) classify as registry-relevant
  because their roots emit front-nearby interaction blockers even without
  explicit listeners
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
  - current public stats payload schema is version 17; renderer paint-layer
    stats keep aggregate admission/cache counters and `prepare`/`draw_hit`
    timings, while the `drm` section exposes EGL/GBM/atomic page-flip timing
    splits, sampled asynchronous GPU queue completion spans, and kernel
    flip-event, sequence-step, and missed-vblank counters; on V3D the timer
    uses Mesa CPU-queue timestamps around submission and dependency completion,
    so it is deliberately not labeled as active GPU execution time
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
- shared pipeline timing helpers now compute layout-output queue timestamps,
  retain earliest submitted times across dropped/direct frames, and record
  draw-start/present-complete spans for Linux actor, Wayland, and macOS paths
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
- refresh scene rendering emits explicit paint layers from tree facts rather
  than renderer-side diffing or retained-subtree discovery
- `RenderPaintLayer` content is canonicalized as `own_nodes` plus ordered
  `child_refs`; content after the first nested paint-layer boundary is kept in
  child refs so dirty child layers preserve paint order relative to later clean
  siblings
- retained `Nearby` fragments suppress damage-only child paint-layer boundaries
  because the enclosing nearby layer already isolates the overlay; otherwise a
  localized interaction can persist a sparse boundary topology that leaves later
  borders and siblings direct. Scroll and declared/active-animation boundaries
  remain independent and are preserved. Raspberry Pi 5 validation after interaction
  held 50.0 presentations/s with zero missed vblanks, two panel candidates, and a
  5.305 ms sampled GPU queue-completion span. Age-only stale-payload cleanup is
  amortized to one GPU payload per frame so a group crossing the 120-frame age
  threshold cannot all retire on one interaction frame; budget-pressure eviction
  remains immediate.
- paint-layer cache proof benchmarks are wired into Criterion for scrolling and
  animation; each case asserts cache store/hit behavior before measurement. The
  demo-like rich Borders showcase is also wired into Criterion layout animation
  and scroll-plus-animation benchmark groups.
- `RenderState::set_scene(...)` updates a scene and its derived paint-layer
  presence flag together, so Wayland, DRM, and macOS cannot silently bypass
  paint-layer cache traversal after installing a cacheable scene
- render-cache regression benchmarks cover retained refresh paths, including
  cold full layout+refresh after upload/switch, paint-only animation,
  scroll-moving paint-layer reuse, and CPU neutral/no-benefit paths
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
- `Emerge.UI.Input.slider/2` is implemented as a first-class controlled numeric
  input with `Emerge.UI.Input.Slider.config/1` aliased by `use Emerge.UI`
- slider track, filled-track, and thumb slots are normal Emerge elements; the
  slider owns track widths while callers control cross-axis sizing and visuals
- slider interactions support pointer press/drag, track click, focus, keyboard
  arrows, Home/End, PageUp/PageDown, typed float `:change` payloads, and
  controlled-value reconciliation parallel to text inputs
- slider geometry is horizontal in local coordinates; rotated presentations use
  existing layout-aware rotation and transformed hit testing
- slider layout reserves endpoint thumb space, supports custom SVG/image slots,
  and lets focus/shadow effects bleed outside non-scroll ancestor clips while
  preserving scroll-axis clipping
- Hex package inputs include required native sources and assets while excluding
  native Rust tests, benchmark-only fixtures, and external fixture payloads

## Next recommended implementation order

### 1. Fixture macOS request/notify protocol payloads

The low-level frame/init protocol now has parity fixtures. Before expanding the
macOS protocol, add request/notify-specific fixtures for start session, raw
input notify, element notify, asset config, and offscreen request payloads on
both the Rust host and Elixir sides.

### 2. Tune paint-layer cache only from live traces

The paint-layer cache model is implemented. Any next cache decision should start
from fresh `../emerge_demo` stats: check moved-hit reuse, stale eviction churn,
suppressed-by-parent counts, payload budget pressure, and whether current
paint-layer boundaries are too coarse or too fine before adding heuristics or
new composition behavior.

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
