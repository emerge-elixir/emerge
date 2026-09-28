# Large Borders screenshot cache investigation

Investigated after committing the synthetic borders benchmark fix as `641d355`.
Neither the screenshot benchmark nor production renderer was changed during that
investigation.

Subsequent user-directed policy change: the default count budget is now **64**,
with byte limits and admission policy unchanged. Measurements below describe the
pinned 16-payload baseline and explicit 64-payload control, except for the follow-up
smoke below. The screenshot coverage assertion remains unchanged.

## Conclusion

`emerge_demo_showcase_borders/screenshot_1909x2148_scale_1_5/cache_steady_hits`
has **two stale expectations**, not an endlessly missing cache:

1. Two warm-up frames cannot populate 57 payloads at the default budget of 16.
2. `hits + bypassed_low_value >= visible_candidates` excludes deliberate direct
   drawing of three continuously changing shadow runs. That inequality still
   fails after all cold misses and stores have stopped.

Unlike the earlier synthetic case, this selected viewport really does contain
visible animation. After warm-up, its eight animation states have eight distinct
GPU pixel hashes; a frozen-state control has one.

## Current behavior

Default-budget animated trace at committed source `641d355`:

| Frame (one-based) | Hits | Misses | Stores | Budget rejects | Admission rejects | Entries |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| 1 | 0 | 57 | 16 | 41 | 0 | 16 |
| 2 | 16 | 41 | 16 | 25 | 0 | 32 |
| 3 — assertion fails | 29 | 25 | 16 | 9 | 3 | 48 |
| 4 | 45 | 9 | 9 | 0 | 3 | 57 |
| 5 onward, usual phase | 54 | 0 | 0 | 0 | 3 | 57 |
| Revisited cached shadow phase | 57 | 0 | 0 | 0 | 0 | 57 |

Every frame has 123 visible candidates and 66 low-value bypasses. During normal
steady animation the accounting is:

```text
54 cached hits + 66 cheap direct runs + 3 animated direct runs = 123
```

The assertion counts only the first two terms: **120 < 123**. There are no
post-warm-up misses, stores, evictions, stale evictions or preparation failures
across the 32-frame animated probe. The intermittently fully cached phase depends
on which pose first obtained a cache entry; it is state 1 with budget 16 and
state 0 with budget 64. This is reuse of an identical pose, not stale-image reuse.

Increasing the diagnostic budget to 64 fills all 57 entries on the first frame,
but the three animated admission rejections persist on the other poses.
Freezing state 0 from startup converges to 57 hits plus 66 bypasses and satisfies
the original coverage inequality. Neither increasing the production budget nor
freezing the workload is an appropriate fix.

## Identity and admission policy

All persistent admission rejections in the current fixture are:

| Semantic layer | Run slot | Payload |
| --- | ---: | --- |
| node 33, `ScrollContent` | 44 | Two stacked outer shadows |
| node 33, `ScrollContent` | 48 | Orbiting outer shadow |
| node 33, `ScrollContent` | 52 | Counter-rotating soft outer shadow |

The shadow offsets/content hashes change across the eight authored states.
`render_paint_run_with_cache_tracking` records `AdmissionThreshold` and immediately
calls `render_paint_run_direct` for them. They are rendered, not dropped.

`moving_layer_store_admission_satisfied` requires **30 consecutive observations
of the same key** before replacing a known GPU run-family payload, with at most
**one GPU replacement per frame**. This intentionally prevents rerasterization
and eviction on every animation sample. Existing native tests cover this policy.

A separate control ran an animated cycle and then held state 7. After its 30th
consecutive observation, one shadow replacement was admitted per frame for three
frames. Those frames each recorded one store and one replacement eviction. The
cache then settled at 57 hits, zero misses/stores/rejections, and unchanged entry
count. This confirms deliberate probation and bounded replacement, not permanent
admission failure or runaway allocation.

Relevant code:

- `native/emerge_skia/src/renderer.rs`:
  `GPU_PAINT_LAYER_REPLACEMENT_MIN_VISIBLE_FRAMES`,
  `GPU_PAINT_LAYER_REPLACEMENT_STORES_PER_FRAME`,
  `moving_layer_store_admission_satisfied`, `render_paint_run_with_cache_tracking`.
- `native/emerge_skia/benches/renderer.rs`:
  `assert_emerge_demo_showcase_borders_steady_hits`,
  `steady_paint_layer_coverage` (hits plus low-value bypasses only).

## Historical boundary

Unmodified benchmarks were built from isolated Git archives, with explicit
package release cleans/recompilation between revisions:

| Revision | Screenshot result |
| --- | --- |
| `f58d013` — benchmark introduced | Pass |
| `2d452d8` — combined refresh/moving payloads | Pass |
| `c9a44d9` — immediate parent of first reproduced bad commit | Pass |
| `9c3364f` — broader dynamic-layer admission | Fail: 11 hits + 3 bypasses < 16 candidates; 2 admission rejections |
| `68af5c7` — exact dynamic-content generations | Fail: 10 hits + 3 bypasses < 16; 3 admission rejections |
| `f08d80b` — ordered runs and GPU replacement probation | Fail: also exceeds the two-frame warm-up assumption |
| `d10d294`, `641d355` | Same current failure |

