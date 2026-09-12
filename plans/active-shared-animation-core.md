# Shared animation core: layout lengths and change transitions

Status: unified implementation proposal; no implementation yet.
Branch: `plan/shared-animation-core`. Code baseline: `641d355`.
The two predecessor documents are preserved in commit `e38f0b0`; this is their
single replacement, not a third parallel plan.

## Goal and complexity

Support the full width/height length matrix and `Animation.change/3` through one
animation core. Reuse timing, easing, compatible interpolation, real layout,
refresh and hit geometry. Refactor the boundary that currently makes endpoint
queries unsafe: **geometry evaluation versus committing live presentation**.

| Area | Scope |
| --- | --- |
| Shared frame inputs and update effects | Bounded changes across animation, layout preparation, patches and tree update |
| Geometry query/commit boundary | Main structural work: native layout helpers, workspace/cache ownership and compatibility outputs |
| Endpoint resolution and change admission | Small shared layer over those boundaries; correctness-sensitive source/context handling |
| Public metadata | Animation helper, normalization, validation, native/Elixir attr codecs and format compatibility |
| Rendering, input and backends | Preserve existing consumers/interfaces; integration and regression tests, not redesign |

This is a native animation/layout refactor with targeted integration, not an engine
replacement. It simplifies feature code but is more structural work than a one-off
probe helper. Complexity is assessed by coupling and migration risk, not time units.

## Audit decisions that unify the predecessors

| Tension / code finding | Unified decision |
| --- | --- |
| Incremental probe copies versus optional architectural investment | One staged route to query/commit isolation; a bounded layout-copy adapter may bridge migration, not become a separate animation engine |
| Declared attrs already hold old targets | Use sparse applied-update effects; no persistent duplicate target-history registry |
| `SetAttrs` and text patch fast paths overwrite presentation before sync | Keep committed output stable across model updates; transitional source captures must precede destructive writes |
| `resolve_element` also mutates scroll, paint order and registry/cache state | Evaluation computes workspace results; only live commit publishes derived effects |
| `Measure` can be caused by animation presence, not actual size-input changes | Separate endpoint-context changes from generic work invalidation |
| Runtime-start hints ignore `SetAttrs` | Explicit change effects wake the runtime, including paint-only changes |
| Sampling can inherit `latest_animation_sample_time` from an idle period | New runs use a fresh/presentation-aligned start; share existing anchoring mechanics |
| Patches apply sequentially and can fail after a successful prefix | Report applied effects and errors; do not invent whole-batch rollback |
| Pixel conversion and independent probes can change semantics | Preserve compatible interpolation and prove coherent symbolic endpoint projections |
| Query isolation does not define asynchronous endpoint behavior | Treat concurrent context/retargeting as a correctness gate, not a solved optimization |

Evidence is in `tree/{element,animation,layout,patch,invalidation}.rs` and
`runtime/tree_update.rs` under `native/emerge_skia/src/`. In particular,
`NodeLayoutState` mixes outputs/workspace, `update_scroll_state` mutates offsets,
`classify_attrs_change` conservatively marks animation-bearing attrs, and
`patches_may_start_animation_runtime` currently ignores `SetAttrs`.

## Public contract

```elixir
Animation.animate([[width(px(40))], [width(fill())]], 1000, :linear)

# First tree for an element:
Animation.change(width(px(40)), 1000, :linear)
# Later retained update of that element:
Animation.change(width(fill()), 1000, :linear)
```

The change update creates the equivalent two-keyframe run in the same core.
The only difference is admission/source selection: explicit keyframes versus a
retained target change. No separate interpolation or layout implementation.

### Complete length matrix

Accept every ordered pair of valid length expressions: pixels, fill, content,
weighted fill and recursive min/max, including differing expression shapes.
Cover both axes, reverse/multiple segments, regular/enter/exit and change triggers.
No pixel-first, owner-specific or reduced-matrix release.

- Keep same-attribute-set validation for explicit keyframes and existing rules
  for other attr families. Omission is not an implicit content keyframe.
- Preserve compatible interpolation: pixel numbers, weighted-fill weights and
  recursively compatible min/max. Choose compatibility per adjacent segment.
- Incompatible lengths resolve the complete expression to endpoint box sizes,
  then interpolate those numbers. Do not silently hold mismatched leaves.
