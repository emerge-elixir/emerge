# Shared animation: remaining implementation plan

Status: in progress; P1–P6 partially implemented, including field-composed coupling,
native historical goal receipts and actor/direct lifecycle cases. No package is complete.
[Coverage and remaining gaps](artifacts/shared-animation-remaining/coverage.md),
[proof decisions](artifacts/shared-animation-remaining/decisions.md) and
[locked cost baseline](artifacts/shared-animation-remaining/performance/README.md)
record implementation slices and remaining gaps. Covered coupled mixed deadlines
and ongoing unchanged-context motion now pass; self-axis and broader inputs remain open. No remaining
gate is satisfied merely by documenting it.
Worktree: `/workspace/emerge-animation`, branch `plan/shared-animation-core`.
This is the execution detail for [the active checklist](active-shared-animation-core.md),
not another feature, approval stage, or public-enablement plan.

## 1. Scope, baseline and completion contract

Finish every unchecked item in the active checklist:

| Remaining item | Work packages |
|---|---|
| Runtime/media/scroll provenance and combined causes | P2, P3 |
| Mixed dependency/context retargeting and deadline changes | P2, P3 |
| Phase changes, cancellation, arrivals and terminal sources | P5 |
| Removal/reparent/wrap/float/Nearby transport and ghost baselines | P4, P5 |
| Actor/direct/headless and final-damage qualification | P6, P8 |
| Full-model memory, release work and platform checks | P1, P7, P8 |
| Final API/artifact/documentation audit | P9 |

Existing baseline, not new work to repeat:

- One native core, full allocation footprints, independent field clocks, automatic
  finite groups, transactional publication, frozen inputs and bounded retries.
- Public list-based `Animation.change/3`, resolved-content watchers, EMRG 9 and
  macOS protocol 14; no separate enablement remains.
- Mixed ancestor boundary releases now cover current full samples, old/new native
  destination witnesses, late/skipped frames, both axes and rendered/input behavior.
- Original baseline full CI: 1,296 Rust units, 14 integration tests, 499 Elixir
  tests/doctests, zero Dialyzer errors. Standalone Mix: 494 passed, 8 excluded.
- The publication probe still copies a full private layout model. Recorded 20k-node
  length/moving cases use approximately 313–315 MiB process RSS, versus approximately
  164–165 MiB for paint/pixel controls. Release takes approximately 14–19 ms.
  These are desktop publication observations, not exact live-heap sizes or device results.

**Implementation completion** requires all valid supported scenarios below to
publish and settle correctly, with no unexplained persistent release failure.
**Qualification completion** additionally requires the named performance and actual
backend/device evidence. Missing hardware remains an explicit open qualification
item; it must not be described as a successful test or silently deleted from scope.

### Non-negotiable constraints

1. Preserve existing intentional changes in this and neighboring worktrees. Do not
   use stale restore/migration scripts or reset the accumulated implementation.
2. Keep one animation core, one renderer-local private query workspace and the
   existing native layout engine. No per-owner trees, global animation caches,
   mandatory whole-tree future dependency graph or iterative fixed-point solver.
3. Preserve `AxisFootprint` geometry, charges, parent extents, policy channels,
   placement roles and units. Visible pixels or interpolated fill weights are not
   substitute allocation evidence. Animated min/max remains unsupported; static
   bounds must continue to affect native layout normally.
4. Never weaken `ReleaseMismatch` tolerances, restore old live inputs, silently
   cancel valid animations, invent a remaining duration, or use retries to hide a
   missing proof. Distinguish an external layout change from a sample-removal jump.
5. Same inputs and schedule must replay identically regardless of query order.
   Context-retargeted trajectories need not be identical across different schedules.
6. Preserve first-write published sources, successful patch prefixes, generation
   checks, admission times, independent clocks and exclusive publication authority.
7. Queries must not hydrate assets, publish scenes/registries, mutate live state,
   or hold font/asset locks during layout. New native work must respect existing
   thread ownership and Rustler scheduler/lifetime rules.
8. Count actual work and retained allocations. Sparse projection records do not
   establish group-local evaluation, low RSS, or cheap destruction.

## 2. Execution order and evidence

Execute these as small reviewable slices, each with its failing regression first:

| Package | Dependency | Main output |
|---|---|---|
| P1 — evidence and work accounting | none | Durable baseline, phase timings and coverage ledger |
| P2 — dependency-aware continuation | P1 | Independent branches and bounded mixed feedback handling |
| P3 — composed input provenance | P2 proof interfaces | Combined-cause historical/current native witnesses |
| P4 — topology and allocation transport | P3 provenance interfaces | Published structural evidence and role-aware source transport |
| P5 — lifecycle and terminal sources | P2–P4 | Complete phase, hold, cancellation and ghost behavior |
| P6 — runtime/publication integration | start fixtures in P1; finish after P5 | Shared direct/actor traces, damage and backend-facing correctness |
| P7 — performance optimization | profile in P1; finalize after P4–P6 | Smaller private model and bounded cold/warm/release costs |
| P8 — platform/device qualification | P6–P7 | Actual backend and device reports |
| P9 — audit and closure | all applicable evidence | API/artifact/docs audit and explicit completion status |

P6 fixtures and P7 profiling may begin early, but do not run performance measurements
concurrently with builds/tests. Avoid simultaneous broad edits to the same resolver,
publication or node-storage code. There is no date estimate until P1 identifies the
size of the structural transport and memory changes.

Store new evidence under `plans/artifacts/shared-animation-remaining/`:

- `coverage.md`: scenario, contract, reproducer/test name, status, commands, result.
- `decisions.md`: demonstrated dependency/transport rules, alternatives rejected,
  native evidence and any unresolved design blocker.
- `performance/`: immutable source/build identity, host, raw trials, phase metrics,
  retained-memory measurements and comparisons.
- `platforms/`: backend/renderer/device, driver versions, artifacts, tests, exclusions
  and presentation/cadence evidence.

Use exact source snapshots or commits for qualification. A dirty worktree's HEAD
alone is not its implementation identity. Temporary logs are useful during work but
are not the only surviving evidence for closing a gate.

## P1. Establish the remaining failure and cost baseline

**Files:** `animation/lengths/tests/{driver,support,continuation,rendered}.rs`,
`layout/projection.rs`, `animation/lengths/inspection.rs`, `stats.rs`,
`benches/shared_animation.rs`, existing runtime/backend test harnesses.
Path shorthand: `animation/...`, `layout/...` and `layout.rs` refer to
`native/emerge_skia/src/tree/`. Other native `tree/...`, `runtime/...`, `events/...`,
backend paths and top-level Rust files are relative to `native/emerge_skia/src/`.
Benchmarks are under `native/emerge_skia/benches/`; Elixir paths are repository-relative.

### Tasks

- [x] Create a coverage ledger for every unchecked active-plan item. Distinguish a
  reproduced failure, a passing-but-unqualified case, and an unavailable platform.
- [ ] Add minimal traces for: unrelated mixed branches; upward mixed dependence;
  multiple simultaneous boundary drivers; boundary plus viewport/font changes;
  seed membership changes; reparent between allocation roles; interrupted ghost
  cleanup; final output under queue pressure.
- [ ] Extend query inspection to explain which dependency/provenance check failed,
  without exposing a public group API or retaining an unbounded diagnostic history.
- [ ] Add actual counters for dependency/ancestry visits, model copies and bytes,
  native measure/resolve visits, seed resets, projection slots and source-journal
  records. Keep live/peak retention separate from cumulative work.
- [ ] Split timings into admission, source capture, private-model construction,
  query/layout work, attrs application, live layout, render/registry construction,
  commit, workspace destruction and ghost cleanup.
- [x] Preserve terminal-query and destruction counters after the workspace is
  dropped. The current probe loses those workspace-local counters at release;
  export a bounded measurement receipt or renderer-local numeric totals, not the model.
  Implemented for ordinary dimension-runtime retirement, with repeated-settle tests.
  Totals live on one tree incarnation; whole-tree shutdown/replacement and internal
  resolver rebuild disposal still need separate phase attribution.
- [ ] Re-run the 5k/20k × 1/64-owner baseline, adding mixed boundaries, content
  watchers, failures/recovery, topology edits and repeated start/settle cycles.
  First locked matrix: 72 processes, original four cases plus mixed boundary and
  independent-panel cases, with subsequent mixed-loop cancellation/settle. Immutable
  source archive, binary digest and ordered warm samples are retained. Other cases,
  live/peak bytes and full phase accounting remain open; no cost gate passed.

**Exit:** every remaining behavior has an executable reproducer or an explicitly
identified evidence gap; release time is attributed rather than assumed to be only
allocator destruction. No performance improvement is claimed from instrumentation.

## P2. Make mixed continuation dependency-aware

**Files:** `animation/lengths.rs`, `animation/lengths/continuation.rs`,
`animation/groups.rs`, `layout/dimensions.rs`, `layout/projection.rs`.

### P2.1 Independent and disjoint contexts

- [ ] Classify changed projection inputs relative to each consumer: ancestor,
  descendant, same allocation pool, cross-axis/wrapping dependency, or potentially
  independent branch. Use retained ancestry/group data; do not build future states.
