# Historical animation design notes

Superseded by `../active-shared-animation-core.md`. Retained for allocation oracles
and investigation details, not as an active phase or enablement policy. The current feature excludes animated
min/max expressions; references below to supporting them are superseded.

# Shared animation core: detailed implementation plan

Status: partial native implementation of automatic direct-child groups, still test-gated.
Finite sibling targets, resolved weighted footprints, full holds and validated live-group
release now run through the shared core. The full matrix, transactional lifecycle/public integration and qualification
remain incomplete; this is not an enabled public feature.
Branch/worktree: `plan/shared-animation-core`, `/workspace/emerge-animation`.
Code baseline: `641d355`. This is the single active implementation plan for this feature.

## 1. Scope and decisions

Implement the complete width/height length matrix and `Animation.change/3` in the
same native core as explicit regular/enter/exit animations. Users do not assign groups.

```elixir
Animation.animate([[width(px(40))], [width(fill())]], 1000, :linear)

# First mount is immediate; a later retained target change starts a once run.
Animation.change(width(px(40)), 1000, :linear)
Animation.change(width(fill()), 1000, :linear)
```

The normalized shape remains an ordinary target plus policy, not serialized history:

```elixir
%{width: :fill, animate_change: %{width: %{duration: 1000, curve: :linear}}}
```

### Agreed direction

- Group ordinary direct children by their retained parent. Nested groups expose
  **current** animation state, not predictions of their final state.
- Resolve a group's destination together. Each member finishes motion on its own
  clock; completed dimensions can remain owned until the group releases them together.
- Different pulse schedules may produce different context-retargeted trajectories.
  Document and test schedules individually. Same-input/same-schedule replay and
  query-order independence remain required.
- Animate real layout. Preserve ordinary descendant layout, wrapping, alignment,
  images, scroll, clipping and input geometry. Do not freeze rendered subtrees.
- Keep the existing layout engine and one renderer-local private query workspace.
  Local group ownership does not imply isolated or O(group-size) layout evaluation.

### Hard constraints

- Complete pixels/fill/content/weighted-fill/recursive-min/max matrix on both axes;
  reversals, multiple segments, finite repeats, loops and all owners. No partial public release.
- No new NIF entry points, per-frame BEAM geometry calls, user group API, global
  animation cache, mandatory dependency graph, second layout engine or recursive
  endpoint/fixed-point solver.
- Keep backend pacing/threading unchanged. Admission, sampling, resolution, ownership
  and completion must not become separate implementations for explicit and change runs.
- Preserve successful patch-prefix effects, first-write sources and renderer/asset
  isolation. Queries do not publish scenes, rebuild registries or request assets.
- No idle target-history map. Held fields are active layout ownership and must be
  released when their group ends or their owner is cancelled.
- Preserve paint-only fast paths and the performance lock in
  [low-resource animation smoothness](active-low-resource-animation-smoothness.md).
- Do not relax public validation or apply the public draft before the native gates pass.

### D1 — settled: consistent joint-target and hold semantics, including weights

The user reaffirmed that consistent joint targets and holds are the existing decision,
not a separate weighted-fill choice awaiting approval. Predictable, smooth UI motion
without artificial jumps takes precedence over preserving symbolic weight interpolation.
Do not reopen this as an approval blocker or optimize for a competing mathematical model.

| Example | Legacy symbolic behavior | Required resolved-footprint behavior |
| --- | --- | --- |
| Weight 1→3 beside static weight 1 in 600px | Midpoint 400px | Endpoints 300→450 give midpoint 375px |
| A weight 1→3 / 1s, B 40px→fill / 2s | At 1s B=95, A=505 | At 1s A=450 held, B=95; both reach joint destinations 450/150 at 2s |

**Decision:** use resolved footprints throughout grouped non-pixel dimension runs,
including recursively compatible weighted bounds. Keep ordinary nonanimated lengths
unchanged and preserve direct numeric interpolation only where it is provably equivalent.
Apply the same easing to complete endpoint footprints, reach each member's joint target
on its own motion clock, hold as required, and release together without a sample-removal
jump. Never switch from symbolic weights to endpoint-box interpolation at completion.
No public mode option or special weighted exception is wanted.

Determinism, query-order independence and continuity remain acceptance requirements.
The existing acceptance of schedule-dependent context retargeting is unchanged.

This settles the semantics, not implementation acceptance. The gated core now replaces
legacy weighted sampling and has trajectory/hold/release tests; complete the context,
lifecycle and entrypoint gates before enabling it for applications. Keep
legacy arithmetic as explicitly labelled historical controls, not as the acceptance target
for the new grouped core. Public enablement still waits for the remaining native,
lifecycle/context, protocol and qualification gates.

### Implementation defaults to prove comprehensively

| Topic | Planned rule |
| --- | --- |
| Axes | One parent group for both axes and relevant layout fields; no axis dependency graph |
| Static siblings | Included in layout context, not timed members |
| Paint/paint transforms | Independent clocks/completion; do not extend a layout barrier |
| Finite arrivals/restarts | Join/update the active parent group; recalculate its targets and barrier |
| Continuous arrivals | May keep that parent group owned longer; never block unrelated parents |
| Unbounded layout loops | Current-state context for finite groups; do not enter an infinite wait barrier |
| Multi-segment runs | Joint targets use members' current segment destinations; segment changes refresh the target epoch |
| Finite repeats | Retain existing repeat clocks/resets; final run completion, not each cycle, permits final group release |
| Context change while moving | Distinguish continuous context continuation from discrete-event reanchoring; prove P3.2 |
| Context change while held | If group time remains, use a group-relative retarget interval; proposed linear interpolation |
| No remaining group time | Resolve the release set against current context; prove release/late-input behavior before enabling it |
| Nearby/ghost/root | Explicit native scope variants, not accidental membership in a normal flow group |

P0 makes these defaults executable test cases. An implementation failure must revise the
mechanism or explicitly revisit a semantic choice, not silently add a special-case public restriction.

## 2. Starting point and native evidence

### Reuse rather than reimplement

| Existing component | Current responsibility | Remaining change |
| --- | --- | --- |
| `tree/layout/dimensions.rs` | Full axis footprints, sparse samples, scoped allocation and cache keys | Reuse for group holds/release and prove scope conversion |
| `tree/layout/projection.rs` | One isolated model-only workspace and retained native layout caches | Group projections, sparse context/metric wiring and staged release evaluation |
| `tree/animation/source.rs` | First-write declared/effective state, scale, footprints and provenance | Consume across group admission, interruption and lifecycle handoff |
| `tree/animation/timing.rs` | Shared segment/repeat/easing selection and remaining-curve arithmetic | Explicit motion/hold/release timing without changing repeat clocks |
| `tree/animation/lengths.rs` | Test-gated joint targets, non-pixel footprints and combined release preparation | Late-context continuity, sparse context and boundary/lifecycle parity |
| `tree/animation/change.rs` | Internal policy types and once-spec admission candidate | Group-aware field ownership, deferred handoffs and complete validation |
| `tree/animation.rs` | Existing owner maps, sampling and completion effects | One authoritative group lifecycle and field-level completion intents |
| `runtime/tree_update.rs` | Shared actor update/preparation/publication path | Integrate group results, errors, clocks and final damage |

The current candidate retains a `cfg!(test)` early return in mixed resolution and group admission.
Typed preparation errors now reach the actor and the fallible measured-layout helper.
The `animation_error` string remains diagnostic for older wrappers; complete entrypoint
unification and public failure handling are still gated.
`QueryContext.metrics_epoch` is still hard-coded by that candidate; runtime seeds and
image metrics are gathered conservatively. These are explicit implementation tasks below.

The unapplied [public API draft](artifacts/shared-animation-public-api.patch) is only
source material. It lacks completed group semantics, validation, tests, docs and platform
compatibility work. Do not treat successful patch application as implementation acceptance.

### Established evidence

[Six group investigation tests](../native/emerge_skia/src/tree/layout/tests/animation_groups.rs)
use ordinary native layout and manual footprint installation, not a group scheduler:

- Two 40px→fill children in 600px, durations 1s/2s: held A=300/B=170 at 1s;
  both=300 at 2s; simultaneous removal preserves full footprints. Both axes,
  scales .5/1/2 and 30/60/120Hz are covered. Releasing only A at 1s produces 430px.
- A bounded fill can draw 50px while reserving 360px. Replacing its held footprint
  with px(50) changes an unanimated sibling from 160px to 470px in the probe.
- A content parent's endpoint changes 40→120 when its child changes 40→120.
- A fill child's endpoint changes 200→400 when its parent's available size changes.
- Groups beneath separate, unanimated sibling containers still exchange layout
  context: a descendant edit changes another group's fill endpoint 560→400.
- Preserving symbolic weight sampling then switching to a joint final box reproduces D1.

The [original six pixel-substitution controls](../native/emerge_skia/src/tree/layout/tests/animation_endpoints.rs)
remain unchanged. [Dimension replay tests](../native/emerge_skia/src/tree/layout/tests/dimension_samples.rs)
and [projection isolation tests](../native/emerge_skia/src/tree/layout/projection/tests.rs)
remain mandatory foundations.

### Implemented native group slice (not phase/release acceptance)

[`animation/groups.rs`](../native/emerge_skia/src/tree/animation/groups.rs) now provides
one `AnimationRuntime`-owned ledger with typed owner/group identities, retained-parent /
mount / Nearby-slot / root / ghost lookup and finite barriers. It refreshes from selected
owners rather than scanning the tree for membership. Completed regular/change/enter
layout owners can outlive their motion clock. Completed enter paint is not held with layout.

[`animation/lengths/tests.rs`](../native/emerge_skia/src/tree/animation/lengths/tests.rs)
now exercises the actual runtime, in addition to the unchanged manual probes:

- 1s/2s sibling motion, full holds, all-frame arithmetic and simultaneous release on both
  axes, scales .5/1/2 and 30/60/120Hz. Near-release geometry agrees with removal and fresh
  terminal native layout. Fixed-context runs use two initial queries, no warm queries and
  one combined release query when ready.
- Bounded 50px/360px reservation with an unanimated fill; regular, enter and change holds;
  source/workspace cleanup at the barrier and paint completion independent of enter geometry.
- A held 300px box retargets continuously to 400px over remaining group time after a resize;
  the moving peer keeps its own remaining interval. This proves stable-membership retargeting,
  not cancellation/arrival during that interval.
- The adjacent `[40, fill, 40]` example reuses its preceding 300px joint endpoint and
  reanchors the sibling toward 560px. Finite repeats preserve source resets and wait for
  final run completion rather than releasing at a cycle boundary.
- Change admission precedes barrier completion: a new change at the old deadline joins
  before release, retaining 560/40 and retargeting continuously to 300/300. Cancelling the
  last moving change releases completed peer sources in the same successful frame,
  rather than requiring a later idle cleanup pulse.
- A layout loop is foreign current context, not a finite waiter. The former per-schedule
  268.50662/267.7409px oracles remain independently pinned at 30/120Hz using a looping peer.
  Finite siblings now follow the fixed joint 170px midpoint instead.
- D1 is implemented in the gated core: weight 1→3 beside static weight 1 has a 375px
  midpoint; beside the 2s pixel→fill peer it reaches 450px at 1s and holds there. Both axes,
  scales .5/1/2, all four curves and recursive weighted bounds are covered. Only direct
  Px→Px pairs retain the numeric fast path. Crossing pixel bounds also resolve endpoints
  rather than interpolating branches (150px midpoint, not 250px).
- Invalid cross-scope sources leave all geometry tracks and live samples unchanged;
  restoring the scope recovers. Release validation additionally compares every footprint
  channel: equal visible size with a stale charge is rejected.
- Ready live groups retain owners/sources until successful layout. All groups ready now
  share one release projection. A failure in one group's footprint validation releases
  neither group; successful preparation removes their samples before one layout, and
  only the subsequent commit drops owners and tracks.
- Enter terminal release is published separately from an intentional different-base
  handoff; completion requests and dirties the necessary follow-up frame. Pending changes
  capture terminal footprints before the old geometry records are freed.
- The actor propagates preparation failures under ReturnErr and skips publication under
  LogAndContinue, preserving pending effects and sources. A regression restores valid
  context and recovers through an empty batch without restarting/resending the animation.
