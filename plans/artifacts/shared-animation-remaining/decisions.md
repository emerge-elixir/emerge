# Dependency and accounting decisions — first remaining-plan slice

## D1. An unrelated global projection input is not necessarily a dependency

Reproducer: a finite 40→Fill child under a 200→Fill mixed parent, plus a second
mixed loop in an independent panel. At .5s, the child was 70 instead of 95 because
the foreign panel made the ancestor-only continuation check reject the entire
projection. The subsequent finite release was also obstructed.

Implementation:

1. Keep ordinary numeric/mixed causal witnesses and unchanged model/context guards.
2. Classify changed inputs relative to each consumer using retained ancestry.
3. Potentially disjoint inputs can be replaced with current projected inputs in a
   temporary **hybrid projection**, never in the live model or published source.
4. Native layout of the **unmodified prior projection** must match the recorded
   prior consumer target. This prevents a corrupted old target that happens to
   equal the new target from masquerading as independence.
5. Native layout of the hybrid projection must also match that prior full footprint.
   A mismatch here alone means dependency: decline continuation and use ordinary
   retarget/release validation. It is not permission to ignore corrupted causal
   pixel/foreign witnesses or injected query failures.
6. Current destinations still come from ordinary frozen endpoint projections;
   combined release remains one joint native contract.

Same-pool controls demonstrate that sibling topology alone is insufficient: the
hybrid target changes from 400 to 310 while moving. Such a candidate is rejected.
The source, intrinsic/initial/visible geometry, charges, parent extent, scope,
units and policy channels are not reduced to pixels or visible-size equality.

## D2. Share native proof work only for matching input classifications

The frame-local cache key contains original/target projection identities, previous
evaluation time, release scope and a sorted replaced-node set. Causal ancestry and
replaced-node directions are checked again per consumer. Proofs with opposite
consumer relationships cannot inherit the first caller's permission.

Only the individual mismatching consumer declines continuation. A shared batch
must not restart a second consumer whose own complete footprint is independent.
The 64-owner fixture uses at most eight native queries per attempt and one private
model copy. This is evidence for batching, not a claim that native layout touches
only those owners or that classification is globally linear.

Co-releasing owners are not replaceable external inputs for a releasing consumer.
This preserves joint pool-allocation checks, including the existing one-query bad
charge regression. A still-running unrelated owner may test independence from a
finishing owner; it is not itself part of that owner's release barrier.

## D3. Check release against actual installed peer samples

An ordinary frozen release projection can precede updates to another owner's
continued target/anchor. If that owner will apply a different sample, using only
the earlier projection can certify the wrong output.

After candidate tracks are computed, rebuild the release projection with actual
nonreleasing samples. If it differs, perform native layout of the complete release
again and compare every released footprint. This happens before live application
or release-receipt installation. The injected `CurrentRelease` failure regression
preserves the old scene/registry and clocks through a later retry.

This is a safety check, **not a completed cyclic evaluation algorithm**. It may
reject a valid but still-unsupported coupled schedule. It does not silently mutate
foreign clocks or choose a new source/duration to make the query pass.

## D4. Coupled mixed clocks remain an explicit design blocker

Minimal native traces now reproduce both axes:

- Fixed 600 pool, finite 40→Fill/1s owner, 200→Fill/2s looping peer:
  the 1s release rejects node 2.
- Finite 200→Content/1s parent with 40→Fill/2s looping child:
  the 1s release rejects node 1.

See `validation/coupled-negative-baseline.log`. The passing negative-proof test
only prevents misclassifying these as independent; it does not qualify desired
release behavior. P2.2 must replace its rejection assertions when implemented.

Remaining proof obligation: define a bounded shared snapshot/retarget schedule for
these feedback relations, including actual peer samples, previous/current native
foreign destinations, current joint release and segment resets. Ordinary endpoint
queries cannot silently become recursively consumer-derived endpoints. Loops stay
outside finite barriers; unchanged clocks and real remaining intervals remain
intact. Permanent retry, extra duration, cancellation and weakened mismatch checks
are not solutions. This slice does not select an unproved feedback rule.

## D5. Disposal is synchronous, counted and still expensive

