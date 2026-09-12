# Shared animation core: layout lengths and change transitions

Status: allocation-aware gate repair planned; native parity proof pending.
The six reference tests are implemented; the repair, resolver and `Animation.change/3` are not.
Branch: `plan/shared-animation-core`. Code baseline: `641d355`.
Predecessors are preserved in `e38f0b0`; the first consolidation is `5475831`.
This remains the single implementation plan.

## Implementation gate: resolved boxes lose allocation information

Native reference tests in
[`animation_endpoints.rs`](../native/emerge_skia/src/tree/layout/tests/animation_endpoints.rs)
reproduced a blocking counterexample, not just an arithmetic hypothesis:

| First child's length in a 600px row | First box | Equal-fill sibling |
| --- | --- | --- |
| `fill` | 300px | 300px |
| `px(300)` | 300px | 300px |
| `min(px(50), fill_weighted(1))` | 50px | 300px |
| `px(50)` | 50px | 550px |
| `min(px(50), fill_weighted(3))` | 50px | 150px |

The planner reserves weighted space before applying a child's bound. Equal visible
box sizes therefore do not imply equivalent allocation. A 40px → bounded-fill
transition lowered to pixels approaches a 50px box beside a 550px sibling; restoring
the symbolic destination jumps the sibling to 300px. This is also reproduced for
columns/heights, global scales 0.5/1/2 and a recursively bounded expression.
Compatible bounded-weight animation already changes the sibling through weight
interpolation while the first box remains 50px; preserve that behavior too.

**Stop condition triggered:** a box-size-only numeric overlay cannot satisfy the
full-matrix/terminal-handoff contract. More endpoint queries, cache invalidation or
query/commit isolation cannot recover information discarded by that overlay.
No public validation was relaxed and no partial `change` API was exposed.

## Gate repair: allocation-aware layout samples

Keep the private workspace and shared sampler. Replace **pixel-only lowering** with
an internal per-axis layout sample: visible dimensions plus the layout contributions
and policies that produced them. Do not change ordinary bounded-fill allocation,
redistribute its unused space, insert spacer nodes, or narrow the matrix.

### A. Preserve the parent's allocation charge

In the existing row/column planners, record each child's contribution to the space
budget **before bounds are applied**:

- A pooled fill child's charge is `fill_weight × fill_unit`.
- A fixed/non-pooled child's charge is the amount actually included in the planner's
  fixed sum, after its existing dependent-measure/reflow handling.
- Other layout roles carry their own context; a wrapped row is not a shared-fill
  pool. Do not apply a row reservation to an `el`, float, Nearby slot or another axis.

For a mixed segment, interpolate visible box size and charge independently using
one eased progress. In its matching pool, remove the sampled child from the fill
weight total and include its sampled charge in the fixed budget instead. Use its
visible size for placement, alignment, wrapping, extents, clips and hits.

For the failing 600px-row case:

| Progress | Visible box | Budget charge | Fill sibling |
| --- | --- | --- | --- |
| 0 | 40px | 40px | 560px |
| 0.5 | 45px | 170px | 430px |
| 1 | 50px | 300px | 300px |

At completion the remaining fill unit is unchanged: removing weight `w` and charging
`w × u` from a pool with unit `u` leaves that same unit for the other fill children.
This also works for a jointly projected subset, including all fill children. Subtract
spacing once as today; do not add padding/borders again to border-box charges.

A charge is **not** `max(visible, reserved)` or a positive hidden margin. For example,
`max(px(700), fill)` can reserve 300px while drawing 700px beside a 300px sibling.
Preserve overflow and use visible totals for center/space-evenly alignment.

This fixes the demonstrated allocation loss without basis/weight blending: ordinary
40px → fill still has a 170px midpoint. Compatible weighted/min-max animations keep
the existing symbolic sampler, including the bounded-weight midpoint `(50, 200)`.

### B. Preserve the other layout roles, not just two numbers

Introduce a small native dimension-input view used by layout helpers:
`Declared(length)` or `Sampled(footprint)`. Store samples alongside effective attrs,
not in serialized `Length` tags. Retain original expression provenance; a convenient
pixel attr must not become the source of truth for semantic predicates.

