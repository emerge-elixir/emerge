# Caching Implementation Review

Last updated: 2026-04-28.

Status: review/reference. This document reviews caching implemented so far in
the native layout, refresh, registry, and renderer paths. It is not an active
implementation plan.

Primary code reviewed:

- `native/emerge_skia/src/tree/element.rs`
- `native/emerge_skia/src/tree/layout.rs`
- `native/emerge_skia/src/tree/render.rs`
- `native/emerge_skia/src/events/registry_builder.rs`
- `native/emerge_skia/src/runtime/tree_actor.rs`
- `native/emerge_skia/src/renderer.rs`
- `native/emerge_skia/src/stats.rs`
- `native/emerge_skia/src/tree/layout/tests/cache.rs`
- `native/emerge_skia/benches/renderer.rs`

Related plans:

- `layout-caching-roadmap.md`
- `rendering-cache-engine-investigation.md`
- `render-cache-flutter-comparison.md`
- `active-render-cache-children-plan.md`

## Terminology

### Clean subtree

A clean subtree is a retained subtree whose local content can be reused for the
current frame without recomputing or repainting that subtree's internals.

In layout and refresh code, "clean" means the subtree has no relevant dirty
state for the cache being used. For example, a subtree can be clean for
measurement when its measure-affecting attrs, inherited inputs, constraints, and
child topology dependency key are unchanged. It can be clean for render-scene
reuse when its render-affecting attrs, runtime state, frame, scene context,
resources, and render topology dependencies are unchanged.

In the renderer payload cache, a `CleanSubtree` candidate is narrower: it is a
stable local render-node subtree that may be rasterized into an image payload.
The cached payload represents local content only. Composition state such as
integer placement and root element alpha is applied when drawing the payload and
is intentionally not part of the cached content. The candidate stops being
clean when its local render content, size, resources, or unsupported composition
state changes.

## Review Findings

### Finding 1: cache layering is coherent and conservative

Severity: positive finding.

The current cache stack has clear layers:

- layout measurement and resolve caches keep geometry work out of unchanged
  retained subtrees
- render-subtree caches keep retained render-scene construction out of clean
  refresh paths
- registry chunk caches keep event-registry rebuild work out of clean subtrees
  when the fallback path is safe
- renderer clean-subtree payload caches turn stable local render content into a
  GPU image payload when the candidate has proven visible and useful
- asset, font, and vector-raster caches avoid repeated resource setup and vector
  rasterization

These layers are mostly independent. Layout reuse does not require renderer
payload caching, and renderer payload caching can miss without invalidating the
layout result. That separation is the right shape for Emerge because it keeps
correctness in the retained tree and keeps GPU payload caching as an optional
performance layer.

No immediate correctness blocker was found in the cache model during this pass.

### Finding 2: layout cache correctness is stronger than its observability

Severity: medium.

The layout caches have good invalidation guards:

- intrinsic measurement keys are limited to text, text inputs, multiline text,
  images, and videos where leaf measurement is expensive or externally sourced
- subtree measurement keys include the element kind, effective layout-relevant
  attributes, inherited font state, and compact child topology dependency keys
- resolve keys include measured geometry, constraints, inherited state,
  effective resolve attributes, and topology dependencies
- dirty-descendant resolve reuse shifts cached clean geometry and recurses only
  into dirty descendants instead of hiding changed children behind a parent hit
- layout-affecting animation samples are converted into normal dirty paths, so
  unrelated clean siblings can still reuse their caches

The weaker part is investigation visibility. `LayoutCacheStats` intentionally
reports only hit, miss, and store outcomes for intrinsic measure, subtree
measure, and resolve. That keeps benchmark output readable, but it makes it
harder to answer why a miss happened without temporary instrumentation.

Recommended follow-up:

- keep hit/miss/store stats as the default public shape
- add gated, off-by-default diagnostic counters only when a cache miss
  investigation needs them, such as dirty, key mismatch, ineligible kind,
  topology mismatch, or store disabled
- do not make bypass reasons part of the normal stats output unless they prove
  useful in repeated investigations

### Finding 3: layout caches are per-node and bounded by tree size, not memory

Severity: medium.

Per-node layout state stores:

- `IntrinsicMeasureCache`
- `SubtreeMeasureCache`
- `ResolveCache`
- `RenderSubtreeCache`
- `RegistrySubtreeCache`