Normal dimension-runtime retirement now copies numeric last/cumulative query
statistics, then measures actual synchronous disposal. No private tree, projection,
font/image snapshot or deferred-retirement queue is retained for diagnostics.

The receipt lives on the tree incarnation, so totals survive ordinary settle and
later admission but not replacement/destruction of that entire tree. Internal
resolver model replacement and full renderer shutdown still need separate phase
accounting. Diagnostic instrumentation is compiled for tests/`bench-diagnostics`;
publication behavior is the same native path without the diagnostics.

The locked baseline confirms that disposal is a large part of 20k-node release,
but cancellation settle contains substantial other work too. Mixed loops naturally
retain the workspace after a finite child releases; their cheap finite release is
**not** a memory/release optimization. The probe measures their later cancellation
and disposal separately, including the cancellation publication itself.

## Rustler/state review scope

No NIF signature, encoding, atom, scheduler, borrowed BEAM term, resource callback,
thread or lock was added. Queries remain in the existing private native workspace
with frozen inputs and no hydration or live publication. Recoverable failures
precede writes. The new diagnostics retain fixed-size numeric state only. The
broader final NIF/artifact/entry-point audit is still P9 work.

## D6. Next experiment: bounded coupled release

For unchanged-model/context finite release, try the retained mixed clock witness
for nongroup looping descendants/siblings only after independence has failed.
Validate the original consumer and foreign native destinations. Finite release
consumes the once-frozen loop presentation; feedback drivers use ordinary current-
presentation retargeting rather than continuing an anchor against a destination
that release itself changes. Defer their continuation checks until the consumer
has been classified so truly independent loops retain their existing behavior.
Finally certify the combined release against the samples actually installed.
No iterative solver, clock restart, extra duration, tree clone or new group.
This experiment initially targets deadlines, not general continuous feedback or
composed/topology inputs. Record counterexamples rather than accepting mismatches.

### D6 outcome: covered coupled deadlines now pass

The experiment is implemented for unchanged-model/context finite release with
mixed looping descendants and same-pool peers. The earlier D4 rejection guard has
been replaced by positive native-release tests, not disabled. This is deadline
support, not the still-open general continuous-feedback/combined-input contract.

A further counterexample required explicit query provenance: at .5s the Content
parent's target was measured against a **55px loop forecast**, but the loop's own
continued native goal caused it to publish **37.5px**. Reconstructing the parent's
old target from the latest loop track was therefore wrong even with unchanged
model/context. Finite tracks now retain an immutable shared map of the loop clock
inputs used by their endpoint query. `ClockInput` is a leaf type with no forecast
field: it cannot retain a recursive history. A loop track does not retain that map.

When needed, the mixed witness checks the forecast clock, last retained driver
clock and new boundary clock. Every distinct native destination in that bridge is
validated, not only the forecast sample. A regression keeps the forecast sample
unchanged while corrupting its destination; another corrupts the retained driver's
policy at a reset. Both reject publication and recover on the original clock.

Only genuinely coupled consumers use this fallback, after native independence
classification. Potential driver continuation checks are deferred; independent
loops keep their original path. Actual feedback drivers must apply exactly the
once-frozen/boundary sample. Ordinary retargeting keeps their start/generation,
segment clock and real remaining interval. The final combined release uses the
actual installed samples; no live rollback, new duration or iterative solve.

Coverage: 720 native axis/mode/scale/curve/deadline cases including resets and large
skips, multiple weighted loop drivers, both axes together, enter/change Content
parents, 72 sibling regular/enter/change/exit raster-and-hit cases, query/output
failures and corrupted original/forecast/current targets. Single-driver fixtures
stay within 12 queries and one unchanged-model copy; the two-driver fixture stays
within 20 queries. These bounds do not establish group-local evaluation.

New source/build evidence is in `performance/baseline-02`; full test/CI logs are in
`validation/coupled/`. **Performance is not qualified:** a 20k tree with 64 coupled
looping children measures ~107ms warm median and ~86ms finite release. That workload
retains one 64-input forecast set and 130 projections/8,450 projection slots during
motion, and performs 257 native queries per warm attempt. It drops the forecast
set at finite release, but that does not make evaluation or memory cheap. P7 must
address this measured cost; broad P2/P3/P4/P5/P6/P8/P9 gates remain open.