- [x] Reproduce whether an unrelated mixed animation invalidates a finite release.
  The current ancestor-only check must not become a blanket assertion that every
  changed node in a global projection is a dependency.
- [x] Prove independence with native comparison of the **complete combined release**.
  A candidate prior-input projection may retain unrelated current inputs while
  restoring witnessed causal inputs. Its full consumer footprints must match the
  recorded prior targets before it can authorize continuation.
- [x] Preserve joint-release checks: independently valid per-owner targets are not
  sufficient if simultaneous sample removal changes their allocation.
- [x] Keep consumer-relative eligibility outside owner-independent proof caches.
  If proof construction becomes consumer-relative, key it by that relationship or
  a verified common cohort; do not reuse the first consumer's ancestry decision.

Implemented slice: original **and** hybrid native targets are checked before
independence can preserve an anchor. Same-pool mismatches decline continuation;
corrupted original targets still fail. Matching replaced-input classifications
share batches, with eligibility rechecked per consumer. Actual installed foreign
samples get a final combined-release check when they differ from provisional inputs.
The 64-owner fixture stays within 8 queries/attempt and one model copy. Explicit
same-pool/wrapping classification, the rest of the matrix and broader feedback
semantics remain open; these checks do not complete P2.

**Tests:** two independent animated panels; two consumers sharing one witness with
opposite dependency directions; static/content wrappers; sibling weighted pools;
cross-axis wrapping; unrelated paint changes; query/owner traversal permutations.

### P2.2 Upward and mutually coupled mixed inputs

- [ ] Identify finite intrinsic dependencies that should already belong to one
  automatic group. Correct group membership before adding a release exception.
- [ ] Keep loops outside finite barriers. For mixed loop inputs, define one explicit
  current-frame snapshot/evaluation rule shared by regular, enter, exit and change.
- [ ] Prototype a bounded frame-local schedule: freeze known presentations once,
  resolve required current boundary samples from ordinary owner endpoint snapshots,
  then query the affected release with those actual samples.
- [ ] Verify that each supplied foreign sample equals what its owner will actually
  apply in the same transaction. A retarget/continued anchor must not change that
  owner after consumers have used a different provisional sample.
- [ ] Where release itself changes a foreign destination, specify and test the
  discrete retarget rule using the existing clock and source. Do not change an
  unaffected trajectory or add a fabricated duration to make the proof pass.
- [ ] Validate prior/current foreign destinations and prior/current combined release
  footprints. Batch equal immutable requests; cap work by current affected records,
  not elapsed segments or the number of failed attempts.
- [ ] If a cyclic case cannot be certified with this bounded temporal rule, record
  the counterexample and revise the rule/grouping. A permanent retry, unconditional
  acceptance, or recursive convergence loop is not an implementation of the case.

**Tests:** mixed loop child driving finite content ancestor; loop wrapper driving
both ancestor intrinsic size and descendant allocation; two coupled drivers; multiple
boundary resets in one frame; a held ancestor; cancellation during coupling; source
and destination corruption; late and skipped clocks under fixed reproducible schedules.

Implemented P2.2 subset: unchanged-context finite releases use a once-frozen
loop sample, native forecast/retained/boundary destination witnesses and ordinary
current-presentation retargeting for genuine feedback drivers. Leaf clock inputs
preserve query provenance without a history chain. Shared-pool/upward deadlines,
resets/large skips, multiple weighted loops, both axes and owner/raster/retry checks
pass. The rule now also covers eligible active/held consumers: 432 native continuous
traces and retry/reset/hold/raster checks pass. Exact forecast evidence is preferred
even when the latest driver happens to reproduce its sampled value. Self-axis and
composed causes remain open; D7 isolates model-plus-cancellation and first
model-changing change/ghost admission gaps. Do not waive these checks.
The latest locked 64-loop upward benchmark still fails (~76ms warm, ~87ms release,
~377MiB RSS at 20k); 194 steady native queries versus 257 before. No P2/P7
completion claim follows from bounded records. See [baseline 03](artifacts/shared-animation-remaining/performance/README-03.md).


D8 extends the subset with axis-qualified self-node feedback/independence and
field-composed numeric/resolved drivers, including rotation, wrapping and imposed
sizing tests. It fixes first change/ghost admission and combined cancellation
provenance with original native receipts. The broad checkboxes remain open because
cross-axis/phase/input/topology qualification is incomplete.

### P2.3 Noncanonical footprints and phase continuity

- [ ] Keep pixel substitution limited to native-proven equivalence. Extend full-
  footprint witnesses for parent-imposed sizing, fractional policy channels,
  rotation, wrapping and local-scale charge units instead of weakening that gate.
- [ ] Audit equal resolved goals under changed projections: irrelevant input changes
  must not repeatedly restart easing or stretch an established hold interval.
