# Shared animation core: layout lengths and change transitions

Status: implementation started; blocked at the native endpoint-parity gate.
Reference tests are implemented; the resolver and `Animation.change/3` are not.
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

The private workspace remains a viable isolation approach, but the sampled-length
representation must be revised before continuing. At minimum it must retain the
parent's allocation reservation separately from visible size, including source
capture on interruption. Prove intrinsic sizing, wrapping and implicit parent fill
discovery too; two numbers are not yet established as sufficient. Do not silently
change ordinary bounded-fill allocation, existing weighted interpolation, or narrow
the length matrix to bypass this gate.

The design below is the pre-gate proposal, **not ready for production implementation**
until that representation is specified and its native parity tests pass.

## Decision: cached endpoint projections, not a layout-engine migration

Support the full width/height length matrix and `Animation.change/3` through the
existing animation core. Add a lazy endpoint resolver with **one private, reusable
layout tree**. Ordinary frames keep the existing effective-attrs → layout → refresh
path. Both animation frontends use the same sampler and resolver.

Remove the previous prerequisite to introduce `LayoutInputs`, `GeometryResult`,
`commit_geometry` and migrate every layout mutation/consumer. Isolation is needed
for endpoint queries; a universal live-state commit architecture is not needed to
provide it. Preserve required old sources with sparse first-write captures instead.

### Alternatives investigated

| Approach | Structural/runtime trade-off | Decision |
| --- | --- | --- |
| Blend fill weights/bases inside the allocator | Few probes, but changes mixed interpolation; full wrapping/intrinsic cases still need real layout | Reject: different semantics |
| FLIP or interpolate rendered boxes | Cheap motion but different wrapping, sibling allocation and hit/layout behavior | Reject: not layout animation |
| Resolve one joint final tree, then freeze every endpoint | Small code/cache; different deadlines can cause terminal jumps | Reject as general semantics |
| Full retained-tree clone for every query | Reuses layout but copies paint/registry payloads and repeats model allocation | Test oracle only |
| Universal geometry evaluation/commit split | Strong long-term isolation; broad mutable-helper/cache/patch migration, without eliminating endpoint layouts | Defer, not a prerequisite |
| Pure per-container allocation queries | Can be fast in definite independent cases; dependency/fallback machinery duplicates layout knowledge | Defer until profiling justifies a narrow extraction |
| One reusable layout-only projection tree | Existing algorithms, no live rollback, amortized model copy; extra layout on genuine misses | **Choose** |

The choice minimizes new ownership boundaries and repeated work, not just lines.
It is not a claim of measured speed. A private tree costs additional memory, and
asynchronous coupled tracks can still require endpoint layouts on every pulse.

### Code findings behind the choice

Paths below are under `native/emerge_skia/src/`:

- `tree/layout.rs::run_layout_passes` already separates measure/resolve from
  refresh. Invoke that machinery on private state rather than redesigning it.
- `Element::render_snapshot` is not a suitable layout copy: it retains render
  fragments while dropping measurement caches. Construct the query tree explicitly.
- Row/column planners already perform dependent child resolution. A separate
  arithmetic evaluator is not a complete shortcut for intrinsic/cross-axis layout.
- Layout has two `assets::ensure_source` sites plus a read in
  `resolve_element_sizing`. Isolate these narrow resource lookups, not all layout
  writes. `snapshot_tree_sources_for_offscreen` mutates asset state; it is not a
  read-only metric snapshot.
- `patch.rs` overwrites effective attrs and can resize frames before animation
  sync. Capture preimages before those writes, including their affected wrappers.
- `Measure` includes conservative animation invalidation; runtime-start hints
  ignore `SetAttrs`. Neither is a sufficient change-trigger/context-validity signal.

Complexity stays in source capture, endpoint-context correctness and lifecycle/codec
integration. No allocator replacement, new scheduler or backend/render/event rewrite.

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

## Implementation design

