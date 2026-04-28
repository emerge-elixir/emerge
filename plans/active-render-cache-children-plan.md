# Active Render Cache Children Plan

Last updated: 2026-04-28.

Status: implemented, pending normal plan cleanup.

## Purpose

Implement the next renderer-cache slice from
`render-cache-flutter-comparison.md`: Flutter-style parent/child cache lifecycle
semantics, stale-entry aging, and a measured decision on whether to introduce a
new children-cache boundary for changing composition effects.

This plan does not broaden cache eligibility by allowing arbitrary nested
`Alpha`, rotate, scale, fractional placement, active text input, video, or
loading/failed image placeholders inside the existing `CleanSubtree` payload.
The correct next model is narrower: cache stable children as transparent local
content and apply changing composition state while drawing that payload, but
only after a workload proves that extra boundary is faster than direct drawing.

## Investigation Summary

Flutter points to three mechanisms worth copying in Emerge:

- `RasterCache::MarkSeen` and `EvictUnusedCacheEntries` keep per-frame
  encountered state and evict entries that disappeared from the preroll tree.
- `LayerTree::TryToRasterCache` suppresses child payload creation when a parent
  cache successfully prepares, while still keeping child cache items accounted
  for in the frame.
- `RasterCacheItem::kChildren` lets opacity/filter-like layers cache stable
  children separately from the changing layer effect.

Emerge mapping:

- `CleanSubtree` already has retained identity, content generation, visibility
  counters, byte budgets, and LRU eviction.
- Parent cache hits currently return before traversing children, which is fine
  without stale eviction but unsafe once entries can age out.
- Tree rendering currently rejects `Alpha` inside clean-subtree candidates.
  That should remain true. If alpha is expanded later, it should become a cache
  boundary, not a generally allowed child node inside an ancestor payload.

## Non-Goals

- no rotate/scale/fractional-translation cache eligibility
- no complexity scoring or public cache hints
- no shadow/text/picture cache pilot
- no platform-view/RTree preservation
- no cache code path that stays enabled without benchmark improvement

## Implementation Outcome

Implemented:

- renderer-cache entries and access metadata now track `last_seen_frame`
  separately from visible and used frames
- renderer-cache stats distinguish `visible`, `suppressed_by_parent`, budget
  evictions, stale evictions, stale bytes, and payload kinds
- warm parent cache hits and same-frame parent prepares touch existing
  descendant clean-subtree cache entries as suppressed instead of preparing or
  drawing them
- clean-subtree entries age out after a conservative stale-frame window when
  they are no longer seen or parent-suppressed
- stale-entry aging is covered by unit tests and renderer-cache stats formatting
- new parent/child and alpha-child Criterion guards were added before code
  changes

Rejected for this slice:

- A new alpha-specific children-cache candidate kind was not implemented. The
  benchmark gate did not prove that a separate nested-alpha payload is better
  than direct drawing for the measured overlapping-alpha workload, especially on
  the GPU microbench. The existing root clean-subtree alpha composition path
  remains the production path for app selector and todo fade/translate cases.

## Benchmark Gate

Before implementation changes, add or extend renderer benchmarks so each slice
has a measurable target and a guard against broad regressions.

Required benchmark cases:

- nested alpha with stable children, including overlapping primitives where
  group-alpha semantics matter
- app-selector menu alpha animation, as the existing root-alpha guard
- todo-entry translate plus alpha animation, as the existing translate+alpha
  guard
- showcase layout-reflow movement, as the existing movement guard
- mixed no-candidate scene, to verify cache tracking stays out of normal frames
- parent-hit nested-cache scene, to measure the cost of touching suppressed
  child entries

Landing rule:

- if a slice does not improve its target workload or regresses the broad guards,
  do not keep the optimization enabled
- if the rejected path required extra code, remove it or leave only a short code
  comment near the benchmark explaining why the simpler path stayed

Measured results from the implementation pass:

- `native/renderer/cache_children/parent_hit/nested_candidates` stayed neutral
  to better versus direct rendering, so descendant touch accounting is retained
  for correctness with stale eviction
- `native/renderer/cache_children/nested_alpha/candidate_children` beat direct
  drawing in the raster microbench but did not beat direct drawing in the GPU
  microbench, so no new alpha-cache kind was added
- existing raster app-selector, todo-entry, and layout-reflow guards still show
  cached/fallback paths faster than direct drawing in the measured pairs