The adjacent good/bad boundary is therefore **`9c3364f`**, not the later gradient
or synthetic-benchmark commits. Although titled “Harden generic DMA-BUF imports
and DRM diagnostics,” it also made dynamic layers payload candidates, with
stability-gated direct fallback. The benchmark's denominator expanded while its
coverage formula did not. `f08d80b` subsequently added the separate cold-fill
problem through finer run granularity.

## Separate pixel-equivalence observation

Pixel readback also found cached/direct differences; these do **not** cause the
counter assertion, which performs no pixel comparison. The direct control cleared
`RenderState.has_cacheable_paint_layers`, because disabling cache config alone
still permits automatic scroll-moving payload tracking.

For one warmed animated phase (4,100,532 pixels):

| Text-surface control | Pixels differing by >2 in any channel | Maximum channel delta |
| --- | ---: | ---: |
| Normal RGBH output surface | 39,892 | 73 |
| Diagnostic Unknown pixel geometry | 3,586 | 21 |

The output remains RGBA in both controls; Unknown geometry changes text
antialiasing, not the pixel format. Direct output uses RGBH/LCD text while the
transparent cached payload surfaces do not carry those panel properties and use
grayscale text antialiasing. Matching the direct surface's geometry removes most
large differences. Remaining differences localize mainly to shadow boundaries
and thin edges; 512 pixels still differ by more than 16 in the controlled phase.
Their exact cause was not isolated, so do not claim cached/direct pixel equality
or silently change text-rendering policy to satisfy a new screenshot assertion.

## Recommended scoped correction — not implemented

1. Use bounded, budget-aware warm-up, then validate a complete steady animation
   cycle with **zero misses/stores/budget rejects/evictions**.
2. Derive the expected changing shadow-run set from the actual states. Do not
   hardcode today's slots or blindly add every rejection counter to coverage.
3. Use setup-only `render_profiled` data (`RenderPaintRunDrawProfile` contains layer
   identity, slot, outcome and primitive summary) to require that admission
   fallback occurs only for the expected changing shadow runs. All static runs
   must still hit or meet the existing cheap-direct policy.
4. Assert visible temporal animation and static coverage. Add frozen and
   pause-after-animation controls as distinct cases rather than freezing this
   benchmark to make it pass.
5. Track exact cached/direct pixel-equivalence qualification separately, with
   explicit text-antialiasing policy and thin-edge regressions.

## Follow-up: default count budget increased to 64

The Elixir and native renderer defaults now match the payload-cache primitive's
existing 64-payload default. Total bytes (640 MiB), per-entry bytes (256 MiB),
entry capacity (512), stability probation and replacement throttling are unchanged.
Explicit smaller count-budget overrides remain supported.

A native regression admits 64 small payloads in each of two consecutive frames,
rejects the 65th in each frame, checks retained capacity/byte defaults and rejects
an oversized entry. Elixir coverage checks the new default and explicit overrides.

Validation:

- Rust: 1054 unit tests, 1 fixture test and 13 benchmark integration tests passed.
  The first run hit `slot_acquire_fence_closes_exactly_once_before_reuse`'s closed-fd
  assertion in unchanged code. Its immediate isolated rerun and the full parallel
  suite passed; no unrelated test was edited.
- Elixir: 484 passed, 8 excluded. Formatting, Clippy (`--benches --tests
  --features bench-diagnostics -- -D warnings`) and diff checks passed.
- Exclusively locked synthetic borders GPU smoke passed with the new default.
- The unchanged screenshot smoke still fails coverage, now with **zero misses,
  stores and payload-budget rejections**, 54 hits, 66 bypasses and 3 intentional
  admission rejections (scroll Y 984, scale 1.5). This confirms removal of the
  cold-fill obstruction without masking the separate coverage accounting issue.

The immutable source identity is recorded in `/tmp/cache-count-budget-source-id`
as `641d355` plus the SHA-256 of `/tmp/cache-count-budget.patch` (captured before
these validation notes):
`d14a146ee6fc8fa209eb3734ab250ea7dfb283709f7f8846ce74d9d5c8380bf7`.
Logs use the `/tmp/cache-count-budget-*.log` prefix. These are correctness smokes,
not timing or constrained-device qualification.

## Evidence and limits

GPU probes used the shared exclusive performance lock and the same local Mesa
OpenGL device as the earlier investigation. Historical native libraries were
explicitly recompiled; fixture versions and dependencies came from each archive.
No external application or VideoInterop working tree was modified. These are
behavior/quality probes, not comparative performance measurements or macOS/DRM
qualification.

Local evidence is ephemeral:

- `/tmp/screenshot-current-trace.log`: 32-frame animated/frozen/budget controls,
  exact rejected keys and shadow payloads.
- `/tmp/screenshot-hold-trace.log`: animated-then-held state and 30-frame probation.
- `/tmp/screenshot-{c9a44d9,9c3364f,68af5c7,f08d80b}-unmodified.log`: historical
  assertions without diagnostic modifications.
- `/tmp/screenshot-pixels-{normal,gray}.log`: cached/direct pixel comparisons.
- `/tmp/screenshot-probe.py` and `/tmp/emerge-screenshot-641d355720/`: diagnostic
  source, including temporary renderer admission logging and text-geometry switch.