- [ ] Cover source-to-target, hold, terminal, retained-pixel and repeat transitions,
  including differing width/height owners and curves.

**Exit:** valid dependencies release under native full-footprint checks, independent
branches do not block one another, corrupted witnesses still fail, and query-order
permutations do not change output. New query/work bounds are recorded per fixture.

## P3. Compose model, viewport, font, media and runtime provenance

**Files:** `animation/{source,content,frame}.rs`, `animation/lengths.rs`,
`layout/projection.rs`, `renderer.rs`, `assets.rs`, `services.rs`,
`runtime/tree_update.rs`.

### P3.1 Explicit evidence identities

- [ ] Define an internal evidence description binding published model/topology,
  mount identities, relevant clock intervals, declarations, viewport/global and
  inherited scale, font snapshot/metric namespace, image dimensions and runtime seeds.
  Reuse existing records; do not introduce parallel source histories.
- [ ] Compose existing `PreviousModel`, `PreviousViewport`, `PreviousInputs`,
  `PriorTarget` and foreign witnesses through one request-building path where possible.
  Do not let independent per-cause success excuse a failed combined release.
- [ ] Replay old causal inputs together with old clock inputs where required, while
  leaving the actual tree and eventual output on current inputs. Never compare an
  old cached goal against an accidental mixture of old viewport and new loop phase.
- [ ] Bind proof receipts to the exact published source and candidate authority.
  Keep only the committed source plus the latest attempt; discard superseded candidates.


Implemented D8 subset: opaque native goal receipts bind exact original query
inputs and results for historical clock sources. Context release replays the
original clock projection with old environment/model facts, not a mixture of new
clock phase and old environment. D7's two failures now have positive tests; strict
receipt identity and corrupted-target failure/retry checks pass.

### P3.2 Seed membership and resource transitions

- [ ] Journal first-write runtime/scroll preimages, including explicit absence, for
  nodes that become or cease to be scroll, focus, hover or other seed candidates.
- [ ] Replay membership additions/removals only when old state is known. Removing
  the equal-membership guard without replacement evidence is forbidden.
- [ ] Distinguish source offsets from derived scroll ranges; native layout must
  recompute/clamp the latter using the selected context, without writing to live state.
- [ ] Cover image unavailable→available, dimensions/aspect changes, same-size content
  replacement, eviction/reload and stale load completions. Old scenes retain old facts.
- [ ] Cover font fallback→loaded, replacement under the same family, failed loads,
  wrap/baseline changes and renderer-local namespace isolation.
- [ ] Keep opaque changed metric epochs rejected unless the caller supplies genuine
  replayable metrics. An epoch number is not a font snapshot.


Membership addition/removal now uses original native receipts when old seeds
cannot be replayed against the new model baseline. This does not remove the
membership guard from ordinary replay, or authorize opaque changed metrics.
The full scroll/font/media/stale-completion matrix is still required.

### P3.3 Combined-cause matrix

Require directed tests for these high-risk combinations, then pairwise coverage of
remaining factors rather than an impractical full Cartesian product:

| Combination | Required result |
|---|---|
| Pixel/mixed boundary + viewport/global scale | Current native allocation; no old-phase rewind |
| Declaration edit + font/image replacement | Old joint witness replayable; new native geometry/pixels |
| Local-scale change + weighted parent charge | Correct own versus parent units |
| Scroll membership/range change + content transition | Correct clipping, clamping and listener freshness |
| Hover/focus change + font/media input | One coherent frozen query context |
| Several causes + failed query/output + later retry | First source/start preserved, latest inputs published |

**Exit:** valid combined changes recover without another unrelated input; failures
preserve publication and bounded evidence. Queries never hydrate or borrow live mutable
font/asset state. P4 handles structural changes; declaration-only proofs remain barred
from authorizing a changed topology.

## P4. Transport published allocation sources through topology changes

D9 implements the directed native transport slice: sealed published attachment
sources, native reservation conversion, relative scales, role/root/remount tests,
new-role looping forecasts, and held-member migration. See decisions D9 and
`coverage.md` for exact coverage. D10 fixes the same-ID retained-image render gap;
atomic layout/scene provenance under concurrent asset replacement remains open.
The checklist below remains a package-level qualification target, not a claim that
all structural combinations or retained-memory costs are complete.

**Files:** `tree/patch.rs`, `tree/element.rs`, `animation/{source,groups,frame}.rs`,
`animation/lengths.rs`, `layout/{dimensions,projection}.rs`, relevant layout modules.

### P4.1 Structural provenance before mutation

- [ ] Capture the first published parent/mount, child order, placement role, Nearby
  mount information and relevant declaration/runtime preimages before detachment,
  removal or replacement. Include affected animated descendants and retained scopes.