- Restore the original destination expression at completion. Settled symbolic
  dimensions remain responsive; no cached pixel value replaces the declaration.
- This animates real layout, not FLIP/paint scaling. Wrapping can remain discrete.

Reference arithmetic that must not change accidentally:

- 600px row: 40px → fill beside one equal-fill sibling targets 300px; linear
  resolved-box midpoint is **170px**. A basis/weight blend gives **213⅓px** instead.
- Two simultaneous 40px → fill siblings target **300px each**. Independent probes
  leaving the peer at 40px incorrectly yield **560px each**.
- Existing weighted fill 1 → 3 beside weight 1 has midpoint **400px**, not the
  **375px** produced by always interpolating resolved endpoint widths.

### Change and lifecycle rules

- First mount applies the target immediately. Same element means existing
  `NodeId` plus mount identity; preserve reconciliation rules. Remount/full upload
  resets ownership even when numeric ids are reused.
- A different normalized target starts once using the incoming duration/curve.
  Identical-target rerenders do not restart. Timing-only edits affect the next run.
- A new target interrupts from the current native presentation sample over the new
  duration. An environmental endpoint change instead uses the remaining interval.
- Adding a policy to an existing node uses a valid old value/frame when available,
  otherwise seeds immediately. Removing it cancels that property's run and applies
  the ordinary declaration. A later plain attr clears the earlier policy too.
- Coalesce applied updates before the next live frame: first source, latest target,
  no intermediate queue. A→B→A must preserve an original active run when appropriate.
- One ordinary animatable attr per `change` call; multiple properties can have
  different timings. Non-length pairs retain existing compatibility restrictions.
- Keep explicit exit > enter > regular precedence and ordinary restart behavior.
  Merge change on disjoint fields; reject regular/change overlap. Enter-owned
  fields keep at most one latest pending change, starting from the handoff sample;
  an unchanged mount baseline does not create a post-enter transition.
- Enter still restores base attrs/starts regular animation. A mismatched base may
  jump under that existing contract. Removal cancels change runs; exit captures
  the common source presentation. Strip change policies from ghosts.

## Target architecture

```text
model/runtime updates → applied UpdateEffects
                         │
explicit keyframes ──────┼→ shared run admission, timeline and easing
change/lifecycle triggers┘              │
                                      ▼
                       composed FrameInputs / endpoint projections
                                      │
                       lazy numeric endpoint resolution
                                      │
                       ordinary sampled effective attrs
                                      │
                 geometry evaluation when required (or reuse)
                                      │
                  commit live presentation → existing refresh
                                      │
                            RenderScene / registry
```

### 1. Shared frame composition and sparse update effects

Extract a single composition operation used by full, active-only, dirty-subtree,
endpoint and direct/headless paths. Preserve current order: raw animation overlay,
normal scale preparation, then interaction styles. Separate runtime normalization
and dirty marking from composition. Do not allocate full-tree attrs each pulse.

Patch/update processing returns relevant old/new semantic values, policy/mount
changes, actual layout-input changes and any partial error. Compare desired values,
not changing effective samples. Use existing preimages and affected ids; do not
introduce a parallel Elixir diff, native full-tree policy scan or idle history map.

Validate synthesized non-length pairs before committing that node's invalid target.
Keep effects from successfully applied prefixes when another patch fails. Model
application and live-frame commit are distinct boundaries, not atomic transactions.
During migration, preserve required sources before text/other fast paths mutate them.

### 2. Geometry evaluation independent of live commit

Introduce internal `LayoutInputs`, reusable workspace and `GeometryResult` concepts.
These names are schematic; existing `LayoutOutput` remains the render/event refresh
product, not the geometry result.

- Inputs borrow model/topology and runtime context, with sparse frame overrides,
  viewport/scale and available font/media metrics.
- Evaluation uses the existing measure/resolve algorithms. It writes workspace
  geometry, paragraph fragments, derived paint order, scroll proposals and
  context-tagged cache updates—not live presentation or model state.
- Queries read endpoint dimensions and discard temporary effects. Live commit
  installs results/damage once, followed by the existing refresh/publication path.
- Normal resource setup remains explicit. Endpoint evaluation reads metrics without
  starting asset loads; direct/headless paths still perform normal resource setup.
  Immutable fonts and correctly keyed measurement memoization may be shared.
