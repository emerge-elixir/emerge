# Offscreen Paint-Layer Cache Hits Plan

Last updated: 2026-05-13.

Status: completed, superseded by `composited-paint-layer-cache.md`.

The fixed scroll-container payload direction in this plan is not the final
layout-page strategy. The follow-up active plan keeps stable descendants as the
reusable paint-layer payloads and treats the scroll container as a cheap
clip/composition shell.

## Goal

Improve paint-layer cache behavior when a layout-affecting animation is outside
the visible scroll viewport. In this case the visible pixels are unchanged, so
after warmup renderer stats should show cache hits without per-frame
prepare/store/eviction churn.

Also preserve stable cacheable paint layers inside scroll containers after they
have been rendered once. Scrolling a stable layer out of the viewport and back
in should draw the existing payload from cache unless content, resources, scale,
size, or cache budget invalidate it.

Observed `../emerge_demo` layout page stats:

- `frames=1199`, `layout=1544`, `refresh=0`.
- Paint-layer cache per frame: `hits=3.34`, `misses=1.23`, `stores=1.23`.
- `gpu_payload_stores=3157`, `evicted_bytes=51076917264`, resident bytes near
  the default paint-layer budget.
- `dynamic_draw count=1199`, `child_layer count=1199`.

Follow-up demo stats after the first implementation pass still showed the real
layout page rebuilding one layer per frame:

- `frames=300`, `fps=60.0`.
- Paint-layer cache per frame: `hits=2.05`, `misses=0.95`, `stores=0.95`.
- `evictions=283`, `stale_evictions=283`, `gpu_payload_stores=286`.
- `dynamic_draw count=300`, `child_layer count=300`.

User follow-up stats after fixed payload sizing showed the churn had moved into
cached child-layer composition:

- `frames=300`, `fps=60.0`.
- Paint-layer cache per frame: `hits=2.00`, `misses=1.00`, `stores=1.00`.
- `evictions=300`, `stale_evictions=0`, `gpu_payload_stores=600`.
- `dynamic_draw count=0`, `child_layer count=300`, `child_layer avg=2.382 ms`.

## Final Diagnosis

- The misses are mostly fixed paint-layer payload stores, not scroll-moving
  payload stores. `gpu_payload_stores` is much larger than `stores`, which means
  repeated fixed-layer entries with multiple static segments.
- `render_explicit_fixed_paint_layer_scope` keys fixed payloads with both static
  content hash and dynamic-slot hash. This is correct only if the dynamic-slot
  hash describes slot identity/order, not animated content or offscreen geometry.
- Fixed paint-layer payload segments are rendered into full-frame GPU surfaces
  using `context.width_px`, `context.height_px`, and `context.bytes` even for
  nested scroll-container layers. This inflates resident bytes and makes normal
  cache churn evict useful entries.
- `FixedPaintLayerRenderMode` redraws `DynamicRedraw` layers directly. It does
  not first skip dynamic slots whose `RenderPaintLayer.bounds` are outside the
  current clip, so offscreen animation can still contribute to
  `dynamic_draw_time`.
- Scroll-moving payloads are keyed independently of integer placement, which is
  good for scroll reuse, but stale eviction is driven by `last_seen_frame`.
  `render_moving_paint_layer_payload` returns early when a layer is clipped out,
  so an already-rendered stable layer can age out while it is merely outside the
  scroll viewport. That breaks scroll-away/scroll-back cache reuse.
- The real demo puts the scroll-container paint layer behind an ancestor clip.
  Fixed-layer replay was resetting paint-layer eligibility to the root, so
  cached dynamic slots lost ancestor clip context.
- Empty clip intersections were represented as `None`, the same shape used for
  unknown/no clip. That made definitely clipped descendants look visible to the
  paint-layer cache.
- Offscreen layout animation can move static siblings in the same offscreen
  row. Those static primitive changes were still part of the fixed-layer
  content hash, invalidating a visible cache payload even though no visible
  pixel changed.
- Fixed scroll-container payloads are clipped by the cached payload surface
  even when the render scene still contains coarse visible ancestor/section
  nodes. Static hash and resource generation therefore must use the actual
  payload rect as a clip, or off-payload static movement rotates the key every
  frame.
