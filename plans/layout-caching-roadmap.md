# Layout Caching Roadmap

Last updated: 2026-04-26.

This is the active roadmap for native retained-layout caching. It intentionally
references the fuller research notes in `layout-caching-engine-insights.md` and
the implementation lessons in `native-tree-implementation-insights.md` instead
of repeating them all here.

## Current implementation status

### Foundation complete

The repo already has the foundation that the older plans described as future
work:

- native `NodeId` / `NodeIx` identity split
- dense native `ElementTree` storage with `id_to_ix`
- production `NodeIx` topology and parent/host links
- node state split into `NodeSpec`, `NodeRuntime`, `NodeLayoutState`, and
  `NodeLifecycle`
- typed invalidation through `TreeInvalidation`
- origin-agnostic frame update planning that combines external invalidation with
  sampled/effective dynamic invalidation before work selection
- refresh decisions that distinguish skip, cached rebuild, refresh-only, and
  recompute from invalidation plus cached output availability
- upward propagation for measure/resolve dirtiness
- traversal dirtiness for dirty descendants below reusable measurement caches
- first measure dependency boundary for fixed-size `El`/`None` parents
- per-node intrinsic measurement cache
- per-node subtree measurement cache
- coordinate-invariant resolve cache
- resolve-cache eligibility for text-flow kinds (`Multiline`, `WrappedRow`,
  `TextColumn`, and `Paragraph`)
- targeted dirty propagation for layout-affecting animation samples
- gated native stats snapshots/logging through one unified stats path
- retained-layout benchmark cache-counter output

Relevant files:

- `native/emerge_skia/src/tree/invalidation.rs`
- `native/emerge_skia/src/tree/element.rs`
- `native/emerge_skia/src/tree/layout.rs`
- `native/emerge_skia/src/tree/layout/tests/cache.rs`
- `native/emerge_skia/src/stats.rs`
- `native/emerge_skia/src/lib.rs`
- `bench/native_retained_layout_bench.exs`

### What remains

The remaining work is about making reuse broader, cheaper, and more precise:

- relayout/dependency boundaries currently cover only fixed-size `El`/`None`
  parents; row/column, scrollable, nearby, and text-flow boundaries remain
  conservative
- cache keys still clone child/nearby identity lists
- measure/resolve traversal still uses some id-facing compatibility helpers even though topology is ix-based
- `refresh(tree)` does not yet skip clean subtrees
- dynamic list / viewport cache preservation is not specialized yet

## Current benchmark signal

Small retained-layout benchmark smokes show:

- measurement cache reuse is healthy
- subtree measurement misses are low in common retained scenarios
- simple list resolve reuse is good
- text-flow warm-cache scenarios now produce resolve hits with zero resolve
  misses after the text-flow eligibility slice

Focused smoke after text-flow resolve caching:

```text
layout_matrix_50 warm_cache: resolve_hits=1 resolve_misses=0 resolve_stores=0
text_rich_50 warm_cache:    resolve_hits=1 resolve_misses=0 resolve_stores=0
layout_matrix_50/layout_attr after_patch: resolve_hits=4 resolve_misses=3 resolve_stores=3
text_rich_50/layout_attr after_patch:    resolve_hits=4 resolve_misses=3 resolve_stores=3
```

Important caveat: origin-agnostic scheduling now keeps paint-only updates on the
refresh path regardless of whether they came from animation, scroll, patching,
or runtime state. Layout-affecting animations now dirty affected paths and keep
layout caches enabled elsewhere. The first fixed-size `El`/`None` measure
boundary is implemented; broader boundaries and cheaper cache keys remain future
work.

## Completed slice: simplify layout-cache stats

Layout-cache stats now use hit/miss/store outcomes for benchmark-facing cache
families:

- intrinsic measurement hits/misses/stores
- subtree measurement hits/misses/stores
- resolve hits/misses/stores

Dirty, ineligible, animation-disabled, and store-disabled cases are no longer
reported as separate cache-bypass categories. If a cache family cannot reuse a
result, it should be reflected as a miss or as separate dirty/invalidation stats
outside the cache outcome model.

## Completed slice: paint-only animation refresh path

Animation samples are now classified by layout effect. Animation-only pulses and
refresh-only invalidations such as scroll whose sampled animation attrs are
paint-only, including animated `Border.shadow`/`box_shadows`, refresh without
running measure/resolve layout. Layout-affecting animations remain conservative.