- `try_layout_tree_with_animation` provides fallible layout plus ownership commit using
  the caller's measurer/font context. A custom-measurer regression covers source, target
  and release geometry. Older read-only/benchmark wrappers still need P6 unification.

Requests are keyed and batched per group (per owner for loops). Projection construction and
semantic comparison happen once per request rather than once per field; matching fields
use Arc identity checks. This still clones the frozen projection per distinct request and
conservatively gathers context. Shared-base/delta assembly, visit counters and measured
many-owner budgets remain open.

**Implemented identity/receipt prerequisite (P5.1/P4.2 slice):** accepted regular,
enter, exit and change entries now carry checked renderer-local monotonic run generations.
Pure sampling clocks remain separate. Identical/timing-only change targets do not allocate a
run; presentation anchoring preserves the run generation but invalidates a prepared clock.
Group membership/segment/cycle/barrier versions use immutable Arc tokens, reused on unchanged
pulses. These are group versions, not yet the complete dynamic-context target epochs.

Release receipts now bind root/mount, tree/model revision, constraint/scale, frozen-context
identity, sample time, runtime/geometry identity, group versions and exact retiring track
keys/terminal footprints. A stale commit is typed and cannot retire replacement tracks or
sources; clones cannot consume another runtime/workspace's receipt. Generation exhaustion is
typed rather than wrapping, and enter-handoff capacity is checked before release installation.
The actor and mutable direct helper propagate commit errors. Added 13 regressions for these
identities, same-time replacements, no-restart rerenders, stale/repeated commits, clone
isolation, clock changes and capacity failures; existing release-corruption controls still pass.

**R0/R1 implementation progress:** an opt-in, test-only resolver inspection and deterministic
frame driver now distinguish attempt time/revision from publication sequence. Full, active
and dirty preparation record frozen/candidate/native-release poses, complete footprints,
frames/scroll, owner generations and membership/barriers, source journals, scenes/registries
and actual query counters. Source/target/release failures and a pre-layout stop are injectable;
late-resize and bad-charge controls record both stale and native results without claiming
late-context support. The original pre-layout defect control has now been inverted:
rejected preparation preserves the complete published pose and installed samples.

All four owner maps and group/member tables now use one committed table plus a bounded
last-write-wins admission delta. Candidate reads drive the existing sampler; successful
actor/direct frame completion commits metadata, including frames without a release receipt.
Change/regular A→failed-B→A restores the committed generation/clock; change sources remain
identical. Retry B retains its admitted clock; C replaces B from the first published source;
a published B makes A a new interruption. Restoring an expired committed A also restores
its group retention, rather than bypassing release validation. Same-generation staged enter
handoffs survive no-op retries, and a regular remount cannot restore an old mount's clock.
Committed/candidate enter/exit/change metadata shares immutable spec Arcs. Native validation
uses canonical spacing writes and transform conflicts. Moving/held/pending/releasing masks
now drive transient retention and change sampling/blocking. A held enter hands paint to a
pending change while width stays pending; retries preserve that field handoff's clock and
generation. Enter masks are cached at admission, not rescanned across keyframes each pulse.
Whole selected-owner lifecycle/regular handoffs and skipped boundaries are still incomplete.

**Gate detail:** production synchronization still commits eagerly to preserve legacy behavior;
new staged semantics, like resolved groups, remain test-gated. Admission table tests prove
bounded records, not whole-frame transaction safety. Group discovery still visits selected
owners, but now edits only changed parent records instead of rebuilding membership snapshots.
Warm refresh copies zero group/member entries; a phase edit COWs only its affected parent.
Counters include actual input-record passes and copied member entries, not an O(group-size)
claim for native layout evaluation. The driver
uses decoded attrs/viewport inputs; wire-prefix/actor-policy and transport coverage must be
expanded through later packages. Added 23 tests in this slice.

**R2 application foundations:** all full/active/dirty preparations now return staged raw
attributes and a `GeometryDelta`; they do not install samples, mutate committed tracks or
consume first-write journals. Immutable track Arcs retain one committed and latest candidate
record without rollback copies. Application validates tree/attempt/runtime/workspace identity,
model/revision/root/mount/constraint/scale and exact committed track versions before writes.
Runtime epochs invalidate on changed synchronization/anchoring and on owner destruction;
clones have fresh authority. The consumed preparation produces an applied capability for
native layout/refresh. Track and admission deltas/source acknowledgement commit after output;
calling completion without an applied frame cannot commit staged metadata.

Mutable animated/default/refresh/clean-registry/profile entrypoints now expose typed failures,
synchronize with one explicit sample time and finish ownership. Bench/event/test callers were
migrated and benches compile. Actor application failures follow existing policy; source-only /
metadata-only frames cannot silently skip their commit boundary. Gated patches preserve
published effective attrs and bypass destructive manual-text frame shortcuts. Runtime-owned
text/slider marker normalization precedes source capture, preserving registry-only marker
updates. Production admission still commits eagerly outside the public gate.

**Further R1/R2 boundary work:** production advancement now enters a private coordinator
holding exclusive mutable tree/runtime access through application, native output construction
and commit. Actor, direct, refresh and profiling callers use it; split application hooks and
legacy direct overlay mutation are test-only. Lifecycle capacity is preflighted before writes;
a violated post-layout invariant is fatal, not a recoverable transaction with a mutated tree.
The measured boolean-returning helper now constructs (and discards) a complete native output
before acknowledgement; it still is not a retained snapshot API. Publication rejects a
constraint different from the prepared query input and promotes refresh requests whenever
geometry, cleanup or metric changes require native layout. Actor and benchmark decisions
use the same requirement; they cannot commit new samples against stale layout frames.

Mutable node/iterator access invalidates unapplied attempts, including runtime/scroll changes
which do not bump the model revision. Gated text-input and slider events preserve published
effective attrs and first-write sources. Expired ghosts survive failed preparation: terminal
output commits first, retirement creates structural cleanup damage, and a following publication
drains it. This does not yet implement R7's captured exit-footprint/scope transport.

Renderer-local immutable font maps are shared until load/reset, without font or asset locks
through layout. Query contexts bind actual font generations **and** font/cache identities,
so equal counters in different renderers cannot reuse metrics. Preparations carry the same
font environment through live measurement and refresh construction; real changes invalidate
intrinsic caches. Custom measurers may provide revisions; zero retains the existing
caller-managed/unversioned contract. Current-query image facts also reach live measurement;
cleanup-only deltas cannot accidentally freeze an obsolete image context. Actor asset setup
precedes preparation. Font snapshots do **not** yet reach deferred scene rasterization, and
font-load notifications/cause-specific runtime and media versions still belong to R3.

Regular/enter/exit masks are cached at admission, including explicit alignment fields omitted
by the change-policy vocabulary. One phase derivation now drives selected group admission,
enter retention and change blocking. Mount-aware transient retention prevents reused IDs from
keeping old clocks; partial admission retries preserve already admitted enter clocks/specs.
Field-specific **regular handoff** and skipped-boundary lifecycle remain unfinished. Fourteen
new regressions cover these boundaries, font isolation/reset/remeasurement, stale context
cleanup, event-owned sources and ghost rejection/final damage. R1/R2 are still partial; no
continuity tolerance was relaxed and public enablement remains closed.

**Remaining ledger/lifecycle gaps:** membership edits are sparse but discovery still scans
selected owners; field-specific regular lifecycle and full target-context versions remain
incomplete. Held change sources remain whole shared sources until release. Ghost retirement
now follows successful native output, but captured baseline/scope conversion and atomic
transport qualification are not complete. Skipped-boundary source reconstruction, dynamic
retarget deadlines, fresh/snapshot intent and caller parity remain gates below.

**Late-context release is not solved by the guard:** a stale endpoint produces the typed
`ReleaseMismatch(node, axis)` instead of being overwritten at the deadline to hide a jump.
The actor regression deliberately tests rejection/recovery after a deadline-time resize;
it does not qualify that resize as supported animation behavior. P3/P4 must implement and
prove continuous/nested/late-context results and pulse liveness, not rely on rejection,
repeated retries, extra durations or indefinite retention as the final policy.

Last code validation: **1211 Rust unit + 14 integration tests**, **488 Elixir tests/doctests**
(3 excluded), full local CI, Clippy with tests/warnings-as-errors and formatting passed.
Standalone `mix test`: 483 passed, 8 excluded. Logs: `/tmp/closure-cargo.log`,
`/tmp/closure-mix.log`, `/tmp/closure-ci.log`, `/tmp/closure-clippy.log`, `/tmp/closure-bench.log`.
The known unrelated headless fd-reuse assertion intermittently failed during parallel test
runs; final Rust/full-CI reruns passed without backend changes.
This is not public-production mixed-group, macOS, RSS or device qualification.

## 3. Proposed runtime state and ownership

Names below describe responsibilities; reuse existing types where possible.

```text
GroupKey
  Flow(parent NodeId + mount)
  Root(root NodeId + mount)
  Nearby(host NodeId + mount + slot)
  Ghost(ghost root identity + mount)

OwnerFieldKey
  node NodeId + mount
  owner kind (regular / enter / exit / change)
  run generation
  canonical field group

GroupRecord
  member owner-field keys
  target epoch
  finite barrier instant
  pending context-retarget deadline, if any
  dirty reasons / pending release status

DimensionTrack
  owner-field identity + group key
  current segment/cycle identity
  source and target full footprint
  interpolation anchor
  Moving | Held | ContextRetargeting | ReleasePending
  endpoint/context cache identity

FrozenFrame
  one sample time
  current raw overlays and full owned footprints
  immutable layout context, runtime/scroll seeds and image metrics

PreparedGroupFrame
  staged overlays and footprint changes
  release/handoff intents
  layout/paint/registry damage and pulse requirement
```

### Single authority

- Keep owner/group lifecycle authority in `AnimationRuntime`. Geometry records and the
  single `EndpointResolver` belong to the existing geometry runtime. They reference owner
  identities, not independent clocks/specs/target histories.
- Do not duplicate a group ledger in both `AnimationRuntime` and `ElementTree`.
  Existing owner maps can remain while field-level hold/release information is added.
- `sync_with_tree` must stop deleting geometry-owned entries solely because their nominal
  duration expired. Emit completion intents; finalize held-field release through the common
  frame path. Independent paint fields can finish without waiting for geometry.
- Build groups through existing `ParentLink` lookups over admitted/affected owners.
  No whole-tree search is needed to decide membership on warm pulses.
- Group identity is not `AllocationScope`: a row's two axes share a group while only
  one axis may have a pool charge. Keep native scope validation on each footprint.
- Store specs once per owner, not per axis/group/query. Moving fields may share a
  presentation-source Arc. When only a hold remains, retain the terminal field value,
  necessary lifecycle identity and footprint, not an entire finished source journal.
- Completed layout scalars (for example font size or padding) retain their terminal
  raw overlay while the group owns them; they do not need fabricated dimension footprints.
  Paint fields remain independent. Avoid restoring an enter's layout scalar baseline
  early while a held dimension still assumes that terminal scalar in its joint target.
- Group release removes temporary ownership, not the established meaning of an explicit
  once animation's terminal keyframe. Existing declaration/spec state may still supply
  that terminal expression; do not replace it blindly with the node's unrelated base attrs.
- Remove empty groups and released tracks immediately. Release the workspace when no
  retained geometry owner needs endpoint queries. Do not poll idle policy-bearing nodes.

### State transitions

| Event | Transition/action |
| --- | --- |
| Valid finite admission | Add member, capture source, mark group target dirty |
| Ordinary pulse | Sample existing clock; no membership discovery/restart |
| Member motion ends, group still busy | Moving→Held; retain terminal geometry ownership |
| Member enters next segment/cycle | Stay owned; select next segment, preserve appropriate source, refresh group targets |
| Held member sees changed endpoint with time left | Held→ContextRetargeting from current footprint |
| New target on retained field | Replace that run generation from current presentation; update group barrier |
| All finite work completes | Stage ReleasePending for the group; include release/handoff damage |
| Frame preflight succeeds | Validate exclusive application token; apply all samples/removals together |
| Complete frame constructed successfully | Commit owner handoffs, source acknowledgements and publication/pulse state |
| Removal/policy cancellation/reparent | Remove or transfer member explicitly; invalidate both affected scopes |
| Query/admission failure | Do not publish partially installed group geometry; retain recoverable work or return typed failure |

A held width is not a frozen position or subtree. Parent alignment and unowned properties
may keep moving it. A group's deadline is not a new backend scheduler or wall clock.