- Calculate scroll clamping/end-following in the workspace where needed; commit only
  the live result. Simply omitting scroll behavior from queries is not equivalent.
- Retain scale context and unrotated boxes. Convert endpoint/source geometry to
  logical animation units once, not using newly patched ancestor scales.
- Endpoint overrides dirty their real dependencies. Borrowed/copied clean flags do
  not establish cache validity in another projection.

Keep current `element.layout.effective`, frame and scroll fields as compatibility
outputs initially. A commit adapter updates changed nodes and cache/damage state;
render/event builders and backend `RenderScene` interfaces need not be rewritten.
Patch-side geometry fast paths must feed pending output/the same commit path rather
than overwrite the source presentation before a transition is admitted.

A temporary isolated layout-copy adapter can establish parity while helpers migrate
away from broad `&mut ElementTree` access. Do not use live-tree layout plus frame-only
rollback. Do not copy registry/render fragments or retain cloned scenes per run.
Any remaining copy cost must be explicit and pass qualification, not hidden by the
new API. Normal and endpoint paths must end up using the same layout calculations.

No mandatory persistent tree, ECS, global double buffer or full dependency graph.
Reuse native dense storage where suitable; preserve sparse preparation/commit and
paint-only geometry bypass. Moving fields between structs alone is not isolation.

### 3. Minimal shared runtime ownership

| Data | Owner / lifetime |
| --- | --- |
| Desired target and policy | Existing declared attrs |
| Original semantic values and pending applied changes | Sparse update journal until admission/coalescing is resolved |
| Source presentation | Stable committed native output, or explicit captured anchor during migration/handoff |
| Clock/spec, optional anchor and endpoint results | Running/pending instance only |

Generate a once spec only on a real change. Reuse the current sampler and small
start/completion helpers; do not introduce another interpolation engine or rewrite
all owner maps into a general graph. Poll active entries, avoid quadratic id
collection, and compute spec identity at admission rather than hashing debug strings
or building keyframes on every tick.

A source frame alone cannot represent a weighted-fill sample. Preserve compatible
logical sampled values as well as geometry when needed. Committed native output
is not necessarily the last GPU-displayed scene; reuse existing timing/anchoring,
not GPU readback or new renderer acknowledgments.

Completion must emit the existing damage needed to expose the final symbolic/base
state before pulses stop. Idle policies must not keep the runtime active.

### 4. Lazy endpoint resolution and cache validity

Use the same ordered lookup for explicit and generated runs:

1. Compatible pair: existing interpolation, no endpoint query.
2. Pixel-only expression: evaluate constants/min/max directly with normal units.
3. Valid source anchor or adjacent endpoint in matching context: reuse it.
4. Otherwise evaluate a symbolic endpoint projection through the geometry API.

Evaluate the whole keyframe's relevant attrs and both axes together. Deduplicate
identical projections, not merely requests with the same parent. Never substitute
an arbitrary old frame for an explicit keyframe that was not laid out there.

Typical extra work, excluding live layout: zero queries for compatible/constant
pairs and captured symbolic→pixel changes; one unknown endpoint for pixel→symbolic;
up to two for a cold symbolic→symbolic pair; zero for unchanged warm endpoints.
These are conditional counts, not runtime performance claims or a global bound.

Store only needed endpoint values/context on the active segment. Context derives
from actual viewport, scale, layout attrs, topology, interactions, metric changes
and relevant other animation samples—not generic `Measure`, policy-only updates,
unchanged-dimension asset notices or the run's own pixel writes. Begin conservatively
for external changes; refine locality only with evidence.

A pure axis-allocation helper may avoid full queries for independently constrained
rows/columns with valid measurements. Share existing planner arithmetic; fall back
when wrapping, cross-axis reflow, intrinsic parents or stale measurements invalidate
that shortcut. It is an optimization after parity, not a prerequisite framework.

### 5. Concurrent endpoint context: explicit gate

Neither predecessor resolved this fully, and the architecture does not solve it.
Synchronized tracks sharing allocation need a coherent endpoint projection. Different
phases/deadlines cannot all be treated as simultaneous terminal keyframes.

Prototype deterministic frozen sampled contexts with bounded, at-most-once-per-frame
retargeting and no recursive animation/layout fixed point. Verify asynchronous fill
siblings, parent/child animations, mixed durations and final symbolic handoff.
A stale numeric peer snapshot can be wrong when that peer completes.

