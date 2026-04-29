# Active Renderer Instrumentation Refactor Plan

Last updated: 2026-04-29.

Status: implemented, pending normal plan cleanup.

## Purpose

Reduce release-code bloat in the renderer without removing functionality,
diagnostics, tests, or benchmarks.

The highest-value target is the duplicated normal/profiled draw path in
`native/emerge_skia/src/renderer.rs`. The current renderer keeps separate
functions for many operations that are behaviorally the same except that the
profiled version records timings and detail structs. This creates two places to
keep rendering behavior correct.

Target outcome:

```text
render()
  -> shared draw traversal with instrumentation disabled

render_profiled()
  -> same shared draw traversal with instrumentation enabled
```

The refactor should reduce code size and correctness risk while preserving the
same observable render output and slow-frame diagnostics.

## Current Code Facts

The branch currently has about this release/test split versus `main`:

```text
release engine/config, excluding Rust cfg(test): 15,580 added / 3,111 deleted
tests, benchmarks, fixtures, including cfg(test): 20,064 added / 3,150 deleted
```

The largest release-source growth is in:

```text
native/emerge_skia/src/renderer.rs        +4,790 / -258
native/emerge_skia/src/tree/element.rs    +2,904 / -567
native/emerge_skia/src/tree/patch.rs      +1,699 / -619
native/emerge_skia/src/stats.rs           +1,607 / -61
native/emerge_skia/src/tree/layout.rs     +1,589 / -381
native/emerge_skia/src/tree/render.rs     +1,362 / -158
native/emerge_skia/src/events/registry_builder.rs +1,248 / -639
```

`renderer.rs` is the best first refactor target because it contains clear
semantic duplication:

- `render_nodes` and `render_nodes_profiled`
- `render_clip_node` and `render_clip_node_profiled`
- `render_relaxed_clip_node` and `render_relaxed_clip_node_profiled`
- `render_transform_node` and `render_transform_node_profiled`
- `render_alpha_node` and `render_alpha_node_profiled`
- `draw_cached_asset_with_fit` and `draw_cached_asset_with_fit_profiled`
- `draw_image_with_fit` and `draw_image_with_fit_profiled`
- `draw_vector_asset_with_fit` and `draw_vector_asset_with_fit_profiled`
- `draw_outer_shadow` and `draw_outer_shadow_profiled`

The module is also large enough that a behavior-preserving split after the
deduplication will improve maintainability, but module extraction alone is not
the main win.

## Non-Goals

- no renderer cache behavior changes
- no new drawing optimization
- no output/visual change
- no removal of slow-frame detail logs
- no removal of benchmark coverage
- no extra work on layout, registry, or event caching in this slice
- no optimization stays if normal-render benchmarks regress

## Benchmark Gate

This is a refactor, so the primary proof is "same behavior, no normal-render
regression". The branch rule still applies: if the shared instrumentation
abstraction adds overhead to normal rendering, do not keep the refactor in that
form.

Before changing renderer code, capture a short local baseline:

```bash
cargo bench \
  --manifest-path native/emerge_skia/Cargo.toml \
  --bench renderer \
  -- renderer \
  --warm-up-time 0.1 \
  --measurement-time 0.2 \
  --sample-size 10 \
  --save-baseline renderer_instrumentation_refactor_before
```

After each implementation slice that changes hot render paths, compare against
the baseline:

```bash
cargo bench \
  --manifest-path native/emerge_skia/Cargo.toml \
  --bench renderer \
  -- renderer \
  --warm-up-time 0.1 \
  --measurement-time 0.2 \
  --sample-size 10 \
  --baseline renderer_instrumentation_refactor_before
```

Required benchmark attention:

- normal render with stats/profiling off
- profiled slow-frame render path
- image/vector draw cases
- shadow draw cases
- mixed UI scene
- renderer-cache hit and miss cases

Acceptance rule:

- normal render must stay neutral within noise or improve
- profiled render may stay neutral; a small overhead is acceptable only if
  slow-frame diagnostics remain correct and the normal path does not regress
- if a shared helper forces normal rendering to allocate profiles or call
  `Instant::now()`, that design is rejected

## Slice 1: Baseline And Safety Checks

Status: complete.

Work:

- run the renderer Criterion baseline above
- run the focused renderer tests before refactoring
- identify existing tests that compare profiled and normal behavior indirectly
- add one small regression test if a known profiled-only path lacks coverage

Validation:

```bash
cargo test --manifest-path native/emerge_skia/Cargo.toml renderer
cargo bench --manifest-path native/emerge_skia/Cargo.toml --bench renderer -- renderer --save-baseline renderer_instrumentation_refactor_before
```

Result:

- `cargo test --manifest-path native/emerge_skia/Cargo.toml renderer --no-run`
  passed before the refactor.
- the renderer Criterion baseline was captured as
  `renderer_instrumentation_refactor_before`.

## Slice 2: Introduce Draw Instrumentation Context

Status: complete.

Goal:

- create the shared abstraction without rewriting every draw helper at once

Expected shape:

```rust
trait DrawInstrumentation {
    const ENABLED: bool;
    // record_* hooks are no-ops for the normal render path.
}

struct NoDrawInstrumentation;
struct TimingDrawInstrumentation<'a>;
```

Rules:

- the normal path uses static dispatch through `NoDrawInstrumentation`
- the disabled path does not allocate timing/detail structs
- timing code is concentrated in small `record_*` hooks and `measure_draw`
- public render behavior is unchanged

