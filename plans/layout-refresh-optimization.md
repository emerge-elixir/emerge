# Layout And Refresh Optimization Plan

Last updated: 2026-05-15.

Status: completed.

Benchmark reporting rule: every benchmark result written here must include the
previous result next to the new result. For a newly added benchmark, write
`previous: none (new benchmark)` and treat the first measured value as the
baseline for future comparisons.

## Goal

Reduce layout and refresh cost while preserving the current composited
paint-layer renderer behavior. Prefer code deletion, consolidation, and clearer
cache boundaries over adding special-case ladders.

Core model:

- Every frame is composed from semantic paint layers.
- A child paint layer owns its rendering; the parent only owns the child-layer
  reference and composition placement.
- Clean retained subtrees can be any depth. If their inputs are unchanged,
  layout, refresh, and registry rebuild should replay cached products without
  descending.
- Cache keys must be semantic and stable. Do not hide payload churn behind
  frame budgets.

## Locked Gates

- Borders page, continuous hover over code blocks:
  - render avg `<= 0.54 ms`
  - render draw avg `<= 0.23 ms`
  - render flush avg `<= 0.32 ms`
  - refresh avg `<= 0.82 ms`
  - no hover-transition slow frames that burst-store 7-8 payloads or spend
    about `2.4 ms` in paint-layer prepare.
- Layout page, animated layout row visible:
  - render avg `<= 0.96 ms`
  - render draw avg `<= 0.49 ms`
  - render flush avg `<= 0.48 ms`
  - layout + refresh benchmark avg `< 0.50 ms`
  - true layout-pass avg `< 0.20 ms`
- Exact Borders screenshot:
  - `cache_steady_hits` stays around `4.0 ms`
  - warmed misses/stores/evictions stay zero
  - `refresh_scene` stays near the accepted local baseline.
- Full `./ci-tests.sh all` must pass before closing the plan.

## Current Benchmarks

- `borders_screenshot_hover_visible_targets`:
  previous `131.89-132.20 us`, current `132.29-134.01 us`.
- `borders_screenshot_held_nearby_refresh`:
  previous `133.82-135.39 us`, current `130.79-131.45 us`.
- Renderer `hover_transition_replay`:
  previous `4.3945-4.4684 ms`, current `4.2166-4.3752 ms`.
  Max-frame gate passed; previous `stores=1`, `prepare_time<=0.207 ms`,
  current gate passed without payload burst.
- `nearby_hover_toggle_refresh/borders_like/held_show_refresh_only`:
  previous `247.80-300.31 us`, current `205.92-223.28 us`.
- `interaction_virtual_key_full_loop`:
  previous `179.15-182.41 us`, current `147.28-147.53 us`.
- `interaction_virtual_keyboard_text_echo`:
  previous `259.62-261.29 us`, current `130.59-130.93 us`.
- `interaction_scroll_step_cached_refresh`:
  previous `481.11-486.51 us`, current `412.68-421.58 us`.
- `layout_page_visible_animated_row`:
  previous `329.48-344.03 us`, current `315.53-316.60 us`.
- Exact Borders `refresh_scene`:
  previous `131.14-131.81 us`, current `132.87-133.95 us`.
- Exact Borders `cache_steady_hits`:
  previous `3.9791-4.2411 ms`, current `4.0634-4.3256 ms`.

## Completed Summary

1. Refresh path simplification.
   - Removed redundant direct-refresh branches and kept one semantic composited
     refresh path plus one clean-registry refresh path.
   - Removed root dirty scans where bubbled dirty flags already carry the same
     information.

2. Showcase benchmarks and fixtures.
   - Added real `../emerge_demo` backed benchmarks for Borders hover, Borders
     held nearby, Layout visible animated row, Interaction virtual keyboard
     text echo, and Interaction virtual-key full loop.
   - Added `bench/generate_external_fixtures.exs` for external demo fixtures.

3. Layout and registry cleanup.
   - Kept root layout semantics, but made dirty-descendant cache replay cheaper.
   - Split runtime recompute stats so the live `layout` bucket reports actual
     layout work instead of scene/registry rebuild.
   - Pruned registry traversal for subtrees that cannot affect input, focus,
     scrollbars, virtual keys, or hover/front-nearby behavior.
   - Added resize/scale registry regression coverage for the frozen-input bug.

4. Paint-layer cache model cleanup.
   - Removed scroll-content depth and render-node caps for clean static
     subtrees.
   - Made clean scroll children own full static descendant payloads as semantic
     paint layers.
   - Flattened empty paint-layer wrappers.
   - Stabilized moving paint-layer payload keys around per-element paint
     generation instead of global tree revision.
   - Kept content hashing where renderer-cache coverage depends on it.

5. Slider and Interaction fixes.
   - Fixed SVG cover rasterization so slider drag does not re-rasterize huge
     clipped vector variants.
   - Made slider value changes resolve-only and matching runtime-owned slider
     patches no-op acknowledgements.
   - Split focused slider glow from moving child content so the glow remains
     stable and unclipped during drag.
   - Fixed virtual-key mouse-down lifecycle so release/cancel clears press
     styling.

6. Retained render fragments.
   - Added retained render-fragment cache in `NodeRefreshState`.
   - Implemented the first splice at nearby roots so clean mounted nearby
     content reuses the cached wrapped fragment.
   - Added regression coverage proving clean nearby fragments are reused
     without descending.

