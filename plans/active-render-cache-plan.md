# Active Render Cache Plan

Last updated: 2026-04-28.

Status: implemented for the current slice. The renderer-cache production path
now follows the Flutter-shaped lifecycle that fits Emerge: discover candidates
during scene traversal, admit after repeated visibility, prepare a bounded
number of payloads before drawing, prefer GPU-resident payloads on GPU frames,
draw newly prepared images during the same frame when preparation succeeds, and
report cache behavior at the end of the frame.

This plan is intentionally short. Historical benchmark details and rejected
alternatives live in:

- `rendering-cache-engine-investigation.md`
- `drawing-optimization-investigation.md`
- `layout-caching-roadmap.md`

## Completed Summary

Completed renderer-cache foundations:

- saved `render_cache_before` before enabling cache work
- added shared renderer-cache lifecycle and stats under `SceneRenderer`
- added metadata, admission counters, byte limits, per-frame payload budgets,
  eviction, generation clearing, and cache-limit options through
  `EmergeSkia.start/1`
- added clean-subtree candidate emission with stable identity, content
  generation, local bounds, and direct fallback
- added cached-vs-direct parity helpers for the first payload path
- implemented the first CPU-backed clean-subtree payload as a correctness
  scaffold and raster/offscreen fallback
- proved integer `move_x`/`move_y` placement reuse and layout-reflow placement
  reuse without including placement in the content key
- added renderer-cache stats in normal renderer logs

Completed direct-renderer baseline:

- landed unclipped solid-border fast paths
- landed template-image tint without `saveLayer`
- landed narrow single-primitive alpha distribution
- rejected clipped border fast paths, clip combining, Skia shadow utils, and
  warmup behavior when benchmarks did not prove a win

Current gap:

- broader live-demo validation still needs to confirm default budgets across
  longer sessions and resize/interaction traces
- descendant alpha under a cached ancestor remains conservative; root element
  alpha is cache composition state, but nested composited/cache boundaries are a
  future expansion
- rotate, scale, active text editing visuals, active video, and loading/error
  image placeholders remain direct-render paths until each has parity tests and
  benchmark proof

Implemented in this slice:

- GPU frames prepare clean-subtree payloads into offscreen GPU render targets
  and store `GpuRenderTarget` payloads; raster/offscreen frames keep the CPU
  raster payload fallback
- GPU prepare failure now falls back to direct rendering instead of silently
  creating a CPU-backed GPU-window payload
- admitted candidates prepare before drawing and draw the newly prepared payload
  in the same frame when preparation succeeds
- renderer-cache stats distinguish GPU/CPU payload stores, prepare
  success/failure, direct fallback after admission, and rejection reasons
- root element alpha no longer disqualifies a candidate; alpha is applied by
  composition while the cached payload stays keyed by subtree content
- renderer benchmarks now include showcase layout-reflow movement,
  Nerves-style counter translation, app-selector fade, and todo
  translate+fade workloads

Measured GPU Criterion checks from 2026-04-28:

- `gpu_cache_candidates_layout_reflow`: direct reflowed children `1.7046 ms`
  versus cached local content `177.71 us`
- `gpu_cache_candidates_translated/app_selector_menu_alpha`: direct
  `1.1213 ms` versus production cached candidate `396.02 us`
- `gpu_cache_candidates_translated/todo_entry_translate_alpha`: direct
  `929.48 us` versus production cached candidate `389.62 us`
- `gpu_surfaceless/mixed_ui_scene`: no statistically significant change
  (`3.8186 ms`, `+0.5121%`, `p = 0.28`)

## Target Cache Model

The target model is Flutter-shaped but uses Emerge's simpler retained tree and
typed dirtiness.

### Frame Lifecycle

Every renderer frame should follow one explicit cache lifecycle:

1. `begin_frame`
   - reset per-frame cache counters
   - increment frame generation/epoch
   - initialize per-frame prepare budget
2. `mark_candidates`
   - traverse the `RenderScene`
   - mark visible clean-subtree candidates
   - update last-seen frame and visible access count
   - record rejection reasons for ineligible candidates
3. `admit`
   - admit candidates only after repeated visibility and size/resource checks
   - keep lookup separate from admission; a candidate can be seen before it has
     a payload
4. `prepare`
   - prepare at most the configured number of new payloads this frame
   - on GPU backends, prepare into an offscreen GPU render target in the same
     `DirectContext`
   - on raster/offscreen or GPU prepare failure, use CPU raster fallback only
     when that path is explicitly allowed
