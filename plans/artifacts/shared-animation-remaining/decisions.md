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

## D12. Causal registry installation — directed slice complete

A frame queued before an edit previously marked the listener lane fresh and
replayed its buffered successor. Reproduced with genuine native text-input registries.
The shared actor/host driver now sends one opaque native `ListenerBarrier` after a
dispatch that marks the lane stale. The tree engine stamps successful outputs with
the latest processed receipt; cached no-op responses need no scene. Query failure
retains the prior published registry and does not acknowledge the request. A replayed
edit obtains a new identity, so an earlier response cannot acknowledge it. Receipts
use owned Arc identity rather than reusable counters and do not enter query models,
NIF terms, the wire protocol, animation clocks or historical goal evidence.

Older/unrelated rebuilds are skipped during the wait, except for retaining at most
one eligible pending mount-focus request. Both actor coalescers and delayed installs
use shared native eligible-mount/reveal metadata. Removal, same-ID remount,
nonfocusability and a later explicit focus choice cancel only that pending request.
Metadata comes from existing native focus collection, is shared on clone, and is
absent for empty target sets; cache identity includes mount revision. This replaces
the earlier tree-only scan for focus actions rather than retaining two eligibility
algorithms. Event draining is capped at 64 rebuilds and preserves input/control FIFO.
Stop does not wait for an acknowledgment.

Eight new units cover six keyboard backlog/policy schedules; foreign and prior
request receipts; cached responses; native focus carry/removal/remount/eligibility;
metadata sharing/drop; bounded draining; real event-actor recovery and Stop. Three
real tree-actor/direct/headless schedules delay 1/8/32 animation registries around
keyboard, IME and wheel input, then drain FIFO with independent runtime receipts.
They compare messages, input state, registries, fresh/retained pixels, old scenes and
final sample removal. Existing failure/ghost/retry matrices now assert the receipt
appears on successful recovery. The previous headless harness incorrectly reused
one runtime's messages for both engines; each now processes its own opaque receipts.

Validation: **1400 Rust units + 14 integration**, standalone Mix **520** (9 excluded),
full CI **526** (3 excluded), Dialyzer **0**, strict Clippy and formatting pass. Logs,
source identity and evidence limits: `validation/causal-registry/`. No new NIF,
atom, wire version, production thread, global cache or resource lock; no commit/push.

This closes the reproduced causal-acknowledgment and lost-focus defects, not P6.
It does not synchronize registry installation with renderer/compositor presentation.
Remaining: broader concurrent topology/ownership/focus/input histories, render failure
and recovery, long failure/input retention, P1 live/peak/disposal accounting (including
new metadata and receipts), current locked performance and real platform/device
qualification. Existing input buffering is not claimed bounded merely because the
new control state is bounded. No P1–P9 package closes.

D12 functional identity:
`source-sha256:50f673402e7bac56ac463470c4caf0e77095ca6c290c50221496c8bd7102db21`.
During a causal wait, obsolete intermediate registries are not installed, even when
ordinary pointer-transition preservation would prevent queue coalescing. Broader
stationary-hover/drag transition semantics across that wait remain a named P6 gap;
the directed keyboard/IME/wheel cases do not qualify every intermediate transition.

## D13. Pointer recovery across causal waits — directed slice complete

Reproduced two real native upload/rebuild defects: a buffered release clicked a new
mount reusing the pressed widget's ID, and a text-selection drag retained its old
anchor after the input was replaced. D12's causal receipt was correct, but overlay
reconciliation checked only numeric ID/listener presence.

Native `Registry` now records the existing `mounted_at_revision` identity for nodes
emitting listeners or contributing input/scroll state. The index is collected during
the existing walk, shared by clones and absent for empty/window-only registries.
Before accepted-state reconciliation/replay, the previous registry qualifies source
continuation by mount identity. Old captures, hover, pending text/slider edits and
related local state cannot transfer to a replacement. Existing listener/kind/geometry
checks still apply. The previous registry is moved, not cloned or retained as history;
normal synchronous disposal remains inside processing. No animation clock, native
query model, protocol, NIF, atom, thread, global cache or resource lock is added.

Hover contract: obsolete intermediate registries skipped during a causal wait have
no callback authority; do not synthesize historical enter/leave from them. Recover
stationary hover against the accepted native geometry when capture permits it.
Fresh crossings still emit enter/leave. Same-mount captures use current hit regions/
transforms; buffered motion stays before release. Raw observer input is not resent
by synthetic revalidation or buffered replay. This is not display-install sync.

