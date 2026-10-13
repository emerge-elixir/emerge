# Large Borders screenshot cache follow-up

## Remaining benchmark correction

`emerge_demo_showcase_borders/screenshot_1909x2148_scale_1_5/cache_steady_hits`
still excludes intentional direct drawing of three changing shadow runs. At the
current default 64-payload count budget, the steady-state failure is:

```text
54 cached hits + 66 cheap direct runs + 3 animated direct runs = 123 candidates
```

Misses, stores and budget rejections are zero, but the assertion counts only
`hits + bypassed_low_value` (120). Raising the budget or freezing animation is
not a correction. Explicit smaller budgets also need bounded budget-aware warm-up.

1. Warm until bounded cache convergence, then check a complete steady animation
   cycle with zero misses/stores/budget rejects/evictions.
2. Derive the changing shadow-run set from actual states, not hardcoded slot IDs
   or every rejection counter. Current diagnostics identify stacked, orbiting and
   counter-rotating outer shadows under the same ScrollContent layer.
3. Use setup-only `render_profiled` / `RenderPaintRunDrawProfile` identity, slot,
   outcome and primitive summaries to require admission fallback only for that
   expected changing set. Static runs must hit or meet cheap-direct policy.
4. Assert visible temporal animation and static coverage; add frozen and
   pause-after-animation controls separately. Preserve stability probation and
   bounded replacement rather than changing production policy to pass a benchmark.

Entry points:

- `native/emerge_skia/benches/renderer.rs`:
  `assert_emerge_demo_showcase_borders_steady_hits`, `steady_paint_layer_coverage`.
- `native/emerge_skia/src/renderer.rs`:
  `moving_layer_store_admission_satisfied`, `render_paint_run_with_cache_tracking`.

## Separate pixel-quality work

Cached/direct pixel differences do not cause the counter assertion. Direct RGBH
output uses LCD text; transparent cached surfaces use grayscale antialiasing.
A diagnostic Unknown-geometry control reduced pixels differing by >2 from 39,892
(max delta 73) to 3,586 (max delta 21) in a 4,100,532-pixel phase. Residual shadow
boundaries/thin edges included 512 pixels differing by >16; their cause remains open.

- Establish an explicit text-antialiasing policy before claiming equivalence;
  do not silently change output policy merely to satisfy a screenshot assertion.
- Isolate remaining shadow/thin-edge differences and add regressions separately
  from coverage accounting.
- For a truly direct control, clear `RenderState.has_cacheable_paint_layers`;
  disabling cache config alone still permits automatic scroll-moving payloads.

Use the exclusive performance lock for GPU probes and record immutable source
identity. These are behavior/quality checks, not timing or macOS/DRM qualification.
The [benchmark guide](../bench/README.md#rich-borders-cache-workload) owns the
corrected synthetic workload; historical investigation logs remain in Git history.