- The root fixed cache treats a child scroll-container paint layer as a dynamic
  composition slot. Timing that whole cached child layer as `dynamic_draw`
  made `dynamic_draw` mirror `child_layer` even when no visible
  `DynamicRedraw` work occurred.

## Implementation Summary

- Added Criterion coverage for
  `native/renderer/paint_layer_cache/offscreen_layout_animation/cache_steady_hits`
  and `native/renderer/paint_layer_cache/scroll_return/cache_after_clipped_frames`.
- Stabilized fixed-layer dynamic-slot hashes so animated content generation and
  primitive changes inside a dynamic slot do not invalidate cached parent static
  segments.
- Touched already-rendered scroll-moving payloads while clipped so scene-present
  layers do not stale-evict merely because they are outside the viewport.
- Skipped definitely clipped-out dynamic redraw slots before timing direct
  dynamic work.
- Prepared fixed-layer static payload segments in layer-sized surfaces for nested
  fixed paint layers, while keeping the root scope full-surface.
- Preserved ancestor clip eligibility through fixed-layer payload replay and
  direct fallback paths.
- Added explicit empty-clip tracking so empty intersections stay distinguishable
  from unknown clips.
- Made fixed-layer static content hashes and resource generations ignore static
  nodes that are definitely outside the current clip.
- Stopped recording cached child paint-layer composition as `dynamic_draw`;
  only visible `DynamicRedraw` work contributes to that timing now.
- Derived fixed scroll-container payload bounds from the visible child/clip
  intersection, then used that payload rect as the clip for fixed-layer static
  content hashes, resource generations, and static-segment admission.
- Prepared and replayed only static segments that contribute pixels inside the
  payload rect.
- Tightened the offscreen layout-animation benchmark to match the demo topology:
  a fixed scroll-container layer whose own payload clips visible pixels while
  phase-dependent static siblings and a dynamic animation row keep changing
  below the viewport.

## Benchmark Baseline Protocol

Run the benchmarks before production behavior changes, then run the same set
after implementation. The new offscreen benchmark may be added first as
diagnostic benchmark-only scaffolding; capture its baseline before changing
renderer logic.

- [x] Record the current renderer paint-layer cache baseline:
  `cd native/emerge_skia && cargo bench --features bench-diagnostics --bench renderer -- paint_layer_cache --sample-size 10`.
- [x] Record current layout showcase baselines that must remain wired into
  Criterion:
  `cd native/emerge_skia && cargo bench --features bench-diagnostics --bench layout -- rich_borders_showcase --sample-size 10`.
- [x] Add the offscreen layout-animation renderer benchmark in diagnostic form
  and run it against current production behavior:
  `cd native/emerge_skia && cargo bench --features bench-diagnostics --bench renderer -- offscreen_layout_animation --sample-size 10`.
- [x] Add the scroll-return renderer benchmark in diagnostic form and run it
  against current production behavior:
  `cd native/emerge_skia && cargo bench --features bench-diagnostics --bench renderer -- scroll_return --sample-size 10`.
- [x] Save the before numbers in this plan: misses, stores, evictions,
  stale evictions, `moved_hits`, `moved_misses`, `gpu_payload_stores`,
  `evicted_bytes`, `dynamic_draw` count/time, and resident bytes.
- [x] After implementation, rerun all commands above and record the after
  numbers next to the baseline.

## Implementation Checklist

### 1. Reproduce the Demo Shape

- [x] Inspect `../emerge_demo/lib/emerge_demo/showcase/view/layout.ex` and map
  the layout page into a minimal native benchmark fixture: visible static rows,
  scroll viewport, and a layout-transform animation row below the viewport.
- [x] Keep the fixture small enough for stable Criterion runs, but include the
  important cache topology: a cacheable fixed scroll container with static
  segments separated by one `DynamicRedraw` animation layer.
- [x] Add helper builders in `native/emerge_skia/benches/renderer.rs` near the
  existing paint-layer cache scene helpers:
  `offscreen_layout_animation_paint_layer_states`,
  `offscreen_layout_animation_scene`, and a stats helper that returns warm and
  steady frame `RendererCachePaintLayerFrameStats`.
- [x] Add a benchmark id under
  `native/renderer/paint_layer_cache/offscreen_layout_animation`.