## 4. Per-frame algorithm

Implement this once for actor, direct/default, specialized-measurer and headless paths.

### 4.1 Apply inputs and admit owners

1. Apply validated patches/runtime changes, preserving successful-prefix effects.
2. Retain the first valid presentation source per mount until admission has consumed it.
3. Stage actual target changes and lifecycle starts against committed owner state. Identical
   targets/timing-only edits do not restart a change run. Resolve priority/field conflicts
   before candidate group membership; failed replacements do not become presented owners.
4. Form candidate membership deltas; update target epochs/barriers only for real changes.
   Commit these deltas with the successful frame, not during query preparation.
5. Use fresh/presentation-aligned time for idle starts; never reuse a stale previous pulse
   as the start instant. Presentation-anchor changes must update group deadlines coherently.

### 4.2 Freeze current inputs

1. Use shared timing to sample each moving owner at the same instant. Held owners export
   their retained presentation. Include completed-but-not-released geometry in preparation.
2. Freeze one immutable input set before any endpoint query. Shared Arcs/views should avoid
   copying all foreign attrs for every member or group.
3. Cross-group inputs are current owned samples and native layout context, not declared
   animation finals or newly calculated query results. Preserve ordinary unowned attrs.
4. Cold explicit sources require an isolated source projection if no valid presentation
   exists. Project new members' source keyframes together while existing members retain
   their captured current presentation. Never restart held peers from their original first
   keyframes or call an arbitrary previous frame the first explicit keyframe.
5. Perform normal asset setup before freezing available metrics. A later metric notice
   invalidates subsequent context; a query must not synchronously load missing assets or
   manufacture old intrinsic dimensions from a newly patched source.

### 4.3 Prepare joint targets

For each dirty group:

1. Build its joint destination by replacing all member-owned fields with current segment
   destinations (terminal values for held members). Clear matching footprint overrides.
2. Preserve foreign groups' frozen current samples. Include local layout fields such as
   padding, fonts, layout scale/rotation and spacing when owned by the group.
3. Resolve all requested axes together using `EndpointResolver`. Reuse captured/derived
   fields when the **complete** footprint is known. A known pixel size alone is insufficient.
4. Deduplicate equivalent requests. Stage all results; do not feed one refreshed group's
   endpoints into another group's current input set in the same batch.
5. Select continuation by context provenance (P3.1/P3.2). Discrete target/context edits
   reanchor from current full presentation over remaining admitted time. Continuous
   animated context must not repeatedly reset the anchor and leave a stale terminal
   target. Held context-retarget intervals are bounded; unchanged holds stay motionless.
   Prove the final prepared pose and release result together before changing this rule.

No repeated query-until-convergence loop. Bound and count any extra source, rebasing,
policy or release query. A target refresh must not silently regenerate animation specs.

### 4.4 Release and publish

1. Identify all groups due for release at this sample time, after processing admissions,
   segment changes and context notices. Do not remove each node as soon as its clock ends.
2. Stage a release projection with the release set's terminal expressions and other groups'
   current samples. Groups due in the same frame must not observe half-released neighbours.
   A combined release query is allowed: it concerns current terminal state, not future timelines.
3. Verify endpoint/release parity against current context. Tests must inspect the limit with
   samples still installed; overwriting a stale endpoint at the last instant cannot conceal a bug.
   Distinguish releasing to the terminal expression from a subsequent intentional lifecycle
   handoff (such as enter terminal→different base/regular attrs). Compare each against its
   own oracle rather than calling a pre-existing enter/base mismatch a footprint defect.
4. Validate the complete prepared-frame token at the exclusive application boundary; all
   recoverable checks precede mutation. Install samples and remove ready overrides before one
   ordinary live layout/refresh. Queries never apply live scroll/scene/registry effects.
5. After successful output construction, commit owner/track/group and lifecycle deltas plus
   source acknowledgements. Publish final damage before disabling pulses. The release frame
   must not be lost to an early `is_empty` check. A prepared query alone is not commit.

**Late-input/zero-remaining-time gate:** a genuine resize, removal, interaction switch or
loop reset can change ordinary layout discontinuously. Tests distinguish that from a
spurious sample-removal jump. A changed endpoint with no remaining interval must have a
specified current-context result and liveness outcome; it must neither wait forever nor
invent an arbitrary extra duration. P0/P4 must prove this using cached/fresh release oracles.
If the proposed ordering fails, revise it while gated—do not add an unbounded settling loop.

### 4.5 Failure handling

- Return structured group preparation errors to the existing update/decode policy; the
  current diagnostic `animation_error` field alone is not sufficient.
- Validate sources/scopes and finish required queries before mutating live sample ownership.
  A failed query may discard its private workspace, not poison the live tree or other groups.
- Keep admitted sources/pending effects needed for a return-error recovery. Log-and-continue
  must not publish a half-prepared group or discard successful-prefix work silently.
- No `unwrap`/index assumptions on externally influenced lookup results. No asset locks
  across layout, NIF scheduler changes or retained BEAM terms in group records.

## 5. Endpoint representation and context validity

### Full footprints remain required

Retain existing channels: intrinsic contribution, initial basis, final visible box,
parent charge, parent placement extent, scope and policy coefficients. Box channels
use node logical units; charge/placement use parent logical units. Prepare scale once.

- Sampled fill leaves the normal weight pool and enters its scoped fixed debit.
- Charge is not `max(visible, reserved)`. Bounded overflow remains valid.
- Wrapped-row and float placement facts are not row/column pool charges.
- Keep intrinsic/initial/final distinctions for content sizing and child resolution.
- Preserve image syntax policy, fractional fill demand/distribution and growth policy.
- Blend finite policy candidates, not boolean thresholds or numeric infinity.
- Parent-imposed sizing still wins. Do not interpolate explicitly sampled children twice.
- Keep layout-derived placement and `shift_subtree` behavior; no paint-scale substitute.

Reference: in a 600px row, 40px→min(50px, fill) beside ordinary fill has midpoint
visible=45, charge=170, peer=430. Replay and release must preserve all three.

### Context inputs and invalidation

| Input | Source/update route | Expected response |
| --- | --- | --- |
| Declared layout model/topology | Actual applied patch/model effects | Bump model epoch; refresh private model on demand |
| Membership/segment targets | Owner/group admission and timing | Bump group target epoch; no model copy solely for a clock change |
| Parent available size/insets/scale | Native layout/boundary context | Invalidate affected endpoint context |
| Descendant intrinsic/current output | Existing native measurement/dirty propagation | Invalidate dependent context, not merge future schedules |
| Foreign owned samples/layout attrs | Frozen frame | Refresh dependent targets; retain same model snapshot |
| Text/image metric changes | Genuine metric notices/frozen image map | Bump metric/context versions, not model epoch for unchanged model |
| Scroll/runtime/interaction | Sparse changed node seeds | Re-seed relevant input; never reuse a previous query's clamp output |
| Paint-only changes | Existing paint/transform path | No dimension workspace/query/model copy |

Exclude a group's own replaced sampled attrs from its target cache identity. Do not
exclude legitimate changes to its parent constraint or descendant context merely because
its animation indirectly contributed to them. Do not use generic `Measure` damage or
unchanged asset dimensions as a reason for unconditional endpoint invalidation.

Start conservatively with existing dirty propagation and renderer-local input versions;
no ancestor/descendant dependency graph. Sparse context updates must replace the candidate's
per-pulse scan/poll of all relevant nodes/assets. Same-context cache reuse needs proof,
not just equal visible widths.

### Projection assembly cost

Per-field full projection cloning and linear semantic-query lookup have been replaced
with per-request preparation and keyed lookup. Per-request full clones still remain.
Prefer one shared frozen base plus group deltas,
stable target versions and reusable request identities. Apply/reset only changed overlays
when the base is unchanged, preserving the evaluator's isolation rules.

Do not key every group solely by the whole frozen-frame Arc: internal progress would
invalidate an otherwise unchanged group destination. Count projection entries visited,
cloned bytes and equality/hash work as well as native layout queries.

## 6. Nested groups, joins, repeats and lifecycle

### Nested groups

- `G(parent)` owns its children's fields; the parent's fields belong to the next group
  outward. Native measurement and constraints cross these boundaries in both directions.
- A content endpoint must update when a descendant's current output changes; a fill
  endpoint must update when available parent space changes. Static ancestors also carry
  these effects between groups.
- Keep one immutable current-input view per preparation. Query order must not turn into
  an accidental top-down/bottom-up iterative animation solver.
- A held member may resume context-driven motion while its group remains busy. It must
  not snap to a refreshed target merely because its own original progress is already 1.
- Initially use the existing workspace, not extracted mini-trees. Subtree-only endpoint
  queries require separate parity/performance evidence and are not part of the first release.

### Multi-segment and repeat rules

- A group target epoch represents members' **current next keyframes**, not all future
  keyframes. Segment changes update that epoch without releasing ordinary symbols.
- On an adjacent segment join, reuse the preceding presented/full terminal sample in its
  valid scope. Re-resolving a symbolic `fill` source against a different peer epoch can jump.
- Example: A `[40px, fill, 40px]` over 2s and B `[40px, fill]` over 2s. First joint targets
  are 300/300; at 1s A's new segment starts from 300 and the new joint targets are 40/560.
  B re-anchors from its then-current presentation over its remaining interval.
- Finite repeat resets remain intentional declared transitions, not holds that pause the
  repeat clock. Only final finite completion makes a member eligible for final release.
- Looping layout tracks use the same timing/source/resolver machinery but do not extend
  finite barriers. Their current samples are context; next-cycle/source evaluation must
  not be a second sampler or a renderer-global completion wait.
- Skipped pulses select the current segment/cycle by absolute elapsed time. Preserve a
  known preceding endpoint where valid; explicitly query missing source footprints.
  Do not replay one native layout for every missed frame/cycle. Test long gaps/tiny durations
  for bounded work and liveness. Schedule-dependent paths are permitted, clock drift is not.

### Lifecycle table

| Situation | Required behavior |
| --- | --- |
| First mount with change policy | Apply target immediately; no idle history or artificial source |
| Add policy to rendered node | Animate only a real target change with a valid old presentation; otherwise seed |
| Identical/timing-only rerender | No restart, target-epoch churn or new group membership |
| Coalesced A→B→A | Preserve the original active run where target is unchanged; first source/latest successful target |
| Interrupted moving/held field | New duration/curve from current full sample; new run generation, no recursive source history |
| Policy removed | Cancel that field and apply ordinary declaration; update affected group/context |
| New sibling animation | Join active parent group; update barrier/targets without snapping existing members |
| Enter motion ends while width held | Preserve geometry ownership; independent paint completion is not unnecessarily delayed |
| Pending change under enter | Keep only latest target; start overlapping fields from actual release/handoff sample |
| Regular after enter | Respect held geometry; retain existing intentional enter/base mismatch behavior explicitly |
| Node removal/exit | Leave live group; capture whole ghost baseline even for alpha-only exits |
| Reparent | Remove old membership/reservation; rebase current presentation into new scope before admission |
| Nearby | Use host/slot identity and coordinate context, not a flow-pool debit |
| Root remount/full upload | Discard old groups, sources and workspace; mount identity prevents id aliasing |
| Last owner/group disappears | Publish final damage, free retention and stop pulses |

Field priority remains exit > enter > regular, with change allowed on disjoint fields;
regular/change overlap is rejected. Canonical field groups must handle aliases such as
spacing/spacing_xy and existing transform conflicts consistently in Elixir and Rust.

Reparent/ghost conversion is not a copy of old charge. Convert physical/node/parent units
using captured and destination contexts; derive new-role charge through native layout of
the captured presentation there. Preserve full source provenance. Hard parent constraints
and actual topology changes may move geometry; unrelated scope leakage must not.

## 7. Implementation phases and exit gates

All phases remain internal/test-gated until P7. Check off deliverables only when their
exit tests and regression suites pass. No time estimates or unmeasured speed claims.

### Remaining native work — ordered implementation packages

This is the execution plan for **late-context continuity and remaining lifecycle/entrypoint
parity**, rebased on the implemented generations and release receipts. R0–R9 below are
ordered, reviewable work packages (split into smaller commits as needed), not new public
phases or separate plans. They
refine P3–P6; their acceptance tests are required in addition to the existing phase gates.
R0, R1 and R2 now have partial implementations described above; the remaining packages are
pending. Do not redo the completed identity slice or treat it as full transaction safety. No further user approval is needed for the settled motion contract.