## D7. Ongoing finite motion with mixed-loop feedback (covered subset implemented)

Extend the already certified frozen-loop temporal rule to active/held finite
consumers, not only their release. Preserve a consumer's admitted curve anchor only
after original/native foreign evidence passes and native independence has been
considered. Actual feedback loops retarget from the once-frozen presentation using
their existing clock/remaining interval; independent loops stay on their prior
path. Owner endpoint queries remain ordinary immutable frozen-input queries.
No fixed-point iteration, caller-visible groups or fabricated duration. Initially
retain the same model/context and interval guards; test resets, holds, mixed lanes
and native/rendered outputs rather than assuming they follow automatically.


### Result and stronger source checking

`finite_feedback` admits eligible active/held finite consumers to the existing
native proof and actual frozen-driver verification, not just releasing consumers.
The shared-pool .5s counterexample is now 175px rather than 220px: a native 310px
goal is sampled halfway from the original 40px source, instead of reanchoring at
halfway to the previous 400px goal. The upward .5s example is 127.5px (200→55),
not 110px; under the same discrete schedule its 1s loop/parent size is 140/3px,
not the earlier 35px. Different schedules legitimately have different targets.
The fixed-pool frame-rate test now uses a scalar predict-once/retarget recurrence,
not regenerated hardcoded trajectory values.

An equal current-driver presentation must not bypass an available exact forecast.
Mixed witnesses now prefer the recorded query input, then certify each distinct
forecast/current/boundary destination. The balanced corrupt-forecast regression
still fails even when it leaves the old sampled presentation unchanged. This adds
one native query to some ancestor/cohort fixtures (4→5, 8→9), explicitly counted.

Evidence: 432 axis/upward-or-pool/mode/scale/curve/30–60–120Hz traces; full original
source/anchor/run and applied frozen-driver footprints checked on every active
frame. Native failures, balanced corruption and delayed retries preserve output
and clocks. Exact/late nonrelease loop resets reanchor current presentation but
not the admitted start/generation. Cancellation without a layout-model edit keeps
the finite hold's admitted end. The 72-case regular/enter/change/exit raster test
now also checks .5→.75s curve/source continuation before release and failed output.

### Newly isolated P3 gaps — not waived or hidden by retries

1. In `coupled_hold_keeps_its_admitted_interval_when_the_other_finite_owner_is_cancelled`,
   after `attrs.animate = None` also set the cancelled peer's animated-axis
   declaration to `Some(Length::Fill)`. This changes the layout model at 1.75s.
   The 2s release rejects `ReleaseMismatch(NodeId(2), axis)`. The owner query uses
   the new model/context but its frozen loop forecast originated in the previous
   model; the same-context witness correctly cannot certify that clock bridge.
   `validation/continuous/model-cancellation-gap.log` retains the failing query
   trace. The passing test covers cancellation **without** that declaration edit;
   it does not establish combined-model cancellation support.
2. `coupled_peer_release_matches_pixels_and_hits_for_every_owner` changes the
   declaration or creates a ghost at t=0 for change/exit admissions. Their first
   .5s motion still reanchors at 220px rather than the unchanged-model 175px oracle:
   the exact forecast carries the preceding model. Once coherent inputs have been
   published, .5→.75s source/curve continuation passes for all four owners. The
   175px raster assertion is therefore restricted to unchanged-model regular/enter
   admissions. This is a remaining semantics gap, not an intended owner difference.

These cases require joint historical model/context/clock input provenance, not
ignoring the forecast, relabeling its model, trusting equal visible pixels or
restarting clocks. Self-axis, combined numeric/resolved causes and more general
footprints also remain open. No P1–P9 package is marked complete.

Validation: `validation/continuous/{cargo,mix,ci,clippy}.log`: 1,314 Rust units +
14 integration, standalone Mix 494 and full CI 499 Elixir tests/doctests, Dialyzer 0.
[Locked baseline 03](performance/README-03.md) counts fewer upward warm queries
(194 versus 257 steady), but ~76ms warm / ~87ms release / ~377MiB RSS at 20k/64
still fails performance. Source identity and raw evidence are retained there.

