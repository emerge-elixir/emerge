# Universal gradient color qualification

Implementation is complete. The background-only restriction is removed across
all UI color consumers; the SVG validation bypass is fixed. The durable design
and verification map are in
[`guides/internals/gradient-colors.md`](../guides/internals/gradient-colors.md).
Public migration instructions are in
[`guides/migrations/0.4.md`](../guides/migrations/0.4.md).

This file retains only open qualification work, not an implementation log.

## Remaining gates

- [ ] On macOS, build the v13 host and run Metal/raster rendering and lifecycle
  tests with EMRG v9. Linux protocol tests cover acceptance/rejection, but are not
  a replacement for running the actual host.
- [ ] Qualify constrained-device performance and memory with gradient text,
  borders, shadows and SVGs. Compare pinned solid baselines, cold/warm cache
  behavior, SVG temporary-layer area, and three/many-stop gradients. Local
  raster/GPU Criterion smokes establish execution, not a no-regression timing
  claim. Follow the shared/exclusive locking in `bench/README.md` and record an
  immutable source identity for measurements.

## Independent renderer benchmark qualification

`rich_borders_showcase/cache_steady_hits` is corrected: geometry-selected visible
animation, direct/cached pixel checks and bounded budget-aware warm-up now pass.
The full renderer suite next reaches a separate failure in
`emerge_demo_showcase_borders/screenshot_1909x2148_scale_1_5/cache_steady_hits`.
That assertion predates gradients: `9c3364f` expanded dynamic-layer candidates
without accounting for intentional direct admission fallback; ordered runs later
outgrew the fixed warm-up. It remains unchanged. A correction must validate the
specific changing shadow runs, not count arbitrary rejections as cache coverage.
See [`screenshot-cache-benchmark-investigation.md`](screenshot-cache-benchmark-investigation.md)
for the good/bad boundary, convergence controls and separate pixel-quality findings;
[`borders-cache-benchmark-investigation.md`](borders-cache-benchmark-investigation.md)
records the already-corrected synthetic case.

## Constraints

- Preserve unrelated worktree changes and do not modify external applications
  without authorization.
- Keep `Background.gradient/2,3`, legacy raw tuples and retired wire variants
  removed. No production compatibility decoder.
- Keep all public color consumers supported; do not solve a platform/performance
  issue by silently substituting a gradient endpoint or restricting it to a
  background again.