#### R0 — deterministic driver and mutation-boundary audit (P3.1, P6.1)

**Partial implementation:** `animation/lengths/inspection.rs` and
`animation/lengths/tests/{support,driver}.rs` provide opt-in real-resolver observations,
full/active/dirty replay, native geometry controls, scene/registry comparisons and injected
source/target/release/pre-layout failures. Extend this same driver for later context,
transport, actor-prefix and application-boundary cases; do not add another sampler.

**Mutation audit after staged application:**

| Seam | Current writes and remaining action |
| --- | --- |
| Patch/invalidation shortcuts | Gated attrs preserve published effective values and bypass manual frame resizing; runtime marker normalization avoids spurious source capture. Remaining runtime/topology seams need R3/R7 coverage |
| `sync_changes`, regular/enter/exit sync and unheld completion | Candidate owner deltas, not committed owner replacement in the gated core; lifecycle selection remains partly node-wide |
| Group refresh | Candidate membership/version deltas over committed tables; only changed parents are COWed; discovery still scans selected owners |
| `prune_completed_exit_ghosts` | Owner removal is staged, but live topology/ghost removal is still destructive; R7 must transact it |
| `LengthRuntime::resolve` | Private queries produce a sparse sample/track delta; committed immutable tracks survive preparation and application until commit |
| Full/active/dirty preparation | Raw attrs, sampled footprints and traversal inputs are staged; effective composition occurs only after application validation; journals survive until commit |
| `finish_prepared_frame` / actor output | Non-release and release frames require applied authority; tracks/owners/sources commit after native output. Runtime/metric stamps, transactional ghosts and fully exclusive low-level orchestration remain open |

**Implement first:** add shared native test support, preferably
`animation/lengths/tests/support.rs`, and scenario suites for context, transactions and
caller parity. Reuse the existing fixtures and full-footprint comparison helpers.

- Drive an ordered `InputBatch` and explicit `Instant`; never use sleeps or elapsed wall
  time for assertions. Keep applied-input revision, attempted sample time and committed
  presentation sequence distinct. Equal timestamps need not mean the same transaction.
- Capture `published_before`, `frozen_before_queries`, `candidate_before_removal`,
  `native_with_samples`, `native_without_release_samples`, and `published_after` separately.
  Include intrinsic/initial/visible/charge/parent extent/policy/scope, affected descendant
  frames, scroll, clips, registry and ownership/source/generation/damage/pulse state.
- Record input causes and query/visit/copy counters with each attempt. Fresh native oracle
  trees are test-only controls; do not add live rollback clones to production.
- Add failure injection at admission, source/target/transport query, consistency validation
  and immediately before application. Verify the whole publication tuple, not only the
  output scene pointer. Distinguish stale-token unit tests from real failed-frame recovery.
- Continue the mutation audit in the table above. Owner and track changes are staged;
  ghost pruning and runtime/metric producers remain outstanding. Separated low-level
  apply/layout/finish calls still require an exclusive coordinator. A cached scene alone
  is never proof of preserved presentation facts.

**Tests/exit:** existing 13 identity regressions run through unchanged helpers; new driver
reproduces the deadline-resize rejection and stale charge control deterministically. Add
positive expected-output cases with the subsequent implementation, not ignored tests
counted as completed support. Full/active/dirty attempts expose the same checkpoints.

#### R1 — bounded admission delta and canonical field ownership (P5.1, P4.1)

**Partial implementation:** `animation/admission.rs` supplies committed/candidate views
without rollback clones or superseded chains. Group updates edit only changed parent records,
with warm/no-copy and changed-parent copy-count controls. Enter/change phase masks and
paint-only handoff under held geometry are integrated; complete regular/exit field lifecycle
and skipped-boundary behavior are not. All owner and group tables use the ledger; specs are
shared on metadata copy-on-write. Native tests cover failed/coalesced/published reversals,
retry timing, cancellation, source disposal, staged handoff preservation and remount identity.
`animation/fields.rs` supplies canonical write/conflict masks for policy validation. Remaining:
field-specific regular handoff admission, skipped-boundary sources and sparse membership
producers. R2 now supplies the exclusive application/output/commit coordinator, but its full
lifecycle/context exit is not complete. Do not mark R1's full exit complete yet.

**Touchpoints:** `animation.rs`, `animation/change.rs`, `animation/groups.rs`,
`animation/source.rs`, `patch.rs`.

1. Define a canonical field mask from existing `Field`/attribute classification. List
   alias conflicts explicitly (including spacing versus spacing_xy and transform aliases).
   Maintain moving, held, pending and releasing masks; layout holds cannot retain unrelated
   paint. Specs remain stored once per run, not copied for each axis/mask/group.
2. Replace eager replacement/removal with an `AdmissionDelta` over committed owner state.
   It contains only the latest proposed starts, cancels, field handoffs and group deltas.
   Queries see a candidate owner view; the committed maps remain unchanged until R2 commit.
   This is one ledger plus a sparse transaction delta, not a second lifecycle authority.
3. Retain at most the committed owner and one latest staged replacement per affected field.
   Preserve first-write source and latest successfully applied target/timing. Discard
   superseded candidates without retaining their source chain. Monotonic generations may
   have gaps after discarded attempts; never reuse one or allocate on a no-op retry.
4. Decide A→B→A against committed ownership as well as applied declarations. If B never
   commits and A remains desired, keep A's original generation/start/source. If B commits,
   returning to A is a new interruption sourced from B's actual presentation. Do not use
   equality with the first-write declaration alone to keep an unpublished B entry.
5. Admit every selected owner before deriving finite barriers. Apply membership deltas by
   retained parent/mount/slot and update group versions only on semantic changes. Replace
   snapshot-wide membership rebuilding once equivalence tests pass.

**Tests/exit:** within-batch A→B→A; failed B then A; failed B then C; retry B unchanged;
identical/timing-only edits; policy cancellation; same-time regular/enter/change arrivals;
new mount reusing a numeric id. Compare source Arc identity, generations, original versus
new clock starts, masks, member visits and bounded staged-record counts. Retrying a rejected
frame must not start a fresh clock; ordinary absolute-time progression after missed frames
is distinct from restarting or adding duration.

#### R2 — prepare/apply/commit boundary before continuity changes (P4.2, P6.1)

**Touchpoints:** `LengthRuntime::resolve`, `FrameAttrsPreparation`, all
`prepare_*frame_attrs*` variants, `finish_prepared_frame`, `finish_patch_frame`,
`TreeUpdateEngine::layout_effect`. A small `animation/frame.rs` is appropriate only if it
removes duplicated orchestration rather than creating another sampler.

Proposed internal shapes (adapt existing types rather than exposing a new API):

```text
FrameInput     = intent + explicit time + constraint/scale + inherited font
                 + stable supplied measurer + frozen context versions + applied causes
PreparedFrame  = owner/group delta + raw overlay delta + track/sample delta
                 + lifecycle intents + source acknowledgements + damage/pulse intent
                 + versioned single-use validation token
CommittedFrame = layout/scene/registry output + committed sequence + pulse decision
```

- Make geometry preparation return track updates and sample installs/removals without
  changing committed tracks, effective attrs or source journals. The sole query workspace
  may update private caches on failure; that is not publication state.
- Move every recoverable check ahead of live application: generation capacity, node/mount,
  field ownership, scope, all native query/continuity results, removals and handoff sources.
  Expand the existing receipt rather than introducing a parallel token system.
- Validate the token once at the exclusive application boundary. The coordinator owns
  mutable tree/runtime access through ordinary live layout, refresh and commit; callers
  cannot edit or reanchor between validation and retirement. Asset notices arriving during
  this interval belong to a later input batch. Do not hold asset locks across layout.
- Audit manual text/attribute/scale shortcuts that write frame facts during patch handling.
  Stage their presentation writes or bypass them while an animation transaction is pending;
  keep the applied declaration/model prefix and original first-write source. Preserve the
  existing successful-frame fast paths where they can obey the same boundary.
- Move `finish_patch_frame` source acknowledgement out of full/active/dirty preparation
  and into successful commit, including empty-root and final-cleanup paths. A successful
  query is not enough to consume a successful patch prefix.
- Commit owner/group deltas, source consumption, ghost intents and final-damage/pulse
  bookkeeping only after constructing the complete successful output. Keep actual damage
  calculation in native layout/refresh; the prepared frame carries requirements, not
  fabricated final pixel bounds.
- Audit all fallible work after live mutation. The recoverable path must be preflighted
  or moved before application; keeping the old scene while capturing sources from mutated
  geometry is not rollback safety. Do not add a full live-tree copy. Unexpected panics or
  native failures are not recoverable transactions and must not be silently continued.
- Keep existing public behavior gated. Use the shared coordinator first in the actor and
  mutable measured helper; R8 migrates the remaining callers once the contract is stable.

**Tests/exit:** every injected recoverable failure leaves committed owners, presentation
frames/samples, source journal, scene and registry unchanged while retaining the applied
model/prefix. This does not restore removed nodes into the desired-model index: compare
publication state and necessary captured presentation facts, not the old entire model.
Retry commits once; clone/remount/stale/replayed tokens cannot delete replacements. Include
paint-only and no-owner final frames so commit is not conditional on resolved dimensions.

#### R3 — real context inputs and cause-specific invalidation (P3.1, P3.3)

**Producer audit (notification/provenance wiring pending):** `renderer::{load_font,
clear_renderer_asset_context,font_cache_generation}` already owns a renderer-local font
revision. `services::load_font_bytes` / `load_font_nif` currently do not notify the tree;
wire existing routes rather than inventing a new NIF. `TreeMsg::AssetStateChanged` compares
cached intrinsic media dimensions, then requests Measure even for unchanged raster facts.
Actor asset setup now precedes preparation. `FrameMeasurer` supplies frozen query image
facts to native measurement; renderer font snapshots bind generation and cache identity
through endpoint queries and native layout/refresh. Deferred scene rasterization still lacks
that snapshot, and custom providers must keep their supplied metrics stable. Complete
producer notifications/provenance and the rendered-scene environment before closing R3. Avoid holding asset/font
locks through layout or retaining an entire renderer/pixel cache just to hold font facts.

**Touchpoints:** `actors.rs` message inputs, `runtime/tree_update.rs`, metric-notice handling
in `runtime/tree_actor.rs`, `element.rs`, `invalidation.rs`, `layout/projection.rs`.

- Carry renderer-local model, viewport/scale, text/font metrics, image dimensions,
  interaction/scroll seeds and owner-target versions separately. Audit their actual producer
  routes before choosing fields; do not invent a notification which no caller updates.
- Replace the runtime's `metrics_epoch: 0`. Asset hydration remains normal setup outside
  queries. Freeze image dimensions and retained font environment for an attempt; distinguish
  readiness/dimension/font changes from a raster-only revision with unchanged layout facts.
- Represent invalidation as a set of applied causes, with affected nodes/scopes and before/
  after version evidence. Allow continuous motion, discrete edits and declared resets in
  one batch. Generic `Measure`, arbitrary epoch changes and float differences are not
  provenance and never authorize a release mismatch.
- Initialize newly relevant runtime/scroll/image seeds on admission or state changes and
  update them from sparse notices. Reset old query seeds before applying deltas. A paint-only
  patch must not copy the model or allocate a geometry workspace.
- Prepare one frozen base and sparse group overrides. Reuse current request deduplication;
  remove full foreign-projection clones per request. Initially conservatively dirty affected
  context through native propagation, then measure visits; do not add a future dependency graph.

**Tests/exit:** genuine font/image changes, pending asset becoming ready, same-size raster
refresh, newly scrollable node, hover/focus/press and independent renderers. Queries cannot
hydrate, clamp live scroll or contaminate another request. Count cold/warm context visits,
projection entries/bytes and model copies, not just resolver calls. Receipts bind these real
versions; notices after freeze cause a new attempt rather than changing the frozen one.

#### R4 — prove continuous-context continuation and bounded holds (P3.2)

**Touchpoints:** `Track`, target preparation and sampling in `animation/lengths.rs`,
`animation/timing.rs`, group finite-work records and native dimension inputs if needed.

1. Separate anchor lifetime from endpoint-cache lifetime. A target may refresh without
   resetting `from`, anchor progress or interval end. Store an explicit reason for an
   anchor change; the current unconditional `anchor = progress`/`hold_start = now` on a
   changed query is not the replacement rule.
