# Borders cache benchmark divergence

## Result

The current failure was introduced at `f08d80b15b2dee9f570a3529527276302ea07061`
(**Add Vulkan rendering and deterministic video composition**, 2026-08-14).
Its parent `e6bb0c88a3` passes the original, unmodified
`rich_borders_showcase/cache_steady_hits` benchmark; that commit fails with
18 misses, 16 stores and 2 payload-budget rejections on the third frame.
The same failure persists at `d10d294` (universal gradients).

Two benchmark assumptions needed repair. The investigation phase left code
unchanged; the correction described below now fixes the original benchmark
without changing production renderer code or cache limits.

## 1. Payload granularity changed, warm-up did not

`f08d80b` replaced separate own-node/child collections with ordered semantic
composition and `RenderPaintRun`s. Each run has its own slot and payload key.
Clip, shadow-pass, transform and alpha boundaries preserve composition order;
the renderer caches runs rather than the earlier coarse layer payloads.

Relevant current code:

- `native/emerge_skia/src/render_scene.rs`: `build_composition_layer_content`,
  `append_composition_layer_node`, `assign_layer_run_metadata`.
- `native/emerge_skia/src/renderer.rs`: `render_paint_run_with_cache_tracking`,
  `PaintLayerMovingPayloadKey`, `RENDERER_CACHE_DEFAULT_NEW_PAYLOADS_PER_FRAME`.
- `native/emerge_skia/benches/renderer.rs`:
  `assert_rich_borders_showcase_cache_hits`.

The renderer budget remained **16 new payloads per frame**. The benchmark still
renders two warm-up frames and asserts at most two misses/stores on frame three.
That assertion has not changed since its introduction in `f58d013`.

Default-budget trace at `d10d294`, with the benchmark's original selected viewport:

| Frame (one-based) | Hits | Misses | Stores | Budget rejections | Entries |
| --- | ---: | ---: | ---: | ---: | ---: |
| 1 | 0 | 50 | 16 | 34 | 16 |
| 2 | 16 | 34 | 16 | 18 | 32 |
| 3 — assertion | 32 | 18 | 16 | 2 | 48 |
| 4 | 48 | 2 | 2 | 0 | 50 |
| 5–24 | 50 | 0 | 0 | 0 | 50 |

All 50 initial missed keys belong to node `49000`, role `ScrollContent`, with
distinct run slots. Later miss sets are strictly shrinking subsets of those
initial keys: 50 → 34 → 18 → 2 → 0. No new missed keys, evictions, stale evictions,
preparation failures or post-warm-up churn appear over three eight-state cycles.
A frozen-state control gives the same counters. A 64-payload diagnostic budget
stores all 50 on frame one and has zero misses thereafter.

Historical controls:

| Revision | Default viewport scroll Y | Cold stores | Frame-three misses |
| --- | ---: | ---: | ---: |
| `f58d013` — original assertion | 5032 | 5 | 0 |
| `e6bb0c8` — immediate parent | 1784 | 3 | 0 |
| `f08d80b` — composition change | 5032 | 16 | 18 |
| `d10d294` — current | 5008 | 16 | 18 |

Holding scroll Y at 1784 also isolates granularity from viewport selection:
the parent stores three payloads on the first frame, while `f08d80b` initially
misses 52 run payloads and requires four frames to finish filling them. Current
code initially misses 54 there. Both post-change versions converge; active
shadow runs subsequently use admission fallback while static payloads hit.

## 2. The viewport selector lost its animation coverage

The benchmark selects scroll position by scoring counts of nodes, primitives,
text, shadows and layers against historical targets. In `f08d80b`, semantic-layer
changes removed `dynamic_layers` from that score and replaced the animation
coverage assertion `summary.dynamic_layers > 0` with
`summary.cacheable_layers > 0`. A cacheable scroll layer does not prove that an
animated element is visible.

The three authored animated cards (`65000`, `66000`, `67000`) occupy document
Y **2537–2631**. The parent chooses viewport **1784–2684**, which includes them.
The changed benchmark chooses **5032–5932** (`5008–5908` currently), well below
the animated cards.

GPU pixel readbacks confirm the coverage loss:

- Parent's selected viewport: eight distinct post-warm-up pixel hashes across
  the eight animation states.
- `f08d80b` and current selected viewports: one post-warm-up pixel hash.
- Force scroll Y 1784 in either post-change revision: eight distinct hashes
  return, and animated/frozen controls differ as expected.

