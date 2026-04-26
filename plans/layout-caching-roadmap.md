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
- small bounded detached-layout reuse for removed/reinserted nearby subtrees
- resolve-cache eligibility for text-flow kinds (`Multiline`, `WrappedRow`,
  `TextColumn`, and `Paragraph`)
- targeted dirty propagation for layout-affecting animation samples
- compact child/nearby topology dependency versions in resolve cache keys, and
  compact child-only topology dependency versions in subtree-measure keys
- gated native stats snapshots/logging through one unified stats path
- retained-layout benchmark cache-counter output
- incremental effective-attrs preparation for animation-only refresh frames
- no full registry-payload clone on unchanged cached-registry refresh
- render-scene culling for clipped/offscreen subtrees with conservative shadow
  and transform bounds

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

- relayout/dependency boundaries currently cover fixed-size `El`/`None`
  parents and nearby overlay topology; row/column, scrollable, and text-flow
  boundaries remain conservative
- cache keys no longer clone child/nearby identity lists, and render subtree
  keys no longer allocate joined debug strings, but attrs and some traversal
  helpers still allocate/clone in hot paths
- measure/resolve traversal still uses some id-facing compatibility helpers even though topology is ix-based
- registry chunk-cache infrastructure exists but stays conservative; damaged
  no-cache and escape-nearby rebuilds still fall back to the full registry path
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
boundary is implemented, nearby topology no longer invalidates host/ancestor
subtree measurement or resolve geometry, topology dependencies now use compact
version keys, render refresh can skip clean retained subtrees, and registry
refresh has conservative chunk-cache infrastructure. Broader layout boundaries,
additional typed version keys, and cheaper registry chunk seeding remain future
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

## Completed slice: compact topology dependency cache keys

Subtree-measure and resolve cache keys now use compact child/nearby topology
versions instead of cloning child `NodeId` and nearby mount lists.

Implemented shape:

- `NodeLayoutState` owns `LayoutTopologyVersions`
- `set_children_ix(...)`, `set_paint_children_ix(...)`, and `set_nearby_ixs(...)`
  bump per-node topology versions only when order/membership/slot data changes
- `TopologyDependencyKey` captures child and nearby versions plus counts
- `SubtreeMeasureCacheKey` stores `TopologyDependencyKey` instead of
  `children: Vec<NodeId>` and `nearby: Vec<NearbyMount>`
- `ResolveCacheKey` stores the same compact dependency key
- cache hit/miss/store behavior remains centralized in existing measure/resolve
  cache lookup paths
- ix-aware dependency-key extraction was added, while broader measure/resolve
  traversal cleanup remains future work

Focused tests cover child and nearby version bumps plus no-op writes. Existing
cache tests cover keyed reorder, nearby changes, text-flow resolve caching, and
relayout-boundary behavior with the new keys.

Focused smoke after this slice:

```text
layout_matrix_50 warm_cache: resolve_hits=1 resolve_misses=0 subtree_measure_hits=1 subtree_measure_misses=0
nearby_rich_50 warm_cache:  resolve_hits=1 resolve_misses=0 subtree_measure_hits=1 subtree_measure_misses=0
text_rich_50 warm_cache:    resolve_hits=1 resolve_misses=0 subtree_measure_hits=1 subtree_measure_misses=0
layout_matrix_50/keyed_reorder after_patch: subtree_measure_hits=11 subtree_measure_misses=2 resolve_hits=13 resolve_misses=2
nearby_rich_50/keyed_reorder after_patch:  subtree_measure_hits=11 subtree_measure_misses=2 resolve_hits=13 resolve_misses=2
text_rich_50/keyed_reorder after_patch:    subtree_measure_hits=11 subtree_measure_misses=2 resolve_hits=3 resolve_misses=12
nearby_rich_50/nearby_slot_change after_patch: subtree_measure_hits=3 subtree_measure_misses=2 resolve_hits=5 resolve_misses=2
```

`text_rich_50/keyed_reorder` remains a future optimization target; correctness is
preserved and cache outcomes remain hit/miss/store.

## Completed slice: refresh subtree skipping

Goal: after layout state is reused, avoid rebuilding render/event output for
subtrees that did not change.

Implemented shape:

- refresh-specific dirty/descendant-dirty state for render vs registry damage
- patch/runtime/scroll/animation sources mark refresh damage separately from
  layout-cache dirtiness
- refresh-only frames can reuse the cached full event registry when registry
  damage is clean
- duplicate registry updates are avoided when the cached registry payload is
  reused
- render scene refresh can reuse clean retained render subtrees
- registry rebuilds have a conservative retained-chunk path with safe full
  fallback when no retained chunks exist or escape-nearby precedence is involved
- render-cache and registry-cache regression guards compare cached/chunked paths
  against safe full/uncached baselines
