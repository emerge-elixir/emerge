# Animation implementation evidence ledger

**Shared animation implementation complete:**
[final evidence](../shared-animation-closeout/README.md) records the bounded audit,
D23 separation, full source-built validation and current performance disclosure.
Functional closure does not close deferred performance/platform/runtime work.

**Current scope:** [minimal closeout](../../shared-animation-closeout.md)
replaces the old package-wide completion gates. Broader matrices, accounting,
input/runtime work and platform qualification are assigned to
[later work](../../later-animation-runtime-qualification.md).

The entries below are accumulated historical evidence, not the current remaining
implementation checklist. Older “gap”, “next” and “no package closes” statements
must be read with later checkpoints; they do not override the new closeout plan.
Existing passing regressions count toward its ten-row contract audit. Final source validation is recorded in the closeout report; no historical result
is being relabelled as current.

## Current implementation evidence

Native paths below are relative to `native/emerge_skia/src/tree/animation/lengths/`.

| Case | Contract / evidence | Status |
|---|---|---|
| Unrelated mixed panels | `tests/continuation.rs::unrelated_mixed_panels_do_not_restart_or_block_a_finite_child`; old midpoint 70 instead of 95; now preserves the curve and releases | Reproduced failure fixed; broader P2 still open |
| Independent boundary panels | `independent_boundary_panels_follow_curves_at_each_scale_and_query_order`; 2 axes × 3 modes × 3 scales × 4 curves × 2 sibling orders; native full footprints, segment reset and late release | Passing, limited to unchanged model/context |
| Cohort sharing | `independent_consumers_share_only_matching_native_input_cohorts`; 64 finite owners, both axes/modes; at most 9 queries/attempt, one model copy, at most 5,000 continuation ancestry lookups/attempt in this fixture | Passing; not group-local layout or a general linear-work claim |
| Consumer-relative cache | `shared_clock_evidence_checks_each_consumers_dependency_direction`; causal and replaced-input directions checked for each consumer in different orders | Passing |
| Same-pool motion | `same_pool_mixed_changes_are_not_mistaken_for_independence`; hybrid native target differs; feedback proof preserves finite curve from 40px to the native 310px goal (.5s = 175px) | Passing dependence/control; not misclassified as independence |
| Cached target corruption | Same test replaces the old cached target with the exact new native target; unmodified old projection must still reject it | Passing |
| Witness failures | `independence_witness_failures_preserve_sources_and_retry_clocks`; prior/hybrid/foreign queries, destination-policy corruption and pre-layout failure; later retry uses original clocks | Passing, both axes/modes |
| Actual versus provisional peer | `release_checks_the_actual_continued_foreign_sample_before_publication`; mixed Content loop driven by numeric descendant; final query uses actual installed samples; injected final-query failure preserves publication | Passing guard; no general feedback schedule |
| Combined release corruption | Existing `simultaneous_parent_groups_use_one_release_query_and_commit_after_layout` still rejects a bad charge with one joint query | Passing, unchanged assertion |
| Raster and hit regions | `tests/rendered.rs::mixed_parent_reset_releases_all_owners_with_matching_pixels_and_hits` expanded from 72 to 144 cases: original and independent-panel variants, regular/enter/change/exit, both axes/modes/scales | Passing; old scene replay, fresh/retained renderer equality and retry-safe hits |
| Terminal accounting | `retired_query_work_includes_terminal_queries_without_retaining_a_workspace`; repeated admission/settle, terminal native queries survive disposal | Passing; per-tree-incarnation numeric receipt |
| Shared-pool/upward mixed deadlines | `coupled_mixed_deadlines_use_frozen_loop_presentations_and_native_release`; 720 axis/mode/scale/curve/deadline traces including resets and large skips | Earlier failures fixed for unchanged-context finite release; broader continuous feedback remains open |
| Forecast provenance and corruption | `coupled_release_failures_and_corrupt_forecasts_do_not_publish_or_restart`; original/forecast/current destinations, native query and output failures, later retry versus control | Passing; leaf-only immutable clock inputs, no history chain |
| Multiple drivers / axes | `multiple_coupled_loop_boundaries_use_one_joint_release_and_stable_clocks`, `coupled_release_certifies_both_axes_together` | Passing weighted-loop boundaries and both-axis joint release; at most 20 queries in the two-driver fixture |
| Owner integration | `upward_enter_and_change_owners_release_against_native_loop_forecasts`; `tests/rendered.rs::coupled_peer_release_matches_pixels_and_hits_for_every_owner` | Passing enter/change Content parents and 72 sibling regular/enter/change/exit raster/hit cases |

