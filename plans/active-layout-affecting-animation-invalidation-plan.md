# Active Plan: Precise Layout-Affecting Animation Invalidation

Last updated: 2026-04-26.

This is the active implementation plan for the next layout-caching slice. It
follows the completed origin-agnostic invalidation/work-scheduling refactor.

## Motivation

The scheduler now chooses work from combined invalidation instead of update
source. Paint-only changes from animation, scroll, patching, and runtime state
can refresh without asking layout any questions.

The remaining animation limitation is inside the layout path: when an active
animation has measure- or resolve-affecting sampled attrs, layout still uses a
conservative cache mode for the whole tree. That is safe, but it prevents
unrelated subtrees from reusing layout caches during layout-affecting
animations.

Target behavior:

```text
paint-only animation       -> refresh only, no layout cache counters
resolve-affecting animation -> dirty affected resolve paths, layout with caches enabled elsewhere
measure-affecting animation -> dirty affected measure paths, layout with caches enabled elsewhere
```

Cache outcomes should remain simple: hit / miss / store.

## Current code shape

Relevant files:

- `native/emerge_skia/src/tree/animation.rs`
  - `AnimationOverlayResult`
  - `classify_animation_sample_attrs(...)`
  - `apply_animation_overlays(...)`
- `native/emerge_skia/src/tree/layout.rs`
  - `prepare_frame_attrs_for_update(...)`
  - `run_layout_passes(...)`
  - `use_measure_cache_for_animation(...)`
  - `use_resolve_cache_for_animation(...)`
- `native/emerge_skia/src/runtime/tree_actor.rs`
  - `FrameUpdatePlan`

Current conservative behavior:

- active measure-affecting animation disables subtree measurement cache reuse for
  the full layout pass
- active resolve-affecting animation disables resolve cache reuse and marks all
  resolve state dirty
- paint-only animations are already handled before layout and should remain on
  the refresh-only path

## Target design

### 1. Record per-node animation layout effects

Extend animation overlay sampling so it can report which nodes produced which
layout-relevant invalidation class.

Possible shape:

```rust
pub struct AnimationOverlayResult {
    pub active: bool,
    pub invalidation: TreeInvalidation,
    pub effects: Vec<AnimationLayoutEffect>,
}

pub struct AnimationLayoutEffect {
    pub id: NodeId,
    pub invalidation: TreeInvalidation,
}
```

Naming and storage are flexible. Keep it simple and bounded to active sampled
nodes. Paint-only effects may not need to be stored for layout, but storing them
is acceptable if it keeps the API straightforward.

### 2. Convert animation effects into ordinary dirty state

Before `run_layout_passes(...)`, mark dirty paths for layout-affecting sampled
animation effects:

```text
Measure -> tree.mark_measure_dirty_for_invalidation(id, Measure)
Resolve -> tree.mark_measure_dirty_for_invalidation(id, Resolve)
Paint   -> no layout dirtying
```

The exact helper can be existing or new. The important point is to reuse the
same dirty propagation semantics patches/runtime state use, instead of passing a
whole-tree animation cache mode.

### 3. Remove broad animation cache disabling

After targeted dirtying exists, layout should generally call:

```rust
measure_element(..., true);
resolve_element(..., true);
```

and let per-node dirty bits plus cache keys decide hit/miss/store.

Do not keep a hidden broad equivalent of:

```rust
!animation_result.active || !animation_result.invalidation.requires_measure()
!animation_result.active || !animation_result.invalidation.requires_resolve()
mark_all_resolve_dirty()
```

except as a temporary fallback for explicitly unsupported cases documented in the
code and tests.

### 4. Keep conservative correctness for hard cases

If a kind or animation case is not safe yet, prefer a localized conservative
fallback over global cache disabling. Examples to evaluate carefully:

- enter animations and exit ghosts
- completed exit ghost pruning that changes topology
- nearby mounts whose placement depends on host geometry
- paragraph/text-flow content that is not resolve-cache eligible
- scale changes and uploaded/replaced trees

Structure changes from ghost pruning should still force the structure/layout path.

## Acceptance criteria

Required behavior:

- paint-only animation still refreshes with no layout:
  - `layout_performed=false`
  - layout-cache counters remain zero
- width/height/font-size animation still reaches layout
- a measure-affecting animation on one child does not globally disable cache
  reuse for unrelated clean siblings
- align animation dirties/uses resolve paths without unnecessarily dirtying
  measurement where safe
- layout-cache stats remain hit/miss/store only

Suggested tests:

- width animation on first child in a row:
  - sibling subtree measurement cache hits
  - affected child/parent miss as needed
- align animation:
  - no intrinsic text remeasure when only resolve changes
  - resolve cache misses on affected path
- paint-only animation regression:
  - still no layout and zero layout-cache stats
- exit ghost completion:
  - topology change still recomputes safely

## Non-goals

- Do not implement refresh subtree skipping in this slice.
- Do not redesign text-flow/paragraph resolve caching here.
- Do not add benchmark-facing bypass counters.
- Do not add public/NIF knobs for animation cache modes.

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

Focused benchmark smoke:

```bash
cargo bench --manifest-path native/emerge_skia/Cargo.toml --bench layout -- \
  layout_animation_paint_only/shadow_showcase \
  --sample-size 10 --warm-up-time 0.2 --measurement-time 0.5
```

Add a targeted benchmark only if tests show the cache behavior but performance
needs measurement.

## Completion protocol

When this slice is implemented and validated:

1. Fold durable notes into `layout-caching-roadmap.md`.
2. Fold implementation lessons into `native-tree-implementation-insights.md`.
3. Update `layout-caching-engine-insights.md` if the final design changes.
4. Update `plans/README.md` next-step ordering.
5. Ask before deleting temporary active plan files.
