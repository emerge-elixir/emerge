# Active Plan: Nearby Relayout Boundary

Last updated: 2026-04-26.

Status: partially implemented. The benchmark guard, nearby topology
classification, and measurement boundary are implemented. Resolve traversal is
safe but still conservative: ancestor resolve caches are not allowed to hide
dirty nearby descendants, but broader work remains to avoid visiting clean
siblings/ancestors in large trees.

## Motivation

The Borders page hover/unhover case that shows a nearby code block still runs
layout work that is too broad:

```text
layout: avg=1.802 ms min=1.393 ms max=2.238 ms count=16
patch tree actor: avg=2.023 ms min=1.506 ms max=2.549 ms count=16

layout cache
  intrinsic measure: hits=0 misses=168 stores=168
  subtree measure:   hits=176 misses=496 stores=496
  resolve:           hits=176 misses=496 stores=496
```

That is roughly per hover/unhover patch:

```text
intrinsic misses ~= 10.5
subtree misses   ~= 31
resolve misses   ~= 31
```

This does not mean cache lookup is broken. It means invalidation is still too
conservative for nearby topology changes. `SetNearbyMounts` and nearby subtree
insert/remove currently report broad structural invalidation, and structure
invalidation dirties host/ancestor measurement and resolve state even when the
host's measured size is independent of the nearby overlay.

## Desired behavior

A hover/unhover that mounts a nearby code block should behave like this:

```text
nearby overlay shown/hidden
  -> no repeated full layout of unrelated page rows
  -> measure only newly inserted/changed nearby subtree when needed
  -> keep host and ancestor measurement caches hot when their measured sizes do
     not depend on the nearby mount
  -> traverse enough dirty descendants so cached ancestors do not hide the
     changed nearby subtree
  -> update render and registry output for ordering/hit-testing/precedence
```

The eventual demo target is that toggling a single nearby code block on the
Borders page should not routinely produce ~2 ms patch/layout samples or hundreds
of subtree/resolve misses.

## Non-goals

- Do not rework refresh subtree caching in this slice.
- Do not add layout-cache bypass counters.
- Do not change layout-cache stats semantics: keep hit / miss / store only.
- Do not make scheduling depend on hover, animation, scroll, or patch source.
- Do not broaden all structure invalidation at once; only nearby topology is in
  scope.
- Do not optimize viewport/repeater identity here.

## Design hypothesis

Nearby mounts are not normal measured children.

Current measurement already computes normal child sizes separately from nearby
mount sizes. The host's measured size does not use nearby mount sizes. Nearby
subtrees still need their own measurement and resolution, but a host should not
miss its own subtree-measure cache merely because an escape-nearby overlay was
added, removed, or moved.

Resolve is trickier: nearby placement depends on the host's resolved frame and
nearby slot, so a nearby change must keep the host path traversable. However,
that should be traversal dirtiness, not necessarily broad ancestor resolve
misses.

Likely implementation direction:

- distinguish nearby topology invalidation from normal child structure
- add or refine traversal dirtiness so dirty nearby descendants are reachable
  below clean ancestors
- keep host measurement clean when only nearby mounts changed
- keep render/registry damage conservative because paint order and event
  precedence do depend on nearby topology

## Slice 1: benchmark and deterministic guard first — done

Do not change invalidation until the regression surface is captured.

Tasks:

- add a focused benchmark or retained-layout scenario for nearby overlay toggles
  that resembles the Borders hover/unhover code-block case
- include at least:
  - show nearby code block
  - hide nearby code block
  - no-op nearby write if applicable
  - existing `nearby_rich_50/nearby_slot_change` as a small synthetic baseline
- make benchmark output grep-friendly and include layout-cache counters
- add deterministic tests that capture current and target invariants:
  - host measured frame is unchanged by adding/removing escape-nearby overlays
  - unrelated sibling measured/resolved frames are unchanged
  - nearby subtree receives measurement/resolve when newly inserted or changed
  - event/render dirtiness still propagates for nearby topology changes

Acceptance:

- benchmark compiles/runs opt-in only; normal `cargo test` / `mix test` do not
  run timing benchmarks
- baseline cache-counter shape is recorded in this plan before implementation
- deterministic tests can prove correctness without timing assertions