| Ongoing native motion | `continuous_feedback_follows_native_targets_on_original_curves_across_schedules`; 432 axis/pool-or-upward/mode/scale/curve/frame-rate traces, native full target/frozen-driver checks each frame | Passing unchanged-model/context subset; source, anchor and run preserved |
| Ongoing failures / phases | `ongoing_feedback_retries_preserve_published_sources_and_the_original_clock`, `ongoing_feedback_resets_only_the_anchor_at_foreign_interval_boundaries`, `coupled_hold_keeps_its_admitted_interval_when_the_other_finite_owner_is_cancelled` | Passing native/corrupt-forecast/output retries, exact/late resets and cancellation without layout-model edit |
| Ongoing rendered owners | Existing 72-case coupled raster/hit test also checks .5→.75s continuation for regular/enter/change/exit | Passing, including the formerly failing first change/ghost midpoint; D8 native receipts fix the model/forecast bridge |
| Model + cancellation / admission | D7 historical failures now have positive assertions: static Fill cancellation releases; first change/ghost midpoint is 175px | **Fixed by D8**; broader P3 remains open |

| Same-node / combined drivers | `self_axis_wrapping_feedback_preserves_the_height_curve_and_width_clock`, `self_axis_independence_does_not_reanchor_the_loop`, `numeric_and_resolved_inputs_compose_under_native_rotation_and_scale`, `a_rotated_mixed_driver_can_also_have_a_numeric_axis` | Passing field-composed witnesses and axis-qualified independence; not the whole P2 matrix |
| Parent-imposed sizing | `composed_feedback_respects_native_parent_imposed_sizes` | Both axes/modes release to native Slider-imposed allocation |
| Historical goal integrity | `historical_clock_receipts_reject_corruption_and_retry_the_original_admission` | Goal/model/projection/context corruption and historical-evidence failure preserve publication and original admission |
| Combined context / membership | `model_viewport_and_mixed_boundary_release_replay_original_joint_inputs`, `seed_membership_changes_at_coupled_release_use_native_receipts` | Positive joint declaration/padding/viewport/scale/reset and membership addition/removal/retry cases |
| Actor/direct lifecycle | `runtime/tree_actor.rs::coupled_frames_ghost_cleanup_and_final_hits_match_direct_engine` | Matched registries/raster output, ghost terminal/cleanup, cancellation, replay and idle |
| Queue publication | `runtime/tree_actor.rs::final_scene_survives_registry_backpressure_and_latest_scene_overwrite` | Capacity-one publication channels and explicit rendezvous/drain synchronization; inactive final output retained |


| Structural sources / units | `tests/transport.rs::native_transport_maps_roles_and_units_without_lowering_own_channels` | 270 axis/mode/role/relative-scale/curve cases; native own-channel/policy preservation and release, including wrapped/float/Nearby destinations |
| Native transport integration | Same module: reparent, parent-kind replacement, root promotion/remount, both-axis source, numeric fast-path, rotation and coupled forecast tests | Directed cases pass; no general topology qualification |
| Transport retries / lifecycle | Same module: source/goal corruption, unpublished reversal, incoming change owner, held-member migration and ghost cleanup | First published sources and clocks preserved; new-role holds carry their admitted barrier |
| Joint input provenance | `transport_replays_joint_asset_runtime_scroll_viewport_and_scale_inputs` | 24 font/image/axis/mode/viewport-scale traces with local scales, runtime/scroll seeds, native failure/retry and renderer isolation; retained font rasters replay |
| Same-ID retained image rendering | D9 failure retained in `validation/transport/same-id-retained-raster-gap.log`; D10 `renderer/scene_images/tests.rs` and promoted `transport_replays_joint_asset_runtime_scroll_viewport_and_scale_inputs` assertions | **Fixed for captured native scenes:** records/cached pixels remain bound across replacement, eviction, reset and deferred paint |
| Image binding isolation / retention | `renderer/scene_images/tests.rs` | Raster/SVG and fit/profile matrices, renderer-local generation collision, frozen absence, nested scopes, latest cached generation, grayscale policy, sparse pins and last-reference disposal |
| Stale completion plus animation | `assets/tests.rs::stale_completion_after_replacement_publication_preserves_animation_and_retained_images` | Real loader completion and tree-update engine under both decode policies; late stale result cannot replace the current record or restart the original deadline; old raster replay passes |

