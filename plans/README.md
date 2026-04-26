# Plans

Last updated: 2026-04-26.

This directory tracks the native layout-caching roadmap and the background
investigation that led to the current implementation.

## Files

### `active-nearby-relayout-boundary-plan.md`

The current temporary active implementation plan. It targets the next layout
caching slice: making nearby overlay mount/unmount work proportional to the
nearby subtree instead of dirtying broad host/ancestor layout paths. The plan
starts with a benchmark/test guard for the Borders hover/unhover code-block
shape before changing invalidation.

### `layout-caching-roadmap.md`

The active implementation roadmap.

Use this when deciding what to build next. It reflects the current repo state:
initial identity/storage/invalidation/cache work, origin-agnostic scheduling,
targeted layout-affecting animation invalidation, text-flow resolve-cache
eligibility, the first relayout/dependency boundary, compact topology version
cache keys, and refresh subtree skipping are done. The next work is nearby
relayout/dependency boundaries, broader boundaries, and viewport/repeater-aware
caching.

### `layout-caching-engine-insights.md`

Cross-engine research notes.

This preserves the useful findings from Taffy, Yoga, Flutter, Slint, Iced, and
Servo. It is intentionally more detailed than the roadmap because it records why
certain design directions fit Emerge.

### `native-tree-implementation-insights.md`

Implementation lessons from the completed node identity, `NodeIx` storage, and
native topology cleanup work.

This replaces the old separate node-identity / phase-4 / phase-5 plan files with
a single status-and-insights document.

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
- subtree-measure and resolve cache keys use compact child/nearby topology
  dependency versions instead of cloned child/nearby identity lists
- native stats collection is gated/default-off and exposed through one unified
  stats path:
  - `stats: true` enables collection without periodic logs
  - `renderer_stats_log: true` enables collection and periodic logs
  - `Native.stats/2` and `EmergeSkia.stats/2` expose peek/take/reset snapshots
- retained-layout benchmarks print grep-friendly layout-cache counters
- refresh-specific dirty state tracks render vs registry damage separately from
  layout-cache outcomes
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

## Next recommended implementation order

### 1. Add a nearby relayout/dependency boundary

Refresh subtree skipping is validated conservatively. The next active slice is
nearby overlay topology: `SetNearbyMounts`/nearby subtree changes should not
force broad host/ancestor measurement and resolve misses when the host's measured
size is independent of the nearby overlay.

### 2. Broaden other relayout/dependency boundaries

The first boundary covers fixed-size `El`/`None`, and the active slice targets
nearby overlays. Broader row/column, scrollable, and text-flow boundaries should
be added only with focused correctness tests.

### 3. Revisit registry chunk seeding if profiles justify it

The guarded registry chunk infrastructure is in place. Leave damaged/no-cache
and escape-nearby cases on the full-rebuild fallback unless a future profile
shows registry rebuilds are the dominant cost and cheap seeding is proven safe.

### 4. Repeater/viewport-aware caching

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