Schematic endpoint footprint, captured from the **same native layout evaluation**:

```text
AxisFootprint
  intrinsic_outer          // contribution returned to bottom-up measurement
  initial_box              // pre-child-resolution sizing basis
  final_box                // unrotated visible border-box size
  parent_charge + scope    // parent/mount, planner role and axis
  policy                   // intrinsic/definite mode, fill demand, growth, aspect rules
  coordinate_context       // node-box and parent-planner units, layout generation
```

The initial basis and final box can differ after content reflow. Keep them distinct;
forcing a final box before child resolution can itself change the endpoint's children.
Intrinsic contribution is not generally the visible size either: native fill preserves
intrinsic content during measurement, even though it expands/shrinks during resolution.

Route the existing length-dependent decisions through the view, without reimplementing
measurement or introducing an allocation solver:

| Consumer in `tree/layout.rs` | Sample behavior |
| --- | --- |
| `resolve_intrinsic_length`, outer measurement and child measured-size reads | Use the sampled intrinsic contribution; keep normal live measurement for unowned axes |
| `resolve_element_sizing`, content finalization/alignment | Use the initial basis for descendant resolution and the final box at normal finalization, not a paint transform |
| `build_row_layout_plan`, `build_column_layout_plan` | Separate budget charge from visible child extent; keep existing arithmetic and reflow inputs |
| `container_prefers_fill_*`, `length_requests_fill` | Preserve parent-facing fill demand; do not infer it from a pixel surrogate |
| `available_*`, fill-enabled planning, `length_allows_content_expansion` | Preserve descendant allocation/growth policy instead of switching intrinsic layout to definite layout accidentally |
| Image inference, wrapped rows, text-flow/float planning, `force_child_width` | Respect the actual role and parent-imposed constraints; do not blindly replay a shared-pool charge |

**Policy interpolation must also be explicit.** For mixed samples, numeric channels
use the same eased progress. Predicates that choose different sizing behavior become
typed policy blends, not `t > 0` / `t >= 0.5` boolean switches:

- Implicit parent fill demand blends from 0 to 1; multiple child demands combine as
  `max`, preserving ordinary `any` behavior at the endpoints. Blend the finite intrinsic
  and fill-sizing candidates before resolving children, rather than snapping the parent.
- Intrinsic→definite child allocation blends the ordinary unpooled/pooled planner
  results from shared seeds. An explicitly sampled child still uses its own box/charge;
  do not interpolate it twice. Keep native 0/1 fast paths and normal final placement.
  Reuse prepared reflow inputs, never run two mutating live branches. Any missing
  dependent geometry must be an explicit private-workspace query, counted in `Q`.
  Queries consume frozen inputs; no recursive resolver or hidden fixed-point loop.
- Content-growth and image-inference choices blend their ordinary numeric candidates
  in the appropriate layout stage. Preserve normal reflow; do not blend paragraph
  fragments, freeze descendant frames, or make an owned final axis grow past its sample.
- Do not interpolate `MaxContent` as infinity. Retain the available-space policy and
  blend finite sizing candidates. Preserve existing paragraph and axis-specific rules.

These are **new mixed-length semantics**, not changes to compatible/static lengths.
A small dimension-policy extraction is now required; the former "ordinary pixel
attrs need no allocator changes" assumption is withdrawn. The exact footprint fields
must earn their place through the native equivalence tests below, not a generic graph
of every layout operation. Policy blends are specified here but not yet validated.

### C. Capture, interruption and cache integration

- Record the last live planner charge while the planner has it, including cache-hit
  restoration. Existing frames cannot reconstruct it. Reuse measured/cache data for
  other facts where valid; measure the cost of any additional per-node result fields.
  This is current layout output, not an idle target-history map.
- The first-write journal captures the **whole footprint** before patch/scale/topology
  writes. Interruption and context retargeting re-anchor every channel over the existing
  timing/remaining-curve rules. Do not construct a fresh pixel source that loses charge.
- A captured mixed sample remains a mixed footprint even when its visible source and
  new destination are both pixels. Only genuinely compatible semantic sources use the
  legacy interpolation path. Capture flat values/policy coefficients, not recursively
  nested prior animations or unbounded source-expression history.