2. Implement the stable-anchor candidate specified in P3.2 first for a foreign direct-pixel
   driver. Prove fixed-context reduction and continuous input behavior with exact arithmetic.
   Then add symbolic foreign motion, nested fill/content and unanimated-ancestor coupling.
3. A discrete target/context event captures the current committed presentation once and
   establishes remaining motion from it. Coalesced events use the first source/latest target;
   retries reuse that admission rather than capturing failed candidates as new sources.
4. Held retarget intervals are explicit finite work, not independent group clocks/specs.
   An interval started from 1.25s to 2s remains work until 2s even if its original moving peer
   is cancelled at 1.5s. A new arrival ending at 3s may extend ownership, but must not stretch
   an unchanged interval to 3s. After reaching its destination it holds again if necessary.
5. Keep target batches immutable and independent of traversal order. Build the complete
   candidate pose, then evaluate combined release/consistency against that pose. This catches
   a foreign track whose newly prepared sample differs from its frozen pre-query sample.
6. If that bounded construction fails under coupling, write the smallest native counterexample
   and add scoped context response/transport to ordinary dimension evaluation. Specify units
   and intrinsic/initial/charge/placement/policy behavior; do not patch only visible widths.
   Prove the revised mechanism before proceeding—neither the formula nor a second query
   automatically proves a solution. No repeated settle/correct/query loop is allowed.

**Budget:** at most one joint-target batch and one complete-pose consistency/release batch,
plus distinct counted missing-source/transport requests. Each batch may cost O(N); report
actual Q/V work. Repeated queries until agreement or work proportional to dropped pulses fail.

**Tests/exit:** both axes, .5/1/2 scale, all curves, 30/60/120Hz plus sparse final intervals;
finite group beside a loop; simultaneous parent/child completion; content ancestor coupling;
cancel/arrive during held retarget. Use independently pinned schedules, permuted query order
and cache-disabled replay. Valid continuous context releases without `ReleaseMismatch`,
extra near-terminal pulses, terminal endpoint overwrite, guessed time or weakened tolerance.

#### R5 — deadline-time external transitions with native witnesses (P3.3)

**Depends on R3/R4:** this is not a generic exception path for unresolved continuous lag.

- Define a witness binding the specific applied event, old accepted endpoint/context evidence,
  new context, transported channels/scopes and owner/target generations. Reuse trusted native
  endpoint evidence where sufficient; otherwise count the required native projection using
  retained first-write/event facts. Do not retain an old full tree per event or synthesize
  historical layout from today's assets. Design capture timing before destructive patches
  for any evidence that would otherwise be unavailable.
- Preserve event ordering: sample the unchanged absolute clocks, select candidate ownership,
  account separately for continuous context, then apply the recorded discrete native-context
  transition. Coalesced causes cannot let an unrelated resize forgive pre-existing bad charge.
- For zero remaining time, compare the ordinary terminal-expression layout under the new
  input with replay of the witnessed endpoint in that same new context. Both must agree on
  all full-footprint channels and affected geometry before sample removal is authorized.
  A native no-animation control explains the external discontinuity; it does not excuse a
  different discontinuity caused by removing ownership.
- No extra duration, restored old input, indefinite hold, blanket mismatch bypass or special
  weighted mode. Keep the existing corruption validator active before and after transport.

**Concrete mandatory oracles:**

| Case | Required evidence |
| --- | --- |
| Weight 1→3 at 1s; viewport/root context 600→800; static weight 1 | Old terminal 450/150; new native terminal 600/200; sampled replay and removal agree in 800; release succeeds on that attempt |
| Bounded `min(50, weighted_fill(3))` in the same contexts | Visible 50 remains; charge changes 450→600 and peer 150→200; equal visible width must not hide a bad debit |
| Resize at 1.25s with group deadline 2s | Existing 300→350→400 held trajectory remains continuous; this is not the zero-time path |
| Deadline event plus foreign/nested continuous motion | New context stays installed, complete candidate/release pose agrees; no quiet-frame retry |
| Unrelated paint edit plus corrupted charge/policy | Typed failure; event provenance grants no geometry waiver |

Add scale, metric replacement, interaction/wrap and topology witnesses separately. A passing
viewport witness does not qualify them. Removal/reparent must also satisfy R7 transport.
**Exit:** valid late input succeeds while the new input is retained; recovery tests that
restore old context remain failure controls, not continuity acceptance.

#### R6 — field handoffs, boundaries and source compaction (P4.1, P5.2)

**Touchpoints:** `selected_runs`, `sample_animation_for_element`,
`finish_active_enters`, `finish_unheld_changes`, `handoff_changes`, timing and source records.

- Replace node-wide handoff activation with mask-specific intents from R1. Build each field's
  source from its actual committed terminal state; never overlay the entire enter terminal
  attrs on independent paint/font/change fields. Overlapping fields wait only for their owner.
- Preserve terminal-expression publication followed by an intentional different-base/regular
  publication. Carry required follow-up damage even when all moving maps are empty. Equal-base
  handoffs require no redundant visible frame. Commit pending starts with the existing shared
  clock/presentation-anchor convention; backend presentation acknowledgement cannot invent a
  second run or double-advance it.
- Preserve latest pending target/timing and whole-source interruption, then compact completed
  sources to only the fields/context still needed for hold, conversion or handoff. Use weak
  references/record counts in tests to prove old specs/sources are freed.
- Select current segment/cycle arithmetically for long gaps. Reuse a valid adjacent endpoint;
  for missing boundaries, form bounded source requests in the correct group epoch. A cold
  first frame after several cycles cannot replay each cycle or pretend the original first
  frame was just presented. Declared reset discontinuities remain explicit.
- Validate nonfinite/nonpositive/unrepresentable timing at admission rather than allowing
  `finite_deadline(None)` to silently turn invalid finite work into loop-like context.

**Tests/exit:** enter width held while alpha hands off; pending width and disjoint color;
regular/change conflicts and aliases; terminal→base with a concurrent resize; all-owner
boundary-time joins; reversals, 3+ segments, finite repeats and cold/warm skips. Query count
is independent of skipped cycle count, and source retention returns to baseline after commit.

#### R7 — ghost, removal and allocation-scope transactions (P5.3/P5.4)

- Replace pre-preparation ghost pruning with intents consumed by R2. An expiring alpha-only
  ghost beside a failing geometry group must remain in the last committed scene/registry.
  Its absolute exit deadline still passes; successful recovery removes it without replaying
  an unnecessary terminal ghost frame.
- Capture the whole committed exit baseline before destructive removal, including held
  layout for paint-only exits. Stage departure from the live pool and admission to ghost
  coordinates together. Audit reuse of an exit entry by numeric id: new mount/ghost identity
  must not inherit an old captured spec, generation or finished clock.
- Represent scope conversion as source node/parent units, origin, scale and placement role
  plus a native destination projection. Derive destination debit/placement; copying charge
  or scaling only the visible box is invalid. Remove old reservations and install the new
  source in one frame; dirty both native ancestor chains without combining future groups.
- Preserve successful patch prefixes and first-write sources through failed conversion,
  removal and upload. Retain only uncommitted removal data actually required for the next
  transaction, not indefinite deleted-tree history. Stop/upload discards obsolete attempts.

**Tests/exit:** moving/held removal; nested ghosts; same-id remove/reinsert; row↔column,
flow↔wrapped/float, flow↔Nearby, Nearby slot changes, differing parent scales and hard
constraints; failure during each conversion and empty-batch recovery under both policies.
Inspect reservations, positions, registry hits, source/ghost retention and final pulse state.

#### R8 — actual caller migration and bounded recovery pacing (P6.2, P4.2)

Use R2's common coordinator; do not retain separate timing/application logic in wrappers.

| Caller set | Concrete migration/acceptance |
| --- | --- |
| `runtime/tree_update.rs` full/active/dirty paths | Same input freeze, preflight, commit and error policy; visitation mode is the only difference |
| Mutable measured helper and immutable animated wrappers in `tree/layout.rs` | Advancement requires mutable owner state and typed result; split read-only presentation intent explicitly; remove swallowed animation errors |
| `events/test_support.rs`, animation callers in `events/runtime.rs` and `events/registry_builder.rs`, layout/cache/transform tests | Declare advance versus committed snapshot intent; migrate tests without weakening expected registry/geometry behavior |
| `benches/layout.rs`, `benches/renderer.rs`, profile/dirty-id/clean-registry wrappers | Compile against the common coordinator; cache/diagnostic toggles cannot bypass lifecycle or select legacy sampling unnoticed |
| `services::render_tree_offscreen`, pixels/PNG | Preserve fresh-tree intent and asset setup; no invented retained history; asset `Snapshot` mode is not animation snapshot semantics |
| Retained headless/raster and renderer readback | Owning runtime advances; repeated readback uses committed presentation, changes no clock/owner and consumes no journal |

- Return typed frame errors internally (a common frame-error wrapper may replace using
  `ProjectionError` for admission/commit). Format at existing actor/service boundaries;
  no new NIF, per-frame BEAM data, resource strategy or scheduler change is needed.
- Add one bounded failed-attempt record: relevant input/context/owner versions, failure stage
  and retry eligibility. Do not key retry solely by ever-increasing pulse time: that would
  turn the same invalid input into perpetual new attempts.
- Under LogAndContinue, retain the last complete publication and applied-prefix recovery
  state, suppress identical failed-input pulse retries, and retry once on an explicit recovery
  request or relevant newer input/context. ReturnErr reports the same failure without losing
  that state. Preserve the existing explicit empty-batch recovery route.
- Distinguish `moving`, `held`, `pending-final-damage`, `blocked-on-input` and `idle` in the
  coordinator's pulse result. A blocked attempt is not successful inactivity. Legitimate
  changing foreign context can make another attempt eligible, but stale/corrupt input must
  not busy-loop merely because unrelated paint or a backend tick arrived.
- Drain final damage through existing pacing before becoming idle. No new timer, indefinite
  retry queue or automatic restart. Stop and upload clear obsolete failure records.

**Tests/exit:** identical-input errors over many pulses perform no repeated queries; a relevant
notice/explicit empty batch recovers without resending the patch or consuming sources twice;
final handoff and last-ghost removal publish before pulses stop. Compile benchmarks and audit
all animated call sites, not only the helpers exercised by current unit tests.

#### R9 — combined native acceptance, then hand back to P7/P8 (P6.3)

- Generate the ordered representative length-pair matrix on both axes for all owners in a
  small deterministic fixture. Use targeted cross-products for nested/context/lifecycle cases
  rather than an unbounded Cartesian product. Preserve all original allocation controls.
- Replay identical successful and failing input/time histories through actor full/active/dirty,
  measured direct and retained headless paths. Normalize opaque identity tokens only for
  cross-runtime comparison; assert their real isolation separately. Compare footprints,
  frames, pixels, scroll/clip state, registry hits, lifecycle effects and pacing decisions.
- Test read-only and fresh-tree intents independently; neither must manufacture advancement
  history to match a retained renderer. Supply custom measurers/fonts and real metric notices.
- Vary group/query order, disable query/layout caches, clone/remount/upload, and inject errors
  in a later group while an earlier group and an expiring ghost are ready. Assert one complete
  publication or none, correct same-schedule replay, and bounded retention after recovery.
- Report source/target/transport/consistency/release query counts, actual visited entries,
  projection bytes, current/peak active and staged records, and workspace lifetime for cold,
  warm, held, retarget and release phases. Keep paint/direct-pixel fast-path controls.
- Run the code-phase commands in section 10 and compile affected benches. Report unavailable
  macOS/device/RSS qualification separately. Current ExUnit success remains legacy behavior
  protection until P7 deliberately enables production-build mixed-core integration.

**Definition of done for this request's implementation scope (not yet met):** R0–R9 and the corresponding P3–P6 exits
pass together. Deadline resize succeeds with new input, continuous context has no extra
release jump, all lifecycle effects transact together, no caller bypasses commit/error
semantics, and failures/final damage have bounded liveness. This is the native completion
checkpoint—not permission to skip P7 API/protocol work or P8 public release qualification.

### P0 — lock executable semantics

- [x] Record the reaffirmed joint-target/hold decision, including weighted lengths (D1).
- [ ] Turn membership, segment-epoch, held-retarget, loop and late-release defaults into
  small scenario tables with expected values, deadlines and ownership states.