This slice proved the desired behavior on important demos and was followed by
the origin-agnostic scheduler refactor below.

A native criterion benchmark modeled after the demo Borders shadow showcase was
added under:

```text
native/layout_animation_paint_only/shadow_showcase
```

Local smoke result:

```text
full_layout_plus_refresh_each_frame       ~1.10 ms
paint_only_refresh_each_frame             ~0.81 ms
full_layout_plus_refresh_scroll_frame     ~1.12 ms
paint_only_refresh_scroll_frame           ~0.83 ms
```

## Completed slice: origin-agnostic invalidation/work scheduling

Goal: make work selection depend only on invalidation/damage class, not update
source.

Target model:

```text
external invalidation from messages
+ dynamic invalidation from sampled/effective state
= combined TreeInvalidation
-> skip / cached registry / refresh / layout
```

Implemented shape:

- the tree actor builds one `FrameUpdatePlan` per batch
- `AnimationPulse` requests dynamic sampling instead of forcing `Measure`
- frame/effective attrs are prepared before work selection when needed
- sampled animation invalidation is joined with patch/scroll/runtime invalidation
- `decide_refresh_action` no longer receives broad active-animation state; it
  uses `TreeInvalidation` plus cached output/geometry availability
- prepared refresh/layout helpers avoid doing frame preparation twice
- source-equivalence tests cover paint-only shadow patching, paint-only shadow
  animation, scroll with paint-only animation, and paint-only patch plus
  paint-only animation

Paint-only updates should now have no layout sample and no layout-cache counter
movement because measure/resolve layout is not asked.

## Completed slice: precise layout-affecting animation invalidation

Animation sampling now records per-node layout effects. Before layout runs,
measure- and resolve-affecting animation effects are converted into ordinary
dirty state through the same propagation helpers used by patches/runtime state.
Layout caches stay enabled for the pass, so unrelated clean sibling subtrees can
hit while the animated path misses/stores as needed.

Implemented shape:

- `AnimationOverlayResult` records per-node `AnimationLayoutEffect` entries
- `run_layout_passes(...)` marks only layout-affecting animation effects dirty
- broad animation cache disabling and `mark_all_resolve_dirty()` fallback were
  removed from the layout root path
- discrete `align_x` / `align_y` animation samples are now applied and
  classified as resolve-affecting
- tests cover sibling measurement-cache reuse during width animation and no text
  remeasure during align animation

## Completed slice: text-flow resolve caching

Text-flow-heavy kinds are now eligible for coordinate-invariant resolve-cache
reuse:

- `Multiline`
- `WrappedRow`
- `TextColumn`
- `Paragraph`

Implementation notes:

- wrapped rows now pass the normal resolve-cache flag through child resolution
  instead of forcing child resolve misses
- text columns pass cache eligibility to normal and floating children
- paragraphs can store even when inline children do not have independent resolve
  caches, because the paragraph owns the inline fragment layout
- text columns can store paragraph child layout that is owned by the text-flow
  parent
- paragraph fragment positions are shifted with the subtree on cache hits

Tests cover unchanged warm hits, width/font changes that miss and match uncached
layout, text columns with paragraph children, wrapped rows with changed wrapping
width, and paragraph fragment shifting after parent alignment changes.

Remaining text/layout-rich misses should now be investigated through relayout
boundaries, key construction costs, nearby placement, or refresh-output reuse
rather than blanket kind ineligibility.

## Completed slice: first relayout/dependency boundary

Dirty propagation now distinguishes a node's own measurement dirtiness from
measurement traversal needed by dirty descendants. This lets a parent keep its
own measurement cache hot while still descending to update a dirty child.

Implemented shape:

- `NodeLayoutState` has `measure_descendant_dirty` in addition to
  `measure_dirty`
- measure invalidation from patches/runtime/animation marks the changed node
  dirty, marks ancestors for traversal, and keeps resolve dirtiness conservative
- structure invalidation remains fully conservative
- `measure_element(...)` will not let a clean ancestor subtree cache hide dirty
  descendants; it traverses first, then can reuse the ancestor's cached measured
  frame when the ancestor itself stayed clean
- first safe boundary: `El`/`None` parents with child-independent explicit width
  and height (`Px` or `Minimum`/`Maximum` wrappers around child-independent
  lengths)
- content-sized `El`/`None`, row/column, text-flow, scrollable, and nearby cases
  remain conservative until separately proven safe