- [ ] Distinguish same-mount reparenting, new mount with a reused numeric id, ghost
  cloning, root replacement and an unpublished insertion. Only published identity
  may supply an interruption source; remounts cannot inherit stale tracks implicitly.
- [ ] Retain only the changed structural closure and required removed-node facts.
  Repeated edits before publication retain the original published preimage and latest
  candidate, not B/C/D history. Large edits may require O(changed subtree) evidence;
  report it rather than claiming constant storage.
- [ ] Extend private replay to reconstruct that old closure in the one workspace,
  or obtain a native old-model witness before replacing its baseline. In either
  design, retries must retain sufficient proof after a failed new-model query.
- [ ] If using receipts captured before model replacement, bind them to the exact
  prior combined projection, context, mount and target—not just copied `Track.to`
  values or hashes. They must not authorize a later unrelated candidate.

### P4.2 Source transport contract

Before coding role conversions, put executable rules in the evidence ledger:

- Same mounted owner + discrete reparent: source is its published full presentation;
  existing run clocks/deadlines remain authoritative. New group membership must
  preserve already admitted hold work.
- Root/ancestor position changes are ordinary layout unless a position field is
  animated. Width/height transport does not introduce implicit FLIP motion.
- Descendants lay out under current inheritance, constraints and clipping. Preserving
  an owner's source box must not freeze its subtree or scale a cached painting.
- Own geometry and pool charges use different scales. Record their origin units;
  do not multiply every channel by the same factor.
- Old reservations belong to the old scope. New reservations must be derived and
  checked under the new native role; copying a scope id onto the old footprint is
  not a valid conversion. Avoid double charging or orphaning reservations.

### P4.3 Implement and validate role conversions

- [ ] Begin with same-role reparenting between fixed-size flow parents, then unequal
  global/local scales and static bounded siblings.
- [ ] Add Row↔Column/cross-axis behavior and changes to content/fill ancestry.
- [ ] Add wrapped-line migration, paragraph/baseline effects, floated children and
  transitions between flow and float placement.
- [ ] Add flow↔Nearby, Nearby slot/host changes and nested Nearby boundaries; retain
  their blocker/clip/input semantics instead of treating them as host-pool children.
- [ ] Add parent removal, root replacement, subtree replacement and remove/reinsert
  with both retained and new mount identities.
- [ ] Where `AxisFootprint` cannot express transported origin and destination
  obligations safely, extend the internal dimension-input representation explicitly.
  Prototype each role mapping against native layout before generalizing it.
- [ ] Atomically migrate group membership, tracks, source evidence and registry
  invalidation only after preflight. On failure, desired patch state may remain,
  but the last published scene/registry/source must remain valid.

**Tests:** both axes/modes/scales; source width and charge preservation; destinations
against native layout; static sibling allocation; wrap/float/Nearby role changes;
local-scale descendants; old-scope/mount corruption; failed first move, second move,
reversal to original parent and retry; successful decode prefixes under both policies.

**Exit:** transport rules have native allocation oracles, not just screenshot equality.
Valid role changes settle without permanent release failure; no stale-mount source,
extra history tree, leaked hold or duplicated charge remains.

## P5. Complete phase, ownership and ghost lifecycle behavior

**Files:** `animation/{regular,timing,fields,admission,change,groups,source,frame}.rs`,
`tree/patch.rs`, `runtime/tree_update.rs`, lifecycle tests.

- [ ] Cover three or more segments, finite repeats and loops at just-before/exact/
  just-after boundaries, huge skips and terminal finite-repeat exhaustion. Work must
  not grow with skipped cycles.
- [ ] Test width/height and disjoint paint lanes with different clocks, including
  enter→regular, regular/change interruption, exit precedence and terminal handoff.
- [ ] Preserve original-first-segment captured sources only where eligible. Later
  segments/cycles and new mounts must not accidentally reuse them.
- [ ] Audit terminal field application before ownership disappears: final attrs,
  removal of samples, source acknowledgement and subsequent ordinary preparation
  must agree in Full/Active/Dirty paths.
- [ ] Test early/late cancellation, policy removal, node removal, arrivals during
  motion/hold/release and repeated interruptions. Cancellation cannot truncate
  previously admitted hold work; an unrelated arrival cannot stretch an unchanged
  trajectory. No loop or paint-only lane becomes a finite waiter.
- [ ] Complete ghost source tests for text baselines, wrapping, images/SVG, padding,
  borders, clipping, scroll ranges, local scales and nested animated descendants.
- [ ] Capture interrupted enter/change/regular presentation for exit; newest exit
  declaration wins. Cleared or invalid exit policy must not create a stale ghost.
