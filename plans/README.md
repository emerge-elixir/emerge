# Plans

Last updated: 2026-04-26.

This directory tracks the native layout-caching roadmap and the background
investigation that led to the current implementation.

## Files

### `active-performance-merge-readiness-plan.md`

Completed merge-readiness evidence for the remaining
`performance-improvements` review concerns and the follow-up demo regressions.

Use this when checking what was validated before merging the performance
branch. It records full CI, benchmark evidence, broader native patch roundtrip
coverage, cache/topology watch-list hardening, benchmark fixture policy, and
the later animate-exit / todo-input regression fixes.

### `layout-caching-roadmap.md`

The active implementation roadmap.

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

### `native-tree-implementation-insights.md`

Implementation lessons from the completed node identity, `NodeIx` storage, and
native topology cleanup work.

This replaces the old separate node-identity / phase-4 / phase-5 plan files with
a single status-and-insights document.

### `performance-improvements-branch-review.md`

Branch review and revisit notes for `performance-improvements`.

Use this when checking merge readiness. It records the original blocker, the
resolved fixes, and the completed merge-readiness checklist.
The one-off completed fix plan was folded into this review and removed to keep
`plans/` focused on active or durable reference documents.

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
  - `Native.stats/2` and `EmergeSkia.stats/2` expose peek/take/reset snapshots
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

### 1. Merge or archive the performance branch

No active plan remains open. The completed merge-readiness plan records the
validation and hardening done before merge.

### 2. Broaden other relayout/dependency boundaries

Nearby overlay topology no longer forces broad host/ancestor measurement or
resolve misses. The next layout-cache work should broaden boundaries for other
container/dependency shapes one at a time with focused correctness tests.

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