Eleven added units cover:
- Both decode policies for same-ID replacement during a buffered click release.
- Text drag/pending edit reset on replacement; same-mount selection under a 50px
  native translation and coalesced movement followed by release.
- Stationary hover recovery with/without a final crossing; a new mount receives its
  own enter rather than inheriting the previous hover stack.
- Same-mount release inside/outside the accepted animated hit region.
- Scroll drag and scrollbar-thumb drag, retained versus remounted source, release
  and absence of inherited scroll/inertia; slider drag/value continuation versus reset.
- Real event actor: remounted releases cancel, ordinary releases still request their
  click response, and Stop remains independent of that response.
- Empty/shared/drop behavior for the new native source index.
- Real tree-actor/direct/headless traces with 1/8/32 delayed registries, text drag,
  release and Nearby hover. Compare messages, native registries, selection, raw
  observer input, fresh/retained damage pixels, old-scene replay and final samples.

Validation: **1411 Rust units + 14 integration**, standalone Mix **520** (9 excluded),
full CI **526** (3 excluded), Dialyzer **0**, strict Clippy and formatting pass.
Reproductions, logs and source identity: `validation/pointer-recovery/`.
D12 work and safety stash remain preserved; no commit/push.

No P1–P9 package closes. Broader topology/ownership/focus/hover/virtual-key/inertia
histories, render failure/recovery, long blocked-input retention and physical
presentation synchronization remain open. Source-index construction/merging,
additional state reconciliation and synchronous disposal still require P1 accounting
and current locked benchmarks; sparse/shared records do not establish low cost or
an accepted live-memory budget. No device/GPU qualification is claimed.

Next P6 ordering target: event-to-tree commands already in flight when replacement
is processed, not just capture reconciliation after its registry is installed. D13's
remount fixtures process the initial action batch before replacement; they do not
qualify the reversed cross-producer order. Pointer callbacks still use the installed
registry, not a renderer/display acknowledgment.

D13 functional identity: `source-sha256:662fb39c1eaf3d68334af5a43a1d420bc86f3924cefad1c7db29f2161f8ab64a`.

## D14. In-flight event commands across remount

Reverse D13's producer order: queued old text/selection commands reproduced focus
and mutation of a replacement after native upload. The event driver now collects
one dispatch's tree effects and fence into a packet with sorted, deduplicated native
source-mount evidence. No registry/model is retained in flight. Registry collection
also records focused listenerless nodes so a legitimate blur has source evidence.

The tree engine preserves envelopes through ordinary batch flattening and validates
all target mounts at their consumption point. Missing/stale evidence rejects the
operation's node mutations together: stale focus destinations cannot partially blur
still-valid sources. Resize/rebuild/fence/Stop controls remain deliverable. Nested or
model-mutating event payloads fail closed; raw external upload/patch messages retain
their existing semantics. Rejected operations request current registry state; only
successful output acknowledges a receipt. Unrelated revision changes do not reject
same-mount work. Deferred scroll/thumb/hover/style/focus accumulators use mount keys
and recheck them after later topology edits in the same tree batch.

Eight new in-flight unit functions plus one tree-actor/direct/headless function cover
text/selection/slider (both error policies), 32 deferred order/policy/message cases,
whole-operation rejection and controls, same-mount revisions, malformed/missing
proof, focused listenerless evidence and actual event-actor delivery. Headless parity
includes registry/IME, retained/fresh pixels and old-scene replay. Existing observation
taps inspect command leaves; actual delivery never strips the envelope.

Validation: **1420 Rust units + 14 integration**, standalone Mix **520** (9 excluded),
full CI **526** (3 excluded), Dialyzer **0**, strict Clippy/fmt. Evidence:
`validation/inflight-commands/`; functional identity
`source-sha256:c50d357e2dc201db56f860807f24882228a2f6c4edc0788e3a02dbf5372f6f9d`.