Validation:

- cargo test
- clippy
- short renderer benchmark comparison if any hot call sites changed

Result:

- implemented as static-dispatch instrumentation rather than a runtime enum, so
  the compiler can specialize normal and profiled render paths without forcing
  normal rendering through timing branches.

## Slice 3: Unify Scene Traversal

Status: complete.

Goal:

- remove duplicated traversal between `render_nodes` and
  `render_nodes_profiled`

Work:

- change traversal helpers to accept `RenderDrawContext`
- merge normal/profiled handling for:
  - shadow pass
  - clip
  - relaxed clip
  - transform
  - alpha
  - cache candidate fallback traversal
  - primitive dispatch
- keep special detailed profiling for images and shadows through context hooks

Validation:

- renderer tests pass
- profiled slow-frame logs still contain draw detail, image detail, and shadow
  detail
- normal renderer benchmarks stay neutral or improve

Result:

- removed the duplicated profiled traversal helpers:
  - `render_nodes_profiled`
  - `render_clip_node_profiled`
  - `render_relaxed_clip_node_profiled`
  - `render_transform_node_profiled`
  - `render_alpha_node_profiled`
- normal traversal, profiled traversal, and cache-candidate fallback traversal
  now use the same generic traversal with either no-op or timing
  instrumentation.
- profiled cache-candidate traversal now records draw detail through the same
  instrumentation hooks as the non-cache traversal.
- `native/emerge_skia/src/renderer.rs` dropped from 8,176 lines to 7,993 lines
  after formatting.

Benchmark result:

- the full short renderer comparison was mostly neutral or improved, but it
  flagged a few raster/cache micro-regressions in noisy short samples.
- a focused rerun with longer warmup/measurement cleared the suspect cases:
  - `assets_like_loaded_tiles/direct_children`: improved about 3.3%
  - `toast_panel_move_y/raster_miss_store`: improved about 2.4%
  - `floating_card_move_xy/picture_warm_hit`: improved about 2.9%
  - `floating_card_move_xy/raster_miss_store`: improved about 1.3%
  - `cache_candidates_layout_reflow/direct_reflowed_children`: neutral
    at about +0.1%

Decision:

- keep this traversal refactor. The benchmark gate did not confirm a normal
  render regression.

## Slice 4: Unify Image And Shadow Draw Helpers

Status: deferred to a separate benchmarked slice.

Goal:

- remove the duplicated `*_profiled` image/vector/shadow functions without
  adding normal-path overhead

Work:

- fold `draw_cached_asset_with_fit_profiled` into a shared implementation that
  optionally records `RenderImageDrawProfile`
- fold `draw_image_with_fit_profiled` and `draw_vector_asset_with_fit_profiled`
  into the same shared path
- fold `draw_outer_shadow_profiled` into a shared shadow helper with optional
  `RenderShadowDrawProfile`
- keep existing image tint, vector cache, placeholder, and shadow behavior
  unchanged

Validation:

- image/vector renderer benchmarks
- shadow renderer benchmarks
- slow-frame logs checked on a profiled image and shadow scene

Decision point:

- if optional profiling makes the normal path more complex or slower than the
  duplicated functions, stop and keep the simpler duplicated path for that
  helper; document the rejected refactor in this plan.

Decision:

- do not fold the image/vector/shadow helper pairs in this pass. The traversal
  deduplication already removes the largest safe duplicate block and keeps the
  hot normal path benchmark-neutral. The image/vector/shadow helpers contain
  asset lookup, vector-cache, tint-layer, placeholder, and mask-filter details,
  so combining them should be a smaller follow-up with focused image and shadow
  baselines. Until that benchmark proves value, the simpler duplicated helper
  code is preferable.

## Slice 5: Module Split After Deduplication

Status: deferred.

Goal:

- make the reduced renderer code easier to maintain without mixing unrelated
  behavior changes into the refactor

Candidate modules:

```text
native/emerge_skia/src/renderer/cache.rs
native/emerge_skia/src/renderer/draw.rs
native/emerge_skia/src/renderer/images.rs
native/emerge_skia/src/renderer/shadows.rs
native/emerge_skia/src/renderer/borders.rs
native/emerge_skia/src/renderer/instrumentation.rs
```

This slice is optional if the deduplication is already large and reviewable.
Do not split modules before the shared draw path is proven.

Validation:

- no behavior changes
- `git diff --stat` should show movement/reorganization rather than hidden
  rewrites
- cargo test, clippy, mix test

Decision:

- defer module extraction. This change is already a behavior-preserving
  traversal refactor with measurable LOC reduction. Moving modules in the same
  patch would add review noise without additional performance proof.

## Definition Of Done

- release-source LOC decreases meaningfully, preferably by several hundred
  lines or more
- normal render benchmarks do not regress
- profiled slow-frame diagnostics keep equivalent detail
- renderer cache behavior is unchanged
- tests and benchmarks stay in place
- active plan records any rejected helper-level refactor where the simpler
  duplicated implementation is measurably better

Final validation:

```bash
cargo test --manifest-path native/emerge_skia/Cargo.toml
cargo clippy --manifest-path native/emerge_skia/Cargo.toml --benches -- -D warnings
mix test
git diff --check
```

Result:

- `cargo test --manifest-path native/emerge_skia/Cargo.toml`: passed
- `cargo clippy --manifest-path native/emerge_skia/Cargo.toml --benches -- -D warnings`: passed
- `mix test`: passed
- `git diff --check`: passed