D9 adds 18 directed tests; broad P2–P5 completion is not claimed. Source transport
uses the existing planner, not historical-tree reconstruction or scope relabeling.
Pure numeric frames still avoid a dimension workspace unless a structural source
needs native unit/role conversion. No benchmark of D9 was run.

The earlier negative-proof test was replaced by positive release assertions, not
ignored or disabled. Historical rejection evidence remains in
`validation/coupled-negative-baseline.log`; D8 full results are under
`validation/provenance-fields/`; D9 validation is under `validation/transport/`.
See decisions D6–D9 for the bounded temporal rule and the
forecast-versus-published-sample counterexample.

## Mapping every open active-plan entry

| Active-plan entry | Packages | Existing evidence / remaining gap |
|---|---|---|
| Broader runtime/media/scroll provenance and combined causes | P3 | Existing font/image replacement, viewport/model/font/padding raster tests pass in full CI. **Gap:** joint seed membership/absence, scroll source versus clamp, local-scale plus runtime/media/deadline, stale completion and renderer-isolation matrix. D8 fixes D7 model/clock failures and adds original-query replay plus receipt-backed membership cases. |
| Broader continuous-context retargeting and deadlines | P2–P3 | Independent panels fixed above. Shared-pool/upward loop deadlines now pass the covered native and raster matrices. Existing mixed ancestor boundary suite passes. Ongoing shared-pool/upward motion now passes the unchanged-context subset. D8 adds self-axis, combined numeric/resolved and imposed/rotated/wrapped cases. **Gap:** complete cross-axis/phase/failure and combined input matrices, not the earlier D7 admission failures. |
| Skips, cancellations/arrivals, terminal sources, ghost baselines and structural transport | P4–P5 | Existing skipped-cycle, hold/cancel/arrival and ghost/raster suites pass. D9 adds sealed published attachment sources, native unit/role conversion and directed reparent/root/remount/hold/ghost tests. **Gap:** complete structural closure/order/phase/owner and baseline/terminal-source/cleanup matrices, including arbitrary orphan-root and multi-edit topologies. Declaration-only historical replay still rejects topology changes; native transport is a separate explicit query. No structural package closed. |
| Remaining actor/direct/headless and final damage | P6 | D11–D18 directed tests cover blocked output/Stop, causal responses, mount-safe pointer recovery, in-flight command rejection, controlled binary-render recovery, finite stalled-input work/retention, bounded replay/lossless feedback, outgoing pressure/shutdown signaling, input modes and final raster output. **Gap:** broader joint topology/context/interaction histories, renderer failure/recovery, long blocked-input retention and physical presentation/device synchronization. |
| Performance optimization and available platforms | P1/P7–P8 | Numeric retirement receipt and actual continuation-ancestry counter added. Locked 72-process original, 96-process coupled and 96-process ongoing matrices, immutable source archives, binary digests and ordered samples in `performance/`. **Last measured failure (baseline 03, before D8):** 20k release over 12ms; 64-loop upward motion ~76ms warm / ~87ms release, ~377MiB RSS (194 steady native queries). D8 receipt and D9 source-lineage/transport costs are unmeasured. **Gap:** all-phase attribution, source/model live bytes and peak retention, model rebuild/full-tree shutdown disposal, sparse native visits and remaining lifecycle benchmark cases. Actual Wayland socket and AMD Vulkan devices are available; animation presentation qualification has not run. macOS/constrained target remain unavailable. |
| Final API/artifact/docs audit (part of the final active entry) | P9 | Public list API, EMRG9 and macOS14 tests pass in current full CI. No interface or protocol changed in this slice. **Gap:** final cross-entry-point/Rustler/artifact audit, matching external host binaries, `mix docs`, final device/backend evidence and closure audit after P2–P8. |

## Accounting scope

- `AnimationQueryRetirement` retains only numeric last/cumulative query statistics,
  runtime retirement count and disposal duration. Normal dimension-runtime disposal
  sites use `ElementTree::clear_length_runtime`; there is no deferred free queue.
- `last.source_slots` is the last retired workspace's gauge, **not live retention**.
  `workspace=false` remains authoritative for absence of the dimension workspace.
- Counter lifetime is one tree incarnation. Whole-tree replacement/destruction and
  internal resolver model-rebuild disposal are not separately attributed yet.
