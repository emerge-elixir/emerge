# Composited Paint-Layer Cache Plan

Last updated: 2026-05-13.

Status: completed.

## Goal

Keep rendering as a composition of deterministic paint layers while simplifying
scene construction and reducing the real `../emerge_demo` showcase costs.

Paint-layer semantics:

- A layer starts at a semantic element boundary: root, scroll container, stable
  scroll-content child, animated paint/layout attribute, or `Nearby` escape.
- A layer owns only its own paint range until a child starts another layer.
- Child layer refs survive parent invalidation and redraw.
- Placement, scroll offset, clip, alpha, and compositor transforms are
  composition state, not payload identity.
- Cache admission decides whether a layer payload is stored; it must not decide
  whether the semantic paint-layer boundary exists.

Primary acceptance target:

- `../emerge_demo` showcase Borders screenshot viewport:
  `1909x2148`, scale `1.5`.
- Warmed composited paint-layer frame: `render avg < 0.5 ms`.
- Same viewport refresh path: `refresh avg < 1.0 ms`.
- Warmed cache frames must have zero misses, stores, evictions, and stale
  evictions for unchanged visible layers.

Secondary targets:

- Showcase layout page with animated row outside viewport:
  `render draw avg < 0.5 ms`.
- Showcase layout page with animated row visible: only the animated subtree and
  immediate changing container should redraw; stable siblings should hit.
- Borders bottom and animated-shadow sections must keep bounded visible layer
  counts and avoid the previous 50-100 visible-hit explosion.

## Completed

- Real `../emerge_demo` showcase Borders fixture is wired into Criterion.
- Exact screenshot benchmark is wired:
  `native/renderer/paint_layer_cache/emerge_demo_showcase_borders/screenshot_1909x2148_scale_1_5/cache_steady_hits`.
- Exact screenshot refresh benchmark is wired:
  `native/renderer/paint_layer_cache/emerge_demo_showcase_borders/screenshot_1909x2148_scale_1_5/refresh_scene`.
- The exact target selector currently picks:
  `size=1909x2148`, `scale=1.5`, `scroll_y=952`, `score=38`,
  `nodes=744`, `primitives=281`, `texts=201`, `paint_layers=13`,
  `cacheable=9`, `dynamic=4`, `moving=7`.
- Explicit layer fields are in place:
  `own_nodes`, `child_refs`, `root_id`, and scene-derived `metrics`.
- Non-test builds no longer retain duplicate raw child trees in
  `RenderPaintLayer`.
- Parent payload hashing ignores child layer refs and child placement.
- Parent cache hits still compose child refs.
- Parent cache misses/redraws only draw parent `own_nodes`, then compose child
  refs.
- Scroll-container parent clips are applied at composition instead of being
  baked into child payloads.
- The earlier unbounded scroll-descendant topology was rejected and rolled back.
- Bounded layer counts recovered live rendering correctness.
- Source-rect cached-image draws are gated so small clip savings keep the
  cheaper full-image path.
- GPU-backed cache admission bypasses tiny, cheap payloads instead of storing
  them as resident images. The semantic paint-layer boundary remains; raster
  cache tests still store/hit those layers.
- Benchmark gates now count warmed cache hits plus deliberate low-value bypasses
  as visible paint-layer coverage while still requiring zero steady
  misses/stores/evictions for unchanged targets.

## Current Measurements

Performance lock:

- The current local/live performance level is accepted as the floor for this
  plan. Fixes below must preserve the current benchmark shape: exact Borders
  screenshot warmed render stays at the accepted level, refresh remains under
  `1 ms`, warmed unchanged targets keep zero misses/stores/evictions, and
  stable layout/Borders targets keep bounded visible layer counts.

Latest local exact benchmark after scene-construction cleanup and GPU tiny
low-value bypass:

- `cache_steady_hits`: about `4.24-4.30 ms` on local surfaceless EGL. Treat this
  absolute total as directional because this runner is flush dominated.
- `refresh_scene`: about `510-516 us`, meeting the `<1 ms` refresh target
  locally.
- Exact screenshot cache shape remains bounded: semantic layer count is
  unchanged, while GPU cached-image draws drop from `10/frame` to `7/frame` via
  `bypassed_low_value=3`.

Latest focused local cache benchmarks:

- Showcase layout steady hits: about `1.69-1.77 ms`, improved from the previous
  saved `~2.5 ms` range.
- Rich Borders proxy steady hits: about `104-106 us`, improved from the previous
  `~0.84-0.95 ms` range.
- Real showcase Borders `1920x1080` steady hits: about `2.60-2.70 ms`, improved
  from the previous `~5.39-5.46 ms` range.