5. `paint`
   - draw prepared payloads when valid
   - if preparation fails, is over budget, or the key is invalid, direct-render
     the subtree exactly as today
   - a newly prepared payload may be drawn in the same frame, avoiding
     direct-render-then-store double work
6. `end_frame`
   - record stats
   - age entries not seen this frame
   - evict by byte budget, entry budget, and stale-frame policy

This is the part of Flutter to copy: explicit candidate marking, repeated-use
admission, bounded preparation, GPU payloads, cached image draw, and visible
stats. Emerge should not copy Flutter's broad heuristic surface when retained ids
and typed dirtiness give better signals.

### Entry State

Each clean-subtree cache entry should be a small state machine:

```text
Observed
  candidate was visible, but no payload exists

Admitted
  candidate crossed visibility/size/resource gates and may be prepared

PreparedGpu
  GPU render-target payload exists and is valid for the current backend context

PreparedCpu
  CPU raster payload exists for raster/offscreen fallback or correctness tests

Rejected
  candidate is known ineligible this frame, with a reason
```

Required entry fields:

- stable cache id from retained element identity
- content generation
- local physical size and scale
- resource generations for assets/fonts/backend textures where applicable
- payload kind: `gpu_render_target` or `cpu_raster`
- byte estimate
- visible access count
- last seen frame
- last used frame
- last rejection reason, when any

### Keys and Composition State

The payload key represents local subtree pixels. It must include:

- cache kind
- retained subtree id
- content generation
- local physical size
- scale
- resource generations
- payload backend/context generation

The payload key must not include pure composition state:

- integer `move_x` / `move_y` placement
- layout-reflow placement where local subtree content and size are unchanged
- element alpha for caches rooted at that element

Element alpha is general in Emerge: alpha on an element is composition state for
that element's subtree. If a subtree is cached into a transparent payload, the
same payload can be drawn with different alpha values. If a descendant element's
alpha changes inside a cached ancestor, the renderer must either use a nested
composited/cache boundary for that descendant or invalidate the ancestor
payload.

Still conservative until proven by benchmarks and parity:

- fractional placement
- rotate
- scale
- active text input/caret/preedit visuals
- active video
- image loading/failure placeholders
- volatile content that changes subtree pixels every frame

### GPU Payload Model

GPU backends should not cache by copying the already-composited framebuffer.
That captures final output instead of local transparent content and can force
readback/synchronization.

The store frame should route admitted subtree drawing through the cache target:

1. allocate an offscreen GPU render target sized to rounded local physical bounds
2. clear it transparent
3. draw localized candidate children into that target
4. snapshot/store a GPU-resident image
5. composite that image to the main surface with placement and element alpha
6. fall back to direct rendering if any step fails or exceeds budget

CPU raster payloads remain useful as:

- raster backend payloads
- offscreen/headless correctness harnesses
- fallback when GPU payload creation fails, only if benchmarks allow it

### Admission and Budgets

The cache should be harder to create than to hit.

Initial policy:

- require at least two visible frames before preparing a payload
- reject zero/empty/non-finite bounds
- reject entries above `clean_subtree.max_entry_bytes`
- prepare at most `max_new_payloads_per_frame`
- obey `clean_subtree.max_entries` and `clean_subtree.max_bytes`
- evict least-recently-used entries when over budget

Future policy can add complexity/cost scoring, but only after stats show the
simple repeated-visibility rule is not enough.

### Layout-Cache Signal

A full clean resolved-layout cache hit is a strong renderer-cache signal, not a
renderer-cache proof.

Use it to:

- mark clean-subtree candidates as high confidence
- reuse placement changes without content generation changes
- avoid broad heuristics when layout already proved the subtree is clean

Still validate:

- content generation
- local physical size
- scale
- resource generations
- payload kind
- backend/context generation
- residency/eviction state

### Stats

Renderer logs should show cache behavior clearly enough to reject bad
optimizations.

Required clean-subtree stats:

- candidates
- visible candidates
- admitted
- hits
- misses
- stores
- evictions
- rejected
- current entries
- current bytes
- evicted bytes
- payload kind counts
- prepare success/failure counts
- prepare time
- hit draw time
- direct fallback count after admission
- rejection reason counts

Frame timings must be judged together:

- render
- draw
- GPU flush
- submit
- present
- patch-submit to present pipeline time
- cache prepare
- cache hit draw

Draw-only wins do not count if GPU flush, present, or full pipeline regresses.