- `continuation_ancestry_visits` counts actual retained parent-link lookups in the
  continuation classifier, including unsuccessful searches. It does not count all
  grouping/dependency work, HashMap comparisons, native measure/resolve visits or
  heap bytes. Hash iteration can alter lookup counts without changing output.
- Finite targets may retain a shared map of leaf `ClockInput` values used by their
  endpoint queries. Leaf types cannot retain forecasts recursively. The forecast,
  retained driver and boundary native destinations are all verified when needed;
  actual feedback samples must equal what the transaction installs. Projection
  diagnostics include these inputs. Forecast sets are released when finite tracks
  retire, but this does not imply low live/peak memory or cheap construction.
- Independent proofs use one existing resolver/workspace. Matching source/target
  pointers, evaluation time, release scope and sorted replaced-node classifications
  share native batches; each consumer rechecks eligibility. Only failed consumer
  footprints decline continuation, not their whole batch.
- Cold/warm/live/release work is still not fully phase-split. New measurements make
  ordinary workspace disposal visible; they do not claim that all other costs are
  allocator destruction or that sparse records imply cheap evaluation.

## Validation

At native source snapshot
`source-sha256:8c620913b2e4d0b9d05dabccd9312f1c0a276015eea59b1ea02bae1c73022fe5`:

- `cargo test --manifest-path native/emerge_skia/Cargo.toml`: **1,324 units + 14 integration tests**, all pass.
- `EMERGE_SKIA_BUILD=1 CARGO_TARGET_DIR=/workspace/emerge-animation/native/emerge_skia/target mix test`: **494 tests/doctests**, 8 excluded.
- Same environment, `./ci-tests.sh all`: **499 Elixir tests/doctests**, 3 excluded; Rust suites pass; **Dialyzer 0**.
- Clippy including benches/tests/`bench-diagnostics`, `-D warnings`: pass.
- Rust formatting and `git diff --check`: pass.
- No new benchmark run. Prior immutable baseline 03 remains historical performance evidence.

Durable raw logs: `validation/provenance-fields/{cargo,mix,ci,clippy}.log`. The functional source manifest identifies accumulated dirty-worktree source, **not merely HEAD**. No
commit/push was made. Green functional tests do not close the listed design,
performance, integration or platform gaps.


D8 adds opaque native result-map retention (`native_receipts`, `receipt_goals`).
Historical lookup observations do not increment native layout-query counters.
Neither bounded receipt retention nor successful CI establishes cheap memory/work.

D9 validation: 1342 Rust units + 14 integration; standalone Mix 494; full CI 499
Elixir tests/doctests and Dialyzer 0. Functional source `source-sha256:aab1a31e9e1e611b98c2ab057b4dcfc089de2db7c56c428d4edd2ff6b57edf3f`; logs and
manifest: `validation/transport/`. No D9 benchmark or package closure.

## D10 checkpoint

Eight new tests and stronger assertions in the 24 D9 combined-input traces fix the
same-ID retained-image rendering gap. Scene capture pins only referenced source
records or available cached pixels; absence stays absent. No asset history map,
NIF/atom/protocol, production thread or lock was added (existing short lookup
locks are reused). Weak source-owner identity avoids pinning the renderer runtime.

This is not full P3/P5 closure: atomic layout-to-scene asset provenance under
concurrent replacement, broader stale-completion/epoch schedules, mixed ghost
appearance baselines and other combined-input/lifecycle matrices remain open.
Record/pixel reference charges and capture visits are exposed, not exact live heap,
parsed-tree bytes or a measured memory budget. No D10 benchmark or platform run.

D10 validation: 1350 Rust units + 14 integration; standalone Mix 494; full CI
499 Elixir tests/doctests and Dialyzer 0. Source `source-sha256:6989691401905f206dc4891207aaccaec917605207e23e19faf5fef09587a6d5`; logs and manifest
in `validation/image-bindings/`. No benchmark or package closure.


## D11 checkpoint

Atomic prepared-frame image provenance is implemented and has 24 controlled writer/
publication schedules plus pending-readiness and decorative-source regressions.
Structural coverage adds 144 multi-edit/orphan/root schedules and two root authority
regressions. Noncanonical coupling adds 1,536 two-axis self-/cross-axis repeat,
interruption and failure schedules; Slider now keeps imposed width resolve-only.
Complex ghost transport/remount/cleanup and hold-arrival/cancellation add 24 traces
each. A permanently blocked registry channel no longer blocks actor Stop; actual
actor coalescing and pending mount focus are tested. Direct/actor/native-headless
traces compare focus/IME/drag/wheel/Nearby events, pixels, final damage and replay.
See decisions D11 and `validation/atomic-publication/` for scope and full checks.