- [x] In benchmark diagnostics, print or assert the per-frame stats needed to
  prove the bug before the fix and the steady-state behavior after the fix.

### 2. Add Focused Regression Coverage

- [x] Add a Rust unit test for fixed-layer dynamic-slot hashing in
  `native/emerge_skia/src/renderer.rs`.
- [x] Assert the dynamic-slot hash stays stable when a dynamic child layer keeps
  the same slot identity but changes animated content generation or primitive
  content.
- [x] Assert the dynamic-slot hash changes when dynamic slot identity or order
  changes.
- [x] Add a renderer-cache regression test or diagnostic benchmark assertion
  where the first warm frame may store, but the next offscreen animation frame
  has `misses == 0`, `stores == 0`, and `evictions == 0`.
- [x] Add a scroll-return regression test where a stable scroll-moving layer is
  rendered and stored, scrolled fully outside the clip for more than the stale
  window, then scrolled back into view. The return frame should have a cache hit,
  `moved_hits == 1`, `misses == 0`, `stores == 0`, and no stale eviction for
  that payload.
- [x] Keep the existing never-rendered offscreen behavior: a scroll-moving layer
  that has not previously been admitted should not be prepared or stored while
  clipped out.
- [x] Keep existing scrolling and visible-animation cache benchmarks unchanged
  except for any shared helper rename needed by the new fixture.

### 3. Retain Rendered Scroll-Moving Layers While Clipped

- [x] Audit the zero-visible branch in
  `SceneRenderer::render_moving_paint_layer_payload`.
- [x] Compute the moving payload key early enough to identify an existing cached
  payload even when `visible_pixels == 0`.
- [x] Add a renderer-cache method that marks an existing moving payload as
  present-but-clipped without incrementing visible count, without admitting a
  new store, and without preparing pixels.
- [x] Use that method on zero-visible scroll-moving layers when the payload key
  is valid and an entry already exists. This should update `last_seen_frame` so
  stale eviction treats the layer as still present in the scene.
- [x] Keep stale eviction for truly absent layers: if the layer is no longer in
  the render scene, no touch occurs and the payload can expire normally.
- [x] Keep byte-budget and entry-budget eviction intact. This requirement
  protects against stale eviction while clipped, not against legitimate pressure
  from newer cache entries.
- [x] Ensure scroll-back uses the existing placement-independent moving payload
  key and records a moved hit rather than a miss/store.

### 4. Stabilize Fixed-Layer Keys Around Dynamic Slots

- [x] Update `paint_layer_dynamic_slots_hash` in
  `native/emerge_skia/src/renderer.rs` so it hashes the ordered dynamic slot
  structure, not volatile animated content.
- [x] Replace `hash_paint_layer_metadata` usage for dynamic slot nodes with a
  narrower slot identity hash: stable id, placement, policy, reason, and static
  or dynamic segment separators.
- [x] Exclude `content_generation`, animated primitive values, dynamic child
  subtree hashes, and offscreen child geometry from the parent fixed-layer key.
- [x] Keep `paint_layer_static_content_hash` strict so actual visible static
  content changes still invalidate the fixed payload.
- [x] Run the dynamic-slot hash unit tests before moving to render behavior.

### 5. Skip Clipped-Out Dynamic Redraw Work

- [x] Generalize or reuse `moving_paint_layer_payload_visible_device_rect` so
  any `RenderPaintLayer` can be tested against the current transform and clip.
- [x] In `FixedPaintLayerRenderMode::render_paint_layer`, check visibility
  before direct-rendering `PaintLayerPolicy::DynamicRedraw`.
- [x] If the dynamic layer bounds do not intersect the current eligibility clip,
  return without drawing the layer and without recording `dynamic_draw_time`.
- [x] Preserve existing behavior when visibility cannot be proven, including
  unknown clips, unsupported transforms, and visible dynamic layers.
- [x] Add test or benchmark assertions that the offscreen animation row does not
  contribute steady-state dynamic draw work.

### 6. Reduce Fixed Payload Surface Size

- [x] Audit `render_explicit_fixed_paint_layer_scope`,
  `prepare_fixed_paint_layer_gpu_payload_segments`, and
  `render_explicit_fixed_paint_layer_payload_with_dynamic_slots`.