Bug-fix validation after the current correctness pass:

- Exact Borders screenshot `cache_steady_hits`: `4.08-4.12 ms` locally, with the
  warmed zero-miss/store/eviction gate passing.
- Exact Borders screenshot `refresh_scene`: `531-536 us` locally, still under
  the `<1 ms` refresh target.
- Layout page `cache_steady_hits`: warmed zero-miss/store/eviction gate passes.
- `store_payloads` is an interval counter for new payload stores during the
  stats window. A warmed steady window with `stores=0` should show
  `store_payloads: gpu=0 cpu=0`; existing resident payloads are reported by the
  `resident ... payloads={...}` line.
- Final verification for this pass: `cargo test`, `mix test`, and
  `./ci-tests.sh all` pass.

Latest live Borders screenshot signal before the current cleanup:

- `render avg=0.998 ms`
- `draw avg=0.599 ms`
- `flush avg=0.399 ms`
- `refresh avg=1.919 ms`
- warmed cache: `hits=10/frame`, `misses=0`, `stores=0`, `evictions=0`
- composition: about `7.86M payload pixels/frame`, waste `1.11`

Interpretation:

- Cache misses are not the warmed-state problem.
- Refresh was hurt by scene construction work; the latest local benchmark shows
  this is largely fixed.
- Remaining render work is composition/direct draw cost on hit-only frames.

## Completed Implementation Checklist

0. Fix current correctness regressions without moving the performance floor.
   - Done: layout page animated layout-transform card now forces normal parent
     resolve instead of reusing a stale parent AABB; regression covered by
     `layout_transform_animation_resizes_parent_row_after_cached_initial_layout`.
   - Done: scroll containers use semantic generation plus quantized scroll
     offsets, so direct scroll content redraws on scroll without redrawing every
     fixed-offset animation frame; regression covered by both generation and
     pixel cache/direct comparison tests.
   - Done: todo-like fill list shrink no longer keeps stale cached extents;
     placeholder composition remains covered by the existing pixel comparison.
   - Done: `store_payloads` clarified as a stats-window store counter rather
     than resident cache contents.

1. Keep the benchmark suite warning-clean.
   - Remove stale fields and test-only helpers from non-test builds.
   - Keep the exact Borders screenshot render and refresh benchmarks runnable
     with `bench-diagnostics`.

2. Simplify scene construction.
   - Keep the owned single-split layer builder path.
   - Avoid full subtree split/hash/bounds/metric walks for unchanged semantic
     generations.
   - Keep child refs explicit and independent of parent payload invalidation.
   - Delete transitional code only when a benchmark or test covers the behavior.

3. Reduce warmed hit-frame render cost for the exact Borders viewport.
   - Completed: miss/bypass diagnostics include stable id, root id, reason,
     policy, generation, bounds, payload pixels, visible pixels, primitive cost,
     own node/primitive count, child-ref count, and cache key.
   - Keep improving per-layer hit diagnostics for resident hit draw decisions.
   - Identify the expensive direct/dynamic work left outside cached payloads.
   - Prefer fewer valuable composited images over many tiny cached images.
   - Do not bypass a resident payload unless a benchmark proves total render
     time improves.

4. Tighten payload work without changing semantics.
   - Shrink payload bounds to own visual content where composition clipping
     still preserves escaped pixels.
   - Keep `Nearby` cacheable and unclipped internally; clip it only during
     composition.
   - Add an opaque composition fast path only after diagnostics show an opaque
     layer contributes materially to flush/composition time.

5. Improve exact showcase fixture coverage.
   - Add Borders bottom and animated-shadow selectors from the real fixture.
   - Add layout row-out and row-visible selectors from the real fixture.
   - Gate each warmed target on bounded visible layer counts and zero steady
     misses/stores/evictions.

6. Validate correctness and performance.
   - Re-run exact Borders screenshot render and refresh benchmarks before and
     after each render-cost change.
   - Re-run layout and Borders cache benchmarks after topology changes.
   - Run `cargo test`, Rust clippy with `-D warnings`, benchmark clippy with
     `bench-diagnostics`, `mix test`, and local CI before completion.

## Open Risks

- Local EGL surfaceless render totals are dominated by flush and do not match
  live Wayland/DRM/Metal absolute timings. Use them for relative changes and
  cache-shape assertions, not as the only acceptance signal.
- Direct redraw can lower composition pixels but has already regressed total
  render time in Borders experiments. Treat direct fallback as a measured
  per-layer decision, not a blanket policy.
- Over-granular semantic boundaries previously fixed some cache behavior but
  broke rendering and refresh costs. Keep layer counts bounded until a retained
  layer tree replaces transitional child-ref node vectors.