No P1–P9 package closes. Rejection does not undo callbacks already emitted to
Elixir/host. This remains installed-registry causality, not renderer/display
acknowledgment. Broader joint input/topology/virtual-key/inertia and renderer-failure
histories, prolonged buffering, dispatch collection/sorting/target checks, live/peak
retention and synchronous disposal still need qualification and current locked
benchmarks. No claim of a bounded input queue or exact memory budget. No NIF/atom,
wire version, production thread, global cache, animation clock or resource lock is
added. The existing dirty-I/O hover-observation NIF only gains envelope-aware reading;
its return encoding and scheduling do not change. D12/D13 work, safety stash and
D14-start supplementary backup are preserved. No commit or push.

## D15. Headless binary render failure/recovery

The production binary headless branch logged draw/readback errors, discarded the
scene and armed no recovery. Extraction into `headless/binary.rs` preserved that
behavior; a controlled terminal-frame failure then reproduced the stall without
another tree message. The extraction is the test seam, not a production fault switch
or a second renderer. Rustler delivery remains on the existing native render thread.

Retain one newest failed `RenderState` and retry it independently of tree input at
16/32/64/128/250 ms (then capped). New scene input attempts immediately and
synchronously disposes the older pending state; accumulated backoff resets only on
success. Stop/disconnection remain selectable between attempts. Failure suspends
further output-driven animation ticks; successful drawing resumes native wall-clock
sampling, preserving original admission time. Static/terminal retry emits no tree
pulse. Failure advances neither output sequence nor successful-frame publication.
The binary conversion/latest-frame/Rustler-delivery path otherwise retains its
existing semantics; its conversion/delivery errors are not covered by this fix.
PRIME remains on its existing terminal synchronization path.

Six added unit functions cover standalone terminal retry, virtual backoff/cap/reset,
64 weak-payload supersession checks, pending Stop/disconnection, animated recovery
with original pipeline timing, and 12 actor/direct/headless schedules. The latter
combine pre/post CPU-raster draw faults with 1/8/32 native updates, text/drag/release/
Nearby interactions, same-mount continuation versus remount, stale command rejection,
registry/IME parity, final sample removal, newest recovered/fresh/retained pixels and
old-scene replay. Controlled channels/latches qualify ordering; timeouts bound failures.

Validation: **1426 Rust units + 14 integration**, standalone Mix **520** (9 excluded),
full CI **526** (3 excluded), Dialyzer **0**, strict default-feature Clippy/fmt.
`headless-all` compile check passes with an existing Vulkan unused-method warning;
not all-feature lint or GPU execution qualification. Evidence:
`validation/render-recovery/`; functional identity
`source-sha256:23d37e6a138c376a16e0f7e76407b898d9ecf39a1c0508d67039802afce9572d`.

No P1–P9 closure. Installed event/registry state intentionally can progress while
rendering fails; receipts do not acknowledge pixels or physical presentation. Tests
inject native CPU-raster boundary errors, not actual GPU faults. Persistent backend
errors can remain pending until success or Stop; lost-context reconstruction,
terminal PRIME synchronization, conversion/delivery failures, blocked native driver
calls and broader ghost/virtual-key/inertia failure histories remain open. One pending
state is not a bounded input/output queue or an exact memory budget. Supersession is
synchronous and attempt start precedes disposal; failed-attempt cost, total live/peak
retention/disposal and current locked benchmarks still need P1/P7 attribution.
No new NIF/atom/wire version, production thread, resource lock, global cache or
animation clock. D12–D14 work, stash and D15-start supplementary backup are preserved;
no commit or push. Next: prolonged blocked-input retention and broader failure policy.

## D16. Stalled input retention and replay work

Native queue counters reproduced quadratic enqueue work: 2,000 retained commits
normalized/coalesced 2,001,000 records. Paused replay also rebuilt untouched tails:
128 edit acknowledgments caused 341,376 extra coalescer visits. Both defects are
retained as failing-before logs, not timing benchmarks.

Use an owned `VecDeque` in the listener lane. A shared adjacent-coalescing primitive
serves FIFO enqueue and the existing fresh-input vector path, preserving scroll
normalization/accumulation and cursor/resize replacement. Other input stays ordered
and lossless. Replay pops only while fresh, then transfers its untouched remainder.
If nested dispatch buffered a new prefix, coalesce only the join and prepend that
prefix, rather than reinserting the old tail. Full drain disposes the empty deque's
capacity synchronously; no deferred frees. Replay logs preview only 16 labels.

