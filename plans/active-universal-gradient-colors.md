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

## Known independent benchmark failure

`rich_borders_showcase/cache_steady_hits` still fails its cache-budget assertion:
18 misses, 16 stores, 32 hits and 2 payload-budget rejections during warmup. The
same failure and counters were reproduced on a clean pre-gradient baseline.
Do not weaken that assertion or attribute it to gradients without new evidence.
General renderer/cache qualification should resolve it separately.

## Constraints

- Preserve unrelated worktree changes and do not modify external applications
  without authorization.
- Keep `Background.gradient/2,3`, legacy raw tuples and retired wire variants
  removed. No production compatibility decoder.
- Keep all public color consumers supported; do not solve a platform/performance
  issue by silently substituting a gradient endpoint or restricting it to a
  background again.