- [x] Add group-runtime tests separate from manual footprint investigation tests.
- [ ] Define current-context release parity, allowed external discontinuities and failure
  recovery without an unbounded settle/replay algorithm.
- [x] Preserve the earlier schedule-specific oracles with a genuinely foreign looping peer;
  do not impose general cross-rate equality.

**Files:** `animation/lengths/tests.rs`, new `animation/groups/tests.rs`, existing
`layout/tests/animation_groups.rs`, this plan.
**Exit:** no implementation needs to guess whether a run is moving, held or released;
D1 assertions implement the settled joint-target/hold contract; legacy arithmetic remains
labelled historical evidence, not a blocker to changing the grouped runtime.

### P1 — group ledger and lifecycle separation

- [ ] Add typed group/run/field identities and parent/slot/ghost/root lookup.
- [ ] Add membership deltas, target epochs and finite-barrier calculation.
- [ ] Separate motion completion from geometry release in existing owner synchronization.
- [ ] Include held geometry in active preparation without treating paint-only owners as waiters.
- [ ] Stage release/handoff intents rather than deleting entries before frame preparation.
- [ ] Define snapshot/clone behavior so groups or mutable workspaces cannot cross renderers.

**Partial implementation:** finite owner/barrier ledger, regular/enter/change retention
and post-layout live-group release commit are present. Sparse membership/epochs,
field-level identity, ghost lifecycle and complete entrypoint finalization remain open;
P1 is not complete.

**Files:** new `tree/animation/groups.rs`, `animation.rs`, `animation/change.rs`,
`animation/lengths.rs`, `tree/element.rs` only where needed for existing ownership seams.
**Exit:** pure state-machine tests cover joins, different durations, completed holds,
independent parents, paint loops, empty groups, remount and final pulse state. Membership
work is proportional to admitted/affected owners; no per-pulse tree discovery.

### P2 — finite sibling group geometry

- [ ] Replace per-track target preparation/release with group-wide target projections.
- [ ] Batch both axes, preserve full foreign samples and cache unchanged joint targets.
- [x] Use resolved footprints for non-pixel pairs, including compatible weighted bounds;
  preserve the direct Px→Px numeric fast path in the gated core.
- [ ] Retain early finishers' footprints and remove the release set before one live layout.
- [x] Return staged group preparation failures, not partial sample installation.

**Partial implementation:** joint targets, full holds, stable-context release, both-axis /
scale / static-sibling runtime tests, D1 sampling, combined release evaluation and
all-or-nothing geometry updates pass. Complete runtime query-order/visual/context oracles,
late-context continuity and lifecycle/entrypoint parity remain open.

**Files:** `animation/lengths.rs`, `animation/groups.rs`, `layout/projection.rs`,
`layout.rs` preparation seams, group runtime tests.
**Exit:** real runtime produces 300/170 at 1s and 300/300 at 2s; full held/released
footprints, sibling positions and scopes match. Test axes/scales/bounds/static siblings,
zero-query fast paths and no additional warm queries for fixed context. Manual tests alone
are insufficient. Query permutation and fresh evaluator results agree.

### P3 — late-context continuity and context preparation

**Goal:** replace deadline-time rejection for valid context changes with motion that
approaches its current-context terminal layout before symbolic release. D1 and joint
holds are settled; the continuation mechanism below is an implementation candidate to
prove, not a new user decision. Do not remove `ReleaseMismatch` to make tests pass.

#### P3.1 — executable failure cases and context-change provenance

- [ ] Add a deterministic frame driver recording input batches and explicit sample times.
  Record, separately, the last published footprint, cached target, pre-query sample,
  prepared sample, pre-removal sample and fresh terminal-expression result. Include
  complete footprints, affected frames, scroll extents and ownership/pulse state.
- [ ] Preserve the current deadline-resize rejection test as a failure/recovery control;
  add a separate positive test that keeps the new size instead of restoring old context.
- [ ] Classify target invalidation from applied events, not from floating-point differences
  or a generic `Measure` flag. Coalesced frames may contain more than one cause:

| Cause | Examples | Required treatment |
| --- | --- | --- |
| Stable context | Ordinary progress within a fixed joint-target epoch | Reuse target and original easing; no new query |
| Continuous animated context | Moving ancestor, descendant intrinsic output, foreign layout animation | Continue a context-dependent trajectory; do not reset its anchor on every pulse |
| Discrete target/context event with time left | Retained target edit, group arrival, resize, metric replacement, interaction switch | Capture current full presentation once; reanchor over remaining admitted time |
| Intentional declared reset | Repeat boundary, explicit new first keyframe | Preserve the declared reset and absolute clock; do not disguise it as context drift |
| External discontinuity at/after the deadline | Resize/topology/asset/interaction change delivered with release | Apply the documented native-context transition; no invented animation duration |
| Invalid input or stale transaction | Bad scope, malformed timing, old owner generation | Typed failure; no partial publication or ownership commit |

- [ ] Propagate cause plus model/metric/runtime/constraint and target-epoch stamps into
  preparation. A model epoch alone cannot authorize arbitrary release differences.
  Presentation-anchor adjustment is clock bookkeeping, not a fresh UI target change.
- [ ] Prove these cases before replacing the current retargeting rule:

| Fixture/event | Oracle/required result |
| --- | --- |
| Fixed weighted pair in 600px | Preserve 375px midpoint and native terminal 450px |
| Existing held resize at 1.25s, group ends at 2s | Preserve 300→350→400 over the established remaining interval |
| Weight 1→3 ends as root changes 600→800 | New terminal is 600px beside weight 1, not stale 450px; ordinary resize explains the external change, removal adds no further change |
| Parent and nested fill child finish together | Both approach their fresh native terminal frames; dense and skipped pulses must not require a retry to release |
| Finite group beside an unbounded layout loop | Use the loop's current pose, not its future endpoint; release the finite group without waiting for the loop |
| Continuous child growth through an unanimated content ancestor | Update the neighbouring group's context without merging groups or scanning future schedules |
| Same visible box, wrong charge/placement/policy | Still reject; provenance must not waive full-footprint validation |

**Files:** `animation/lengths/tests.rs`, `animation/groups/tests.rs`,
`layout/tests/animation_groups.rs`, `runtime/tree_update.rs` tests.

#### P3.2 — stable-anchor continuation, not repeated end-of-interval chasing

The current resolver reanchors every changed target at the current progress. Repeated
continuous context changes can therefore leave the cached target one context sample
behind when no easing time remains. Fix the trajectory during motion, not only its last
frame.

- [ ] Introduce an explicit continuation record: complete anchor footprint, anchor time /
  progress, motion or context-retarget deadline, target epoch and context provenance.
  Reuse the existing owner clock and `timing::remaining_progress`; do not generate specs.
- [ ] Prototype continuous-context sampling with a stable anchor within one uninterrupted
  context episode:

  ```text
  candidate(t) = interpolate_full(anchor_footprint,
                                  joint_endpoint(current_context(t)),
                                  remaining_curve(anchor_progress, progress(t)))
  ```

  A discrete edit creates a new anchor from the current applied presentation. Continuous
  foreign motion refreshes the endpoint without creating another anchor. Fixed context
  reduces to the already-tested interpolation. This formula is not sufficient evidence
  on its own: current-context construction and coupled layout must pass the tests below.
- [ ] For completed motion with remaining group work, establish a bounded context-retarget
  interval once. Hold unchanged targets exactly. Continuous context updates may update
  its destination but must not keep moving the interval's end forward.
- [ ] Separate target evaluation from installation. Prepare all group queries from one
  immutable input batch; never let hash-map traversal feed one new target into another.
  Validate release against the actual complete pose selected for publication, not a
  mixture of old foreign samples and newly installed ones.
- [ ] Explicitly test the coupling hazard: replacing repeated reanchoring with the formula
  can change a prepared foreign sample after the original context was frozen. Audit both
  sides of the release boundary. Do not accept a formula that only matches a release
  query against stale foreign inputs.
- [ ] If numeric footprints cannot carry a required ordinary constraint response, prototype
  a scoped context-transport input in `layout/dimensions.rs`, using the existing native
  measure/resolve/allocation passes. Specify every transported channel and its units.
  Do not implement ad-hoc weight arithmetic or a parallel layout solver in animation code.
- [ ] Bound preparation to one joint-target batch and one combined release/consistency
  batch, plus explicitly counted missing-source/rebase requests. A batch may traverse N
  model nodes. Needing a repeated settle/correct/query loop fails this implementation gate.
- [ ] Keep existing schedule-specific trajectories as labelled historical controls if the
  continuation mechanism intentionally changes them. Pin new schedules independently;
  do not silently loosen tolerances or impose general cross-rate equality.

**Exit:** finite groups release with no artificial removal jump under continuous nested /
foreign motion, including a long last pulse. The test driver evaluates the pre-removal
limit under the same event history; adding a convenient extra near-terminal pulse must
not be the only way to make a sparse schedule pass. If the candidate fails, repair the
bounded native context mechanism before continuing; rejection is not the final behavior.

#### P3.3 — zero-time external transitions and real context versions

- [ ] Keep actual external discontinuities distinct from animated-context lag. At zero
  remaining time, use ordinary native terminal expressions in the new external context;
  do not add a guessed duration, globally freeze layout or wait for a future quiet frame.
- [ ] Build a context-transition witness from the applied event and native endpoint facts:
  old accepted terminal context, new context, affected scopes and resulting full channels.
  Compare with a no-animation native control for that same external event. Only the
  external transition is permitted to move geometry discontinuously; removing ownership
  in the new context must add no further change. Never broadly ignore `ReleaseMismatch`
  merely because some model or metric revision changed.
- [ ] Exercise resize/scale, font/image replacement, hover/focus/press, wrap transitions,
  reparent/removal and loop resets separately. Unrelated paint updates authorize no
  geometry discontinuity. Mixed continuous and discrete causes must have explicit order.
- [ ] Wire real metric revisions and sparse changed runtime/scroll/image seeds. Replace
  `metrics_epoch: 0` and per-pulse asset polling; initialize a newly relevant seed when
  scrollability or interaction state appears without a structural edit.
- [ ] Supply the same viewport, inherited font, scale and frozen metrics to target,
  transport, release and live layout. Normal asset setup precedes freezing; queries
  remain read-only and do not call asset hydration helpers.
- [ ] Preserve one private workspace across context-only changes. Count context visits,
  projection entries/bytes, comparisons, query kinds and native measure/resolve visits.
  Build shared frozen-base/group-delta requests instead of cloning a full map per group.

**Files:** `animation/lengths.rs`, `animation/timing.rs`, `layout/dimensions.rs`,
`layout/projection.rs`, `element.rs`, `invalidation.rs`, `patch.rs`, metric-notice routes
in `runtime/tree_update.rs` / `runtime/tree_actor.rs` and their tests.
**Exit:** deadline-time resize succeeds without restoring old input; valid continuous
context does not hit `ReleaseMismatch`; corrupt-channel controls still fail. No unsupported
case is hidden by an arbitrary delay, retry loop, special weighted path or extra layout engine.

### P4 — boundary admission, finite barriers and release transactions

#### P4.1 — event order, held deadlines and skipped boundaries

- [ ] Form one admission batch for regular, enter, exit and change before closing any
  parent barrier. Current change-arrival coverage is insufficient for a new regular run
  or an enter→regular handoff at the same timestamp.
- [ ] After accepted admissions, select segments/cycles by absolute clock, update target
  epochs, then derive release candidates. Use run generations rather than timestamp
  equality to distinguish same-time replacements; timing-only rerenders still do not restart.
- [ ] Make an already-started context-retarget interval explicit finite group work.
  Cancelling the peer that originally extended the barrier must not truncate that interval
  and release a half-retargeted hold. A later arrival may extend group ownership, but must
  not stretch an unchanged interval unnecessarily. An unbounded loop contributes no barrier.
- [ ] Reanchor a held field once when a discrete new target arrives. Test arrivals and
  cancellations during the existing 300→400 retarget, not just while a hold is stationary.
- [ ] Retain adjacent-segment endpoint provenance. Distinguish a known preceding endpoint
  from a missing segment/cycle boundary and from an intentional repeat reset.
- [ ] Resolve a missing source with the appropriate parent-group boundary expressions;
  do not accidentally evaluate one fill source against peers from a different target epoch.
  Specify cold first preparation after several missed boundaries as well as a warm skip.