This keeps lookup cheap and makes cache invalidation local. It also means memory
is bounded mainly by retained tree size rather than by an explicit cache byte
budget. That is probably acceptable for normal Emerge trees, but large dynamic
trees and future repeater/viewport work need better visibility.

The detached layout cache is explicitly bounded and small:

- at most 16 detached subtrees
- each detached subtree limited to 128 nodes
- scoped by structural signature, attachment context, scale, and host state

Recommended follow-up:

- add optional memory-oriented stats for retained tree cache occupancy before
  introducing any eviction policy for per-node layout caches
- keep detached layout cache limits conservative until a benchmark proves larger
  detached caches matter
- for future repeaters, design cache preservation around stable item identity
  instead of broadening global memory retention

### Finding 4: layout key construction still has likely hot-path overhead

Severity: medium.

The implementation removed some earlier allocation-heavy key patterns, such as
joined debug strings for render subtree keys and cloned child topology lists for
layout cache topology. Remaining likely costs are:

- effective attribute hashing/cloning in layout, render, and registry cache keys
- id-facing compatibility helpers in paths that are otherwise `NodeIx` based
- repeated full attribute hashing where a typed version or precomputed compact
  key could be cheaper

This is not a correctness issue. It is a next performance target if profiles
show cache-key work becoming noticeable after layout and rendering reuse are
warm.

Recommended follow-up:

- benchmark key-building cost before replacing hashes with more state
- prefer typed dirty/version counters for proven hot keys
- avoid speculative precomputed hashes for every node unless cache-key time is a
  measured bottleneck

### Finding 5: render-subtree cache is correctly separate from renderer payload cache

Severity: positive finding.

`RenderSubtreeCache` stores retained render-scene fragments at the tree/render
construction layer. It is not the same thing as the renderer clean-subtree GPU
payload cache.

Important current properties:

- refresh-only frames can reuse clean retained render subtrees
- damaged refreshes with no existing cache avoid building lookup keys and take
  the uncached path
- scroll-offset contexts bypass render-subtree lookup
- dirty scroll containers do not store large immediately stale render caches
- cache keys include asset source status generation so an image placeholder
  cached before load cannot survive after the asset becomes ready or failed
- focused text input cursor state remains part of the cached render subtree
  state and is treated conservatively

This is the right division of work. Render-subtree cache reduces CPU scene
construction. Renderer clean-subtree cache reduces draw work and GPU command
work. They should stay independently benchmarked.

### Finding 6: registry chunk cache is useful but intentionally narrow

Severity: low.

The registry cache stores reusable registry chunks for clean subtrees and merges
them into the current event registry. It falls back to a full rebuild when:

- there are escape-nearby mounts at the tree level
- registry damage exists but no retained registry cache is available
- a subtree has escape-nearby mounts
- the per-refresh registry cache budget is exhausted

That conservative fallback is appropriate. Event registry correctness is more
important than skipping all possible work, especially with nearby overlays,
scroll contexts, focus state, and deferred subtrees.

Recommended follow-up:

- only broaden registry caching if profiles show registry rebuilds as a
  meaningful bottleneck
- if broadened, start with better chunk seeding for damaged cleanable subtrees,
  not with removing escape-nearby fallbacks

### Finding 7: renderer clean-subtree cache is now GPU-first and correctly scoped

Severity: positive finding.

The production renderer payload cache is now close to the useful part of
Flutter's raster cache model:

- candidates are discovered during retained render traversal
- visibility is counted before admission
- payload creation is limited per frame
- GPU render-target payloads are used when a `DirectContext` is available
- CPU raster payloads are used for raster/offscreen fallback
- hits draw a stored `skia_safe::Image`
- misses draw direct until admission
- rejected or failed prepares fall back to direct rendering
- root element alpha and integer translation are composition state, not content
  key state
- parent hits and same-frame parent prepares touch descendant entries as
  `suppressed_by_parent`
- entries age out when not seen for the stale-frame window
- stats expose hits, misses, stores, entries, bytes, payload kinds, rejections,
  prepare timing, hit draw timing, suppressed-by-parent, and stale evictions

This is the right baseline. It specifically covers the common Emerge patterns
that motivated the cache: stable todo rows moving/fading, app selector alpha
composition at the root cache boundary, interaction page typing with stable
surrounding subtrees, and layout-reflow movement where local content is stable
but placement changes.

### Finding 8: renderer cache admission is intentionally simple and may need scoring later