- [x] Pass a payload bounds rectangle into fixed paint-layer rendering. The root
  scope keeps full surface bounds; nested fixed scopes use
  `RenderPaintLayer.bounds`.
- [x] Derive payload `width_px`, `height_px`, and `bytes` from payload bounds,
  clamped to valid positive device pixels.
- [x] When preparing cached static segments, translate the payload canvas by
  `-bounds.x, -bounds.y` so existing scene coordinates render into the smaller
  surface.
- [x] When drawing cached fixed payloads, place each static segment image at
  `bounds.x, bounds.y` instead of assuming `(0, 0)`.
- [x] Include payload dimensions in the fixed payload key; do not use full-frame
  dimensions for nested fixed layers.
- [x] Keep a direct fallback for non-finite bounds, zero-sized payloads, and any
  Skia surface creation failure.
- [x] Verify shadows, borders, clips, and nested scroll-container paint are not
  cropped. If layer bounds are too tight for an existing effect, either use the
  renderer's conservative visual bounds for the payload or leave that case on
  the full-surface path with a test.

### 7. Benchmark and Test After Each Risky Slice

- [x] After key stabilization, rerun the offscreen benchmark and record whether
  stores/misses fall without payload sizing.
- [x] After clipped dynamic redraw skipping, rerun the offscreen benchmark and
  record `dynamic_draw` count/time.
- [x] After scroll-moving clipped retention, rerun the scroll-return benchmark
  and record `moved_hits`, stale evictions, misses, and stores.
- [x] After payload sizing, rerun the full benchmark protocol and record resident
  bytes, evicted bytes, stores, and evictions.
- [x] Run `cd native/emerge_skia && cargo test`.
- [x] Run `mix test`.
- [x] Run `./ci-tests.sh` for final validation.
- [ ] Manually recheck `../emerge_demo` layout page stats with the animated row
  out of the scroll viewport. User-supplied first-pass stats exposed remaining
  churn; a final interactive demo rerun was not performed here. The tightened
  Criterion reproducer now covers the same cache topology and asserts the
  target cache stats.

### 8. Follow Up On Real Demo Stats

- [x] Treat the user-supplied 300-frame demo stats as a failing first-pass
  result because they still showed per-frame miss/store/stale-eviction churn.
- [x] Move the offscreen benchmark's scroll clip outside the fixed
  scroll-container paint layer to match the real render-scene topology.
- [x] Add phase-dependent offscreen static sibling movement to the offscreen
  benchmark so visible cache keys must ignore definitely clipped static changes.
- [x] Preserve ancestor clip eligibility during fixed-layer dynamic-slot replay.
- [x] Track empty clip intersections explicitly and make moving/fixed/dynamic
  layer visibility respect that state.
- [x] Add unit coverage proving clipped static primitive changes do not affect
  fixed-layer static hashes.
- [x] Add unit coverage proving visible `DynamicRedraw` detection respects
  ancestor clips and does not count cacheable child paint layers as dirty
  dynamic work.
- [x] Rerun the tightened offscreen benchmark with assertions for zero steady
  misses, stores, evictions, payload stores, and dirty dynamic redraw time.
- [x] Add unit coverage proving fixed-layer static hashes use the payload clip,
  not just ancestor clip state.
- [x] Use the payload clip to count/prepare only visible fixed static segments.

## Target After-State

- Warm steady offscreen animation frames have `misses == 0`, `stores == 0`, and
  `evictions == 0` for the fixed paint-layer cache.
- The offscreen animation row does not add steady-state `dynamic_draw` work.
- A stable scroll-moving paint layer that was rendered once, scrolled out, and
  scrolled back in hits the existing payload instead of preparing a replacement.
- Resident bytes for nested fixed paint-layer payloads scale with layer bounds,
  not full window size.
- Existing Criterion coverage still includes paint-layer cache scrolling,
  paint-layer cache animation, and rich Borders showcase cases.
- Renderer pixel/parity tests still pass.
- `cargo test`, `mix test`, and `./ci-tests.sh` pass.

## Measurement Log

Fill this in during implementation.