Remaining: exhaustive owner/layout/Hz and joint input cross-products, randomized
multi-edit/lifecycle schedules, exact frame-source/receipt/live-peak/disposal costs,
new locked performance and device/constrained/macOS qualification. The new source
index includes inactive styles/orphans, and per-frame capture visits referenced
sources/image nodes; bounded retention is not a performance claim. No package closes.

D11 full validation: **1362 Rust units + 14 integration**, standalone Mix **494**,
full CI **499**, Dialyzer **0**; fmt/Clippy/source-manifest/diff checks pass.
Functional identity: `source-sha256:f60614ecc898495d7e36241764ebeccd3689ea1d49952e02348d1d419106b96a`.
No new benchmark or platform-presentation qualification.

## Post-D11 headless rebase

Base `39997f0`; rebased planning HEAD `84906f7`. Both upstream and animation suites
pass after adapting two upstream test call sites to transactional runtime/snapshot
APIs. An additional 18-case paragraph decoration/alignment/transport/ghost regression
passes. Rust 1392 units + 14 integration; standalone Mix 520 (9 excluded), full CI
526 (3 excluded), Dialyzer 0. See `validation/headless-rebase/` and
`plans/animation-headless-rebase.md`. Cache budget 16→64 and new inline-box storage
remain unbenchmarked with animation; new architecture/device targets are unqualified.

## D12 checkpoint — causal installation

- Native keyboard regression: 2 decode policies × 1/8/64 queued old registries;
  old/foreign receipts cannot release input, and replay creates its own request.
- Event coalescing and causal waits: same eligible mount survives; removal,
  remount and eligibility loss discard the pending target. Existing tree-actor tests
  retain explicit-focus precedence and unrelated-focus removal coverage.
- Cached no-op requests acknowledge without a scene; existing native query/ghost/
  backoff failure matrices acknowledge only successful recovery.
- Real event actor: recovery and Stop while input is buffered. Event draining is
  capped at 64; immutable focus metadata shares storage and releases with its owners.
- Real tree actor/direct/native headless: three 1/8/32 deferred-registry schedules
  compare keyboard/IME/wheel state, messages, registries, fresh/retained raster output,
  old-scene replay and final inactive/sample-removal output. Each event runtime owns
  its request identities; the harness no longer substitutes the other's messages.

Eight added unit tests; **1400 units + 14 integration**, standalone Mix **520** (9
excluded), full CI **526** (3 excluded), Dialyzer **0**; fmt/Clippy pass. Evidence:
`validation/causal-registry/`. No performance, memory-budget, device or P1–P9 closure.
Causal event responses do not acknowledge renderer/compositor installation. Broader
mixed lifecycle/input and render-failure histories, long-lived input buffers and
actual receipt/metadata construction/retention/disposal costs remain unqualified.

## D13 checkpoint — pointer recovery

Two native remount failures reproduced and fixed: buffered click release activated a
replacement ID; text drag retained its old anchor. A shared native source-mount
index now qualifies captures and local-state continuation before accepted replay.

Nine pointer unit functions cover 17 directed schedules: remount clicks (both error
policies), text drag/edit reset, translated selection/release, hover recovery and
remount enter, current-geometry click hit tests, scroll/thumb and slider continuation
versus remount, and real event-actor release/Stop. One index-sharing/drop unit and one
real tree-actor/direct/headless test add three 1/8/32-backlog drag/release/Nearby traces.
The headless traces retain registry/message/selection/raw-input parity, fresh versus
retained pixels, old-scene replay and final sample removal.

**1411 Rust units + 14 integration**, standalone Mix **520** (9 excluded), full CI
**526** (3 excluded), Dialyzer **0**, fmt/strict Clippy pass. See
`validation/pointer-recovery/`. Obsolete crossing registries are deliberately not
installed: no historical hover callbacks are invented. Accepted/fresh geometry and
same-mount release remain authoritative; physical presentation is not acknowledged.

Still open: broader combined lifecycle/input/virtual-key/inertia histories, renderer
failure/recovery, long stalled-input retention and real presentation. New index
construction/merging, state scans, live/peak memory and synchronous disposal are not
benchmarked or fully attributed. No package, performance or device gate closes.

## D14 checkpoint — in-flight event commands

