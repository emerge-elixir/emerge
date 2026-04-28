# Active Scroll Viewport Culling Plan

Last updated: 2026-04-29.

Status: active. Benchmark baseline and the first shared render/registry
viewport participation gate are implemented. Remaining work is focused
exception auditing and live-demo validation.

## Purpose

Large scroll containers can contain far more retained content than is visible.
Even when offscreen children produce no draw primitives, Emerge can still spend
CPU walking those children, building scene context, checking cache eligibility,
building cache keys, rebuilding event metadata, or carrying offscreen subtree
state through refresh.

The target improvement is not another renderer payload cache. The target is
viewport-aware retained traversal:

```text
scroll container viewport
-> known child frames and visual bounds
-> skip whole offscreen child subtrees before doing render traversal work
```

## Current Code Facts

Current render code already has a conservative cull:

- `should_cull_render_subtree(...)` checks an element's visual bounds against
  the inherited visible clip
- `render_visual_bounds(...)` accounts for box shadows
- subtrees with nearby mounts are not culled at that point
- scene context carries `visible_clip` and scroll offsets through
  `tree/scene.rs`
- scroll-offset scene contexts bypass render-subtree cache lookup and storage

This means the renderer probably already avoids many offscreen draw primitives.
The likely remaining cost is earlier traversal work: visiting each child to
discover that it is offscreen.

## Hypothesis

The highest-value optimization is to skip entire offscreen scroll children from
the parent scroll container before descending into each child.

Expected wins:

- lower refresh time for large scroll containers
- fewer render-subtree cache lookups/key builds
- fewer render nodes allocated for offscreen content
- fewer renderer cache candidates considered for offscreen content
- less event-registry rebuild work if later extended to registry traversal

Expected non-wins:

- draw time may not change much if draw primitives were already culled
- layout time may not change initially because layout still needs content size
- small scroll containers may not improve enough to justify extra branch work

## Non-Goals

- no virtualized layout or item recycling in this slice
- no change to scroll content measurement
- no event-registry culling until render-refresh culling is benchmarked
- no cache broadening
- no behavior change for nearby overlays, focused text input, videos, or active
  animations without targeted tests
- no optimization stays in production if benchmarks do not show a win

## Benchmark-First Rule

Before implementation, add benchmarks and diagnostics that answer:

1. How much time is spent in refresh/render-scene construction for large scroll
   containers today?
2. How many retained nodes are visited to produce one visible scroll frame?
3. How many subtrees are already culled by `should_cull_render_subtree`?
4. How many offscreen child subtrees could be skipped earlier at the scroll
   container boundary?
5. Does current renderer draw time already stay low, confirming traversal is
   the bottleneck?
6. Does event registry rebuild become the next cost after render traversal is
   fixed?

The first code change should therefore be benchmark and diagnostic support, not
the culling optimization.

## Implemented Benchmark Baseline

Implemented in:

- `native/emerge_skia/Cargo.toml`
- `native/emerge_skia/src/tree/render.rs`
- `native/emerge_skia/src/tree/layout.rs`
- `native/emerge_skia/benches/support/mod.rs`
- `native/emerge_skia/benches/layout.rs`

Added:

- `bench-diagnostics` feature for benchmark-only render traversal counters
- render traversal diagnostics:
  - element visits
  - existing element-level culled subtrees
  - render-subtree cache lookup key builds
- benchmark-only render-scene refresh helpers to separate render traversal from
  full refresh/event-registry work
- `native/scroll_viewport_culling` Criterion group
- 2,000-row simple and paint-rich scroll viewport scenarios
- top, middle, scroll-step, cached, uncached, and render-only lanes

Short baseline command used:

```bash
cargo bench \
  --manifest-path native/emerge_skia/Cargo.toml \
  --features bench-diagnostics \
  --bench layout \
  -- scroll_viewport_culling \
  --warm-up-time 0.1 \
  --measurement-time 0.2 \
  --sample-size 10 \
  --save-baseline scroll_viewport_culling_before
```

Important caveat: this was a short local baseline intended to guide the next
implementation slice. Use a longer run before accepting final before/after
numbers.

### Baseline Results

Simple rows:

```text
top diagnostic:
  retained_nodes=4002
  traversal_visits=2020
  culled_subtrees=1982
  scene=nodes=130 primitives=38 texts=18

middle diagnostic:
  retained_nodes=4002
  traversal_visits=2021
  culled_subtrees=1982
  scene=nodes=96 primitives=39 texts=18

top_cached_refresh:              ~18.3 us
middle_cached_refresh:           ~291 us
middle_uncached_refresh:         ~288 us
middle_render_only_cached:       ~296 us
middle_render_only_uncached:     ~299 us
scroll_step_cached_refresh:      ~2.70 ms
scroll_step_render_only_cached:  ~293 us
scroll_step_render_only_uncached:~291 us
scroll_step_uncached_refresh:    ~2.73 ms
```

Paint-rich rows:

```text
top diagnostic:
  retained_nodes=10002
  traversal_visits=2034
  culled_subtrees=1992
  scene=nodes=156 primitives=50 texts=24 shadows=8 gradients=8

middle diagnostic:
  retained_nodes=10002
  traversal_visits=2032
  culled_subtrees=1993
  scene=nodes=133 primitives=48 texts=22 shadows=8 gradients=8

top_cached_refresh:              ~1.07 ms
middle_cached_refresh:           ~377 us
middle_uncached_refresh:         ~367 us
middle_render_only_cached:       ~367 us
middle_render_only_uncached:     ~348 us
scroll_step_cached_refresh:      ~7.15 ms
scroll_step_render_only_cached:  ~354 us
scroll_step_render_only_uncached:~351 us
scroll_step_uncached_refresh:    ~7.04 ms
```

### Baseline Interpretation

The current element-level render cull already skips most offscreen descendants:

- simple middle viewport emits only 18 text rows but still visits about 2,021
  retained elements
- paint-rich middle viewport emits about 22 text rows but still visits about
  2,032 retained elements
- the extra visits are mostly offscreen row roots being entered only to discover
  that they are outside the inherited clip

This confirms the first render-side optimization target: skip offscreen scroll
children before descending into each child root.

The render-only lanes also show that full scroll-step refresh is not primarily
Skia scene construction:

- simple scroll-step render-only is about 0.29 ms, while full scroll-step refresh
  is about 2.7 ms
- paint-rich scroll-step render-only is about 0.35 ms, while full scroll-step
  refresh is about 7.1 ms

That means early render traversal skipping is still worthwhile, but event
registry traversal/rebuild is likely a separate larger scroll-step cost and
should remain an explicit follow-up slice rather than being mixed into the
render culling change.

### Validation Run

Completed for the benchmark/diagnostic slice:

```bash
cargo test --manifest-path native/emerge_skia/Cargo.toml
cargo test --manifest-path native/emerge_skia/Cargo.toml --features bench-diagnostics
cargo clippy --manifest-path native/emerge_skia/Cargo.toml --features bench-diagnostics --benches -- -D warnings
mix test
```

## Benchmark Design

### Native Criterion Benchmarks

Implemented a focused group under `native/emerge_skia/benches/layout.rs`, because the
work being measured is retained layout/refresh and render-scene construction,
not Skia primitive drawing.

Suggested group:

```text
native/scroll_viewport_culling
```

Cases:

- `large_column_simple_rows`
  - 1,000 to 5,000 rows
  - fixed-height rows
  - simple rect/text content
  - only 10 to 30 rows visible
- `large_column_paint_rich_rows`
  - shadows, borders, gradients, text, and images where practical
  - tests visual bounds and expensive render-node construction
- `large_column_interactive_rows`
  - event handlers on rows
  - establishes whether render-only culling leaves registry as a later cost
- `large_column_nearby_rows`
  - some rows have nearby overlays
  - verifies culling keeps conservative behavior around overlay escape paths
- `large_column_transformed_rows`
  - selected rows use translate/scale/rotate where supported by render bounds
  - catches incorrect culling around transformed visual bounds

Per case, benchmark:

- warm `refresh_reusing_clean_registry_for_benchmark`
- uncached refresh baseline
- scroll step refresh at top, middle, and bottom
- no-scroll warm refresh as a broad-regression guard

### Metrics Captured So Far

The benchmark reports enough counters to separate the first costs:

- total retained nodes in tree
- render traversal visits
- render subtrees culled by existing element-level cull
- render-subtree cache lookup key builds
- emitted render-scene node count and primitive count
- refresh duration

Still useful later:

- estimated nodes below culled subtrees
- visible row count
- render-subtree cache hit/miss/store counters
- registry rebuild duration or traversal counters

### Live Demo Trace

After the native benchmark exists, run a live trace in `../emerge_demo` or a
temporary demo page with a large scroll container.

Useful stats/log fields:

- `refresh`
- `render draw`
- renderer cache per-frame candidates/hits
- scene summary node and primitive counts in slow-frame logs
- new traversal/cull counters if exposed through stats