Forced-viewport controls bypassed only the diagnostic copy's historical density
assertions. At 1784 the current summary has 309 nodes / 93 primitives / 33 texts,
so it cannot satisfy the existing 500-node / 150-primitive / 100-text guard.
This is further evidence that the old structural-count heuristic needs redesign,
not a new arbitrary scroll offset. The source fixture's animated card frames
remain at the same coordinates across the adjacent good/bad commits.

Pixel hashes here establish temporal variation after warm-up, not exact
cached-versus-direct pixel equivalence. Cold/warming frames can have different
hashes as the mix of direct and cached rendering changes; that difference was
not quantified by this investigation.

## Implemented correction

The benchmark now centers its viewport on the three animated cards, validates
that their shadow-sampling regions and a static recipe are visible, and removes
the topology-scoring search and historical scene-count thresholds. The selected
scroll Y is 2134 on the current fixture, computed rather than hardcoded.

Setup checks pixel variation around every card and unchanged, nonblank static
recipe detail using both direct and cached rendering. The direct control
explicitly clears `RenderState.has_cacheable_paint_layers`: merely disabling the
cache config does not disable automatic scroll-moving payload tracking. Readbacks
and controls run outside Criterion timing.

A finite warm-up bound allows every state's initial visible working set to fill
at the configured payload budget, plus two cycles for admission/convergence. A
complete consecutive eight-state cycle must have real hits, at most two
misses/stores, and zero budget rejections, evictions, stale evictions or prepare
failures. Another complete cycle receives the same assertions and pixel checks.
Timing continues from the next state and includes GPU completion. No production
cache budget or steady-state threshold was relaxed.

`tests/borders_cache_benchmark.rs` adds CPU regressions for geometry, actual raster
pixels, small budgets, convergence deadlines, cycle-boundary churn, real hits,
frozen coverage and static detail. The corrected GPU case passes its smoke and
short isolated timing run. The timing is not comparable to the former static
viewport / GPU-enqueue-only workload; it is not a no-regression claim.

## Separate remaining screenshot assertion

Running the full renderer smoke suite now passes the original case and the
ordinary external demo Borders case, then fails
`emerge_demo_showcase_borders/screenshot_1909x2148_scale_1_5/cache_steady_hits`.
An independently rebuilt archive of unmodified `d10d294` reproduces it exactly:
123 visible candidates, 66 low-value bypasses, 29 hits, 25 misses, 16 stores,
9 payload-budget rejections and 3 admission rejections. The assertion expects
hit/bypass coverage of all 123 candidates and zero misses/stores after two
warm-up frames. This screenshot case was not modified by the correction and
requires separate qualification rather than silently weakening its assertions.
Baseline evidence: `/tmp/borders-screenshot-baseline.log`.

## Reproduction and evidence

Original benchmark command, from the chosen source checkout:

```bash
scripts/performance-lock.sh exclusive \
  cargo bench --manifest-path native/emerge_skia/Cargo.toml \
  --bench renderer --features bench-diagnostics -- \
  rich_borders_showcase/cache_steady_hits --test
```

Historical builds used isolated Git archives, an exclusive lock, and explicit
package release cleans/rebuilds before each source switch. Dependencies and
fixtures came from their corresponding source versions. The old local
VideoInterop overrides were supplied from read-only Git archives:
`fc53b27` for the parent and `3c1e123` for `f08d80b`; no external working tree was
modified.

Diagnostics replaced the assertion helper only in temporary archives, rendering
24 frames under `(budget=16, animated)`, `(16, frozen)` and `(64, animated)`.
Existing `EMERGE_BENCH_DIAGNOSTICS=1` supplied run keys. A second probe read back
GPU pixels and forced scroll positions. Finally, the original benchmark source
was restored in both adjacent historical archives: parent passes, child fails.
These are behavior probes, not timing measurements.

Local evidence (ephemeral):

- `/tmp/borders-e6bb0c8-unmodified.log`, `/tmp/borders-f08d80b-unmodified.log`.
- `/tmp/borders-current-trace.log`: complete per-run key trace.
- `/tmp/borders-{e6bb0c8,f08d80b,d10d294}-pixels.log` and corresponding
  `-scroll1784.log` / `-scroll5008.log`: pixel and fixed-viewport controls.
- `/tmp/borders-probe.py`, `/tmp/borders-probe-more.py`,
  `/tmp/borders-analyze.py`: diagnostic preparation and analysis.