Document the chosen projection/retarget rule and prove its reference cases before
general implementation is accepted. If it fails, revise this plan explicitly—do not
silently narrow the matrix, introduce a solver, or claim two global queries suffice.

### 6. Declarative metadata and compatibility

Normalize wrappers to ordinary targets plus per-property policies, e.g.:

```elixir
%{width: :fill, animate_change: %{width: %{duration: 1000, curve: :linear}}}
```

Merge policies per canonical property/field group and retain them in normal attr
hashing/equality. Reuse duration/curve/attr validation. Remove only width/height
cross-variant rejection; validate other old/new pairs through shared rules.

Add one policy attribute to native/Elixir codecs and update EMRG/macOS host
compatibility/documentation. Existing length tags stay unchanged. Do not duplicate
targets on the wire, serialize presentation history, overload `:animate`/`:on_change`,
or add NIF entry points or per-frame BEAM calls. Keep BEAM collection work linear.

## Implementation phases and stop conditions

1. **Reference contract:** capture legacy interpolation/layout behavior, full-matrix
   endpoint cases and the concurrent-context rule. Stop if semantics remain ambiguous.
2. **Shared seams:** frame composition, applied effects and fresh clock admission;
   preserve current pixels, patch-prefix behavior and incremental preparation.
3. **Evaluation/commit migration:** establish the geometry API and compatibility
   adapter, isolate resources/derived effects, migrate patch geometry fast paths.
   Stop on cache/pixel/scroll divergence or unconditional full-tree warm-frame cost.
4. **Shared endpoint and change runtime:** lazy resolution, source anchors, cache
   context, sparse change admission, coalescing and lifecycle completion.
5. **Public acceptance:** policy encoding/docs, complete matrix and owner coverage,
   explicit/change differential tests, host/device qualification and measured tuning.

A copy adapter is a migration tool, not a competing feature path. A broader allocator
or dependency refactor needs evidence and a plan revision; no separate architecture
project is implicitly authorized by these phases.

## Acceptance and validation

- Generate representative length Cartesian products on both axes/all owners,
  including recursive bounds, reversals and multiple segments. Compare explicit
  `animate([A,B])` with settled `change(A) → change(B)` at matching times/context.
- Preserve numeric/weighted/min-max baselines, curves, finite repeats/loops and
  segment joins. Non-length malformed/incompatible inputs must fail clearly.
- Verify initial/identical/policy-only updates, idle start, A→B→A coalescing,
  interruption/reversal, per-property timing, keyed/unkeyed identity, remount/full
  upload, owner conflicts, enter/base handoff and exit/ghost cleanup.
- Verify old semantic/source data through text fast paths, scale/topology edits and
  successful patch prefixes followed by failure; no corrupted next transition.
- Compare old/new geometry, pixels, clips, hits, nearby border-box alignment, wrapped
  text, image aspect ratio, intrinsic parents, insets, scroll end-following and rotation.
- Prove query isolation and exactly-once live derived effects; no asset requests,
  leaked probe frames, stale dependency flags or unqualified cross-projection reuse.
- Confirm cold/warm query counts, zero endpoint work on compatible/compositor paths,
  no idle discovery scans and no regenerated specs. Measure visited nodes, copied
  bytes, workspace peak and retained-cache hits on small updates in large trees.
- Test shared `TreeUpdateEngine`, direct/headless helpers and macOS integration;
  backend orchestration remains unchanged. Use constrained-device evidence for
  performance acceptance, not code-reading estimates.

```bash
cargo test --manifest-path native/emerge_skia/Cargo.toml
EMERGE_SKIA_BUILD=1 mix test
EMERGE_SKIA_BUILD=1 ./ci-tests.sh
```

Respect the performance lock in [animation smoothness](active-low-resource-animation-smoothness.md).
That existing device/performance plan is not superseded by this feature plan.
See also [layout/cache flow](../guides/internals/layout-refresh-render-flow.md),
[layout caching roadmap](layout-caching-roadmap.md) and
[platform orchestration](platform-runtime-architecture-differences.md).

This consolidation audited code and design documents only. No implementation,
Rust/Elixir tests or runtime benchmarks were performed.