- [ ] Select current cycle/segment directly. Bound missing-source queries by current affected
  owners/requests, not elapsed frames or skipped cycles. Reject invalid/nonfinite and
  unrepresentable timing at admission; do not classify overflow as an ordinary loop.

**Tests:** 3+ keyframes, all curves, reversals, unequal durations, finite repeats, tiny
valid durations, long gaps, repeated timestamps, newly admitted regular/enter/change runs,
arrival exactly at the old deadline, cancellation during a held retarget and looping peers.
**Files:** `animation/groups.rs`, `animation/timing.rs`, `animation/lengths.rs`, `animation.rs`.

#### P4.2 — versioned preparation and bounded error recovery

- [x] Replace unversioned `(NodeId, Axis)` release removal with a non-cloneable receipt
  binding current root/mount/model/constraint/scale, sample time, frozen-context identity,
  runtime/workspace identities, immutable group versions and exact track generations/targets.
  Identical synchronization can commit; stale inputs/clocks/owners and clone transplants cannot.
- [ ] Extend this release receipt to the complete frame: real external context and target
  epochs, canonical owner-field masks, and all application/lifecycle effects. Validate before
  live mutation, not just before retirement; keep no rollback tree or receipt history.
- [ ] Validate all required queries, installations, removals and lifecycle effects before
  applying a frame. Release candidates due together use one combined release state.
  Queries cannot mutate published geometry, registry state, sources or owner clocks.
- [x] Commit only prepared live-group track generations after successful frame construction;
  protect same-node replacement sources and discard receipts on geometry clone. A consumed
  receipt cannot commit twice, and recreated group membership cannot reuse its old version.
- [ ] Extend the same guarantee to staged owner admission, every field handoff, ghosts,
  upload/remount publication and all callers; transaction-wide rollback-free atomicity is open.
- [ ] Treat handoff/removal damage and pulse continuation as receipt outputs, not facts
  reconstructed after entries were deleted. Define the commit boundary once for actor
  and direct callers; rendering later on the backend must not change animation clocks.
- [ ] Separate recoverable preparation failure from ordinary active motion in the actor.
  Preserve the last published frame and pending successful-prefix effects. Under
  LogAndContinue, do not continuously resubmit the same failed input on animation pulses.
  Use the existing pacing/message protocol to request another attempt only for explicit
  recovery or a relevant newer input/context version; no new backend scheduler.
- [ ] Test ReturnErr and LogAndContinue during admission, target/source/rebase query,
  release validation and handoff. Successful recovery must not restart the run, duplicate
  completion, consume a source twice or require resending an already-applied patch.

**Exit:** valid terminal frames commit once and stop pulses after final damage. Actual
errors cannot publish half a release set or cause busy retries. No stale-generation removal,
indefinite normal hold or work proportional to missed time is introduced.

### P5 — field-level lifecycle and allocation-scope conversion

#### P5.1 — finish owner identity and transactional admission first

This identity work is a prerequisite for P3/P4 receipts; it may be implemented before the
continuation changes, rather than waiting for the rest of P5.

- [x] Add monotonic generations to admitted regular/enter/exit/change runs, separate from
  pure sampling clocks. Change identity is no longer a field enum or fingerprint; no-op and
  timing-only targets retain the generation. Reject counter exhaustion without wrapping.
- [x] Version group membership/segment/cycle/barrier state with immutable tokens, preserving
  the token on unchanged pulses; invalidate stale versions even after identical recreation.
- [ ] Add canonical moving/held/pending/releasing field masks and complete target-context
  epochs. Finish sparse membership deltas through existing parent links; current refresh
  still builds and compares member snapshots.
- [x] Share immutable enter/exit/change specs across committed/candidate metadata; retain
  only a committed value and latest staged replacement per key.
- [x] Introduce bounded owner/group admission deltas, keeping committed metadata through
  failed preparation. Preserve legacy eager behavior outside the test gate.
- [x] Test change/regular restoration after failed B, within-batch A→B→A, retry B, failed B
  then C, published B then A, cancellation, same-generation handoff and remount identity.
- [ ] Complete field-level committed presentation ownership and all-owner lifecycle intents;
  couple every delta to R2's pre-mutation frame token, not merely a post-layout caller hook.
- [ ] Compact held sources and complete sparse membership producers/target-context epochs.

#### P5.2 — field handoffs and source lifetime

- [ ] Replace `handoff_changes`' node-wide pending activation with field-specific release.
  A finished enter paint field may hand off while width stays held; an overlapping width
  change waits. A disjoint regular/change field must not be blocked by another held field.
- [ ] Capture each handoff's actual committed terminal attrs and complete dimensions before
  releasing old records. Never reconstruct the source from only visible pixels or apply
  all of the enter's final attrs over independently updated fields.
- [ ] Keep only the latest pending target and its incoming timing. No artificial run on
  first mount, policy-only changes or timing-only rerenders. Canonicalize aliases/conflicts
  consistently with the planned Elixir validation, without changing the wire format yet.
- [ ] Preserve the explicit terminal→different-base/regular contract as a separate handoff
  publication. Request final damage even when runtime maps become empty; avoid a redundant
  extra frame when no attrs, ownership or event geometry actually need another publication.
- [ ] Replace whole finished source retention with compact held-field provenance once
  interruption, conversion and error recovery no longer need the full source. Free it on
  commit/cancellation/remount; assert Arc/record counts rather than only `is_empty()`.

**Tests:** paint completes while geometry is held; latest pending width and independent
alpha/font changes; enter→regular and enter→change under resize; equal-base and different-base
handoffs; scaled interruption; repeated timestamps; full source cleanup after final damage.

#### P5.3 — ghost creation/removal in the transaction

- [ ] Replace destructive pre-preparation `prune_completed_exit_ghosts` with staged removal
  intents. Include paint-only exit completion: an unrelated group's failed release must
  not irreversibly remove a ghost before any replacement frame is published.
- [ ] Capture the whole current presentation for an exit, including alpha-only exits from
  active or held layout. The baseline must not fall back to declared target geometry.
- [ ] Give ghost roots their explicit ownership/scope identity; remove live pool membership
  when the exit is admitted and derive ghost-coordinate samples through native layout.
- [ ] Commit ghost removal, registry/scene damage and final pulse state together. Preserve
  existing declared exit timing; do not add a visible terminal ghost frame by accident.
- [ ] Cover nested ghosts, pending enter/change cancellation, remove/reinsert with the same
  numeric id, failed release beside an expiring ghost and upload during an exit.

#### P5.4 — reparent/Nearby conversion and prefix recovery

- [ ] Capture source before changing parent links. Carry node/parent logical scales,
  placement origin and allocation role; convert position/box units independently from debit.
- [ ] Use the existing native evaluator to derive destination-role allocation/placement
  for the captured presentation. Never copy a flow charge into a wrapped/float/Nearby scope
  or infer charge by scaling the visible width.
- [ ] Stage removal of the old reservation and admission in the new scope atomically;
  invalidate both parent groups and native ancestors. Treat actual hard-constraint changes
  as native layout effects, not a reason to weaken scope validation.
- [ ] Preserve first-write sources and successful patch prefixes under both error policies,
  including empty recovery batches and manual text shortcut fallback. Model upload,
  remount, policy cancellation and rejected conversions must have bounded retention.

**Files:** `animation.rs`, `animation/{groups,change,source,lengths}.rs`, `element.rs`,
`patch.rs`, `layout/{dimensions,projection}.rs`, `runtime/tree_update.rs` and actor tests.
**Exit:** every owner uses the same generation/field ledger and completion transaction;
no ghost is pruned early, no field waits for unrelated paint/layout ownership, and sources,
reservations, final damage and pulse state survive failure and clean up exactly once.

### P6 — one orchestration contract and entrypoint parity

#### P6.1 — consolidate frame inputs, intent and result

- [ ] Define a shared frame input containing sample time, constraint/scale, inherited font,
  supplied measurer, frozen metric/runtime context and an explicit preparation intent.
  Reuse current types; do not duplicate clocks or create a second sampler.
- [ ] Separate clocked advancement from read-only presentation rendering and fresh-tree
  rendering. Only clocked advancement admits/completes owners or consumes a receipt.
  Do not obtain hidden wall-clock time in a read-only helper.
- [ ] Return a typed prepared-frame result containing pose changes, release/lifecycle
  receipt, layout/paint/registry damage and pulse requirement. Remove `.unwrap_or(false)`
  and diagnostic-string-only failure handling from animated wrapper paths.
- [ ] Keep one evaluate/apply/layout/refresh/commit sequence. Full, active and dirty
  preparation differ only in visitation optimization and receive identical context.
  Failure before application preserves the previous published frame and recovery inputs.

#### P6.2 — migrate the actual call sites

| Current path | Required migration |
| --- | --- |
| `TreeUpdateEngine` full/active/dirty branches | Shared fallible preparation and one post-frame receipt commit; identical failure/final-damage behavior |
| `try_layout_tree_with_animation` | Retain caller-supplied measurer/font and explicit mutable runtime; use the common receipt contract |
| `layout_tree_default_with_animation` | Stop swallowing errors or leaving release uncommitted; make callers' advance versus snapshot intent explicit |
| `layout_and_refresh_default_with_animation`, `layout_or_refresh_default_with_animation` | Delegate to the same fallible orchestration, not an immutable-runtime partial lifecycle path |
| Benchmark/reusing-clean-registry/profile wrappers | Use the same behavior as the feature under measurement; diagnostics must not silently benchmark legacy sampling |
| `services::render_tree_offscreen` / pixels / PNG | This decodes a fresh tree, not a retained animation snapshot. Preserve/document fresh-tree behavior without inventing prior targets or advancing a renderer |
| Retained raster/headless renderer and any existing presentation readback | Advance only through the owning runtime; readback renders the selected committed pose without releasing its owners |

- [ ] Audit all callers rather than replacing argument types blindly. Propagate typed errors
  through existing Rust/NIF/service boundaries; no new NIF entrypoint, scheduler change,
  per-frame BEAM geometry traffic or cross-renderer mutable cache.
- [ ] Keep offscreen asset preparation separate from read-only animation/layout queries.
  `OffscreenAssetMode::Snapshot` is an asset policy, not evidence of a retained animation
  snapshot; `snapshot_tree_sources_for_offscreen` is not a read-only metric getter.
- [ ] Align actor/direct commit timing and final pulses, including enter/base mismatch,
  last-ghost removal, no-owner final refresh and repeated invocation at the same timestamp.

#### P6.3 — generated parity, visual and retention gates

- [ ] Run the complete ordered representative length matrix on both axes and all owners;
  preserve direct Px, weighted/bounded and original allocation controls. Add targeted
  image/video, text/wrapping, parent-imposed width, scale/rotation, Nearby and scroll cases.
- [ ] Replay identical model/input/time sequences through full, active, dirty, direct and
  headless paths; use custom measurers/fonts and genuine metric revisions. Compare full
  footprints, frames, pixels, clips, registry hits and scroll extents, not just widths.
- [ ] Compare advancement paths with the same committed owner state. Test fresh-tree and
  read-only paths against their distinct documented intent; do not require them to invent
  equivalent clocks or history.
- [ ] Vary group/query order, disable endpoint-cache reuse, inject failures and clone/remount
  trees. Verify deterministic replay, no stale receipt commit and renderer-local isolation.
- [ ] Assert final damage is published before pacing stops; errors do not hot-loop; sources,
  groups, pending handoffs and workspaces are freed when no longer needed.
- [ ] Report cold/warm/held/retarget/release costs separately, including projection/context
  work. Preserve paint/transform-only zero-workspace behavior and existing device budgets.

**Exit:** no supported live path can report inactive merely because preparation failed,
finish owners before their frame, silently use Skia instead of a supplied measurer, or
succeed only because it bypasses the new lifecycle. P7 then removes the production gate
with codecs/protocols and runs production-build integration; desktop P6 tests alone do
not constitute public, macOS or constrained-device qualification.

### P7 — public API, codecs, documentation and platform enablement

- [ ] Rework—not blindly apply—the public draft against completed native semantics.
- [ ] Implement `Animation.change/3`: one ordinary animatable attr, duration and curve.
- [ ] Normalize ordinary targets plus per-field policies; preserve policies in attr hashes
  and reconciliation; later plain attrs clear corresponding canonical policies.