## Active Implementation Slices

### Slice 1: Benchmark and Stats Gate

Status: completed.

Work:

- add/update renderer benchmarks for:
  - direct renderer
  - current CPU payload lifecycle
  - GPU payload lifecycle
  - admitted prepare-before-draw frame
  - warm hit frame
  - no-candidate mixed-scene guard
- include demo-shaped workloads:
  - Nerves-style `move_x` animated counter
  - vertical and combined integer translation
  - showcase layout-reflow card movement
  - app-selector element-alpha fade
  - todo-entry translate plus element-alpha fade
  - warm loaded assets
  - border/text/clip-heavy mixed subtree
- extend stats with payload kind, prepare result, direct fallback after
  admission, and rejection reason counts if missing

Landing rule:

- no cache implementation may stay enabled unless the benchmark suite proves it
  improves full-frame timing for the target workload and does not regress the
  broad no-candidate guard

### Slice 2: GPU Render-Target Payload

Status: completed.

Work:

- thread `DirectContext`/GPU preparation access into renderer-cache preparation
- create an offscreen GPU render target for admitted clean-subtree candidates
- draw localized children into that target before main-surface composition
- snapshot/store the GPU image
- draw the prepared image to the main canvas in the same frame
- keep CPU raster payloads as raster/offscreen fallback
- clear GPU payloads on video resource reset and explicit cache clear; backend
  reconfigure creates a new renderer when it creates a new GL environment, while
  same-context resize keeps payloads valid

Rejection rule:

- if GPU payloads do not beat the direct renderer on full-frame benchmarks, keep
  the simpler direct path and document the rejected optimization near the
  benchmark/code.

### Slice 3: Prepare-Before-Draw Admission

Status: completed.

Work:

- change admitted/store behavior from direct-render-then-store to
  prepare-before-draw
- if prepare succeeds inside budget, draw the newly prepared payload and skip
  direct subtree rendering
- if prepare fails or is over budget, direct-render exactly as today
- suppress child payload preparation when a prepared parent payload covers the
  same frame, unless the child has an explicit independent reason to exist

Benchmark gate:

- store-frame max time must not become a new slow-frame source
- warm-hit improvement must survive draw, GPU flush, submit, present, and
  pipeline timing

### Slice 4: Element Alpha Composition

Status: completed for root element alpha.

Work:

- model root element alpha as cache-hit draw state for the cached subtree
- keep root element alpha out of the payload key when alpha is applied while
  drawing the cached image
- add root-alpha cached-vs-direct pixel parity and retained-tree candidate
  generation tests
- benchmark app-selector menu fade and todo-entry translate+fade animations
- keep descendant alpha conservative by rejecting nested `Alpha` inside a
  candidate until nested composited/cache boundaries have their own parity tests

Landing rule:

- root element alpha ships because pixel parity passes and full-frame benchmarks
  show a win over direct rendering

### Slice 5: Cleanup and Policy Tightening

Status: completed for this slice; keep budget tuning as live-trace follow-up.

Work:

- remove or disable CPU-backed GPU-window payload usage if GPU payloads are the
  proven production path
- keep CPU fallback only where it is tested and useful
- document any rejected optimization in code comments near the benchmark-gated
  path
- keep existing conservative default cache budgets until longer live traces show
  pressure
- update `plans/README.md` and investigation docs with measured results

## Rules

- Benchmark before implementation.
- Do not land a more complex cache path when benchmarks do not prove a full-frame
  win.
- Keep the current direct renderer as fallback for every cacheable subtree.
- Do not hide first-use asset loading, shader compilation, or presentation
  stalls behind generic cache claims.
- Prefer Emerge's retained ids and typed dirtiness over broad heuristics.
- Keep cache limits configurable through `EmergeSkia.start/1`.
- Do not cache active text editing visuals, active video, or volatile pixel
  content until each has targeted parity tests and benchmarks.

## Success Criteria

The active work is complete when:

- GPU-backend clean-subtree payloads are GPU-resident render-target images
- admitted candidates can prepare before drawing and use the new payload in the
  same frame
- CPU raster payloads are fallback/test harnesses, not the primary GPU-window
  production path
- element alpha for caches rooted at that element is applied at payload
  composition time
- full-frame benchmarks prove wins for translation, layout-reflow movement, and
  element-alpha demo cases
- no-candidate and broad mixed-scene benchmarks do not regress
- renderer stats expose enough cache detail to explain hits, misses, stores,
  fallbacks, and rejected candidates