- Charges belong to a parent allocation scope and its coordinates, not to a rotated
  visible AABB. Preserve original source scale context. A reparent/role change releases
  the old reservation as ordinary topology work; establish the source's new-scope charge
  from layout of its captured box there, not by transporting an old pool's debit.
- Ghosts retain the captured footprint as their baseline even when exit animates only
  alpha. Strip change policies/runs, not the geometry/allocation footprint; an exit
  that owns the dimension uses it as its shared-core source. Pruning releases it.
- Foreign mixed peers in endpoint projections carry their full frozen footprints.
  Replacing a peer with a symbolic boundary keyframe clears that field's sample override.
  Changes in charge, intrinsic contribution or policy invalidate context even when the
  visible box is unchanged. Update measure/resolve cache keys and completion dirtying.

### D. Native proof required before public acceptance

1. Keep the six pixel-substitution tests as negative controls. Add footprint replay
   tests on both axes: native symbolic endpoint versus sampled endpoint at progress
   0 and 1 **with the sample override still installed**. Compare all affected boxes,
   intrinsic contributions, positions, content extents, clips/hits and scroll geometry.
2. Exercise 0.25/0.5/0.75 and values approaching both endpoints. Stable wrapping/context
   must not jump when metadata is removed. An actual line break may remain discrete;
   that is not a waiver for allocation or implicit-fill discontinuities.
3. Cover min and max bounds, recursive expressions, nonempty intrinsic children,
   multiple/simultaneous sampled children, fractional/large weights and rounding,
   exhausted/no remaining fill pools, center/space-evenly alignment, overflow and scrolling.
4. Include two policy traps before integration: an automatic parent's fill preference
   changed by a bounded-fill child, and a content-sized row/column whose numeric width/
   height would enable fill distribution. Also cover image syntax-dependent aspect
   inference, wrapped rows/floats, rotation/scales and parent-imposed sizing.
5. Then prove interruption from a mid-sample, policy removal, reparent/ghost handoff,
   different deadlines, cache replay/permuted queries and explicit/change equivalence.
   Ordinary/static and compatible-animation baselines must remain unchanged.

The reservation algebra is established for definite shared pools, not the full repair.
This gate closes only after the footprint **and policy** native tests pass; returning
raw symbols at the last instant cannot conceal a discontinuous sampled limit.

## Decision: cached endpoint projections, not a layout-engine migration

Support the full width/height length matrix and `Animation.change/3` through the
existing animation core. Add a lazy endpoint resolver with **one private, reusable
layout tree**. Ordinary frames keep the existing effective-attrs → layout → refresh
path. Both animation frontends use the same sampler and resolver.

Remove the previous prerequisite to introduce `LayoutInputs`, `GeometryResult`,
`commit_geometry` and migrate every layout mutation/consumer. Isolation is needed
for endpoint queries; a universal live-state commit architecture is not needed to
provide it. Use sparse first-write captures and the allocation-aware dimension view
above; only that layout-semantic seam grows in scope.

Earlier alternatives are preserved in `7c3825c`: FLIP and basis/weight blending
change semantics; static final-tree endpoints fail different deadlines; per-query
render-tree clones waste work; a universal commit split adds unnecessary scope.
The reusable workspace stays selected, with measured—not assumed—performance.

`tree/layout.rs::run_layout_passes` already separates measure/resolve from refresh.
Use it on explicitly constructed private state: `Element::render_snapshot` retains
render fragments and drops measurement caches, so it is not a layout-copy helper.
Preserve existing resource, patch-prefix and incremental-preparation boundaries.

Complexity stays in the dimension-policy/planner-input seam, source capture, cache
validity and lifecycle/codec integration. No allocator replacement, new scheduler or
backend/render/event rewrite. Policy blending and whole-layout parity are the main risks.

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
- Incompatible lengths resolve complete expressions to allocation-aware footprints,
  then interpolate their visible sizes, contributions and policies. Do not silently
  hold mismatched leaves or discard fill reservations.
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

## Implementation design

