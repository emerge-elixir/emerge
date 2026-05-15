# Pending Commit Plan

Last updated: 2026-05-15.

Status: completed.

## Goal

Split the pending work into reviewable commits without mixing fixture setup,
bench infrastructure, core runtime behavior, renderer-local cache policy, and
documentation cleanup.

## Commit Order

1. `bench: add emerge_demo showcase fixtures`
   - Add `bench/generate_external_fixtures.exs`.
   - Add `bench/external_fixtures/emerge_demo_showcase_interaction/`.
   - Include only fixture generation and fixture payloads.

2. `bench: cover layout refresh regressions`
   - Commit `native/emerge_skia/benches/layout.rs`.
   - Commit `native/emerge_skia/benches/renderer.rs`.

3. `feat: retain layout refresh products`
   - Commit frame-attribute, invalidation, patch classification, tree update,
     registry, and retained render-fragment changes.
   - Primary files: `tree/layout.rs`, `tree/invalidation.rs`, `tree/patch.rs`,
     `runtime/tree_actor.rs`, `runtime/tree_update.rs`,
     `events/registry_builder.rs`, `events/runtime.rs`, `tree/render.rs`,
     `render_scene.rs`, `tree/element.rs`, and related tests.

4. `fix: stabilize renderer cache policy`
   - Commit render-cache audit fixes:
     - child paint layers still render through clipped non-cacheable parents.
     - stale payload eviction honors `max_stale_frames`.
     - `min_visible_before_store` gates first-store admission.
   - Commit SVG cover viewport raster/cache behavior and tests.
   - Primary files: `renderer.rs`, `paint_layer_payload_cache.rs`.

5. `docs: document layout refresh and completed plans`
   - Commit `guides/internals/architecture.md`,
     `guides/internals/layout-refresh-render-flow.md`, `plans/README.md`,
     deletion of `plans/active-layout-refresh-optimization.md`, and addition of
     `plans/layout-refresh-optimization.md`.
   - Include `plans/active-render-cache-audit-fixes.md` and this completed
     commit plan.

## Notes

- `native/emerge_skia/src/renderer.rs` and `native/emerge_skia/src/tree/element.rs`
  contained tightly interleaved changes, so the final split kept them at
  file-level boundaries instead of using fragile partial staging.
- `./ci-tests.sh all` passed on the unsplit pending tree before commits.