- render snapshots omit retained layout cache entries, dirty/full rebuilds avoid
  cache seeding, damaged refreshes with no existing caches use the uncached
  renderer, dirty paths avoid lookup key construction before rebuilding, stored
  render subtrees are limited by a small per-refresh store budget and
  render-node-count cap, and scroll-offset subtrees bypass render cache lookup to
  avoid cloning immediately-stale render scenes

Refresh skipping remains separate from layout-cache stats. Paint-only refreshes
should still consult no measurement/resolve caches and therefore move no
layout-cache counters.

## Completed slice: nearby relayout boundary

Goal: make nearby overlay mount/unmount work proportional to the nearby subtree
instead of dirtying broad host/ancestor measurement and resolve paths.

Status: benchmark guard, invalidation classification, subtree-measure boundary,
resolve traversal through dirty nearby descendants, detached reuse for
reinserted nearby subtrees, refresh-only classification for warmed non-registry
nearby toggles, and render-refresh culling are implemented and demo-validated.

Observed motivation from the Borders page hover/unhover code-block case:

```text
layout: avg=1.802 ms min=1.393 ms max=2.238 ms count=16
patch tree actor: avg=2.023 ms min=1.506 ms max=2.549 ms count=16
intrinsic measure misses=168 stores=168
subtree measure hits=176 misses=496 stores=496
resolve hits=176 misses=496 stores=496
```

The likely issue was conservative nearby structural invalidation, not broken
cache lookup. `SetNearbyMounts`/nearby insert/remove now classify as resolve-like
nearby topology changes instead of broad structure, and subtree measurement keys
ignore nearby topology because host measured size is independent of nearby mount
sizes.

Implemented direction:

- benchmark/test guard for nearby overlay show/hide before invalidation changes
- nearby topology invalidation is distinguished from normal child structure
- host measurement stays clean when only nearby topology changes and host size
  does not depend on nearby mounts
- traversal dirtiness keeps dirty nearby descendants reachable below cached
  measurement ancestors
- resolve-cache hits can restore clean ancestor geometry while traversing the
  dirty nearby path, avoiding visits to unrelated clean siblings
- removed small animation-free nearby subtrees keep a bounded detached layout
  snapshot keyed by structural signature/raw attrs/runtime state/scale, so
  repeated `none()`/code-block hover toggles can restore layout caches even when
  the reinserted subtree has fresh node ids
- non-registry nearby remove/restored-show changes classify as paint/render
  damage, allowing refresh-only work selection and cached full-registry reuse
- registry damage remains conservative when the changed nearby subtree or slot
  can affect event listeners, text input, scrollbars, focus, or front-nearby
  blockers

## Completed follow-up: first refresh-path cleanup for hover/animation

After nearby hover no longer produced layout samples, the remaining refresh cost
was traced to refresh-only work: continuous paint-only shadow animations still
prepared effective attrs for every node, and cached-registry refresh cloned the
full registry payload even when the registry did not change.

Implemented shape:

- animation-only refresh frames update effective attrs only for active animation
  nodes once root geometry exists
- patch/resize/runtime-state batches keep the full preparation path
- cached-registry refresh computes IME state from the cached registry reference
  and returns an ignored empty payload when `event_rebuild_changed=false`
- render scene construction culls subtrees whose conservative visual bounds are
  fully outside the inherited clip; the bounds include outer shadow overflow and
  transforms, and hosts with nearby mounts are kept conservative
- a generic render-cache seeding attempt for damaged/no-cache refreshes was
  benchmarked and rejected because it regressed focused animation/hover guards

Focused local signal:

```text
native/layout_animation_paint_only/shadow_showcase/paint_only_refresh_each_frame
  ~539 µs -> ~499 µs; ~503-512 µs when the whole showcase is visible after render culling
native/layout_scroll_paint_only_animation/shadow_showcase/paint_only_refresh_scroll_frame
  ~801 µs -> ~355 µs after render culling
native/nearby_hover_toggle_refresh/borders_like/restored_show_refresh_only
  ~169 µs
```

Borders demo validation after render culling:

```text
animation only:
  refresh avg=0.262 ms count=1200
  render avg=0.778 ms count=1200
  layout no samples; patch tree actor no samples; layout cache all zero

constant hover/unhover:
  refresh avg=0.318 ms count=1260
  render avg=0.902 ms count=1200
  patch tree actor avg=0.776 ms count=30
  layout no samples; layout cache all zero
```

## Later slice: broaden other relayout/dependency boundaries

The first boundary is intentionally narrow. Future boundaries should be added
one at a time with correctness tests for measured frames, resolved frames,
scroll extents, nearby placement, and paragraph fragments.

Candidates:

- fixed-size `Row` / `Column` cases where parent measured size is independent of
  the changed child
- scrollable fixed-size containers where content extents change but parent
  measured size does not
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