- [ ] Verify terminal ghost output, committed retirement, structural cleanup output
  and eventual idle as separate transitions. Neither output may be lost merely
  because activity changes during commit.
- [ ] Remove ghost handlers/focus/hover/drag ownership immediately as specified;
  preserve visual allocation until its authorized release/cleanup.
- [ ] Check retained-record release after cancellation, policy removal, last owner
  completion, ghost cleanup and renderer stop. Idle content watchers are a separate
  intentional state: no active pulse, with their retained workspace counted honestly.

**Exit:** every phase trace preserves publication on failure and reaches the correct
terminal scene plus eventual `Skip` when nothing else is active. No dangling groups,
handler ghosts, stale terminal layer, source history chain or lost cleanup frame.

## P6. Qualify publication, input and damage through real runtime paths

**Files:** `runtime/{tree_update,tree_actor}.rs`, `events/runtime.rs`,
`events/registry_builder.rs`, `actors.rs`, `render_scene.rs`, renderer/backend output
paths, `lib/emerge_skia/test_harness.ex`, native and ExUnit integration tests.

- [ ] Run the same ordered input/update traces through `TreeUpdateEngine` with the
  direct event runtime and through the actor/channel path. Compare meaningful output
  identities, hit results, listener precedence, buffered-input replay and activity.
- [ ] Exercise bounded channel pressure, delayed registry installation, overwritten
  intermediate scenes, pending stop and renderer failure/recovery. Use deterministic
  barriers/latches, not timing sleeps as the sole synchronization mechanism.
- [ ] Define which intermediate frames can coalesce and which semantic states must
  reach publication/installation. A final frame or ghost cleanup must not be stranded
  because no further animation pulse is requested.
- [ ] Verify stationary cursor transitions when geometry moves beneath it; wheel,
  drag-scroll, focus/IME and Nearby blockers must use the installed scene geometry.
- [ ] Test final damage after sample removal, partial preparation and cached payload
  reuse: old-only pixels disappear, newly exposed pixels draw, cache hits cannot retain
  a stale transform/clip, and terminal/cleanup frames reach the render path.
- [ ] Replay old scenes after new input/model/media updates; compare retained and
  fresh renderer raster output. Pair pixel assertions with matching event assertions.
- [ ] Add public ExUnit traces for descendant-only content change, mixed boundary
  release, ownership handoffs, ghost cleanup and supported reconciliation-driven
  topology changes. Also test lower-level native patch operations separately where
  the public reconciler represents a move as remove/mount instead.
- [ ] Keep persistent-error backoff bounded and pulse-only. External changes and
  explicit retries must recover promptly without changing admission clocks.

**Exit:** direct and actor paths agree on semantics; publication and physical drawing
are measured separately. Simulated queue success is not presented as compositor/device
qualification. General backend work remains owned by the Linux GPU plan.


D8 adds real actor/direct coupled regular/ghost traces (registry/raster equality,
terminal/cleanup/cancellation/idle and old-scene replay), plus capacity-one
publication backpressure and latest-scene overwrite. These directed tests do not
close headless parity, permanently blocked stop, all input modes or device damage.

## P7. Reduce full-model memory and cold/release work

**Files:** `tree/element.rs`, `layout/{projection,dimensions}.rs`, `animation/lengths.rs`,
`layout.rs`, invalidation/render/registry caches, `benches/{shared_animation,layout,renderer}.rs`.

### P7.1 Measure ownership and allocation first

- [ ] Measure inline node size, arena capacity/holes and owned heap allocations for
  declarations, base/effective attrs, layout caches, render/registry state, projections,
  fonts/assets and source journals. Separate estimates from allocation-profiler data.
- [ ] Attribute the current 20k-node release spike to native query, live layout,
  output construction, acknowledgement, destructors and page release.
- [ ] Measure repeated start/settle and cancellation/retry cycles after warm-up;
  distinguish allocator-retained pages from increasing live retention.

### P7.2 Shrink the single private model

- [ ] Prototype sharing immutable declarations/topology with copy-on-write mutation,
  while keeping renderer-local mutable layout scratch and frozen query inputs.
- [ ] Remove unnecessary inline render/wire/registry/presentation storage from private
  query nodes. Prefer separating cold renderer state or lazily allocating it in the
  common node model over introducing a second layout algorithm.
- [ ] Ensure query layout cannot accidentally instantiate renderer caches, hydrate
  assets, retain live mutable node state or read a changing declaration through an Arc.
- [ ] Compare fewer/larger allocations, reserved capacity and compacted holes where
  profiling supports them. Do not reserve capacity proportional to lifetime churn.
- [ ] Reconcile the representation with P4 structural evidence: one workspace plus
  bounded changed-source records, not one model per group or proof.