The same counter fixtures now report 2,000 and zero coalescer visits. These counters
exclude other dispatch/layout/text-copy/allocation work. Seven new units qualify
order/boundaries, wrapped-tail joins, finite memory charges, normal receipts, remount
recovery/raw observers and real actor burst recovery/Stop. A 4,096-commit host wait
also skips 1,024 obsolete responses under both decode policies without releasing
input early. Real actor recovery replays 256 Unicode commits; Stop with 4,096 queued
commits does not require their registry acknowledgments.

Measured representation charges on this build: FIFO header 32 bytes, event slot 40
bytes. 100,000 adjacent positions retain one record/160 slot bytes; 4,096 reserved
256-byte commits retain 163,840 slot bytes + 1,048,576 string-capacity bytes. At a
one-record tail, slots still charge 163,840 bytes but strings charge only 256; full
drain leaves zero owned slot/string storage. This spare-capacity retention is an
explicit trade-off, not a whole-runtime memory reduction. Pointer checks verify
ownership transfer does not clone string payloads. Capacity charges exclude allocator
metadata/usable-size rounding, other queues, local edits, snapshots and GPU state.

Validation: **1433 Rust units + 14 integration**, standalone Mix **520** (9 excluded),
full CI **526** (3 excluded), Dialyzer **0**, strict Clippy/fmt. Evidence:
`validation/stalled-input/`; functional identity
`source-sha256:51e3bac45337545965610bc0ce1f334f3297fff7f954ae4d77052cb83e8ee852`.

No P1–P9 closure or locked timing/device claim. Lossless noncoalescible input remains
unbounded; finite storage needs explicit producer backpressure or overflow semantics
with independent control/Stop. No silent drop/cap policy was introduced. Long
non-staling replay may still monopolize one dispatch; end-to-end latency, arbitrary
stall-duration/TTL/resource histories, all queue retention/disposal and accepted
memory budgets remain open. No NIF/atom/wire version, production thread, resource
lock, global cache or animation clock changed. Counters/gauges are test-only.
D12–D15 work, stash and D16-start backup remain preserved. No commit or push.

## D17. Bounded replay/control progress

Before fixtures reproduce unbounded per-install replay, fresh cursor draining beyond
64 inputs, and a dropped ninth request at the macOS host's eight-round feedback cap.
The latter was extracted without changing behavior into a cross-platform helper;
its missing request strands an otherwise valid causal wait.

Replay and fresh cursor draining now use a 64-top-level-input quantum. When replay
has more work and no action has already made listeners stale, explicitly request a
registry rebuild and yield through the existing receipt gate. No extra scheduler,
continuation queue, fabricated animation pulse or second acknowledgment mechanism is introduced.
New raw input stays behind the retained tail; cached no-op responses resume progress
without scenes. Fresh cursor batching never crosses control FIFO boundaries.

`runtime/host_feedback.rs` processes at most eight batches per invocation and checks
the budget before draining the next request. The macOS wrapper uses this shared
helper. Pending work remains in the host event-runtime channel for the next call;
error/Stop paths do not drain it either. No rollback or successful-prefix semantics
change. D16's large remount test now drives bounded continuations explicitly.

Eight new units cover regressions, cached no-scene progress for 4,096 keys over eight
pump calls, composition/new input order and old-receipt rejection, pointer release
across a quantum with/without remount, early error/Stop, and real event-actor recovery
of 2,048 inputs versus Stop without the yield receipt.

Validation: **1441 Rust units + 14 integration**, standalone Mix **520** (9 excluded),
full CI **526** (3 excluded), Dialyzer **0**, strict Clippy/fmt. Evidence:
`validation/replay-yield/`; functional identity
`source-sha256:a898887d8d19aa26031e1a3e5c804c5990cdf10597e2ba7aa9a7d307380d7bce`.

No P1–P9 closure. The shared helper is exercised natively on Linux, not through
macOS UI execution/compilation. Work-count limits do not preempt callbacks, nested
synthetic inputs, reconciliation or blocked sends and are not hard latency bounds.
Fresh cursor batch boundaries can change intermediate observation. Additional yield
receipts/feedback need actual cost accounting. The lossless input queue remains
unbounded: explicit backpressure/overflow semantics are still needed, not silent
input eviction. No new NIF/atom/wire version, production thread, resource lock,
global cache or animation clock. D12–D16 work, stash and D17-start backup remain
preserved; no commit or push.

