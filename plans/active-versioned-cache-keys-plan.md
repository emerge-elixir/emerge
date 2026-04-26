# Active Plan: Versioned Cache Keys and Ix-Native Layout Traversal

Last updated: 2026-04-26.

This is the active implementation plan for the next layout-caching slice after
text-flow resolve caching and the first relayout/dependency boundary.

Status: implemented and validated for compact topology dependency keys. Keep
this temporary active plan until the user confirms deletion.

## Motivation

The current cache keys are conservative and correct, but they still clone child
and nearby identity lists in hot layout paths:

```rust
SubtreeMeasureCacheKey {
    children: Vec<NodeId>,
    nearby: Vec<NearbyMount>,
    ...
}

ResolveCacheKey {
    children: Vec<NodeId>,
    nearby: Vec<NearbyMount>,
    ...
}
```

Layout traversal also still uses id-facing compatibility helpers in hot paths,
including `child_ids(...)`, `nearby_mounts_for(...)`, and `get(&NodeId)`, even
though production topology is `NodeIx`-authoritative.

Goal: make parent cache validation cheaper without weakening correctness.

Target behavior:

```text
same topology/dependencies -> same compact version key -> cache hit
changed child/nearby order or dependency -> version/key change -> miss/store
```

Layout-cache stats must remain hit / miss / store only.

## Current code shape

Relevant files:

- `native/emerge_skia/src/tree/element.rs`
  - `ElementTree`
  - `NodeTopology`
  - `NodeLayoutState`
  - `SubtreeMeasureCacheKey`
  - `ResolveCacheKey`
  - `NodeIx`, `NodeId`, `ParentLink`, `NearbyMountIx`
- `native/emerge_skia/src/tree/layout.rs`
  - `measure_element(...)`
  - `subtree_measure_cache_key(...)`
  - `resolve_element(...)`
  - `resolve_cache_key(...)`
  - `can_store_resolve_cache(...)`
- `native/emerge_skia/src/tree/patch.rs`
  - structure changes that mutate children/nearby topology
  - attr changes that should affect cache keys
- tests under `native/emerge_skia/src/tree/layout/tests/cache.rs`

Current conservative key dependencies:

- element kind
- effective attrs relevant to the cache family
- inherited font/text context
- measured frame and constraint for resolve
- child `NodeId` order
- nearby slot/id order

Do not remove a dependency unless a version captures the same information.

## Proposed model

Introduce compact dependency versions owned by the retained native tree.

A possible shape:

```rust
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct NodeVersions {
    spec: u64,
    runtime_layout: u64,
    measure: u64,
    resolve: u64,
    children: u64,
    paint_children: u64,
    nearby: u64,
}
```

Names and exact fields are provisional. The important property is that cache keys
compare small fixed-width values instead of cloning identity lists.

Potential compact dependency keys:

```rust
struct TopologyDependencyKey {
    children_version: u64,
    nearby_version: u64,
    child_count: usize,
    nearby_count: usize,
}
```

Counts are not a substitute for versions, but they make debug assertions and
collision reasoning easier.

## Slice 1: add topology/dependency versions without changing cache keys — done

Goal: introduce version state and bumping rules with no behavior change.

Tasks:

- add per-node topology/dependency version fields to native layout/topology state
- bump child topology version when `set_children_ix(...)` changes child order or
  membership
- bump paint-child topology version when `set_paint_children_ix(...)` changes
  paint order or membership
- bump nearby topology version when `set_nearby_ixs(...)` changes slot/order or
  membership
- initially preserve current `NodeId` list cache keys for this slice
- add tests proving versions bump on real topology changes and do not bump on
  no-op writes where practical

Acceptance:

- no cache behavior change
- existing layout/cache tests pass
- version bump rules are covered by focused tests

## Slice 2: replace subtree-measure child/nearby list keys — done

Goal: make `SubtreeMeasureCacheKey` use compact dependency versions instead of
cloned `Vec<NodeId>` / `Vec<NearbyMount>`.

Tasks:

- add a fixed-width subtree topology dependency key
- replace `children: Vec<NodeId>` and `nearby: Vec<NearbyMount>` in
  `SubtreeMeasureCacheKey`
- keep attrs and inherited font key unchanged
- ensure child/nearby reorder, insertion, removal, and slot change still miss
- add tests comparing old behavior scenarios:
  - keyed reorder preserves child caches but parent subtree key changes
  - nearby slot/order changes invalidate the host subtree measure cache
  - no-op topology writes do not cause unnecessary misses if no version bump is
    possible

Acceptance:

- subtree measurement cache hit/miss/store behavior remains correct
- fewer list clones in subtree cache-key construction
- no regressions in text-flow, nearby, or animation dirty-path tests

## Slice 3: replace resolve child/nearby list keys — done

Goal: make `ResolveCacheKey` use compact dependency versions instead of cloned
identity lists.

Tasks:

- add a resolve topology dependency key, likely sharing the same topology fields
  plus any resolve-specific order dependencies if needed
- replace `children: Vec<NodeId>` and `nearby: Vec<NearbyMount>` in
  `ResolveCacheKey`
- keep measured frame, constraint, attrs, and inherited key unchanged
- verify `can_store_resolve_cache(...)` still checks child/nearby cache ownership
  and parent-owned flow-layout exceptions