- [ ] Keep only changes that improve measured cold/release cost or live memory
  without materially worsening ordinary paint/pixel paths or mutation cost.

### P7.3 Remove remaining unnecessary work

- [ ] Reuse already-resolved immutable boundary source/target requests where equality
  and complete endpoint coverage are established; include both axes in batches.
- [ ] Profile remaining full-tree dirty clearing, geometry snapshot collection,
  ancestry scans, content-index maintenance and derived registry work.
- [ ] Replace proven hot scans with conservative dirty/indexed traversal. Include
  topology, inherited context, scroll and Nearby invalidation in the design and tests.
- [ ] Reuse native measure/resolve caches only across a verified dependency envelope.
  Do not prune siblings or wrappers merely because their declarations are unchanged.
- [ ] Keep terminal counters observable and return the dimension runtime to its
  correct idle/released state. Do not hide destruction in unbounded deferred work.
  Any later bounded retirement proposal needs separate ownership, queue, shutdown,
  memory-peak and total-work evidence; moving a timer boundary alone is not a win.

### Acceptance and comparison protocol

- Use the performance lock, release binaries and immutable source identity. Run
  separate processes, repeated trials and the same fixtures; record CPU/frequency/
  thermal conditions. Do not choose only the best run or subtract noisy timings
  from separate processes as if they were precise per-component costs.
- Preserve paint/pixel zero-dimension-workspace behavior and unchanged-model reuse.
  Repeated lifecycle churn must reach a bounded live-memory plateau.
- Desktop regression gate: the 20k-node release cases must no longer exceed the
  12 ms tree-work budget in the qualification trials; report p50/p95/max and tails.
- Device goals remain transform-only refresh <=5 ms (ideal <3 ms), animated patch
  tree work <=12 ms (ideal <8 ms), with render/present fitting a vblank where possible.
- Establish an absolute live-memory budget from the target device during P1/P8;
  no such accepted budget currently exists. Do not invent a passed memory threshold.
  The optimization must demonstrably reduce incremental private-model live bytes
  from baseline and satisfy that device budget before memory qualification closes.

**Exit:** functional gates remain green, actual model/retention costs are lower,
release work meets the applicable gate and no timing/retention accounting is hidden.
Missing target measurements leave device qualification open.

## P8. Backend and constrained-device qualification

Coordinate, do not duplicate:
[Linux GPU qualification](active-linux-gpu-qualification.md),
[low-resource smoothness](active-low-resource-animation-smoothness.md),
[renderer asset runtime](active-per-renderer-asset-runtime.md),
[platform runtime differences](platform-runtime-architecture-differences.md).

| Target | Required animation-specific evidence |
|---|---|
| Raster/headless CPU | Native full-footprint/raster/input suites, old-scene replay, final/cleanup outputs |
| Headless OpenGL/Vulkan routes supported by the build | Same lifecycle traces through real output, resize/final frame, synchronization and resource release |
| Wayland | Actual compositor input/resize/scale, frame scheduling, terminal frame and stationary-cursor hits |
| DRM | Actual scanout/page flips, mode/pacing, final frame, input and repeated animation cadence |
| macOS host | Shared-core traces through direct host installation, protocol/artifact match, AppKit/Metal drawing and lifecycle |
| Constrained target | Locked performance matrix, repeated sidepane/content/length/ghost runs, live memory and physical cadence |

- [ ] Inventory available hardware, display/socket access, render devices and drivers
  before claiming a route can run. Separate compile-only, unit, simulated and real
  device results; capture every skip reason.
- [ ] Build relevant feature combinations and run shared semantic tests without
  platform-specific animation forks.
- [ ] Record scenes built/selected/drawn/presented, queue overwrites, cache churn,
  frame-time tails and achieved versus physical refresh rate. Include cold open,
  repeated second open, interruption, input under motion and idle after cleanup.
- [ ] Verify old scenes/resource snapshots release safely at renderer stop and host
  failure. Shared semantic success does not prove GPU synchronization correctness.
- [ ] Obtain macOS and constrained-device execution through the appropriate host/CI
  when available. Produce a reproducible handoff bundle if unavailable here; keep
  those rows open rather than substituting Linux results.

**Exit:** each available route has actual evidence; unavailable routes have named
missing prerequisites. Overall platform qualification requires those outstanding runs.

## P9. Audit, document and close

- [ ] Re-audit public validation, schema, codec and reconcile behavior: list policy,
  empty-list validation, alias/last-write handling, plain-attr removal, numeric ids,
  ownership conflicts, unsupported animated bounds and malformed inputs.