## D8. Native endpoint receipts and field-composed witnesses (implemented subset)

A clock's frozen input can precede the model/context of its consumer query. Keep
an opaque native endpoint receipt at the original resolver evaluation, binding the
actual native result to exact immutable projection/context, model, mount and root
facts. Historical validation uses that receipt, not `Track.to` copied at replay,
not a relabeled model, and not a new per-owner tree. Current-model witnesses still
run native layout. Receipt results cannot contain clock/receipt histories. Count
receipt lookup observations separately from native queries. Reproduce model plus
cancellation and first change/ghost admission before broadening other inputs.


### Implemented and validated

- `EndpointResolver::resolve_certified` constructs opaque `NativeGoalReceipt`
  records from successful native results. They retain exact projection/context,
  original model/root and endpoint mount facts, and the native result map. Only
  the resolver can construct them. Animation code cannot mutate their values.
- `ClockInput` retains its query receipt; forecasts remain leaf-only. Every foreign
  witness validates receipt identity. Current-model/context goals still execute
  native queries; historical foreign goals validate the originally evaluated
  receipt. Copying a projection/context into a new Arc, changing model identity,
  corrupting the goal or injecting historical-evidence failure prevents publication.
- This fixes both D7 counterexamples: cancellation **with** the static Fill model
  edit now releases, and all 72 regular/enter/change/exit raster traces assert the
  175px first midpoint (not the old 220px reanchor). Existing corruption guards
  still pass. Historical failed logs remain evidence, not the current contract.
- Context release now replays each original goal projection with its original
  context and declaration preimages, not current loop phase plus old environment.
  A mixed loop reset with simultaneous model/padding/viewport/scale changes passes
  both axes/modes/scales. Seed membership additions/removals use the opaque native
  receipt, never new workspace defaults for unknown old seeds. Opaque metric-epoch
  changes still require real font snapshots. Native declaration replay retains
  the published-model/mount/unchanged-topology guards.
- Mixed continuation composes per-axis witnesses, including numeric and resolved
  fields on the same node. Numeric substitutions retain their native before/after
  equivalence checks. Same-node foreign axes can be feedback drivers; native
  independence is axis-qualified and keyed per consumer. Wrapping height feedback,
  independent self axes, numeric ancestors plus mixed peers, rotated same-node
  numeric/mixed drivers and native parent-imposed dimensions have positive tests.
- Real actor/channel output and direct `TreeUpdateEngine` now run matched coupled
  regular/ghost traces, comparing registry payloads, retained raster pixels,
  terminal/cleanup output, loop cancellation, old-scene replay and eventual idle.
  Transient admission is acknowledged at the first presentation before advancing
  the test clock; no production timing change was made. A capacity-one event/render
  publication test uses a channel rendezvous and draining barrier to verify final
  inactive scene publication despite registry backpressure and latest-scene overwrite.

`HistoricalTarget` is a receipt lookup observation, **not a new native layout
query**. `ProjectionStats.layout_queries` still counts actual resolver work.
Diagnostics add unique `native_receipts` and `receipt_goals`; records retain result
maps and existing projection/context Arcs, not trees, clocks or receipt histories.
There is no claim that this extra storage or lookup work is cheap. Baseline 03
predates this code; no new performance measurement or backend qualification ran.
No new NIF signature, atom, production thread, lock or resource callback was added.

### Still open in the user's four requested areas

The request remains in progress, not a completed four-package delivery:

1. Broader self-axis/coupling schedules, cross-axis alternatives, all phase/failure
   combinations and noncanonical topology still need qualification beyond the
   directed positive cases. Existing large matrices remain green.
2. The old D7 failures are fixed, but the complete joint font/image/scroll/runtime
   and stale-completion matrix remains open. Native receipts establish historical
   goals; they do not reconstruct arbitrary old topology or authorize opaque metrics.