Baseline guard added:

- scenarios:
  - `nearby_code_show_50/nearby_slot_change` inserts a nearby code block
  - `nearby_code_hide_50/nearby_slot_change` removes a nearby code block
- command:

  ```bash
  EMERGE_BENCH_SCENARIOS=nearby_code_show,nearby_code_hide \
  EMERGE_BENCH_SIZES=50 \
  EMERGE_BENCH_MUTATIONS=nearby_slot_change \
  EMERGE_BENCH_WARMUP=0.1 \
  EMERGE_BENCH_TIME=0.1 \
  EMERGE_BENCH_MEMORY_TIME=0 \
  mix bench.native.retained_layout
  ```

Baseline before invalidation changes:

```text
nearby_code_hide_50/nearby_slot_change after_patch:
  intrinsic misses=0 stores=0
  subtree hits=11 misses=3 stores=3
  resolve hits=11 misses=3 stores=3
  layout_only median ~= 21.8 µs
  patch_then_layout median ~= 20.2 µs

nearby_code_show_50/nearby_slot_change after_patch:
  intrinsic misses=3 stores=3
  subtree hits=11 misses=7 stores=7
  resolve hits=11 misses=7 stores=7
  layout_only median ~= 30.9 µs
  patch_then_layout median ~= 48.0 µs
```

Post-implementation guard shape with the same command:

```text
nearby_code_hide_50/nearby_slot_change after_patch:
  intrinsic misses=0 stores=0
  subtree hits=14 misses=0 stores=0
  resolve hits=11 misses=3 stores=3
  layout_only median ~= 21.7 µs
  patch_then_layout median ~= 28.7 µs  # short-run timing remained noisy

nearby_code_show_50/nearby_slot_change after_patch:
  intrinsic misses=3 stores=3
  subtree hits=14 misses=4 stores=4
  resolve hits=11 misses=7 stores=7
  layout_only median ~= 26.4 µs
  patch_then_layout median ~= 54.4 µs  # short-run timing remained noisy
```

Focused retained-layout smoke after implementation:

```text
layout_matrix_50/nearby_slot_change after_patch:
  intrinsic misses=0 stores=0
  subtree hits=5 misses=0 stores=0
  resolve hits=5 misses=2 stores=2

nearby_rich_50/nearby_slot_change after_patch:
  intrinsic misses=0 stores=0
  subtree hits=5 misses=0 stores=0
  resolve hits=5 misses=2 stores=2

text_rich_50/nearby_slot_change after_patch:
  intrinsic misses=0 stores=0
  subtree hits=5 misses=0 stores=0
  resolve hits=5 misses=2 stores=2
```

The primary improvement in this slice is the counter shape: nearby hide no
longer causes host/ancestor subtree-measure misses, and nearby show only stores
measurement for the newly inserted nearby subtree.

## Slice 2: classify nearby topology invalidation — done

Tasks:

- introduce a narrower invalidation path for nearby topology changes, or an
  equivalent internal patch classification, instead of treating all nearby mount
  updates as broad `TreeInvalidation::Structure`
- distinguish normal child topology from nearby topology in patch application:
  - `SetChildren` remains structure-like
  - `SetNearbyMounts` should mark nearby traversal/resolve/render/registry work
    without forcing host measurement when safe
  - `InsertNearbySubtree` should dirty the inserted subtree for measurement and
    resolution, plus mark host/ancestor traversal as needed
  - `Remove` of a nearby subtree should update host nearby topology and refresh
    output without broad unrelated measurement misses
- preserve ghost/exit-animation nearby behavior
- keep source-agnostic scheduling: this is about dependency class, not hover

Implemented shape:

- `SetNearbyMounts` and `InsertNearbySubtree` now classify as
  `TreeInvalidation::Resolve` instead of broad structure.
- Removing a nearby-mounted subtree classifies as resolve; removing normal child
  subtrees remains structure.
- `set_nearby_ixs(...)` no longer calls broad `mark_measure_dirty_ix(host)`.
  It marks nearby topology traversal dirtiness and render/registry refresh
  damage instead.
- Newly attached nearby roots are marked measure/resolve dirty locally, without
  forcing host measurement dirtiness.