```text
explicit keyframes ───────┐
patch-triggered change ───┼→ shared segment selection, easing and interpolation
enter / exit ownership ──┘                  │
                                  missing numeric endpoints?
                                   no │             │ yes
                                      │   cached resolver / private layout tree
                                      └─────────────┘
                                              │
                                 ordinary effective-attr overlay
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
- Before patch geometry fast paths overwrite frames, save affected nodes' old
  unrotated boxes and scale context once. Capture even if a policy is added later
  in the same coalesced update. Scale/reparent edits must preserve affected sources
  before changing their ancestry; do not reconstruct old units through new parents.
- Preserve compatible logical samples too: a pixel box cannot reconstruct a
  weighted-fill source. Never lay out old content against newly patched children
  and call that the previous presentation.
- Keep these hooks local to update/patch mutations; no universal presentation commit
  adapter. Work is proportional to affected attrs/geometry, not all idle policies.
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
only endpoint numbers/context on runs. Release workspace contents when no mixed
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
- Run normal measure/resolve on the workspace, extract logical unrotated border-box
  dimensions, then discard query effects. Live frames still run and publish normally.
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

1. Compatible pair: keep existing interpolation, including recursive compatibility.
2. Where a transition requires its captured source, use that anchor in original units.
3. Evaluate constant pixel/min/max expressions directly; reuse valid numeric endpoints.
4. Only missing layout-dependent information runs a workspace projection.

Resolve the complete expression and whole keyframe context, including both axes,
insets, fonts, image aspect ratio and layout scale. Preserve terminal expressions.
An arbitrary previous frame is not an explicit keyframe's endpoint. Convert units
using the corresponding source/projection scale once, not a patched scale twice.

Cache by segment/projection identity and actual layout inputs: model epoch,
viewport/scale, relevant metric versions and foreign layout samples. Exclude the
projection's own sampled writes, policy-only changes and unchanged asset dimensions.
Generic `Measure` is work classification, not an endpoint-cache key. Start with
conservative invalidation for other layout animations; no dependency graph.

### 4. Chosen concurrent rule: shared boundaries, frozen foreign samples

Mixed endpoints describe current layout context, not a prediction of future tracks.
Use this deterministic, bounded rule for both frontends:

1. Select all segments/owners at one sample time. Freeze peer samples from existing
   endpoint caches before refreshing any endpoint. Cold entries use captured sources
   or their symbolic source keyframe, not another query's newly produced result.
2. For an endpoint boundary, project fields sharing that exact segment-boundary time
   to the corresponding keyframe together. Other fields keep the frozen sample.
   Group by actual boundary, not merely parent, patch batch or animation owner.
3. Query each distinct missing projection once. Include compatible fields reaching
   that boundary too; otherwise a sibling's final fill weight can be wrong.
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

The chosen rule is selected, not yet proven for the full engine. A row-only linear
arithmetic model preserved the synchronous 170px midpoint and simultaneous 300px
finish. For unequal deadlines, fast completion was about 427.04/428.51/429.25px at
30/60/120 samples per second: retargeting is sampling-dependent, not a closed-form
future-layout solution. Test sparse/skipped pulses, curves and terminal continuity
in native layout; do not claim frame-rate invariance or general correctness from
this model. If reference cases fail, revise the rule, not the promised matrix.

### 5. Declarative metadata and compatibility

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

## Work budget and implementation order

Let `N` be model size, `R` active fields and `Q` distinct missing projections. Added
retention is one layout workspace `O(N)` plus run-local endpoints `O(R)`, not
`O(N × R)`. Extra work is projection preparation plus `Q` real layout evaluations;
no claim that every layout is local or that `Q` is globally at most two.

| Case, excluding ordinary live layout | Expected extra endpoint work |
| --- | --- |
| Compatible pairs / constant-only mixed expressions | No query or workspace allocation |
| Captured symbolic → pixel change | No query |
| Known source → unknown symbolic destination | Typically one projection |
| Cold symbolic → different symbolic | Up to two for one coherent segment context |
| Warm unchanged projection / synchronized group | No endpoint layout or model copy |
| Asynchronous layout peers changing context | Up to one evaluation per missing projection per pulse; reuse the model copy |
| Real layout-model edits while endpoints are needed | Coarse snapshot refresh can be `O(N)`; measure this cost explicitly |

1. **Reference proof:** implement test/oracle cases for the full matrix, boundary rule,
   numeric/symbolic box parity and interruption sources. Pixel substitution can change
   parent fill discovery/intrinsic accounting; stop on divergence, not just wrong boxes.
2. **Minimal shared core:** sparse first-write effects, fresh clocks, shared segment
   selection/composition and change admission; preserve partial preparation/lifecycle.
3. **Cached projection resolver:** one private workspace, narrow metric reads, shared
   layout and cache reuse, plus direct/headless integration. Compare every optimized
   query against fresh evaluation, including permuted query order.
4. **Public/qualification:** metadata/codec/docs, all owners/segments and explicit/change
   equivalence; measure cold starts, warm frames, model edits and asynchronous peers.

Do not add a persistent-tree architecture, pure allocator or incremental shadow-tree
patch engine speculatively. If model copying is the measured bottleneck, first copy
known changed node inputs using existing applied effects; if layouts dominate, inspect
cache validity/visited nodes before proposing allocator or dependency work. A failed
memory/frame budget blocks acceptance; simplicity is not permission to ignore it.

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

No performance benchmark or constrained-device qualification has been run.