3. General same-mount reparenting, allocation-unit/role conversion, Row↔Column,
   flow/wrap/float/Nearby transport and the complete interruption/lifecycle matrix
   remain unimplemented/unqualified. Do not copy scope ids or lower complete
   footprints to visible pixels to make these cases pass. Removal/ghost admission
   and coupled cancellation now have additional positive coverage only.
4. The matched real actor/direct and queue-publication cases pass, but complete
   headless parity, stop while permanently blocked, focus/IME/wheel/drag/Nearby,
   delayed registry installation and backend/device damage/presentation remain open.

Validation logs and exact source manifest:
`validation/provenance-fields/{cargo,mix,ci,clippy}.log`, `source.sha256`, `identity.json`.
**1,324 Rust units +14 integration, standalone Mix 494, full CI 499 Elixir tests/doctests,
Dialyzer 0**, Clippy benches/tests/bench-diagnostics with denied warnings, formatting
and diff checks pass. No P1–P9 package is closed by this slice.

## D9. Native structural source transport (directed implementation)

Structural edits capture first-published full footprint and attachment lineage for
the affected closure before mutation. Same-mount moves preserve physical own-space
intrinsic/initial/visible channels and policy, not an old parent's reservation.
An explicit private-query transport input bypasses old scoped debit/placement;
the existing destination planner derives the new role's charge/extent. The query
returns an ordinary full footprint in destination units/scope for transactional
installation. This is not pixel lowering or scope relabeling: all own channels and
policy are retained, descendants use current layout, and native role planning owns
new reservations. Old pool obligations disappear with detachment. At a due release,
old completed native goal evidence is required before installing the current native
destination. Clocks, mount identities and admitted hold ends remain authoritative.
Reversals to the original attachment before publication must not restart a run.

D9 captures affected descendants before attachment/root changes and before an
existing parent is overwritten. The publication records its actual root/mount;
unreachable stale frames cannot become published sources. Sealed origin records
bind source dimensions, scale, model, mount and attachment path. Failed input edits
keep the first source; returning to the committed attachment restores the original
group member rather than retaining an unpublished migration latch.

Transport queries suppress both pool debits and wrapped/float placement reuse.
The normal native planner derives replacement obligations. Pending declared float
roles are accepted only for successfully transported endpoints under exclusive
frame authority. Transported looping forecasts use the new native clock input that
actually reproduces the frozen new-role presentation, not an old-scope clock.
Existing ordinary fixed-hold cancellation/arrival behavior is unchanged: a migrated
held member carries its prior barrier; an unchanged member does not acquire an
invented remaining interval. Numeric-only motion enters this path only on a real
structural source change and otherwise retains the existing fast path.

Evidence: 18 tests in `lengths/tests/transport.rs`, including 270 role/unit/curve
cases and 24 combined-input traces. A former source-rejection test now corrupts the
sealed source explicitly: legal reparenting is no longer its rejection condition.
The test oracle prepares native roles/scales before applying scoped samples, so a
float is not validated against freshly reset default effective attributes.

**Unfinished:** broad P2–P5 matrices, stale asset completion combined with animation,
complex ghost/terminal handoffs and general structural/order/root closures remain.
A new same-ID SVG replacement probe exposed mutable render-time image bindings:
old retained scenes can paint the replacement image. Its bounded failure is kept
in `validation/transport/same-id-retained-raster-gap.log`. The combined-input test
checks historical native geometry for images and retained raster invariance only
for fonts; it does not misreport immutable image scenes.

**Cost:** attachment paths are retained per captured source and traverse ancestry;
worst-case capture storage/work can be O(affected nodes × depth). A sealed origin
adds fixed footprint metadata and shares its path Arc. Transport uses one batched
native source query in the both-axis fixture; ordinary native goal/release work
still occurs. Optional transport inputs are Arc-backed, not two large inline
footprints in every projection slot. No exact live/peak accounting or performance
qualification is claimed, and no D9 benchmark was run. No NIF/atom/protocol/thread
or production lock was added.

D9 validation: 1342 Rust units + 14 integration; standalone Mix 494; full CI 499
Elixir tests/doctests and Dialyzer 0. Functional source `source-sha256:aab1a31e9e1e611b98c2ab057b4dcfc089de2db7c56c428d4edd2ff6b57edf3f`; logs and
manifest: `validation/transport/`. No D9 benchmark or package closure.