The native host/engine reproduction now rejects old text/selection commands after
replacement rather than focusing/editing the new mount. Sparse dispatch evidence
qualifies the whole operation; a stale target cannot partially blur another target.
Control and response-fence delivery survive rejection, and deferred scroll/style
accumulators retain their original mount through later topology edits.

Nine new unit functions: eight in `events/runtime/tests/delayed/pointer/inflight.rs`
and one real tree-actor/direct/headless trace. Coverage includes both policies for
text/selection and slider, 32 deferred schedules (eight message variants × two
producer orders × two policies), grouped focus/resize/receipt behavior, same-mount
unrelated revisions, missing/malformed evidence with Stop, focused listenerless
sources, real event-actor delivery, native registry/IME parity, retained/fresh raster
pixels and unchanged old-scene replay. Delivery preserves envelopes; diagnostic
observation helpers now inspect them without stripping transport evidence.

**1420 Rust units + 14 integration**, standalone Mix **520** (9 excluded), full CI
**526** (3 excluded), Dialyzer **0**, strict Clippy/fmt pass. Reproduction and manifests:
`validation/inflight-commands/`. No package closes. Already-emitted element callbacks
are not undone. Broader virtual-key/inertia/joint histories, renderer failure,
prolonged buffering, collection/sorting/check/disposal accounting, current locked
benchmarks and physical presentation remain unqualified.

## D15 checkpoint — binary renderer failure/recovery

The existing binary loop was extracted without changing its failure behavior; the
new terminal-frame test reproduced no recovery without another tree message.
Newest-state retention/retry fixes the stall without changing tree publication or
admission clocks. Autonomous delay grows 16/32/64/128/250 ms and caps there; incoming
scenes attempt immediately, preserving the accumulated delay until success. Failed
output does not continue scheduling animation samples; successful output resumes
native wall-clock pulses. Static recovery sends no invented pulse.

Six new unit functions:
- Terminal-frame retry without another tree message.
- Virtual 1000 ms backoff schedule: eight autonomous attempts, cap/reset checks.
- 64 failed-state replacements: weak payload evidence of synchronous supersession;
  successful output leaves no pending retry history. This is not total renderer heap.
- Pending Stop/disconnection with controlled gates, and resumed animated output
  preserving source timing/sequence semantics.
- Twelve real tree-actor/direct/headless schedules: pre/post CPU-raster draw fault ×
  1/8/32 updates × same-mount/remount. Text/drag/release/Nearby events, stale command
  rejection, native registry/IME parity, final sample removal, newest recovered
  pixels versus fresh/retained rendering and old-scene replay all pass. Native
  event state progresses while the output probe still has no successful new frame.

**1426 Rust units + 14 integration**, standalone Mix **520** (9 excluded), full CI
**526** (3 excluded), Dialyzer **0**, strict default-feature Clippy/fmt pass.
`headless-all` check passes with an existing unused Vulkan capability-method warning;
this is not an all-feature warning-free or device-execution claim. Evidence and
source/binary manifests: `validation/render-recovery/`.

No package closes. Real GPU faults, lost-context reconstruction, PRIME terminal sync,
conversion/delivery failures, blocked driver calls and physical presentation remain
unqualified. Persistent errors may remain pending until success or Stop. Prolonged
input buffering, failed-attempt metrics, live/peak/disposal accounting and new locked
benchmarks remain open; one retry slot does not bound queues or whole-runtime memory.

## D16 checkpoint — stalled input FIFO

Before evidence: 2,000 retained commits caused **2,001,000** coalescer input visits;
128 successive edit receipts caused **341,376** tail-reinsertion visits. Incremental
adjacent coalescing and FIFO remainder transfer yield **2,000** and **0** respectively.
Counters instrument coalescer calls, not all dispatch/layout/allocation work.

Seven new units cover both reproductions, cursor/scroll/resize boundaries interleaved
with key/button/UTF-8 composition events, wrapped-tail joins, separate queue capacity
charges, 4,096 buffered commits plus 1,024 obsolete responses under both policies,
remount recovery/raw input once, and real actor 256-commit recovery/4,096-commit Stop.

On this x86-64 build (container header 32 bytes; event slot 40 bytes):
- 100,000 consecutive cursor positions: one record, 160 slot bytes, no string heap.
- 4,096 commits reserving 256 string bytes each: 163,840 slot bytes + 1,048,576
  string-capacity bytes. Payload pointers survive ownership transfer without cloning.