## D18. Full outbound tree channel and Stop

Two before fixtures reproduce control stalls: the event-driver blocking-send fallback
prevents reading Stop, and renderer shutdown's sequential tree-first sends withhold
render/event Stop behind a full channel. These are separate causal dependencies.

Dispatch now uses immediate `try_send` or retains the complete packet in a local
`VecDeque`; later operations cannot bypass deferred packets. The extracted production
actor loop selects send readiness alongside input/control/timers without cloning or
stripping mount/receipt envelopes. A pending send finding the tree disconnected is
terminal; Stop/input disconnection also releases owned state. Empty outbox capacity
is disposed synchronously. The normal send path does not allocate FIFO storage.

The host's existing channel is **bounded at 512**, not unbounded. Its mutable drain
moves channel packets before deferred packets, capped at 512 per call. The existing
eight-round pump leaves the next batch owned by the host. No second acknowledgment
scheme, replay scheduler or animation pulse is introduced.

Renderer shutdown signals render Stop/backend wake before waiting on actor channels.
`send_actor_stops` selects tree/event stop delivery independently; the test harness
shares it. The existing DirtyIo NIF contract and joins remain unchanged. This does
not guarantee shutdown completion against a stuck actor/asset/driver/callback.

Nine new units cover both blockers, same-mount/remount recovery under both policies,
receipt gating, host/actor FIFO and raw ordering, payload moves, one-shot timer,
Stop/peer-loss weak ownership, and both host budgets. At 1,280 host edits, 512 packets
are in the channel and 768 deferred; the outbox has 1,024 requested 64B slots = 65,536B,
excluding all nested/channel/allocator/other-queue storage. At 4,097 edits, eight pump
rounds leave one packet; the next pump applies it before replaying a newly buffered
commit. No dropped ninth batch or fabricated receipt.

Validation: **1450 Rust units + 14 integration**, standalone Mix **520** (9 excluded),
full CI **526** (3 excluded), Dialyzer **0**, strict Clippy/fmt. Evidence:
`validation/outbound-pressure/`; functional identity
`source-sha256:e98b67da5aa6ddf42f947be3dcef2b2a86fb35336d3e1ed082c77e9dd415032c`.

No P1–P9 closure. This moves blocking pressure into **unbounded retained operations**
and can increase memory/activity, including input/callback/BEAM queues. Explicit
backpressure/overflow semantics and measured total costs remain open, not silent
eviction. Work/smoke-test bounds are not general latency guarantees. No macOS/device/
GPU presentation qualification or new locked benchmark. No new NIF/atom/wire version,
production thread, resource lock, global cache or animation clock. D12–D17 work,
stash and D18-start backup preserved; no commit/push.

## D19. Pressure-contract proposal, not an enabled policy

Write the contract before another buffer/cap. `plans/shared-animation-pressure-contract.md`
recommends **explicit terminal renderer failure** when semantic input/effect admission
cannot fit finite configured limits. No silent eviction, automatic retry after a
partially observed operation, or resumption of the failed mount lifetime. Public
behavior, default compatibility and target limits still need approval; no numbers
are inferred from D18's partial slot charges or historical RSS measurements.

Credits must follow owned storage through producers, FIFOs, construction, outbox,
channels and receiver batches. Reserve old/new coexistence before allocating; actual
last-owner disposal or an explicitly budgeted domain handoff releases credit. Include
spare capacity. Controls cannot wait for data credit; large responses/registries need
their own bounded retention/progress contract. This is not a whole-renderer/RSS/BEAM
budget. Already-emitted callbacks and in-flight native work are not rolled back.

The source audit found an existing earlier loss point: Wayland's generic full-channel
helper discards input, including keys/commits. Its nonblocking test does not establish
losslessness. DRM physical input instead retries with Stop checks. The proposal also
identifies macOS peer-length allocation, clipboard/native command entry points and
observer/dispatch ordering that make a late FIFO cap insufficient. This finding
narrows previous lossless claims to admitted actor/host work, not backend ingress.

`pressure-contract/` holds exact source excerpts and an immutable credit model: nine
tests; 45,476 states / 318,533 transitions at depth 12, with remaining frontiers.
These are symbolic declared charges and assumed matching receipts, not production
allocation/concurrency/clock or liveness qualification. Runtime sources remain D18.
No cap/API/wire/default is enabled and no P1–P9 package closes. No commit/push;
D12–D18 work, stash and D19-start backup preserved.

