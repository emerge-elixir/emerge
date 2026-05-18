# Render Cache Audit Fixes Plan

Last updated: 2026-05-15.

Status: completed.

## Benchmark Rule

Every code change in this plan must have a before and after benchmark row in
this file before moving to the next change. Use the same command, sample size,
machine, branch, and relevant environment for the before/after pair. If a new
benchmark is needed to expose a bug, add the diagnostic benchmark first, run it
before changing behavior, and record `previous: none (new benchmark)`.

The minimum benchmark protocol for renderer-cache changes is:

```bash
cd native/emerge_skia
cargo bench --features bench-diagnostics --bench renderer -- paint_layer_cache --sample-size 10
```

For layout/refresh-sensitive fallout, also run:

```bash
cd native/emerge_skia
cargo bench --features bench-diagnostics --bench layout -- layout_refresh/emerge_demo_showcase --sample-size 10
cargo bench --features bench-diagnostics --bench layout -- render_refresh_cache_regression --sample-size 10
```

## Goal

Fix the review findings without regressing the paint-layer cache model:

- child paint layers must still render when a non-cacheable parent paint layer's
  own payload bounds are clipped.
- `renderer_cache.paint_layer.max_stale_frames` must match the public API and
  stale eviction stats.
- `renderer_cache.paint_layer.min_visible_before_store` must match the public
  API and avoid one-frame cache admission churn.
- pending root-level screenshot/debug artifacts must not be accidentally
  carried into the merge.

## Baseline

- [x] Record the branch baseline before the first code change:
  `cargo bench --features bench-diagnostics --bench renderer -- paint_layer_cache --sample-size 10`.
  Current result: partial baseline recorded on 2026-05-15; command failed
  before code changes in
  `emerge_demo_showcase_layout_page/cache_steady_hits` with warmed coverage
  `hits=4`, `misses=1`, `stores=1`, `current_entries=6`.
- [x] Layout/refresh baseline was not required because this plan did not touch
  render-scene construction or tree refresh state:
  `cargo bench --features bench-diagnostics --bench layout -- layout_refresh/emerge_demo_showcase --sample-size 10`.
- [x] No new diagnostic benchmark was added; focused cache-manager and renderer
  tests cover the new behavior.

## Fix 1: Non-cacheable Child Paint Layers

Finding: cache-tracking traversal returns before rendering `child_refs` when a
non-cacheable paint layer has zero visible own-payload pixels.

Plan:

- [x] Add a focused renderer regression test where a non-cacheable parent layer
  is clipped but a child paint layer remains visible through the valid child
  composition path.
- [x] Run the before benchmark row for `paint_layer_cache`.
- [x] Add one shared renderer helper for non-cacheable paint layers: render
  direct own nodes only when their own payload bounds are visible, then traverse
  child refs through the existing child-ref helper. Use it from both
  `CacheTrackingRenderMode` and `ChildPaintLayerRenderMode`.
- [x] Run the after benchmark row for the same command and record delta.
- [x] Run targeted renderer tests plus `cargo test`.

Benchmark log:

| Change | Command | Before | After | Notes |
| --- | --- | --- | --- | --- |
| non-cacheable child refs | `cargo bench --features bench-diagnostics --bench renderer -- paint_layer_cache --sample-size 10` | partial: scrolling/direct `64.372-65.308 ms`; scrolling/cache `12.656-13.005 us`; animation/direct `1.9674-2.0128 ms`; animation/cache `1.0643-1.0749 ms`; offscreen steady `47.539-51.706 us`; visible noop `8.8500-9.0024 us`; stable descendant `76.199-78.131 us`; scroll return `5.8811-6.0542 us`; then pre-existing layout-page assertion failure | partial: scrolling/direct `64.305-65.102 ms`; scrolling/cache `12.737-14.253 us`; animation/direct `1.9684-2.0916 ms`; animation/cache `1.0705-1.0830 ms`; offscreen steady `51.079-52.278 us`; visible noop `8.8721-8.9600 us`; stable descendant `76.136-77.622 us`; scroll return `5.6922-6.0434 us`; then same layout-page assertion failure | Completed rows were statistically unchanged; layout-page failure predates this fix and remains `hits=4`, `misses=1`, `stores=1`. |

## Fix 2: Stale Payload Eviction

Finding: `max_stale_frames` is configured and documented but
`PaintLayerPayloadCache::begin_frame` never evicts stale entries.

Plan:

- [x] Add cache unit coverage proving an entry unseen for more than
  `max_stale_frames` is evicted and reports evicted bytes.
- [x] Add or update renderer coverage proving a clipped-but-still-present layer
  remains marked seen and survives the stale window.
- [x] Run the before benchmark row for `paint_layer_cache`, paying attention to
  `scroll_return/cache_after_clipped_frames` and showcase steady-hit stats.
- [x] Implement stale eviction in `begin_frame` using `last_seen_frame`, with
  wrapping-safe age comparison.
- [x] Ensure `RendererCacheFrameStats.paint_layer.stale_evictions` and
  `stale_evicted_bytes` are populated from the returned eviction bytes.
- [x] Run the after benchmark row for the same command and record delta.
- [x] Run targeted cache/renderer tests plus `cargo test`.

Benchmark log:

| Change | Command | Before | After | Notes |
| --- | --- | --- | --- | --- |
| stale payload eviction | `cargo bench --features bench-diagnostics --bench renderer -- paint_layer_cache --sample-size 10` | partial: scrolling/direct `64.305-65.102 ms`; scrolling/cache `12.737-14.253 us`; animation/direct `1.9684-2.0916 ms`; animation/cache `1.0705-1.0830 ms`; offscreen steady `51.079-52.278 us`; visible noop `8.8721-8.9600 us`; stable descendant `76.136-77.622 us`; scroll return `5.6922-6.0434 us`; then pre-existing layout-page assertion failure | partial: scrolling/direct `64.724-66.416 ms`; scrolling/cache `12.641-13.710 us`; animation/direct `1.9591-2.0266 ms`; animation/cache `1.0781-1.1242 ms`; offscreen steady `44.788-50.920 us`; visible noop `9.0013-9.1012 us`; stable descendant `76.334-78.302 us`; scroll return `5.8986-6.0161 us`; then same layout-page assertion failure | Completed rows had no statistically significant changes; stale sweep is scheduled for the earliest possible stale frame to avoid scanning every frame. Targeted cache/renderer tests passed, and full `cargo test` passed in final gates. |
| wrap-aware stale sweep deadline | `cargo bench --features bench-diagnostics --bench renderer -- paint_layer_cache --sample-size 10` | partial: scrolling/direct `64.708-68.039 ms`; scrolling/cache `12.181-13.503 us`; animation/direct `1.9639-2.0180 ms`; animation/cache `1.0662-1.0757 ms`; offscreen steady `49.714-51.802 us`; visible noop `9.0327-9.1458 us`; stable descendant `76.790-78.106 us`; scroll return `5.9448-6.0394 us`; then same layout-page assertion failure | partial: scrolling/direct `63.174-64.966 ms`; scrolling/cache `12.859-13.334 us`; animation/direct `1.9893-2.0601 ms`; animation/cache `1.0724-1.1105 ms`; offscreen steady `45.139-51.416 us`; visible noop `8.9449-9.1643 us`; stable descendant `76.322-78.221 us`; scroll return `6.0093-6.1070 us`; then same layout-page assertion failure | Completed rows had no statistically significant changes; deadline selection now chooses the earliest target relative to the wrapping frame clock. |

## Fix 3: `min_visible_before_store`

Finding: `min_visible_before_store` is accepted by Elixir/Rust config but never
affects admission.

Plan:

- [x] Decide exact semantics before code: count consecutive visible frames for a
  moving payload key before admission, reset or decay on unseen/clipped frames.
- [x] Add cache-manager or renderer coverage for `min_visible_before_store: 2`:
  first visible miss draws direct with no store, second visible frame stores,
  later frame hits.
- [x] If no existing benchmark exposes one-frame visible churn, add a diagnostic
  renderer benchmark first and record its baseline before changing admission.
- [x] Run the before benchmark row for `paint_layer_cache`.
- [x] Store minimal per-key visibility/admission state in the renderer cache
  manager, not in public scene data.
- [x] Run the after benchmark row for the same command and record delta.
- [x] Run targeted renderer tests plus `cargo test`.

Benchmark log:

| Change | Command | Before | After | Notes |
| --- | --- | --- | --- | --- |
| min visible before store | `cargo bench --features bench-diagnostics --bench renderer -- paint_layer_cache --sample-size 10` | partial: scrolling/direct `64.724-66.416 ms`; scrolling/cache `12.641-13.710 us`; animation/direct `1.9591-2.0266 ms`; animation/cache `1.0781-1.1242 ms`; offscreen steady `44.788-50.920 us`; visible noop `9.0013-9.1012 us`; stable descendant `76.334-78.302 us`; scroll return `5.8986-6.0161 us`; then pre-existing layout-page assertion failure | partial: scrolling/direct `64.708-68.039 ms`; scrolling/cache `12.181-13.503 us`; animation/direct `1.9639-2.0180 ms`; animation/cache `1.0662-1.0757 ms`; offscreen steady `49.714-51.802 us`; visible noop `9.0327-9.1458 us`; stable descendant `76.790-78.106 us`; scroll return `5.9448-6.0394 us`; then same layout-page assertion failure | Default `min_visible_before_store: 1` remained out of the admission-state path; completed rows had no statistically significant regressions. No extra diagnostic benchmark was added because the behavior is covered by renderer/cache-manager tests. Full `cargo test` passed in final gates. |

## Cleanup: Pending Debug Artifacts

Finding: untracked root screenshots look like local debugging output.

Plan:

- [x] Confirm whether the root PNGs are intentional artifacts.
- [x] Remove them from the pending merge or move intentional fixtures under a
  named fixture directory with references from tests/benchmarks.
- [x] No runtime benchmark required if this is file hygiene only; record that
  decision in the benchmark log if files are removed.

Benchmark log:

| Change | Command | Before | After | Notes |
| --- | --- | --- | --- | --- |
| root PNG cleanup | none | not applicable | not applicable | Removed untracked root-level screenshot/debug PNGs only; no runtime code changed, so no benchmark was required. |

## Final Gates

- [x] `git diff --check`
- [x] `cd native/emerge_skia && cargo test`
- [x] `mix test`
- [x] `cd native/emerge_skia && cargo clippy -- -D warnings`
- [x] `./ci-tests.sh all`

## Final Benchmark Summary

All behavior changes were benchmarked with
`cargo bench --features bench-diagnostics --bench renderer -- paint_layer_cache --sample-size 10`
before and after the change. Comparable rows showed no statistically
significant regressions. Each run still stops at the pre-existing
`emerge_demo_showcase_layout_page/cache_steady_hits` assertion with
`hits=4`, `misses=1`, `stores=1`, and `current_entries=6`; that failure was
present before this plan's code changes and remains unchanged.

The layout/refresh benchmark baseline was not run because this plan did not
change render-scene construction or tree refresh state. No new diagnostic
benchmark was added; new behavior is covered by focused cache-manager and
renderer regression tests.