- After all but one commit are consumed: slot capacity remains 163,840 bytes, payload
  capacity drops to 256 bytes. Full drain leaves zero owned slot/string capacity.

This explicitly records the spare-capacity trade-off, not a memory saving or exact
allocator/RSS budget. Requested container/string capacities exclude allocator
metadata, other queues, observers, local edits, trees, renderer/GPU state and global
caches. Finite burst schedules do not qualify arbitrary wall-clock/TTL histories.

**1433 Rust units + 14 integration**, standalone Mix **520** (9 excluded), full CI
**526** (3 excluded), Dialyzer **0**, strict Clippy/fmt pass. Source/binary manifests,
before logs and raw capacity observations: `validation/stalled-input/`.

No package closes. Noncoalescible input is still unbounded; a hard limit needs explicit
backpressure/overflow semantics preserving Stop, not silent eviction. Replay can
still dispatch a long non-staling burst synchronously. End-to-end latency, all owned
queues/allocations, long-duration resource transitions and current locked benchmarks
remain open. No device/presentation or timing-speedup claim.

## D17 checkpoint — replay/control work quanta

Before tests reproduce full-burst replay (128 inputs consumed in one install), fresh
cursor draining past 64 inputs, and loss of the ninth request in the extracted macOS
host feedback loop. Replay/fresh drain now use a 64-input quantum. A yielded replay
requests a cached response through the ordinary opaque receipt gate; no second
continuation queue or animation pulse is introduced. The host pump stops after eight
rounds without draining work it cannot process.

Eight new units cover those regressions; 4,096 no-op keys progress over eight pump
calls/64 acknowledgments without scenes; composition/new input ordering and rejection
of a previous yield receipt; release on the following quantum with same-mount versus
remount behavior; error/Stop preserving undelivered work; and real actor 2,048-input
recovery versus Stop without a yield acknowledgment. D16 remount recovery now drives
the explicit bounded continuations rather than assuming one unbounded callback.

**1441 Rust units + 14 integration**, standalone Mix **520** (9 excluded), full CI
**526** (3 excluded), Dialyzer **0**, strict Clippy/fmt pass. Evidence:
`validation/replay-yield/`. The cross-platform helper used by the macOS wrapper is
executed here; macOS compilation/execution is not claimed.

No package closes. The limits count top-level inputs/pump rounds, not elapsed time,
reconciliation/callback/nested synthetic work or blocked sends. Fresh cursor batch
boundaries can change which intermediate positions are observed. Extra receipts and
feedback calls still need cost attribution. Hard memory bounds require explicit
backpressure/overflow semantics; no input cap/drop policy, performance or device
qualification is inferred.

## D18 checkpoint — outgoing pressure and independent shutdown signals

Two before fixtures expose distinct blockers: event dispatch waits in a full tree
channel send, and renderer shutdown waits on tree Stop before signaling other peers.
The driver now retains complete packets in an outgoing FIFO and selects send/input/
control/timers. The immediate-send path allocates no FIFO storage; no newer packet
bypasses an older one. Pending output peer loss is terminal. Native host drains move
channel then deferred packets, at most 512 per call, preserving the eight-round pump
budget. Renderer shutdown signals render/backend before independently selecting both
actor Stop sends; the test-harness shutdown shares the helper.

Nine new units cover full-channel event Stop; queued receipt gating; 1,281 host edits
with freed-slot ordering/payload pointer preservation; real actor input recovery and
remount rejection under both policies; 128 non-staling preedits in FIFO order/raw once;
Stop and both peer disconnections with 1,024 buffered inputs/weak receipt disposal;
a due one-shot virtual-key timer; 4,097 host edits crossing both pump budgets with new
input held behind the last receipt; and shutdown with tree/event/both channels full.

The 1,280-edit checkpoint owns 512 channel packets + 768 deferred packets. The latter
uses 1,024 requested slots × 64B = **65,536B**, excluding nested payloads, headers,
channel/allocator overhead and other runtime queues. Partial storage persists; full
drain disposes it. This is neither total retained memory nor a memory saving.

**1450 Rust units + 14 integration**, standalone Mix **520** (9 excluded), full CI
**526** (3 excluded), Dialyzer **0**, strict Clippy/fmt. Evidence:
`validation/outbound-pressure/`. No P1–P9 package closes. Pressure moves into memory;
explicit backpressure/overflow semantics, all queue/callback/BEAM costs and current
locked benchmarks remain necessary. Signal progress is not a hard shutdown duration:
delivery/disconnection and joins still wait, and callbacks/assets/driver calls are
not preempted. Native helper tests do not qualify macOS execution or GPU presentation.