Severity: medium.

The current renderer admission model is based on:

- candidate eligibility
- visible count threshold
- per-frame payload budget
- entry byte budget
- total entry and total byte budget
- node-count bounds

That simplicity is good, but it can admit cheap candidates and reject or miss
some small expensive candidates. It also cannot explain which candidate was a
bad cache investment beyond aggregate stats.

Recommended follow-up:

- do not add complexity scoring without a benchmark and a live trace that shows
  current admission making bad choices
- add temporary or gated diagnostics for top cache candidates by id, bounds,
  primitive mix, hit count, store count, and direct draw cost if candidate
  quality becomes the next issue
- keep the public stats readable; detailed candidate dumps should be opt-in

### Finding 9: fixed stale-frame aging is reasonable but not yet configurable

Severity: low to medium.

The renderer clean-subtree cache now ages out entries that have not been seen
for 120 frames. That is safer for Emerge than Flutter's immediate unseen-entry
eviction because Emerge candidates can disappear transiently under parent cache
hits, resize, interaction, or refresh-path changes.

The tradeoff is that the stale window is currently a fixed internal policy,
while max entries, max bytes, and max entry bytes are configurable through
`EmergeSkia.start/1` options.

Recommended follow-up:

- keep the fixed stale window unless live traces show retention or churn
  problems
- if traces show it matters, expose the stale frame window as an option next to
  the existing renderer cache size options
- benchmark memory and hit-rate behavior before changing the default

### Finding 10: nested alpha children cache was correctly rejected for now

Severity: positive finding.

The benchmark gate for a new alpha-specific children-cache kind did not prove a
GPU win. Keeping that optimization out is the right decision.

The current production model remains:

- cache stable clean-subtree content
- apply changing root element alpha at composition time
- reject nested `Alpha` inside a clean-subtree payload
- reject rotate, scale, fractional placement, video, loading placeholders,
  failed-image placeholders, active text input, and broader composition scopes

This keeps the implementation smaller and avoids a second cache boundary model
without measured benefit. If nested alpha returns, it should come back with a
specific workload and a benchmark that proves it beats direct drawing on GPU,
not only in a raster microbench.

### Finding 11: asset, font, and vector caches are correct supporting caches

Severity: low.

The renderer includes global caches for:

- fonts by family, weight, and italic state
- registered raster/vector assets by id
- rendered vector variants with bounded entries and bytes

These caches are not the same as layout or renderer payload caches, but they
matter for correctness and cache invalidation:

- font registration bumps a font generation used by renderer payload resource
  keys
- asset insertion/removal bumps an asset generation used by renderer payload
  resource keys
- rendered vector variants are invalidated for an asset id when the asset is
  replaced
- raster image assets are decoded eagerly when inserted, avoiding first-draw
  decode spikes for already registered assets

Remaining risk is mostly operational:

- font and asset caches are global and mutex-protected
- registered assets are app-owned and not byte-budgeted like renderer payloads
- vector variants are byte-budgeted, but aggregate resource-cache memory is not
  shown in normal stats

Recommended follow-up:

- leave these caches alone unless traces show lock contention or memory pressure
- if memory becomes a concern, add resource cache occupancy stats before adding
  eviction behavior

### Finding 12: cache tests and benchmarks cover the main invariants

Severity: positive finding.

Current coverage is broad:

- layout cache tests cover warm hits, dirty descendant behavior, text-flow
  resolve, animation-only refresh, render-subtree reuse, registry reuse,
  detached layout restore, nearby behavior, and animate-exit ghost layout
- renderer tests cover clean-subtree keying, admission, payload budgets,
  parent/child suppression, stale eviction, root alpha composition, transform
  rejection, placeholder invalidation, vector cache behavior, and stats
  formatting
- renderer Criterion benchmarks cover clean-subtree candidates, translated
  candidates, layout-reflow movement, parent/child cache behavior, alpha-child
  experiments, GPU cache candidates, and GPU translated/layout-reflow variants
- retained-layout benchmarks print cache-counter lines for smoke investigation

The main gap is not test presence. The gap is benchmark interpretation for live
demo workloads, especially when a change is sensitive to compositor timing,
surfaceless GPU variance, or candidate quality.

Recommended follow-up:

- continue the current rule: no cache expansion without a benchmark showing a
  win and broad guards showing no regression
- keep rejected optimizations documented in benchmark comments or plans, not in
  production code