## D20. Measure event pressure before selecting default limits

The user's separate-thread hypothesis is supported for ordinary-rate isolated input:
both 1-node/20k-node fixtures handled 125 edits/sec and 8k pointer updates/sec without
meaningful backlog. The 20k fixture at 1k edits/sec built 736–812 listener entries
while ingress peaked at one: tree acknowledgment throughput, not event reception,
was limiting. Tree pauses and event-thread pauses must not be conflated.

The 4096-slot incoming channel filled under a deliberately paused event thread
(8k/sec for one second: 3904 rejections), and under unpaced millisecond floods. At
zero consumption, 30/sec would take 136.5 seconds to fill; 8k/sec takes 0.512 seconds.
These are not normal scheduling latencies. A one-second tree pause at 30 edits/sec
left only 29 listener entries; 8k pointer updates during a causal wait retained one.

Evidence: `plans/artifacts/event-pressure-probe/README.md`, run-02, 90 separate release
processes under exclusive performance lock; immutable source identity
`source-sha256:3bdf8fb7db17961302c44398ab2b8dc270e4257b8e8526288aae0f4638b011da`.
Run-01 built but did not measure because `/usr/bin/time` was unavailable. Raw source/
build evidence is retained, not relabeled as results. Successful runs claim no RSS.

The harness uses actual native event/tree loops, a forwarding gate/extra tree channel,
atomic callback counters, short text and static trees. No physical input/compositor,
BEAM application work, GPU, animation load, long text or constrained-device execution.
These short runs do not establish maximum throughput or long-term retention. Native
instrumentation is test-only under bench-diagnostics; the measurement test is ignored.
No pressure policy/default is enabled. Evidence does not justify an aggressive cap
for ordinary input; defensive fault policy still needs the D19 approval/integration
and measured device memory envelope. Default Rust 1450 + 14 / full CI 526 pass,
strict Clippy/fmt and Dialyzer 0. No package closure, commit or push.

## D21. Plan async local edits before pressure-limit machinery

The user approved planning local editing with asynchronous tree updates, while
continuing to receive and safely coalesce input during genuine geometry waits.
Implementation plan: `plans/active-async-input-editing.md`. No runtime changes here.

Keep one ordered semantic stream: local edits can pass an outstanding publication
of earlier edits, not an unresolved earlier click/focus decision. Preserve callbacks
and mount authority; coalesce native state publication only across proven-compatible
boundaries. Require covering edit watermarks for geometry replay, not blanket waits
or a continually moving latest target. Resolve stale echoes/authoritative resets,
IME geometry, failed attempts and in-flight ownership before broad enablement.

Both `SetTextInputContent` and callback effects currently make the listener lane
stale. Removing one flag alone is insufficient. The staged plan includes authority,
local dispatch, bounded publication state, safe coalescing, actor/host/public paths,
regressions and a repeat of the unchanged isolated measurement protocol.

D19 terminal overflow/default budget choices are **not approved** by this direction.
No new pressure machinery, callback dropping, geometry engine, rollback clone,
animation clock or performance claim. No implementation/tests run for this planning-
only change; previous measured and validation artifacts remain unchanged. No commit/push.

## D22. Unify host and raw semantic admission before relaxing tree waits

Source inspection found host edit/command/range entry points directly mutating state
while raw input waited for a registry. That let a host edit overtake an earlier raw
commit or focus-changing click and replace its awaited receipt. Fix this prerequisite
before allowing any local edits to pass their own publication dependency.

One private `PendingInput` FIFO now carries raw input, host commands, host edits and
replacement ranges. Pending host admission returns true (retained, not published),
so Cocoa must not perform a second responder fallback. Clipboard/callback effects
happen only at dispatch. Existing raw coalescing, 64-item replay and receipt gating
remain. No second queue, new NIF/atom/wire version or production thread.

Ranges are different from ordinary keys: they address queried text on a particular
mount. Carry an opaque `RangeGeneration` as well as the mount; invalidate when focus
changes (including ABA), earlier editing changes the addressed content/preedit, or
reconciliation accepts an external replacement. Do not select a replacement or new
focus by recycled id. Later ordinary input still targets the current focused mount.
Tokens are local, not tree receipts, and do not pin registries/scenes/text snapshots.
Only when a range retains the token does invalidation allocate a successor.