The live trace is a guard against optimizing a synthetic benchmark that does not
match real page structure.

## Implemented Shared Gate

The benchmark baseline is now in place and Slice 2 has landed as a shared
viewport participation helper in `tree/viewport_culling.rs`.

Model:

```text
if subtree cannot affect the effective scroll viewport
and subtree has no escape/focus/runtime exception
then it does not participate in scene or pointer-registry traversal
```

Traversal classes:

- visible/participating subtree:
  - render scene traversal
  - pointer/mouse hit registry traversal
  - hover, mouse, press, swipe, scroll hit regions
- retained independent state:
  - keyboard focus state
  - tab/focus order data if it is stored outside hit regions
  - text-input session state
  - focus-on-mount requests
  - active pointer/drag runtime followups

Expected code shape:

1. A shared helper uses the current scene visible clip plus child frame,
   transform, and conservative visual bounds.
2. Render traversal calls the helper before descending into retained child
   subtrees.
3. Registry traversal calls the same helper before descending into retained
   child subtrees.
4. Registry culling keeps explicit exceptions for retained keyboard/focus/text
   state: text inputs, focused nodes, focus-on-mount, key bindings, virtual
   keys, active press state, and scrollbar-hover runtime.
5. Pointer-only offscreen subtrees do not participate in hit-region rebuilds.
6. The existing element-level render cull remains as fallback for non-child
   paths and any subtree reached through conservative exception handling.
7. Render-subtree cache, renderer payload cache, and layout caches stay
   independent from this early skip.

Implemented files:

- `native/emerge_skia/src/tree/viewport_culling.rs`
- `native/emerge_skia/src/tree/render.rs`
- `native/emerge_skia/src/events/registry_builder.rs`
- `native/emerge_skia/src/tree/mod.rs`

## Slice 2 Benchmark Results

Command used for before/after comparison:

```bash
cargo bench \
  --manifest-path native/emerge_skia/Cargo.toml \
  --features bench-diagnostics \
  --bench layout \
  -- scroll_viewport_culling \
  --warm-up-time 0.1 \
  --measurement-time 0.2 \
  --sample-size 10 \
  --baseline scroll_viewport_culling_before
```

Saved after baseline:

```bash
cargo bench \
  --manifest-path native/emerge_skia/Cargo.toml \
  --features bench-diagnostics \
  --bench layout \
  -- scroll_viewport_culling \
  --warm-up-time 0.1 \
  --measurement-time 0.2 \
  --sample-size 10 \
  --save-baseline scroll_viewport_culling_after
```

Diagnostics after implementation:

```text
simple top:    traversal_visits=38  culled_subtrees=1982
simple middle: traversal_visits=39  culled_subtrees=1982
paint top:     traversal_visits=42  culled_subtrees=1992
paint middle:  traversal_visits=39  culled_subtrees=1993
```

The important change is that visible scenes still emit the same visible draw
shape, but render traversal no longer enters about 2,000 offscreen row roots.

Short before/after timings:

```text
simple top_cached_refresh:
  before ~18.3 us, after ~18.5-18.9 us
  note: tiny absolute regression/noise on a very cheap no-scroll case

simple middle_cached_refresh:
  before ~291 us, after ~257-264 us

simple scroll_step_cached_refresh:
  before ~2.70 ms, after ~0.45 ms

simple scroll_step_uncached_refresh:
  before ~2.73 ms, after ~0.45 ms

paint top_cached_refresh:
  before ~1.07 ms, after ~0.24-0.25 ms

paint middle_cached_refresh:
  before ~377 us, after ~350-364 us

paint scroll_step_cached_refresh:
  before ~7.15 ms, after ~0.63-0.64 ms

paint scroll_step_uncached_refresh:
  before ~7.04 ms, after ~0.63 ms
```

Decision:

- Keep the implementation: the target scroll-step workloads improved by about
  83% for simple rows and about 91% for paint-rich rows.
- Track the tiny simple top-of-list no-scroll regression as a follow-up risk.
  It is sub-microsecond absolute cost in the short local benchmark, while the
  real target path improved by milliseconds.

## Correctness Requirements

The optimization must not hide visible output or interaction.

Must preserve:

- shadows that extend into the viewport even when the element frame is outside
- transforms that move content into the viewport
- nearby/overlay content that escapes the scrolled host
- scrollbars and scroll content size
- focused text input caret and composition state
- focused text input session state even if the focused input scrolls offscreen
- tab/focus order semantics that are intentionally independent of pointer hit
  regions