Acceptance:

- nearby mount changes no longer force host/ancestor measurement dirtiness when
  host measured size is independent of the nearby overlay
- render and registry refresh damage remains conservative enough for correct
  visual ordering and event precedence
- existing patch/ghost tests continue to pass

## Slice 3: measure dependency key and traversal cleanup — done

Tasks:

- split measurement topology dependencies from render/registry/resolve topology
  dependencies if needed
- remove nearby topology from host subtree-measure cache keys when nearby mounts
  cannot affect host measured size
- ensure dirty nearby descendants remain reachable before a clean host subtree
  measurement cache is reused
- keep normal child topology in measurement keys

Potential implementation shape:

```rust
struct MeasureTopologyDependencyKey {
    children_version: u64,
    child_count: usize,
}
```

or reuse `TopologyDependencyKey` with a measurement-specific constructor that
sets nearby fields only for cases that truly depend on nearby topology.

Implemented shape:

- subtree measurement cache keys now use a measurement-specific topology key that
  includes normal child topology but ignores nearby topology
- nearby topology changes mark host/ancestor measurement traversal dirtiness, not
  host/ancestor measurement dirtiness
- deterministic tests cover slot changes and inserted nearby subtrees preserving
  host measured frames while reusing host/root measurement caches

Acceptance:

- adding/removing an escape-nearby overlay can traverse to the nearby subtree and
  still reuse the host's own subtree-measure cache
- unrelated siblings are not measured again
- layout-cache counters show localized misses for the nearby path

## Slice 4: resolve traversal boundary for nearby changes — partially done

Tasks:

- add resolve descendant traversal state if needed, analogous to
  `measure_descendant_dirty`
- prevent clean ancestor resolve-cache hits from hiding dirty nearby descendants
- avoid broad ancestor resolve misses when only nearby overlay topology changed
- preserve nearby placement correctness for all slots:
  - `BehindContent`
  - `Above`
  - `OnRight`
  - `Below`
  - `OnLeft`
  - `InFront`
- preserve scroll extents, transforms, clips, and paragraph fragments

Implemented so far:

- `NodeLayoutState` now has `resolve_descendant_dirty`
- clean ancestor resolve-cache hits are blocked when a dirty nearby descendant
  must be reached
- resolve dirty state is cleared after successful resolve-cache hits/stores

Remaining work:

- avoid visiting clean siblings/ancestors in large trees when only nearby overlay
  topology changed
- add broader cached-vs-uncached resolved-frame tests for all nearby slots if the
  resolve traversal is optimized further

Acceptance:

- cached and uncached resolved frames match for representative nearby trees
- nearby overlay show/hide updates only affected placement/output
- focus/event registry and render scene remain correct after resolve reuse

## Slice 5: focused demo smoke and docs — partially done

Tasks:

- run the focused nearby benchmark and existing retained-layout smoke
- compare Borders hover/unhover renderer stats before/after if practical
- update stable roadmap/insights with the boundary rules that are proven safe
- delete this active plan only after confirmation

Validation run for the measurement-boundary implementation:

```bash
cargo fmt --manifest-path native/emerge_skia/Cargo.toml --check
mix format --check-formatted
git diff --check
cargo test --manifest-path native/emerge_skia/Cargo.toml --benches --no-run
cargo test --manifest-path native/emerge_skia/Cargo.toml --quiet
mix test
```

Focused retained-layout smoke:

```bash
EMERGE_BENCH_SCENARIOS=layout_matrix,text_rich,nearby_rich \
EMERGE_BENCH_SIZES=50 \
EMERGE_BENCH_MUTATIONS=nearby_slot_change,event_attr,paint_attr,layout_attr \
EMERGE_BENCH_WARMUP=0.1 \
EMERGE_BENCH_TIME=0.1 \
EMERGE_BENCH_MEMORY_TIME=0 \
mix bench.native.retained_layout
```

## Completion protocol

When this slice is implemented and validated:

1. Fold durable notes into `layout-caching-roadmap.md`.
2. Fold implementation lessons into `native-tree-implementation-insights.md`.
3. Update `plans/README.md` next-step ordering.
4. Ask before deleting this temporary active plan file.