7. SVG slider-thumb z-order.
   - Fixed the bug where the SVG thumb could be covered by the dark track when
     track children became scroll-moving child paint layers.
   - Slider media children in scroll-moving contexts now get a narrow
     direct-only paint-layer boundary, preserving sibling paint order without
     broad media layering.
   - Added
     `test_svg_slider_thumb_paints_above_scroll_moving_track_layers`.

8. Borders hover transition replay.
   - Added a renderer replay for the exact Borders screenshot hover path using
     nearby mount/unmount patches.
   - Record max stores/frame, prepare time, render time, and GPU flush time.
   - The replay gates on actual changed layers, not five-second averages. It
     catches regressions where a hover transition creates new payload keys for
     unchanged layers.
   - Current result: previous `4.7219-4.7958 ms`, current
     `4.3945-4.4684 ms`; new max-frame gate previous none, current
     `stores=1`, `prepare_time<=0.207 ms`.

9. Incremental frame-attrs preparation for refresh-only updates.
   - Refresh-only runtime and `SetAttrs` updates now prepare only touched frame
     attrs plus active animation ids instead of re-scaling the whole tree.
   - Non-`SetAttrs` patch refreshes keep the full preparation path so nearby
     mount/structure cases stay correct.
   - Current results: Borders hover previous `238.56-240.95 us`, current
     `129.64-129.93 us`; virtual-key full loop previous `331.03-335.85 us`,
     current `173.95-174.20 us`; text echo previous `399.00-402.79 us`,
     current `259.59-260.01 us`.

10. Interaction scroll benchmark alignment and registry skip cleanup.
   - The Interaction scroll benchmark now uses the same dirty-id frame-attr
     preparation shape as production scroll refresh instead of forcing full
     frame-attr preparation on every scroll step.
   - Full scroll registry rebuild still bypasses subtree chunk reuse because
     scroll-dependent geometry makes chunk keys churn, but clean neutral
     branches now use retained `registry_subtree_affects` state instead of
     recursively rediscovering registry relevance.
   - Current result: previous `647.68-654.43 us`, current `481.11-486.51 us`.
   - Rejected experiment: forcing scroll through registry chunk reuse measured
     previous `647.68-654.43 us`, experiment `909.98-911.47 us`, so the active
     path stays with simple full scroll rebuild plus cheap neutral-subtree
     pruning.
   - Added `cached_registry_rebuild_dirty_child_ignores_stale_affects_flag` to
     prove dirty registry nodes bypass retained `registry_subtree_affects`
     pruning.

11. Scaled Press stale-lane freeze fix.
   - Clicking Interaction `Scaled Press` combines an Elixir `on_mouse_down`
     event with local `mouse_down` style state. That local style message does
     not require a fresh registry by itself, but the Elixir event does.
   - The dispatcher now requests `TreeMsg::RebuildRegistry` when an Elixir
     event is paired only with non-staling local style messages, so buffered
     release input cannot wait forever.
   - The tree actor now honors that explicit request even when the same frame
     only needs a paint refresh and can reuse the clean cached registry.
   - Added `direct_runtime_mouse_event_with_mouse_down_style_requests_rebuild`.
   - Added
     `explicit_registry_rebuild_publishes_after_local_paint_refresh`.
   - Benchmark impact: previous none (logic-only regression test), current
     targeted test passes.

12. Shared registry listener storage.
   - `Registry` now stores listeners behind shared storage, so cloning a cached
     `RegistryRebuildPayload` to update text input or slider runtime maps no
     longer copies the full listener vector.
   - This specifically removes the refresh cost spike on virtual keyboard text
     echo, where the base registry is unchanged but text input state changes.
   - Added diagnostics for uncached registry traversal visits so scroll profiles
     report both cached and uncached registry walkers.
   - Current results: text echo previous `259.62-261.29 us`, current
     `130.59-130.93 us`; virtual-key full loop previous `179.15-182.41 us`,
     current `147.28-147.53 us`; Interaction scroll previous
     `481.11-486.51 us`, current `412.68-421.58 us`.
   - Rechecked rejected scroll chunk-cache experiment: previous
     `487.87-489.21 us`, experiment `728.47-730.42 us`, so scroll keeps the
     simpler full registry rebuild path.
   - Watch item: `borders_screenshot_held_nearby_refresh` measured previous
     `129.96-130.38 us`, current `133.82-135.39 us`; this is small but should
     be rechecked before closing the plan.

## Completion Notes

1. Retained render fragments stay scoped to nearby roots for now.
   - Renderer transition replay does not justify broadening the cache: payload
     churn is bounded, and broader fragment replay would need more stale-cache
     tests before it can remove existing code.
   - No new moving-layer-specific cache family was added.

2. Refresh simplification is closed for this slice.
   - Shared registry listener storage removed the virtual-key text echo clone
     cost.
   - Scroll keeps the simpler full registry rebuild path because the measured
     chunk-cache experiment stayed slower.
   - Offscreen virtual-key subtrees no longer force retained registry traversal.

3. The benchmark and correctness gates passed.
   - Borders held-nearby watch item rechecked at previous
     `133.82-135.39 us`, current `130.79-131.45 us`.
   - Full `./ci-tests.sh all` passed on 2026-05-15.
   - `guides/internals/layout-refresh-render-flow.md` documents the final
     layout/refresh/render traversal and cache model.

## Final Validation

Passed on 2026-05-15:

- `cargo test --manifest-path native/emerge_skia/Cargo.toml`
- `mix test`
- `cargo clippy --manifest-path native/emerge_skia/Cargo.toml -- -D warnings`
- `git diff --check`
- `./ci-tests.sh all`