- focus-on-mount behavior for offscreen nodes
- active pointer/drag runtime followups after an interaction starts visible
- hover/press state correctness for visible elements
- offscreen pointer/mouse hit regions must not remain interactive
- video and image loading behavior for visible content

Conservative fallback is acceptable. If a subtree is hard to prove invisible,
render it normally.

## Slice 1: Benchmark And Diagnostic Baseline

Status: implemented.

Goal:

- prove where time is spent before adding culling behavior

Work:

- add `native/scroll_viewport_culling` Criterion group
- add one simple large scroll-column case and one paint-rich case
- record scene summary and traversal/cull counters for each refresh
- save a baseline before optimization
- document baseline results in this plan

Validation:

- `cargo test --manifest-path native/emerge_skia/Cargo.toml`
- `mix test`
- focused benchmark run:

```bash
cargo bench \
  --manifest-path native/emerge_skia/Cargo.toml \
  --features bench-diagnostics \
  --bench layout \
  -- scroll_viewport_culling \
  --save-baseline scroll_viewport_culling_before
```

Decision gate:

- passed for render traversal: diagnostics show render traversal still visits
  about 2,000 row roots to produce about 20 visible rows
- registry/event traversal must be included in the same participation-gate
  implementation path: full scroll-step refresh is much slower than render-only
  scroll-step refresh

## Slice 2: Shared Viewport Participation Gate

Status: implemented.

Goal:

- skip whole offscreen scroll children before descending into descendants for
  both render traversal and pointer-registry traversal

Work:

- added a scroll-container child participation check using already resolved
  frames and conservative visual bounds
- shared the check between render scene traversal and event registry traversal
- bypassed descendants when no nearby/focus/runtime special-case condition
  requires registry traversal
- preserved non-hit-region retained state conservatively through registry
  exceptions:
  - keyboard focus/key bindings
  - text input session
  - focus-on-mount
  - active press/runtime state currently represented on tree nodes
  - tab/focus order while it still depends on registry rebuild focus entries
- kept current element-level render cull as fallback
- compared against `scroll_viewport_culling_before`

Benchmark gate:

- passed: render-only large-scroll refresh improves materially
- passed: full scroll-step refresh improves materially because registry
  traversal now uses the same gate
- watch: simple top no-scroll refresh shows a sub-microsecond short-benchmark
  regression/noise
- passed by existing render tests and unchanged scene summaries: paint-rich
  visible output is preserved
- added direct render traversal regression coverage for offscreen row-root skips
- pending focused tests: offscreen pointer hit regions and focus/text-input
  exception behavior should get direct regression coverage in Slice 3

## Slice 3: Registry/Focus Exception Audit

Status: next.

Goal:

- audit the remaining event/focus paths after the shared gate lands

Work:

- verify whether tab/focus order is already independent from pointer hit-region
  traversal
- verify focused offscreen text input keeps required state without offscreen
  pointer hit regions
- verify active pointer/drag runtime followups do not depend on rebuilding
  offscreen hit regions after capture starts
- add focused tests for any exception path that was not covered in Slice 2
- investigate whether retained registry exceptions need cached subtree summary
  bits instead of recursive exception scans
- decide whether the simple top no-scroll sub-microsecond regression is noise,
  acceptable fixed cost, or worth a narrower gate

Correctness gate:

- offscreen elements must not receive pointer hits
- keyboard focus behavior must remain explicit and predictable
- focused offscreen text input must not lose required runtime state

## Slice 4: Live Demo Validation

Status: pending implementation.

Goal:

- confirm the optimization improves real pages, not only synthetic rows

Work:

- run `../emerge_demo` large scroll scenario or add a temporary demo page
- compare refresh timing, scene summaries, renderer cache per-frame rates, and
  frame smoothness
- record before/after logs in this plan

Decision gate:

- if live traces do not improve, either revert the optimization or narrow it to
  only the benchmarked case where it is still clearly useful

## Open Questions

- Should cull counters live in renderer stats permanently, or only behind a
  diagnostic flag?
- Do we need a cached conservative visual bounds value per subtree to avoid
  walking descendants for shadow/transform bounds?
- Can subtree measurement/resolve caches expose enough bounds information to
  classify offscreen children without additional per-node state?
- Which focus-order data, if any, still depends on event registry traversal
  rather than independent retained focus state?

## Current Recommendation

Continue with Slice 3 before broadening the gate. The core optimization is
worth keeping, but the focus/text-input exception model should be covered by
targeted tests and audited before adding more aggressive subtree summary state.
