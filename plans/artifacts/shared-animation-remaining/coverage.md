# Remaining animation coverage ledger

Status: **P1–P6 in progress; native structural transport and immutable retained image bindings added; overall unfinished.** This ledger maps
all five unchecked entries in `plans/active-shared-animation-core.md`. Passing tests
are evidence for their stated fixtures, not completion of the broader entry.

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
| Remaining actor/direct/headless and final damage | P6 | Existing actor/content/retry/ghost and public ExUnit suites pass. D8 adds matched real actor/direct lifecycle outputs and capacity-one publication backpressure; this is not full queue/stop or GPU presentation qualification. **Gap:** deterministic queue pressure/coalescing/stop, final and cleanup frame installation, focus/IME/wheel/drag/Nearby and cached damage traces. |
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