```text
explicit keyframes ───────┐
patch-triggered change ───┼→ shared segment selection, easing and interpolation
enter / exit ownership ──┘                  │
                                   missing layout endpoints?
                                   no │             │ yes
                                      │   cached resolver / private layout tree
                                      └─────────────┘
                                              │
                           effective attrs + native dimension samples
                                              │
                                existing live layout and refresh
```

### 1. Sparse applied effects and source capture

Keep desired targets/policies in declared attrs. A first-write journal retains only
pending old values/sources and the latest successfully applied targets; active runs
retain their clock, optional presentation anchor and current segment endpoints.
No persistent idle target-history registry, Elixir diff/clock or per-pulse discovery.

- Reuse patch preimages. Validate non-length pairs before accepting that node's
  invalid target. Preserve successful-prefix effects even when later patches fail;
  the journal must survive error returns until the next admission/frame.
- Use gate C's full-footprint first-write capture, including affected wrappers and
  policies added later in a coalesced batch. Never resolve old content against newly
  patched children and call it the previous presentation. Work is proportional to
  affected attrs/geometry; no universal commit adapter or idle-policy scan.
- Start newly admitted runs with a fresh/presentation-aligned time, not a stale idle
  `latest_animation_sample_time`. Paint-only change effects must wake the runtime.

Create a once spec only for an actual change and use the ordinary sampler. Factor
small segment-selection/start/completion helpers; do not rewrite owner maps into a
track graph. Avoid per-tick keyframe generation, debug-string fingerprints and
quadratic active-id collection. Completion exposes terminal/base attrs before
pulses stop. Captured native presentation is not a GPU readback.

### 2. One layout-only query workspace

The native tree's resolver lazily owns one reusable `ElementTree`-shaped workspace,
shared by all endpoint requests, never one per run/cohort. Build it explicitly from
model/topology, needed runtime/scroll seeds and layout state; omit raw EMRG bytes,
registry/render fragments and renderer resources. Keep mutation private and retain
only compact endpoint footprints/context on runs. Release workspace contents when no mixed
segment needs them; do not create a global animation cache.

- Initially copy on demand; refresh the model snapshot only after actual layout-model
  changes. Use a coarse model epoch first, not another patch-replay/diff system.
  Viewport, metric and sampled-overlay changes update query context, not the model
  copy. Every snapshot refresh is visible in diagnostics.
- Reuse the workspace across projections/pulses. Restore previously overridden attrs,
  apply the new sparse projection, and use existing dependency dirtying and retained
  layout caches. A→B→A queries must equal fresh evaluations. A clean flag from the
  wrong context is never sufficient proof of reuse.
- Seed mutable runtime inputs from the same live context for each query. A prior
  query's scroll clamp/end-follow must not seed the next query. Restoring those inputs
  must dirty affected cached work too. Count reset visits; avoid resetting every node
  when only a few inputs changed. Frames/caches remain keyed results, not new inputs.
- Use shared attr composition: supplied raw overlays → scale preparation → interaction
  styles, retaining full/active/dirty modes. Thread viewport/scale and mutable resolver
  access through these and direct entrypoints, not just the actor. Supply projections
  directly; do not recurse into sampling or accidentally apply its first keyframe.
- Run normal measure/resolve on the workspace, collect footprints at their actual
  measurement/planning/finalization stages, then discard query effects. Live frames
  still use the same dimension view and layout algorithms and publish normally.
  No live rollback, new result/commit API or render/registry query pass.
- Use a narrow read-only image-metrics input for the layout lookup sites. Normal asset
  setup still runs for the real tree, including direct/headless paths; queries do not
  enqueue loads, alter authorization or reuse the mutating offscreen snapshot helper.
  Share immutable fonts/safely keyed text measurements, not asset locks across layout.

This deliberately spends one tree-shaped workspace to avoid a large ownership
refactor. Fresh isolated evaluation is the cache/isolation oracle. Reusing scroll,
paragraph, Nearby and allocation state safely is a test obligation, not an assumed
benefit of `Clone`.

### 3. Shared lazy endpoint resolver

For each adjacent segment:

1. Genuinely compatible semantic pair: keep existing interpolation. A captured mixed
   footprint is not made compatible merely by its visible pixel value.
2. Use a valid full source footprint in its original coordinate/scope context.
3. Derive context-independent footprint fields directly; reuse valid captured/cached
   fields. A constant visible size alone does not prove the complete endpoint known.
4. Only missing layout-dependent information runs a workspace projection.

Resolve the complete expression and whole keyframe context, including both axes,
insets, fonts, image aspect ratio and layout scale. Preserve terminal expressions.
An arbitrary previous frame is not an explicit keyframe's endpoint. Convert units
using the corresponding source/projection scale once, not a patched scale twice.

Cache by segment/projection identity and actual layout inputs: model epoch,
viewport/scale, allocation scope, relevant metric versions and full foreign samples.
Include footprint/policy differences, not only visible sizes. Exclude the
projection's own sampled writes, policy-only changes and unchanged asset dimensions.
Generic `Measure` is work classification, not an endpoint-cache key. Start with
conservative invalidation for other layout animations; no dependency graph.

### 4. Chosen concurrent rule: shared boundaries, frozen foreign samples

Mixed endpoints describe current layout context, not a prediction of future tracks.
Use this deterministic, bounded rule for both frontends:

1. Select all segments/owners at one sample time. Freeze full peer samples from existing
   endpoint caches before refreshing any endpoint. Cold entries use captured sources
   or their symbolic source keyframe, not another query's newly produced result.
2. For an endpoint boundary, project fields sharing that exact segment-boundary time
   to the corresponding keyframe together. Other fields keep the frozen sample.
   Group by actual boundary, not merely parent, patch batch or animation owner.
3. Query each distinct missing projection once, collecting all required footprint
   fields together. Include compatible fields reaching that boundary too; otherwise
   a sibling's final fill weight can be wrong.
4. On context change, anchor at the current sample and retarget over the remaining
   interval. Preserve the remaining easing-curve interval rather than restarting
   ease-in every pulse. Do not feed refreshed endpoints recursively into peer queries.
5. At boundaries/completion, apply normal owner/keyframe/ghost behavior and invalidate
   affected peer contexts. Multi-segment and loop boundaries retain existing semantics;
   a cohort is not a new persistent scheduling object.

Track context per property/field group, not just node: another independently timed
axis on the same node can affect layout. Evaluate whole projections and deduplicate
identical inputs. Different deadlines can require different queries each frame.

Why not cache one final tree? For two 40px → fill siblings in 600px, with durations
1s and 2s, joint final endpoints are 300px. At 1s the slow sibling is 170px under
that scheme; restoring the fast sibling to fill makes it **430px**, a **130px jump**.

The chosen rule is not yet proven for the full engine. The earlier row-only model
showed sampling-dependent retargeting, not a closed-form future-layout solution.
Test sparse/skipped pulses, curves and continuity in native layout. If reference
cases fail, revise the rule, not the promised matrix.

### 5. Declarative metadata and compatibility

Normalize wrappers to ordinary targets plus per-property policies, e.g.:

```elixir
%{width: :fill, animate_change: %{width: %{duration: 1000, curve: :linear}}}
```

Merge policies per canonical property/field group and retain them in normal attr
hashing/equality. Reuse duration/curve/attr validation. Remove only width/height
cross-variant rejection; validate other old/new pairs through shared rules.

Add one policy attribute to native/Elixir codecs and update EMRG/macOS host
compatibility/documentation. Existing length tags stay unchanged; footprints/policy
blends are native-only. Do not duplicate
targets on the wire, serialize presentation history, overload `:animate`/`:on_change`,
or add NIF entry points or per-frame BEAM calls. Keep BEAM collection work linear.

## Work budget and implementation order

Let `N` be model size, `R` active fields and `Q` distinct missing projections. Added
retention is one layout workspace `O(N)`, run-local footprints `O(R)` and compact
last-layout facts where existing node/cache output is insufficient—not `O(N × R)`.
Measure actual bytes, including idle-node result storage. Extra work is projection
preparation, `Q` layout evaluations and local policy arithmetic over existing planner
seeds. Keep plain/compatible fast paths; do not automatically run a full-tree query
for every policy. Count any dependent-policy query in `Q` as well. No claim that every
layout is local or that `Q` is globally at most two.