- add tests for:
  - child reorder invalidates parent resolve cache
  - paint order updates invalidate render-relevant resolve ordering when needed
  - nearby placement changes invalidate host resolve cache
  - paragraph/text-flow parent-owned layout remains correct

Acceptance:

- resolve cache behavior unchanged or improved
- cache hits stay correct after child/nearby mutations
- fewer list clones in resolve cache-key construction

## Slice 4: ix-native traversal cleanup where it naturally falls out — partial

Goal: reduce repeated `NodeId -> NodeIx` lookups in measure/resolve hot paths
without rewriting everything at once.

This slice added ix-aware topology dependency extraction. Broader measurement and
resolve traversal still uses id-facing compatibility helpers and is deferred.

Tasks:

- add ix-facing helper variants only where they remove repeated conversion:
  - child ids/ixs for measurement traversal
  - nearby ids/ixs for measurement and resolve traversal
  - cache key dependency extraction
- keep public/boundary-facing APIs `NodeId`-based
- avoid mixing broad traversal rewrites with cache-key semantic changes unless
  the helper is directly needed

Acceptance:

- hot path allocates/clones less
- correctness tests unchanged
- code remains understandable; no premature all-ix rewrite

## Non-goals

- Do not add cache-bypass counters.
- Do not replace all `NodeId` APIs.
- Do not change public NIF or Elixir APIs.
- Do not implement refresh subtree skipping in this slice.
- Do not broaden relayout/dependency boundaries in this slice unless a small
  helper is required and separately tested.

## Suggested tests

Add focused tests under `native/emerge_skia/src/tree/layout/tests/cache.rs` and
`native/emerge_skia/src/tree/element.rs` tests:

- child topology version bumps on insert/remove/reorder
- nearby topology version bumps on slot change/reorder
- no-op child/nearby writes avoid version bumps if practical
- subtree cache misses after child order changes but leaf caches survive
- resolve cache misses after child order changes and then stores a new key
- nearby host subtree/resolve cache invalidates after slot or order change
- text-flow resolve cache tests from the previous slice still pass
- first relayout-boundary tests still pass

## Benchmark/smoke direction

Run a retained-layout smoke with topology-sensitive and layout-rich cases:

```bash
EMERGE_BENCH_SCENARIOS=layout_matrix,text_rich,nearby_rich \
EMERGE_BENCH_SIZES=50 \
EMERGE_BENCH_MUTATIONS=layout_attr,keyed_reorder,nearby_slot_change \
EMERGE_BENCH_WARMUP=0.1 \
EMERGE_BENCH_TIME=0.1 \
EMERGE_BENCH_MEMORY_TIME=0 \
mix bench.native.retained_layout
```

Use `layout_cache_stats` to confirm cache behavior stays correct. Use profiles or
allocation-sensitive benchmarks to justify further ix-native cleanup.

Focused smoke after implementation:

```text
layout_matrix_50 warm_cache: resolve_hits=1 resolve_misses=0 subtree_measure_hits=1 subtree_measure_misses=0
nearby_rich_50 warm_cache:  resolve_hits=1 resolve_misses=0 subtree_measure_hits=1 subtree_measure_misses=0
text_rich_50 warm_cache:    resolve_hits=1 resolve_misses=0 subtree_measure_hits=1 subtree_measure_misses=0
layout_matrix_50/keyed_reorder after_patch: subtree_measure_hits=11 subtree_measure_misses=2 resolve_hits=13 resolve_misses=2
nearby_rich_50/keyed_reorder after_patch:  subtree_measure_hits=11 subtree_measure_misses=2 resolve_hits=13 resolve_misses=2
text_rich_50/keyed_reorder after_patch:    subtree_measure_hits=11 subtree_measure_misses=2 resolve_hits=3 resolve_misses=12
nearby_rich_50/nearby_slot_change after_patch: subtree_measure_hits=3 subtree_measure_misses=2 resolve_hits=5 resolve_misses=2
```

The `text_rich_50/keyed_reorder` case remains a future text-flow/keyed-reorder
optimization target; correctness is preserved and cache outcomes remain
hit/miss/store.

## Validation

Run:

```bash
cargo fmt --manifest-path native/emerge_skia/Cargo.toml --check
mix format --check-formatted
git diff --check
cargo test --manifest-path native/emerge_skia/Cargo.toml
mix test
cargo test --manifest-path native/emerge_skia/Cargo.toml --benches --no-run
```

Validation status: full validation has passed:

- `cargo fmt --manifest-path native/emerge_skia/Cargo.toml --check`
- `mix format --check-formatted`
- `git diff --check`
- `cargo test --manifest-path native/emerge_skia/Cargo.toml`
- `mix test`
- `cargo test --manifest-path native/emerge_skia/Cargo.toml --benches --no-run`
- focused retained-layout benchmark smoke above

## Completion protocol

When this slice is implemented and validated:

1. Fold durable notes into `layout-caching-roadmap.md`.
2. Fold implementation lessons into `native-tree-implementation-insights.md`.
3. Update `layout-caching-engine-insights.md` if the design meaningfully changes.
4. Update `plans/README.md` next-step ordering.
5. Ask before deleting temporary active plan files.