## D19 checkpoint — pressure-contract design/model only

Proposal: `plans/shared-animation-pressure-contract.md`. Prefer explicit terminal
renderer failure to silently losing semantic input under a claimed finite budget.
Admission precedes managed allocation; transfer/ack does not refund live storage;
construction overlap and spare capacity count; stop/fault and bounded responses
must progress independently. Policy/default approval and numerical limits remain open.

Source audit corrects the scope of earlier lossless-FIFO statements: Wayland's
`try_send_wayland_event` discards any full-channel event, including keys/commits,
before it reaches the actor. DRM physical input uses stoppable send waits instead.
macOS frame allocation, observer-before-buffer and dispatch-before-packet ordering
also require earlier admission gates. Exact excerpts/hashes: `pressure-contract/audit.json`.
These are source findings, not device execution. No new runtime behavior is installed.

Nine Python model tests pass. Depth-12 exploration checks **45,476 states / 318,533
transitions** across three synthetic quota configurations, with remaining frontiers.
Declared-charge conservation, terminal monotonicity and transfer accounting are
checked only under model assumptions. Native allocators, concurrent reservation,
actual receipts/mounts, SDK/BEAM queues, callbacks and rendering are not modeled.

Runtime sources stay at D18. No new Rust/Elixir tests, memory cap, public API/wire
change, default budget, locked benchmark or P1–P9 closure. Next: approve behavior/
limits, implement visible terminal status and earlier accounted ingress/construction,
close Wayland loss, then measure and qualify across public/native/platform paths.

## D20 checkpoint — native event pressure experiment

`plans/artifacts/event-pressure-probe/` preserves 90 release-process measurements,
exclusive lock, immutable source archive/binary hash and host/build identity. Cases
separate 1/20k-node trees, edits/pointer/composition, paced/unpaced producers, and
healthy/tree-paused/event-paused consumers. No production behavior or policy change.

Ordinary-rate tests stayed far below 4096 ingress capacity. At 20k nodes, 1k edits/sec
queued 736–812 listener inputs with ingress peak one; a separate event thread cannot
remove the tree-response dependency. Tree-paused pointer input coalesced to one.
A one-second event-thread pause at 8k/sec produced exactly 4096 admissions and 3904
full-channel rejections. Bursts of 20k edits in milliseconds also filled ingress.

Scope: test-only counters, real native event/tree loops plus a forwarding gate, short
text/static trees/no-op callback sinks, no BEAM application/GPU/device execution.
Loop-boundary/requested-slot peaks are not full memory accounting; two-second
recovery cutoffs explicitly stop remaining work. Not maximum throughput, arbitrary
stall-duration qualification, accepted default budget or animation benchmark closure.
Validation after measurement: Rust **1450 + 14**, standalone Mix **520** (9 excluded),
full CI **526** (3 excluded), strict Clippy/fmt, Dialyzer **0**. No P1–P9 closure.

## D22 checkpoint — ordered host admission prerequisite

Raw input, host commands/edits and replacement ranges share one `PendingInput` FIFO.
No host callback, clipboard effect or edit overtakes a pending raw/focus decision.
Deferred ranges are mount/text/focus scoped, including focus ABA and intervening
content/preedit/accepted external changes. Ordinary later input resolves current focus.

Eleven directed units cover order/receipt retention, select-all/insert/clipboard,
queued focus clicks, remounts, initial registry, failed-batch recovery, coalescing
boundaries, ownership transfer, 4096-command/64-item yielding, context invalidation
and token destruction. Controlled before-guard variant fails 10/11; not old HEAD.
The external-reset case injects reconciliation metadata; it is not a public codec trace.

D18 outbox/feedback tests now explicitly prepare ready effects rather than relying
on the removed host API bypass; their transport assertions remain. Six manually
seeded direct-session tests explicitly mark their fixtures ready. Queue charging and
probe counters use `PendingInput`'s actual size (still 40B in this build); historical
logs and measurements remain untouched. No new locked performance run.

Validation: Rust **1461 + 14**, standalone Mix **520** (9 excluded), full CI **526**
(3 excluded), strict Clippy/fmt, Dialyzer **0**. Evidence `validation/ordered-host/`.
Not async local editing: versioned native publication, callback echo provenance and
batching remain open, as do the real platform/public integration and P1–P9 gates.