## Cache Inventory

### Intrinsic measurement cache

Location: `NodeLayoutState::intrinsic_measure_cache`.

Purpose:

- reuse expensive leaf/media/text intrinsic measurement

Key includes:

- element kind
- relevant text or media attributes
- inherited font state
- constraints and source size where applicable

Strength:

- avoids repeated text/media measurement when content and constraints are
  unchanged

Risk:

- misses are visible only as misses, not categorized by cause

### Subtree measurement cache

Location: `NodeLayoutState::subtree_measure_cache`.

Purpose:

- reuse measured subtree frames when local layout inputs and child topology are
  unchanged

Key includes:

- element kind
- measure-relevant effective attributes
- inherited state
- compact child topology dependency key

Strength:

- can keep clean sibling subtrees hot while one dirty branch is remeasured

Risk:

- still conservative for several dependency-boundary shapes such as broader
  row/column, scrollable, text-flow, and future viewport/repeater cases

### Resolve cache

Location: `NodeLayoutState::resolve_cache`.

Purpose:

- reuse coordinate-invariant resolved geometry and shift it into the new
  placement

Key includes:

- element kind
- resolve-relevant effective attributes
- inherited state
- measured frame
- constraint
- compact topology dependency key

Strength:

- supports dirty-descendant reuse, text-flow resolve reuse, and nearby overlay
  boundary improvements

Risk:

- correctness depends on keeping topology and resolve-affecting attrs precise
  as new layout features are added

### Detached layout subtree cache

Location: `ElementTree::detached_layout_cache`.

Purpose:

- preserve small recently removed nearby subtrees so hover/menu/code-preview
  toggles do not repeatedly pay cold layout cost

Limits:

- 16 detached entries
- 128 nodes per detached subtree

Strength:

- solves a concrete nearby remove/reinsert pattern without turning all removed
  nodes into long-lived retained state

Risk:

- future dynamic list work needs a different identity-preserving model, not just
  a larger detached cache

### Render-subtree cache

Location: `NodeRefreshState::render_cache`.

Purpose:

- reuse retained render-scene fragments for clean refresh subtrees

Strength:

- keeps refresh CPU work low and feeds stable candidates to the renderer
  payload cache

Risk:

- key construction and cache-store budgets should remain monitored if refresh
  CPU becomes hot again

### Registry subtree cache

Location: `NodeRefreshState::registry_cache`.

Purpose:

- reuse event registry chunks for clean subtrees

Strength:

- avoids full registry cloning/rebuilds on clean refresh paths

Risk:

- intentionally falls back for escape-nearby and damaged/no-cache cases

### Renderer clean-subtree payload cache

Location: `SceneRenderer::renderer_cache` and `CleanSubtreeCache`.

Purpose:

- rasterize stable local render content into a cached image and draw that image
  on later frames

Payloads:

- GPU image when a GPU direct context is available
- CPU raster image for raster/offscreen fallback

Strength:

- makes stable translated or root-alpha-composited subtrees cheap to draw

Risk:

- candidate admission is simple and aggregate stats cannot yet identify bad
  candidates directly

### Font, asset, and rendered vector caches

Location: renderer global caches.

Purpose:

- reuse typefaces, registered assets, and rasterized vector variants

Strength:

- resource generations make renderer payload keys invalidated when underlying
  resources change

Risk:

- memory visibility is limited outside vector variant budgeting

## Recommended Next Work

1. Keep broad cache scope unchanged until fresh traces show a specific problem.
2. Add opt-in diagnostics before adding new cache behavior:
   - layout miss reasons
   - renderer top candidate summaries
   - retained cache occupancy and approximate memory
3. If renderer cache quality becomes the bottleneck, benchmark complexity
   scoring or public cache hints against live Emerge demo workloads before
   enabling them.
4. If retained layout memory becomes the concern, add cache occupancy stats
   before designing eviction.
5. Continue treating nested alpha, rotate, scale, fractional translation, video,
   active text input, and placeholder caching as rejected scope until a workload
   and benchmark prove otherwise.

## Bottom Line

The caching implemented so far is structurally sound. The most valuable next
work is not adding another cache immediately. The next work should be better
diagnostic visibility for why existing caches miss, what renderer candidates are
worth keeping, and how much memory retained caches occupy. After that, any
cache broadening should be benchmark-gated and kept only if it improves the
target workload without broad regressions.