| Case, excluding ordinary live layout | Expected extra endpoint work |
| --- | --- |
| Genuinely compatible semantic pairs | No query or workspace allocation |
| Complete footprints provably context-independent | No query |
| Full captured source → fully derivable destination in the same role | No query |
| Known source footprint → unknown destination footprint | Typically one projection |
| Both footprints unknown | Up to two for one coherent segment context |
| Warm unchanged footprints and available policy inputs | No endpoint/policy query or model copy |
| Asynchronous peers or missing dependent policy inputs | Up to one evaluation per distinct missing projection per pulse; reuse the model copy |
| Real layout-model edits while endpoints are needed | Coarse snapshot refresh can be `O(N)`; measure this cost explicitly |

A pixel value alone no longer earns a zero-query claim: role, intrinsic/basis facts
and policy must also be known. This corrects the pre-gate work estimate.

1. **Dimension view and observation:** extract the existing length-dependent operations,
   preserving ordinary behavior. Record missing live planner/basis facts and expose a
   test-only footprint collector; run all existing tests before adding mixed behavior.
2. **Charge repair:** replay endpoints with internal samples and implement independent
   visible/charge interpolation. Pass gate A, including all/subsets of sampled fills,
   overflow and visible-total alignment; retain the six negative controls unchanged.
3. **Role/policy repair:** add intrinsic/initial/final channels and the typed policy
   bridges. Pass the full gate D matrix and near-endpoint tests, not just the 600px row.
   If a consumer still reads surrogate pixel semantics, migrate it before proceeding.
4. **Shared runtime/resolver:** connect full source capture, common sampler, one cached
   workspace, peer footprints, lifecycle and direct/headless paths. Compare cache hits
   and every optimized projection with fresh evaluation and different query order.
5. **Public acceptance and qualification:** only then relax width/height validation,
   add change metadata/codecs/docs and qualify all owners, segments and API equivalence.
   Measure cold starts, warm frames, model edits, policy blends and asynchronous peers.

No persistent-tree architecture, separate allocation solver or shadow-tree patch engine.
Small pure calculations over the existing planner seeds are justified by the policy
bridge; a second endpoint-layout algorithm is not. If copying dominates, copy known
changed inputs from existing effects. If layouts dominate, inspect cache validity and
visited nodes first. A failed memory/frame budget still blocks acceptance.

## Acceptance and validation

- Close gate D with full-footprint/scene parity, including unchanged visible sizes
  with changing allocation, intrinsic contribution or policy.
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
- Prove query isolation and normal live derived effects; no asset requests, leaked
  probe frames, stale dependency flags or cross-query scroll feedback. Compare A→B→A
  and permuted projection orders with fresh layout, including failed/pending assets.
- Confirm cold/warm query counts, zero endpoint work on compatible/compositor paths,
  no idle discovery scans and no regenerated specs. Measure layout/input-reset visits,
  model-copy count/bytes, retained/peak workspace memory and cache hits in large trees.
  Include many asynchronous tracks and frequent unrelated content/model edits; report
  cold-start and worst-frame costs separately rather than hiding them in averages.
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

The planning iteration used a standalone rational-arithmetic row model. The first
implementation step adds six native layout/sampler characterization tests for
endpoint substitution and records the blocker above. Runtime code and the public
API remain unchanged. Validation in `/workspace/emerge-animation` passed:

- `cargo test`: 1059 unit tests and 14 integration tests.
- `EMERGE_SKIA_BUILD=1 mix test`: 483 tests/doctests passed, 8 excluded.
- `cargo clippy --tests -- -D warnings`, `cargo fmt -- --check`, `git diff --check`.

Gate-repair planning re-audited the native length consumers and checked a rational
budget model across 504 pool/subset cases plus the quarter/mid/three-quarter samples
above. The six unchanged native reference tests were re-run and passed. This establishes
the charge algebra and existing behavior only—not repair/policy parity. No repair
implementation, performance benchmark or constrained-device qualification has been run.