## D10. Retained image bindings (directed implementation)

Freeze only image IDs referenced by a published native scene, retaining immutable
source records or already-available cached pixels when the source is evicted.
Render-time lookup and cache identity must use those bindings, not the latest
registration for the same ID. Missing bindings cannot fall through to another
asset generation/renderer. Manual low-level scenes retain their existing live-ID
behavior unless explicitly captured. Native tree publication captures by default.
No asset hydration, decoding or rasterization during capture, no asset lock held
while painting, no global history/cache and no new NIF/protocol are intended.
Record and cached-pixel retention must be exposed and tested; this is not a memory
budget or performance qualification. First reproduce the D9 same-ID raster gap,
then test replacement, eviction/reset, cross-renderer cache identity and disposal.

D10 resolves the D9 retained-raster failure. Native scene assembly captures image
bindings through all render scopes and cached paint-layer content. The captured
records and cached-only variants survive source replacement/cache reset; render,
profile, grayscale-policy and resource-fingerprint paths enter the same scoped
binding. Explicit absence never falls through to a later registration. Nested
manual scenes clear an outer binding and restore it on scope exit.

Raster/vector cache keys verify immutable source identity in addition to the
renderer-local numeric generation. Cache entries retain that identity token;
foreign/old records cannot republish source metadata into the live cache. Source
records hold a weak owner reference, not a strong renderer-state cycle. Where old
variants coexist, live cache metadata and cached-only snapshots select the newest
local generation rather than arbitrary HashMap iteration order.

Validation includes eight new tests: seven in `renderer/scene_images/tests.rs`,
plus a real loader/tree-update integration trace in `assets/tests.rs` under both
decode policies. The D9 24-case combined input test now asserts retained image
raster invariance too, without skipping the image arm. Capture performs no source
I/O or rasterization and holds no asset lock during painting; last-scene disposal
releases pinned records. Historical D9 failure evidence remains unchanged.

Costs are explicit: scenes can retain encoded/parsed records or cached pixels
beyond cache eviction. Per-snapshot reference charges do not measure globally
unique live heap, parsed SVG payloads or GPU memory. Render-graph capture visits
are counted; an empty asset universe skips the walk. Nonempty asset universes
currently incur a render-graph traversal. No performance qualification is claimed.
Binding occurs at scene assembly, not a new atomic transaction spanning layout and
concurrent asset changes; that provenance combination and broader P3/P5 matrices
remain open. No package is closed and no NIF/atom/protocol/thread/new lock was added.

D10 validation: 1350 Rust units + 14 integration; standalone Mix 494; full CI
499 Elixir tests/doctests and Dialyzer 0. Source `source-sha256:6989691401905f206dc4891207aaccaec917605207e23e19faf5fef09587a6d5`; logs and manifest
in `validation/image-bindings/`. No benchmark or package closure.

## D11. Atomic frame assets, bounded actor publication and directed qualification

Execution order: freeze source status, dimensions and render bindings together at
frame preparation; exercise replacement/epoch changes between prepare and publish;
then expand multi-edit/root/boundary, noncanonical coupling and lifecycle traces,
and remove the permanently blocked registry-send stop hazard. Preserve existing
publication authority, independent clocks and failed-attempt sources. No blanket
qualification claim: track each requested matrix and integration boundary against
actual tests. Run Rust/Mix/full CI and retain results.

Asset capture will reuse existing source-state -> pixel-cache lock order, copying
only referenced immutable records/available pixels while locked, then release both
locks before query/layout/painting. Cache the tree's referenced-source list by model
identity. Store one captured frame input on the tree so split layout/refresh calls
cannot silently bind newer media. Historical queries still retain dimensions only,
not asset runtimes or whole frame snapshots.


Implemented this slice:

- `FrameAssets` binds source statuses, dimensions, generation and immutable records/
  pixels under existing source-state → pixel-cache locks with an epoch check. No
  layout, query, hydration, decoding or painting runs under those locks. Weak owner
  checks and thread-affine nesting guards preserve renderer isolation. The tree
  retains the input through split layout/refresh; historical queries do not retain
  these snapshots. Unknown mutable access and model/revision changes invalidate
  the declared-source index; native overlay writes preserve it. Image-node dimension
  changes invalidate measurement locally; decorative bindings do not force layout.
  Empty-reference frames skip asset locks. Frozen lookup/paint does not repopulate
  new-epoch source state or enqueue work; genuinely new (including nested)
  preparations explicitly register live references before capture. Capture/index/
  retention costs remain unmeasured, including inactive styles and orphan declarations retained on the tree.
- 24 prepare→writer→publish schedules cover ID/logical source, epoch reset, stale
  preparation and three finite-clock times; stale loader completions cannot replace
  accepted data. Separate pending/readiness and real paint-only attribute patches
  cover frozen absence/generation and interaction source-list invalidation.
- 144 multi-edit orphan/wrapper/root schedules cover both axes, Full/Active/Dirty,
  attach/remove order, root-edit order, role orientation and repeat boundaries, with
  first-source retention through pre-layout failure/retry. Additional actual-root
  stale Nearby and clear-root/reattach tests exposed two source-loss cases: the real
  root must ignore a stale Nearby slot, and `clear_root` must capture the published
  subtree before changing model identity.
- 1,536 noncanonical self-/cross-axis schedules: both driver axes × same-node/
  descendant coupling × Times(3)/Loop × WrappedRow/Paragraph/TextColumn/Slider ×
  Full/Active/Dirty × four curves × .5/2 scale × target-query/pre-layout failure.
  Multi-segment/reset, incoming interruption, retry, cancellation and original run
  clocks are checked. Slider exposed order-dependent intrinsic width leakage:
  parent-imposed width now lives in resolve facts, not mutable effective attrs.
  A reused-versus-fresh resolver regression independently checks the native result.
- 24 complex ghost schedules combine WrappedRow/TextColumn, text/image/Nearby,
  transforms, transport, terminal image replacement, same-ID remount and failed/
  interrupted cleanup. Another 24 hold schedules combine migrated holds, arrivals,
  cancellation and failure/retry without inventing or extending finite deadlines.
- Production registry publication is nonblocking: at most one unsent full rebuild
  and latest scene, select send/input, batches capped at 64. Stop remains readable
  with a permanently full channel. Pending mount focus is revalidated by mount and
  focusability, with current native reveal geometry; a newer explicit focus target wins.
  Obsolete blocking-only helpers/tests were removed, not retained as a test-only
  publication implementation. Real actor tests cover blocked Stop and final registry/
  ghost cleanup plus consumer disconnection; focus coalescing has directed carry/removal/remount checks.
- Direct engine/real actor/host-event-runtime raster parity compares every input
  registry, emitted messages, IME state, text selection drag, keyboard, wheel and
  Nearby hover. Fresh versus retained-damage rasters and old-scene replay include
  the final inactive scene. This is CPU/native headless evidence, not GPU/device
  presentation or compositor acknowledgment. Publication orders enqueue-before-scene,
  not a cross-thread registry-install acknowledgment; adversarial delayed-install
  races with concurrent input remain part of broader P6 qualification.

No new NIF, atom, wire version, production thread or lock. Fifteen new unit tests
replace three obsolete blocking-publication tests (net +12). Full validation and
source identity are recorded under `validation/atomic-publication/`.
No P1–P9 package is closed: exhaustive owner/layout/Hz/context cross-products,
long-running randomized topology/lifecycle schedules, exact live/peak/disposal
accounting, current locked performance, constrained/macOS and device presentation
remain open. Baseline 03 still predates D8–D11; do not infer speed from bounded queues
or sparse source indices. No deferred disposal or timing exclusions were introduced.


D11 validation: **1362 Rust units + 14 integration**, standalone Mix **494** (8
excluded), full CI **499** (3 excluded), Dialyzer **0**, denied-warning Clippy,
formatting and source-manifest/diff checks pass. Functional source identity:
`source-sha256:f60614ecc898495d7e36241764ebeccd3689ea1d49952e02348d1d419106b96a`.
