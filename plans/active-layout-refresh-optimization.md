# Layout And Refresh Optimization Plan

Last updated: 2026-05-13.

Status: active.

## Goal

Reduce layout and refresh cost now that composited paint-layer rendering is
stable. Keep the current renderer-cache performance as the non-regression gate
while simplifying the layout/refresh paths enough that hover and visible
layout-animation frames spend less time in tree work.

## Locked Gates

Keep these live shapes at or below the current level:

- Borders page, continuous hover over code blocks:
  - render avg `<= 0.54 ms`
  - render draw avg `<= 0.23 ms`
  - render flush avg `<= 0.32 ms`
  - refresh avg `<= 0.82 ms`
  - layout avg `<= 1.72 ms` on hover-triggered layout frames
  - renderer cache stays around `9 hits/frame`, `0.30 stores/frame`, and bounded
    visible layers.
- Layout page, animated layout row visible:
  - render avg `<= 0.96 ms`
  - render draw avg `<= 0.49 ms`
  - render flush avg `<= 0.48 ms`
  - layout avg `<= 0.95 ms`
  - stable siblings keep hitting paint-layer cache while only the animated row
    area churns.
- Exact Borders screenshot benchmark remains under the accepted local baseline:
  `cache_steady_hits` about `4.08-4.12 ms`, zero warmed misses/stores/evictions,
  and `refresh_scene` about `531-536 us`.
- Full `./ci-tests.sh all` must pass before completion.

## Investigation Notes

- `TreeUpdateEngine::process_messages` batches hover state, scroll, patches, and
  animation pulses before choosing skip, cached registry reuse, refresh, or full
  layout. Hover code-block frames still produce layout frames when interaction
  style changes classify as resolve/measure.
- `prepare_frame_attrs_for_update` is still broad for ordinary dirty frames.
  The animation-only path can prepare active ids, but hover/layout patches can
  still force wider attr preparation than the changed ids require.
- `layout.rs::run_layout_passes` always runs measure then resolve from root once
  recompute is required. Current stats show hover has subtree-measure hits and
  no measure misses, so the cost is mostly resolve traversal/cache restore.
- `try_reuse_resolve_cache_with_dirty_descendants` is now correctness-safe for
  normal dirty children, but that also means common row/column cases need a
  more explicit dirty-child splice instead of reusing stale parent AABBs or
  re-resolving too much.
- `refresh_reusing_clean_registry` already avoids rebuilding the registry when
  clean, but render scene construction still walks enough of the tree that
  refresh is visible during high-frequency hover.

## Active Checklist

1. Add real showcase layout/refresh benchmarks.
   - Add a Borders code-block hover benchmark that replays the hover pattern
     from the live stats and reports layout, refresh, and renderer-cache shape.
   - Add a layout-page visible animated-row benchmark distinct from the
     row-out-of-viewport steady-hit case.
   - Gate both on the locked live numbers and on the existing exact Borders
     screenshot render/refresh gates.

2. Make hover invalidation cheaper.
   - Audit `classify_interaction_style` and `set_mouse_over_active` for code
     blocks to prove whether hover is truly layout-affecting or only paint /
     registry.
   - If hover attrs are layout-neutral, keep those frames on refresh-only
     scheduling.
   - If hover attrs are layout-affecting, preserve measurement caches and avoid
     walking unrelated resolve siblings.

3. Optimize resolve-local layout changes.
   - Add a row/column dirty-child resolve splice path: recompute dirty child
     frame/AABB, shift following siblings by the delta, and keep clean sibling
     resolve cache hot.
   - Cover layout-transform animation where the animated card changes AABB but
     stable rows/sections only move.
   - Keep nearby-only resolve reuse as a separate, explicit fast path.

4. Reduce refresh scene construction work.
   - Add traversal diagnostics for refresh frames: element visits, culled
     subtrees, emitted paint layers, split/hash work, and registry reuse.
   - Avoid rebuilding unchanged paint-layer payload metadata when only placement
     or hover paint changes.
   - Keep semantic paint-layer boundaries deterministic; do not reintroduce
     renderer-side subtree discovery.

5. Simplify after the benchmark proves each cut.
   - Remove obsolete refresh/cache helper branches once the dirty-child splice
     and refresh diagnostics cover their behavior.
   - Prefer one narrow invalidation path per case over layered special gates.

## Completion Criteria

- New hover and visible-layout-row benchmarks are checked in and pass their
  non-regression gates.
- At least one measured layout or refresh cost improvement lands without
  regressing render benchmarks.
- The active plan is folded into `plans/README.md` and renamed out of
  `active-` when complete.