| Stage | Command | Key result |
| --- | --- | --- |
| Before existing paint-layer cache | `cargo bench --features bench-diagnostics --bench renderer -- paint_layer_cache --sample-size 10` | scrolling direct 63.490 ms; scrolling cache 54.403 us; animation direct 1.9213 ms; animation cache 1.2247 ms |
| Before rich Borders showcase | `cargo bench --features bench-diagnostics --bench layout -- rich_borders_showcase --sample-size 10` | animation full 655.03 us; animation paint-only 574.50 us; scroll full 321.25 us; scroll paint-only 237.61 us |
| Before offscreen layout animation | `cargo bench --features bench-diagnostics --bench renderer -- offscreen_layout_animation --sample-size 10` | steady stats: `hits=0 misses=1 stores=1 evictions=0 gpu_payload_stores=2 dirty_draw_time>0 current_bytes=11059200`; time 56.140 us |
| Before scroll return | `cargo bench --features bench-diagnostics --bench renderer -- scroll_return --sample-size 10` | stale-window frame: `stale_evictions=1 evicted_bytes=56160`; return frame: `hits=1 misses=1 stores=1 moved_hits=0 gpu_payload_stores=1`; time 463.21 us |
| After scroll-moving clipped retention | `cargo bench --features bench-diagnostics --bench renderer -- scroll_return --sample-size 10` | stale-window frame: `stale_evictions=0 current_entries=2`; return frame: `hits=2 misses=0 stores=0 moved_hits=1 gpu_payload_stores=0` |
| After key stabilization | `cargo bench --features bench-diagnostics --bench renderer -- offscreen_layout_animation --sample-size 10` | steady stats after key fix: `hits=1 misses=0 stores=0 evictions=0 gpu_payload_stores=0` |
| After clipped dynamic skip | `cargo bench --features bench-diagnostics --bench renderer -- offscreen_layout_animation --sample-size 10` | steady stats after skip: `dirty_draw_time=0ns`; final targeted time 39.228 us |
| After payload sizing | full benchmark protocol | renderer full group passed; offscreen 39.600 us, scroll return 17.597 us; rich Borders rerun completed with small local changes: animation +1.4 to +1.8%, scroll full -0.58%, scroll paint-only +1.51% |
| User demo stats after first pass | user-supplied `../emerge_demo` layout page stats | still failing: per frame `hits=2.05 misses=0.95 stores=0.95`; `evictions=283 stale_evictions=283`; `dynamic_draw count=300 child_layer count=300` |
| After internal child clip payload bounds | `cargo bench --features bench-diagnostics --bench renderer -- offscreen_layout_animation --sample-size 10` | passed assertions; targeted time 47.727 us |
| After payload-clip static hash/visible segment filtering | `cargo bench --features bench-diagnostics --bench renderer -- offscreen_layout_animation --sample-size 10` | passed assertions; targeted time 47.180 us |
| After final renderer group | `cargo bench --features bench-diagnostics --bench renderer -- paint_layer_cache --sample-size 10` | scrolling direct 62.851 ms; scrolling cache 33.569 us; animation direct 1.8875 ms; animation cache 1.1655 ms; tightened offscreen 47.786 us; scroll return 27.066 us |
| After final rich Borders showcase | `cargo bench --features bench-diagnostics --bench layout -- rich_borders_showcase --sample-size 10` | animation full 642.55 us; animation paint-only 567.77 us; scroll full 311.67 us; scroll paint-only 235.47 us |
| Final tests | `cargo test`, `mix test`, `./ci-tests.sh` | passed: Rust 821 tests; Elixir 379 tests + 13 doctests; full CI passed including Credo, clippy, full-sweep tests, release Rust tests, full-sweep Elixir tests, and Dialyzer |

## Guardrails

- Do not disable layout-affecting animation sampling; this plan only changes
  paint-layer rendering/cache behavior.
- Do not cache dynamic layer pixels as static content.
- Do not prepare or admit never-rendered scroll-moving layers just because they
  are present but clipped out.
- Do not remove stale eviction for layers that are absent from the render scene,
  and do not bypass entry or byte budget eviction.
- Do not remove or weaken the scrolling, visible-animation, or rich Borders
  benchmarks.
- Do not broaden public renderer stats unless a new counter is required to prove
  this fix.
- Prefer small targeted tests over broad snapshot assertions.
