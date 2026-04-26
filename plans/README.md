# Plans

Last updated: 2026-04-26.

This directory tracks the native layout-caching roadmap and the background
investigation that led to the current implementation.

## Files

### `layout-caching-roadmap.md`

The active implementation roadmap.

Use this when deciding what to build next. It reflects the current repo state:
initial identity/storage/invalidation/cache work, origin-agnostic scheduling,
and targeted layout-affecting animation invalidation are done. The next work is
about broader resolve reuse, relayout boundaries, cheaper cache keys, and
refresh skipping.

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
- native stats collection is gated/default-off and exposed through one unified
  stats path:
  - `stats: true` enables collection without periodic logs
  - `renderer_stats_log: true` enables collection and periodic logs
  - `Native.stats/2` and `EmergeSkia.stats/2` expose peek/take/reset snapshots
- retained-layout benchmarks print grep-friendly layout-cache counters

## Next recommended implementation order

### 1. Improve text-flow/paragraph resolve caching

Recent cache counters show measurement caching is working well, while resolve
caching still misses heavily in text/layout/nearby-rich scenes.

Target areas:

- `Multiline`
- `WrappedRow`
- `TextColumn`
- `Paragraph`
- paragraph flow helpers that currently disable resolve-cache use

### 2. Add relayout/dependency boundaries

Introduce explicit dependency edges similar to Flutter's `parentUsesSize` idea:

- track whether parent layout depends on child size
- stop upward invalidation where parent geometry is isolated from child layout
- record relayout-boundary stop counters

### 3. Replace cloned child/nearby lists in cache keys with versions

Current cache keys still include child/nearby identity lists. That is simple and
correct, but it allocates/clones in hot layout paths. Measure/resolve traversal
also still uses some id-facing compatibility helpers even though production
topology is ix-based.

Future direction:

- add per-node spec/runtime/measure/resolve/subtree versions
- use dependency versions in cache keys
- keep explicit list keys only where topology ordering itself is the dependency
- make hot measure/resolve traversal more directly ix-native where useful

### 4. Add downstream refresh skipping

After layout reuse improves, make `refresh(tree)` skip subtrees with no relevant
layout/paint/registry changes.

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