- [ ] Check EMRG 9/tag 84 and macOS protocol 14 against actual artifacts, not only
  constants. Reject incompatible host binaries; bump a format only if its contract
  really changes, with matching decoder/fixture/documentation updates.
- [ ] Audit all production entry points for the same exclusive publication path;
  remove obsolete bypasses, redundant special cases and temporary experiments.
- [ ] Check Rustler boundaries for scheduler safety, owned term/binary lifetimes,
  resource cleanup and error shapes. Keep callbacks/messages off inappropriate BEAM
  threads and expensive work outside font/asset locks.
- [ ] Document holds, independent fields, context retargeting, reparent/remount source
  semantics, repeat resets, separate ghost cleanup and actual platform limitations.
- [ ] Update changelog, guides, examples and generated docs. Add public examples for
  nested content, mixed loop context and interruption without internal group controls.
- [ ] Re-run complete CI, feature/build checks, representative raster/integration
  traces and locked benchmarks at the final immutable source identity.
- [ ] Cross-check every unchecked active-plan item against evidence. Mark implementation
  and platform qualification separately if hardware is still missing; never call the
  entire feature plan complete on the strength of functional CI alone.
- [ ] When all gates actually close, consolidate durable semantics/measurements into
  guides/tests/artifacts and update the plans index. No commit or push is implied by
  this planning document; preserve the repository's commit-message rules.

## 3. Shared test protocol

For each new behavior family, test both axes and Full/Active/Dirty preparation.
Use scales .5/1/2, unequal local/parent scales where relevant, and all four owners.
Exercise four curves and 30/60/120 Hz in trajectory tests; also test sparse, late and
thousand-cycle schedules. Compare each schedule to its own oracle, not to another
frame rate's context-dependent trajectory.

Required assertions:

1. Complete footprint equality/transport invariants, including intrinsic/initial/
   visible size, charges, parent extent, policy channels and allocation scope.
2. Native declared/current-model oracle where applicable; preserve full samples in
   the oracle when testing mixed context. A pixel-lowered tree is not that oracle.
3. Actual pixels, clipping, hit regions, listener precedence and scroll ranges.
4. Old scene and event publication unchanged on injected admission/query/preflight/
   pre-output failure; successful prefixes and first sources remain eligible.
5. Corrupt valid-valued geometry, fractional policies, charges, scope/mount and
   evidence identities independently. No tolerance relaxation or receipt bypass.
6. Exact/deferred retry with later time and latest desired inputs; no clock/source
   restart, successful recovery and eventual idle where appropriate.
7. Query-order/owner-order permutations, replay after failed queries and renderer/
   font/image isolation. Native evaluation must reset all replaced preimages.
8. Actual work bounds and live/peak retained records; no growth with retry count,
   skipped cycles or completed lifecycle count after warm-up.

## 4. Commands and review gates

Run after each production-code slice (from `/workspace/emerge-animation`):

```sh
cargo fmt --manifest-path native/emerge_skia/Cargo.toml -- --check
cargo test --manifest-path native/emerge_skia/Cargo.toml
cargo clippy --manifest-path native/emerge_skia/Cargo.toml \
  --benches --tests --features bench-diagnostics -- -D warnings
cargo check --manifest-path native/emerge_skia/Cargo.toml \
  --benches --features bench-diagnostics
EMERGE_SKIA_BUILD=1 \
  CARGO_TARGET_DIR=/workspace/emerge-animation/native/emerge_skia/target mix test
EMERGE_SKIA_BUILD=1 \
  CARGO_TARGET_DIR=/workspace/emerge-animation/native/emerge_skia/target ./ci-tests.sh all
git diff --check
```

Add relevant raster-only, `drm`, `headless-opengl`, `headless-vulkan`, Wayland and
Vulkan feature builds using the current Cargo feature definitions. Host tests need
actual host toolchains/devices; a feature build alone does not count as a display test.
Run `mix docs` for the final documentation audit.

Performance example, with the immutable identity replaced by the recorded snapshot:

```sh
./scripts/performance-lock.sh --source-revision <immutable-id> exclusive \
  cargo bench --manifest-path native/emerge_skia/Cargo.toml \
    --bench shared_animation --features bench-diagnostics -- 20000 64 moving
```

Do not close a package solely because its focused test passed. Its review must state
which contract changed, why the native evidence authorizes it, what remains excluded,
which full checks ran and how the work/retention bound changed.


## D11 execution checkpoint

Atomic prepared-frame image inputs, bounded/nonblocking actor output and expanded
structural, coupling, ghost and native-input traces are implemented. See decisions
D11 and `validation/atomic-publication/`; these supersede earlier directed gaps,
not the exhaustive P1–P9 gates. Current-source performance and exact live retention
remain unqualified; no commit/push or package closure is implied.