Focused tests cover fixed-size `El` text changes, content-sized `El`
conservatism, fixed-size `El` animation, traversal through a cached parent, and
continued resolve alignment correctness.

Focused smoke after this slice:

```text
layout_matrix_50 warm_cache: resolve_hits=1 resolve_misses=0 subtree_measure_hits=1 subtree_measure_misses=0
text_rich_50 warm_cache:    resolve_hits=1 resolve_misses=0 subtree_measure_hits=1 subtree_measure_misses=0
layout_matrix_50/layout_attr after_patch: subtree_measure_hits=3 subtree_measure_misses=2 resolve_hits=4 resolve_misses=3
layout_matrix_50/text_content after_patch: subtree_measure_hits=2 subtree_measure_misses=3 resolve_hits=4 resolve_misses=3
text_rich_50/layout_attr after_patch: subtree_measure_hits=3 subtree_measure_misses=2 resolve_hits=4 resolve_misses=3
text_rich_50/text_content after_patch: subtree_measure_hits=2 subtree_measure_misses=3 resolve_hits=4 resolve_misses=3
```

## Next slice 1: versioned cache keys and ix-native traversal cleanup

Goal: reduce hot-path allocation/cloning in cache keys and make parent cache
validation cheaper.

Current conservative keys include child and nearby identity lists. That is
correct and easy to reason about, but list cloning is not the long-term shape.
The code also still has measure/resolve paths that call id-facing helpers such
as `child_ids(...)`, `nearby_mounts_for(...)`, and `get(&NodeId)` after topology
has already resolved to `NodeIx`. That compatibility shape is acceptable today,
but it should be revisited when changing cache-key construction.

Future version fields may include:

```rust
struct NodeVersions {
    spec_rev: u64,
    runtime_layout_rev: u64,
    measure_rev: u64,
    resolve_rev: u64,
    subtree_rev: u64,
    topology_rev: u64,
    nearby_rev: u64,
}
```

Guidelines:

- only replace list keys where a version captures the same dependency
- keep explicit topology/order keys where order itself is the dependency
- use stats/benchmarks to prove the allocation reduction matters

Acceptance criteria:

- no correctness regressions
- less key construction/cloning in profiles
- fewer repeated `NodeId -> NodeIx` lookups in measure/resolve hot paths where practical
- cache hit behavior unchanged or improved

## Next slice 2: refresh subtree skipping

Goal: after layout state is reused, avoid rebuilding render/event output for
subtrees that did not change.

`refresh(tree)` should be able to skip a subtree when:

- geometry did not change
- paint-relevant attrs did not change
- registry-relevant attrs did not change
- descendants have no relevant changes

Potential stats:

- refresh subtrees visited
- refresh subtrees skipped
- scene nodes rebuilt
- registry nodes rebuilt

Acceptance criteria:

- lower refresh time in steady-state / paint-only / registry-only cases
- no event hit-test regressions
- no stale scene output

## Later slice: broaden relayout/dependency boundaries

The first boundary is intentionally narrow. Future boundaries should be added
one at a time with correctness tests for measured frames, resolved frames,
scroll extents, nearby placement, and paragraph fragments.

Candidates:

- fixed-size `Row` / `Column` cases where parent measured size is independent of
  the changed child
- scrollable fixed-size containers where content extents change but parent
  measured size does not
- nearby overlays that should not dirty host measurement
- text-flow containers once wrapping/floats/fragments are fully covered

## Later slice: viewport/repeater-aware caching

Goal: preserve cache identity for large dynamic lists as items are inserted,
removed, reordered, virtualized, or moved through a viewport.

This should build on:

- stable `NodeId` semantics
- keyed reconciliation
- versioned dependency keys
- relayout boundaries

## Rules for future performance work

- stats stay gated/default-off
- avoid per-stat NIFs; use the unified stats path
- normal `cargo test` and `mix test` must not run benchmarks
- Rust benchmarks belong under `native/emerge_skia/benches/`
- Elixir benchmarks belong under `bench/`
- commit completed slices separately
- keep layout caching separate from scene/event refresh caching
- prefer centralized cache logic over scattered per-kind special cases
- choose work from invalidation/damage class, not from update source such as
  animation pulse, scroll, patch, hover, or focus

## Validation

For code changes, run at least:

```bash
cargo test --manifest-path native/emerge_skia/Cargo.toml
mix test
```

For layout-cache work, also run a focused benchmark smoke, for example:

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