- [ ] Reuse validation; remove width/height shape rejection only now. Validate other pairs,
  ownership overlap, aliases, malformed policy data and timing on both sides of the boundary.
- [ ] Add one policy tag, no duplicate wire target/history or NIF endpoint; reject malformed,
  duplicate and trailing payloads. Update EMRG docs and matching macOS host compatibility.
  Do not overload `:animate` or event `:on_change` for the new policy.
- [ ] Keep BEAM work linear: maps for policy lookup, lists for ordering, fixed-width numeric
  ids and iodata construction; no repeated list indexing/dropping or append-in-reducer paths.
- [ ] Add ExUnit helper/merge/codec/reconcile/NIF tests and public examples of grouped holds,
  nested retargeting, schedules, repeats/loops, cancellation and the approved weight behavior.
- [ ] Remove the production early return only after native gates and protocol/tests are ready
  together, so production-build integration can run on this branch. Publishing/release still
  waits for P8. Do not leave an enabled client talking to an unsupported native/host format.

**Files:** `lib/emerge/ui/animation.ex`, `ui/internal/{validation,builder}.ex`,
`engine/{attr_validation,attr_schema,attr_codec,serialization,reconcile}.ex`,
`engine/tree/attrs.ex`, native `tree/attrs.rs`, both macOS protocol implementations,
`CHANGELOG.md`, animation/EMRG guides and tests. Audit the exact compatibility version at
implementation time, rather than assuming the draft's proposed tag/version is still free.
**Exit:** public explicit/change equivalence is covered, old hosts reject incompatibility
cleanly, docs describe actual semantics, and production-build integration tests exercise
mixed groups rather than succeeding only under `cfg(test)`.

### P8 — work/memory/device qualification and cleanup

- [ ] Benchmark cold, warm, held, release, model-edit and context-retarget phases separately.
- [ ] Add many-sibling, deep-nested, many-independent-parent, loop/context and continuous-arrival
  workloads; report projection assembly as well as layout cost.
- [ ] Measure moving/held memory, source retention, workspace/cache/metadata bytes and peak/RSS.
- [ ] Re-run transform-only Nearby and patch workloads under the existing performance lock.
- [ ] Execute constrained-device and macOS validation in their environments; record unavailable
  gates honestly rather than treating desktop CI as equivalent.
- [ ] Delete the superseded per-track release code, obsolete draft and temporary diagnostics
  once replacements are proven. Retain native regression controls and useful counters.
- [ ] Update final docs/changelog/plan status only after release acceptance, not after a subset.

**Exit:** full correctness/CI and measured performance/memory gates pass. If a budget fails,
use measured call/visit/copy costs to select a focused optimization; do not introduce a new
layout engine, dependency graph or scheduler based on assumptions.

### Phase dependencies

P0–P8 remain acceptance phases, not a strict coding order. The remaining implementation
crosses P5 ownership and P6 transaction prerequisites before attempting P3 continuation:

```text
R0 driver/audit → R1 admission/masks → R2 frame transaction → R3 context/provenance
  → R4 continuous-context proof → R5 late-event witnesses → R6 handoffs/boundaries
  → R7 ghosts/scope conversion → R8 all callers/recovery pacing → R9 combined acceptance
R9 closes the corresponding P3–P6 gates → P7 production/API/protocol integration → P8 release
```

R0's tests/counters begin immediately and continue in every package. R2 introduces the
coordinator in actor/direct paths; R8 completes caller migration rather than inventing that
contract again. R5 topology witnesses are accepted only with R7 conversion tests. Do not
parallelize changes which assume ownership/context semantics that an earlier package has not
established. Continuity proof failure keeps R4 open; it is not a new user-approval blocker.

**Next implementation step:** complete the remaining selected-owner mask lifecycle, then
R2's remaining lifecycle/exclusivity/input-stamp seams. Enter/change field handoff and sparse
parent membership edits now have regressions; neither closes all R1/R6 lifecycle cases. Staged geometry/effective attrs and
commit-only source acknowledgement are now present; the original pre-layout preservation
regression passes. R3 must bind actual metric/runtime producers and supply the same frozen
context to live layout before continuity work. Do not weaken the mismatch guard.

**Implementation scope:** R0/R1/R2 are partial; caller migration has begun on their boundary,
not closed R8. R3–R9 remain open. D1, the motion contract and public gate are unchanged.

Internal phases may be committed separately, but are not reduced-matrix public releases.
Do not overwrite unrelated worktrees or discard the existing uncommitted foundations.

## 8. Acceptance tests and oracles

### Test matrix (factor coverage; avoid an unbounded Cartesian explosion)

| Dimension | Required cases |
| --- | --- |
| Length pairs | Representative full ordered Cartesian set of pixels, content, fill, fractional/large weights, min/max and recursive differing shapes |
| Axes/owners | Both axes; regular, enter, exit and change; explicit/change equivalence in matching group/context |
| Time | Start, .25/.5/.75, held interval, near joins/release, terminal; linear/ease-in/ease-out/ease-in-out |
| Schedules | 30/60/120Hz, irregular/dropped pulses, repeated timestamps and long gaps |
| Groups | Single/unequal-duration siblings, static fills, independent parents, nested groups, empty groups and arrivals while held |
| Layout | Rows/columns, el, wrapped rows, paragraphs/floats, content ancestors, images/video fallback, bounds/overflow, padding/borders, scale/rotation and parent-imposed widths |
| Lifecycle | Coalescing, policy-only/no-op updates, interruption/reversal, removal, reparent, Nearby, ghosts, remount/upload and prefix errors |
| Context | Resize, text changes, fonts/images/pending assets, hover/focus/press, scrolling/end-follow and unrelated paint edits |

Use full pair coverage in a small base fixture; use targeted adversarial combinations
for larger contexts. Name expected semantic differences rather than increasing tolerances
to make old/new paths agree. Do not assert that all intermediate sibling widths sum to the
container: independently timed growth/shrinkage can legitimately leave gaps or overflow.

### Required oracles

1. **Joint hold/release:** same-context native terminal layout versus full samples still
   installed, then samples removed together. Compare all affected frames, intrinsic/basis/
   charge/placement facts, descendants, Nearby, pixels, clips, hits and scroll extents.
2. **Limits:** near-release samples approach the accepted held endpoint; a terminal symbol
   assignment must not hide stale geometry. Separate actual wrap/reset/topology discontinuities.
3. **Query isolation:** A→B→A, fresh evaluator, permuted groups/requests, query failure and
   immutable image/scroll seeds. No live scene/registry/asset-side effects.
4. **Same schedule:** deterministic replay for identical inputs/pulses, including cache-disabled
   endpoint evaluation with the same logical run/source state. No general cross-rate equality.
5. **Static-context arithmetic:** 40→300 over 1s and 2s yields 300/170 at 1s, 300/235 at 1.5s,
   300/300 at 2s. These fixed targets happen to be schedule independent.
6. **API/path equivalence:** explicit `[A,B]` and retained change A→B share sampling/ownership
   for the same admitted source/group; actor/default/custom-measurer/headless agree.
7. **Retention/liveness:** no idle owner/group/history after release, no pending final frame,
   no global loop wait, no unbounded work proportional to missed frames/cycles.

Keep the six original pixel-only controls. D1-approved new grouped weight expectations
must be separate from the existing symbolic baseline tests; do not rewrite history as though
375px had always been the existing behavior.

## 9. Work and memory budgets

Let N be model size, R moving retained fields, H held fields, G active groups, Q distinct
missing native projections and V the actual projection/context/layout entries visited.

- Retention: one O(N) workspace plus O(R + H + G) active records and existing per-node facts.
  No per-group tree, per-member copy of the foreign scene or unbounded source chains.
- Warm sampling/membership bookkeeping: O(R + H + G), not pairwise owner scans.
  Context/projection preparation and native layout cost are additionally measured as V/Q;
  do not hide O(G×N) work inside a claim of local grouping.
- Cold model copy is allowed on demand; a new owner or unchanged viewport alone must not
  copy the model. Genuine model changes may conservatively copy O(N).
- Unchanged joint targets should incur zero endpoint queries/model copies, including during
  a stable hold. Paint/compositor-only work must allocate no group geometry workspace.
- Approved direct numeric/equivalent paths retain zero-query behavior where the complete
  required state is known. Resolved weighted endpoints may require queries; do not preserve
  a zero-query weight path at the expense of the settled motion/hold semantics.

Counters to expose through existing diagnostics, not a new NIF API:

- groups created/released; moving/held members; pending handoffs and pulse requirement;
- admission/membership visits; context-reset visits and projection entries/bytes;
- source/target/rebase/release query counts, cache hits and dirty reasons;
- model copies/nodes/bytes, metric invalidations, native measure/resolve visits;
- source/spec/footprint retention, current/peak workspace bytes and error recovery counts.

Earlier structural measurements were approximately 80 added bytes/node for dimension facts
and cache keys, with 72-byte footprints and 144-byte sample pairs. Re-measure after group/
policy changes; those values are not current RSS or a memory budget qualification.

Protect existing constrained-device targets: transform-only refresh <=5ms and patch tree
work <=12ms, with the stricter ideals and evidence requirements in the linked performance
plan. Establish measured geometry-animation budgets on the target; do not claim those
transform-only numbers prove a whole-tree endpoint query is affordable.

## 10. Validation, delivery and historical references

For code phases:

```bash
cargo fmt --manifest-path native/emerge_skia/Cargo.toml -- --check
cargo test --manifest-path native/emerge_skia/Cargo.toml
cargo clippy --manifest-path native/emerge_skia/Cargo.toml --tests -- -D warnings
EMERGE_SKIA_BUILD=1 CARGO_TARGET_DIR=/workspace/emerge-animation/native/emerge_skia/target mix test
EMERGE_SKIA_BUILD=1 CARGO_TARGET_DIR=/workspace/emerge-animation/native/emerge_skia/target ./ci-tests.sh all
git diff --check
```

Run device/backend-specific tests through the existing performance/qualification plans.
Planning-only edits require link/fence/diff checks, not claims of new runtime validation.

Historical design decisions remain available in commits `e38f0b0`, `5475831`, `7c3825c`,
`5d9d64e` and `592c2fe`. The old per-track release assumption—not the native allocation
counterexamples—is superseded by this group plan. The candidate's 30Hz/120Hz discrepancy
is now documented schedule-specific behavior, not a release blocker.

Related constraints and architecture:

- [Low-resource animation smoothness](active-low-resource-animation-smoothness.md)
- [Layout/cache flow](../guides/internals/layout-refresh-render-flow.md)
- [BEAM performance constraints](../guides/internals/beam-performance-constraints.md)
- [Layout caching roadmap](layout-caching-roadmap.md)
- [Platform orchestration](platform-runtime-architecture-differences.md)

Completion means all native, public/protocol, lifecycle, scope/context and qualification
gates above are closed. Passing the current investigation tests is not feature completion.

### R1/R2 continuation — completed slice and remaining exits

- Keep the existing staged maps and immutable track versions; no rollback tree clones.
- Route native advancement through an exclusive apply/layout/refresh/commit boundary;
  expose split-phase hooks only to corruption tests, not runtime callers.
- Preflight lifecycle capacity before live writes. Retire expired ghosts only after a
  successful publication, with explicit cleanup damage for the next frame.
- Invalidate unapplied preparations on mutable model/runtime access; freeze the query's
  image facts and renderer-local fonts through native layout/refresh without asset locks.
- Mount-aware admission and selected-owner mask controls are implemented. Rust, Elixir,
  full CI and benchmark compilation pass (counts/logs above).
- Remaining: field-specific regular handoffs and bounded skipped sources; real runtime/media
  cause versions, font notifications and deferred-raster font binding; full ghost baseline/scope
  transport and actor/caller qualification. These cannot be declared complete from the narrower
  boundary tests. Continue R1/R2 before treating later native acceptance packages as closed.

### R1/R2 exit pass

- Finish regular field admission with bounded selected-field runs sharing one immutable
  specification. Enter overlap blocks only conflicting fields; newly selected fields use
  ordinary run clocks, never a group clock or rewritten specification. Retry/reversal must
  retain admitted clocks and source identities.
- Close first-publication source eligibility, preflight/commit lifecycle authority and actual
  actor rejection controls. Verify all R1/R2 exit bullets, retaining R3–R9 requirements rather
  than treating later continuity, topology transport or snapshot qualification as solved.