The controlled final before-guard variant fails 10/11 units. Actual fixed paths pass
all 11; full validation Rust 1461 + 14, standalone Mix 520, CI 526, strict Clippy/fmt,
Dialyzer 0. D18 transport fixtures now explicitly inject already-resolved effects;
unsafe host admission is no longer their means of filling the outbox. Queue slot
charges/probe sampling use the real pending-item type (40B on this build).

**Partial implementation only.** Native local typing still waits for tree publication;
callback echoes still use value/TTL matching. Versioned local/native authority,
controlled-value provenance, batching, full public/host integration and the benchmark
repeat remain next in `plans/active-async-input-editing.md`. No claimed improvement
to the D20 20k/1k-edit backlog, no D19 policy approval, no P1–P9 closure or commit/push.

## D24. Minimal animation closeout; defer the broader backlog

The user requested a minimal plan to finish animation soon, with all other work
assigned to a later plan. `plans/shared-animation-closeout.md` now controls
completion; `plans/later-animation-runtime-qualification.md` transfers P1–P9 scope.
Older “no package closes”, exhaustive matrix and “next work” text is historical,
not an extra gate. Existing evidence is reused, not silently marked missing.

Closeout retains ten concrete animation contracts, changed production/API safety,
final CI/docs and a small current-source performance snapshot. It does not mandate
async editing, input budgets, COW/model redesign, general GPU recovery or physical
execution on unavailable hardware. Known expensive large-tree cases remain disclosed
and performance/device qualification stays open separately. Supported animation
correctness defects and concrete unsafe changed paths still block implementation
closure; lack of exhaustive permutations does not.

D23 status was not previously checkpointed here: native-only async editing is an
unfinished dirty prototype with 8/9 directed tests passing. The reset/geometry-only
patch test returned empty content instead of `abcXY`. No completed final D23 CI or
measurement qualifies that source. Closeout step 1 preserves a reproducible recovery
artifact then selectively removes D23-dependent source/tests/policy from the closeout
baseline; D12–D22 validated fixes remain. No wholesale restore, discarded evidence
or decision to replace the existing registry-wait contract is authorized.

This checkpoint changes plans only. It does not perform the D23 separation, repair
its failure, close animation, approve overflow defaults, commit or push anything.

## D25. Shared animation implementation closed under the bounded plan

Executed `plans/shared-animation-closeout.md` (retired from its active filename).
Final report: `plans/artifacts/shared-animation-closeout/README.md`.

- Preserved and verified the dirty worktree outside the repository. Original D23
  `/tmp` backups were unavailable; verified the durable D22 archive and freshly
  reproduced D23's 8-pass/1-fail result instead of assuming an old log existed.
- Reviewed/reversed only D23 hunks. Every D22 functional manifest input still
  matches. The retained forward patch recreates all ten original D23 files in an
  isolated roundtrip, including its failing tests; no core assertion was weakened.
- Ten contract rows are covered by existing inspected tests and production seams;
  no new supported animation defect/missing assertion was found. Fixed outdated
  preparation documentation, not animation algorithms or dispatch semantics.
- Rust 1461 + 14; Mix 520; full CI 526 / Dialyzer 0; strict Clippy/fmt; docs including
  internals and 31 screenshots; headless-all check passes with its existing unused
  Vulkan-method warning. Historical archives are not relabelled as these runs.
- Added a bounded preset to the existing benchmark runner, not new instrumentation.
  Eighteen isolated release processes under exclusive lock, immutable source/binary,
  20k/64 × six existing cases × three rotated trials. Source unchanged throughout:
  `source-sha256:bf4c131c8da7882991cdfcbd4058102476abcd36435041c951e5c17a4e66fdff`.
- Length release 15.888ms median; upward warm p50/p95 71.172/86.106ms, release
  76.988ms, warm RSS 378.7MiB. All samples/releases settle; paint/pixel have no
  dimension workspace and mixed loops release it after cancellation. High cost is
  disclosed and deferred, not described as a performance/device-budget pass.

**Implementation complete, not overall runtime/platform qualification.** All other
P1–P9 work has named ownership in `plans/later-animation-runtime-qualification.md`.
Old implementation checklists are durable history rather than active blockers.
D19 policy remains unapproved; async input is not enabled. No commit, push or release.