- `native/renderer/cold_frame/raster_first_frame_mixed_ui` improved in the
  short guard run, indicating the metadata path does not hurt no-candidate
  raster frames
- the surfaceless GPU Criterion groups showed large absolute variance in this
  container and were treated as guard signals, not precise win/loss numbers

## Slice 1: Lifecycle Stats And Stale Model

Status: implemented.

Goal:

- add the metadata needed for stale-entry eviction without changing eviction
  behavior yet

Work:

- add per-entry `last_seen_frame` or equivalent encountered state
- distinguish seen, visible, used, and suppressed-by-parent in renderer-cache
  stats
- add `stale_evictions` and stale bytes to stats, initially expected to stay
  zero
- add tests proving current budget/LRU behavior is unchanged

Validation:

- unit tests for seen/visible/used counters
- renderer-cache stats formatting tests
- Criterion guard that metadata tracking does not regress mixed no-candidate
  frames

## Slice 2: Parent/Child Cache Accounting

Status: implemented.

Goal:

- make parent cache hits compatible with future stale-entry eviction

Work:

- teach the renderer-cache traversal to discover nested cache keys without
  preparing or drawing them when a parent payload hits
- record descendant entries as suppressed by a parent hit or touch them as seen
  for the current frame
- avoid preparing child payloads under a parent cache hit
- expose suppressed/touched child counts in renderer-cache stats

Validation:

- unit test: a parent hit does not prepare child payloads
- unit test: a parent hit keeps existing child entries from becoming stale
- benchmark: parent-hit nested-cache scene shows the accounting cost is small
  enough to keep

Decision point:

- walking descendant render nodes was acceptable in the parent-hit benchmark,
  so explicit parent-owned child key lists stay out for now

## Slice 3: Stale-Entry Eviction

Status: implemented.

Goal:

- evict entries that are no longer encountered while preserving entries
  suppressed by warm parent payloads

Work:

- add a conservative `max_stale_frames` policy for clean-subtree payloads
- keep entry/byte budgets as the hard cap
- do not evict entries touched as suppressed-by-parent
- clear access metadata for entries whose content key changed
- report stale evictions separately from budget evictions

Validation:

- unit test: unseen entries age out after the configured stale window
- unit test: parent-suppressed child entries do not age out
- unit test: changed content generation creates a new key and old entries age
  normally
- live stats review in `../emerge_demo` to confirm stale eviction does not churn
  useful resize/interaction caches

Decision point:

- Emerge uses a conservative stale-frame window instead of Flutter's immediate
  unseen eviction because automatic candidates can disappear transiently during
  interaction, resize, or parent-cache hits

## Slice 4: Alpha Children Cache Boundary

Status: measured and rejected for this slice.

Goal:

- implement the Flutter `kChildren` idea for Emerge alpha scopes

Work:

- benchmark the candidate boundary before adding a new cache kind
- keep active text input, video, image loading/failure placeholders, rotate,
  scale, and fractional placement direct-rendered
- keep ancestor `CleanSubtree` candidates conservative when they contain nested
  composition scopes
- leave the production renderer on the existing root clean-subtree alpha
  composition path because the dedicated nested-alpha boundary did not prove out

Validation:

- benchmark coverage for nested alpha and existing app-selector/todo alpha
  workloads
- cached-vs-direct pixel parity and invalidation tests remain required if this
  boundary is reopened for implementation

Decision point:

- if alpha children caching is neutral or slower, keep the split-boundary tests
  and leave nested alpha direct-rendered; do not keep a more complex renderer
  path without a measured win

## Slice 5: Documentation And Follow-Up Triage

Status: implemented in this document and related cache references.

Goal:

- close this active plan cleanly and avoid expanding cache scope by accident

Work:

- update `render-cache-flutter-comparison.md` with measured results
- update `rendering-cache-engine-investigation.md` with accepted/rejected
  cache semantics
- update `plans/README.md` with the new repo state
- decide whether the next plan should be transform parity, complexity scoring,
  or no cache work

Follow-up candidates after this plan:

- live trace review of stale eviction churn after more demo usage
- fractional translation and scale+translation parity investigation
- direct measured complexity scoring for admitted candidates
- descendant filter/composition effects beyond alpha only if a benchmarked
  workload justifies a new cache boundary
- text or shadow primitive cache only if fresh traces justify it
