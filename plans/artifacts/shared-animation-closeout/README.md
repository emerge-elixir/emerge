# Shared animation closeout

**Status: shared animation implementation complete.** All four minimal closeout
steps pass; performance optimization and physical platform qualification remain
explicitly deferred. No supported animation correctness blocker was found.
Worktree `/workspace/emerge-animation`, branch `plan/shared-animation-core`, reference
HEAD `33ee559`. The implementation includes uncommitted D12–D22 changes; HEAD alone
is not its source identity. This report records the pre-commit closeout checkpoint.
The subsequent [reviewable commit sequence](../animation-review-commits/README.md#completed-history)
commits the same implementation bytes; no push or release is implied.

Final source identity:
`source-sha256:bf4c131c8da7882991cdfcbd4058102476abcd36435041c951e5c17a4e66fdff`.
[Manifest](source.sha256) and [immutable source archive](performance/source.tar.gz)
include native/Elixir/test/build inputs and the benchmark runner/summarizer. The
runner verified every input unchanged after measurement. This broader manifest is
not the old D22 manifest, even though its functional inputs still match.

## Baseline and preservation

The complete tracked/untracked nonignored worktree was archived and verified before
edits: `/workspace/animation-closeout-recovery-20260915-162634/worktree.tar.gz`.
The recovery directory includes per-file hashes, index, staged/unstaged/HEAD patches
and the D22 comparison tree. Build/dependency caches were not archived. The original
D23 `/tmp` backup pointer/logs were unavailable in this session; the durable D22
source archive was verified and D23's test failure was reproduced anew.

- [Preservation details](preservation.txt); 1248 existing paths verified in the archive.
- [Deferred D23 forward patch](deferred-d23.patch) includes the two new source/test
  modules, native publication authority, dispatch/reconcile changes, registry fields
  and macOS geometry-query policy. Review it before applying to a compatible,
  disposable tree; it is not an instruction to restore over later work.
- [Before test](deferred-d23-before.log): 8 passed / 1 failed; reset/geometry-only
  patch expected `abcXY`, received empty text. That prototype was deferred, not fixed
  by weakening the assertion or deleting a supported animation regression.
- Reviewed all hunks, reverse-checked then removed only D23. Every file in the
  [D22 manifest](d22-inputs.sha256) matched byte-for-byte afterward. Completed
  D12–D22 fixes, the core animation implementation and all their tests remain.
- Existing registry waits and native causal/mount receipts are unchanged. No async
  editing, new overflow policy, wire version, NIF, atom or production thread added.

## Ten-row contract audit

All rows are **covered** by existing assertions, inspected during closeout and run
again in final validation. These are supported-contract checks, not proof of every
possible context/history. No concrete missing contract assertion or new supported
animation defect was found; no additional Cartesian matrix was created.

Rust animation test paths below are relative to
`native/emerge_skia/src/tree/animation/lengths/` unless otherwise stated.

| Contract | Inspected evidence and assertions | Verdict |
|---|---|---|
| Public policy and validation | `test/emerge/animation_change_test.exs`: list/shared and independent timing, ordered duplicate/spacing overrides, plain removal, empty-list timing validation, ownership/compatibility rejection, static versus animated bounds, native EMRG roundtrip and immediate first mount | Covered |
| Length pairs, clocks and release | `tests/rendered.rs::all_supported_length_pairs_and_owners_paint_real_geometry_and_replay_retained_scenes`: 16 pairs × 2 axes × 3 modes × 4 owners × 3 scales; five numeric/raster/hit samples and old-scene replay. `tests.rs::finite_sibling_runtime_holds_and_atomically_releases_both_axes_at_all_schedules` checks joint holds; complete-charge corruption still fails | Covered |
| Content detection and interruption | `tests/content.rs::unchanged_content_declaration_animates_descendant_text_growth_and_shrink`, `content_interruption_uses_the_published_pose_and_incoming_timing`, `equal_content_targets_and_timing_only_edits_do_not_restart`, plus `test/emerge_skia/live_length_change_test.exs` descendant-only actor trace | Covered |
| Nested/coupled axes and boundaries | `tests/continuation.rs::noncanonical_self_and_cross_axis_repeats_interruptions_and_failures_preserve_admission_clocks`: wrapping/paragraph/TextColumn/Slider, both axes, finite repeats/loops, four curves, scales, modes, boundary clocks and failed interruption. D6–D8 native continuation/deadline/corruption tests also pass | Covered |
| Combined input provenance | `tests/transport.rs::transport_replays_joint_asset_runtime_scroll_viewport_and_scale_inputs`: source/clock preservation, old raster replay, latest input recovery and renderer isolation. `assets/tests.rs::replacement_and_reconfiguration_between_prepare_and_publish_keep_one_asset_input`: concurrent writer finishes while preparation is alive; old layout/red pixels remain coherent; failed preparation retains prior frame assets; next frame uses new facts | Covered |
| Structural transport/remount | `tests/transport.rs::native_transport_maps_roles_and_units_without_lowering_own_channels`, `transport_failure_second_move_and_unpublished_reversal_keep_first_source`, root/remount and D11 orphan-root tests: native role/unit allocation, first published source/key/anchor, failed move/reversal, no inheritance by remount | Covered |
| Lifecycle/ghosts | `tests/regular.rs::regular_paint_hands_off_while_geometry_waits_then_uses_its_own_admitted_clock`, `skipped_regular_handoffs_are_bounded_and_completed_sources_are_released`; `tests/transport.rs::complex_ghost_transport_and_remount_survive_interrupted_terminal_cleanup`: ghost hits absent, sampled source retained, terminal output survives failed cleanup, remount separate, old raster unchanged, groups retire | Covered |
| Failure/publication/prefix | `tests/application.rs::rejected_moving_and_releasing_frames_preserve_pose_sources_and_committed_tracks_in_all_modes`, stale/cross-runtime preparation tests, `nonrelease_sources_and_tracks_commit_only_after_native_output_and_identical_sync_is_valid`; `runtime/tree_update.rs::successful_patch_prefix_is_published_after_either_error_policy` | Covered |
| Real runtime geometry/final output | `runtime/tree_actor.rs::coupled_frames_ghost_cleanup_and_final_hits_match_direct_engine`; `runtime/tree_actor/tests/backpressure.rs::blocked_registry_coalesces_to_direct_final_and_ghost_cleanup_output`; rendered scroll clipping/hits/clamping; public actor change traces; D12 delayed own-receipt and D15 failed-terminal-output recovery tests | Covered |
| Retirement/isolation/fast paths | `tests/continuation.rs::retired_query_work_includes_terminal_queries_without_retaining_a_workspace`, `tests.rs::direct_pixels_keep_the_zero_workspace_path`, paint-only authority tests; `renderer/scene_images/tests.rs::source_pins_are_sparse_shared_and_released_with_the_last_scene`, renderer-local generation collision and old-resource replay tests | Covered |

### Changed-path production review

- `layout.rs::FrameAttrsPreparation`: noncloneable preparation validates tree,
  runtime, attempt, constraint/scale and frozen inputs, then preflights before
  presentation writes. Private transaction owns apply/layout/output/commit; release
  failures cannot publish candidate samples. Ghost follow-up keeps cleanup active.
- `runtime/tree_update.rs` uses that publication path in both recompute and refresh
  branches; direct animated layout entry points share it. Cached no-op registry
  responses are not fabricated animation frames. One-shot tree-layout NIFs have
  distinct ownership intent; they are not a second live animation clock.
- `attr_validation.ex`, `attr_codec.ex`, `reconcile.ex`: validated list targets and
  policies, iodata encoding, map-backed policy decode, duplicate/trailing rejection,
  and retained-update compatibility. No per-frame BEAM geometry or new indexed-list
  reconciliation algorithm. Field-count scans are over the bounded attribute set.
- Native change-policy decode/admission and static-bound validation are exercised
  by the existing suite. Native serializers/deserializers use EMRG9; tag84 remains
  unchanged. `test/emerge_skia/macos/host_test.exs` accepts handshake14 and rejects
  12/13. No physical macOS host artifact was available/qualified here.
- Relevant NIF boundaries retain owned binaries for async transport, binary rather
  than byte-list output, existing resource ownership and dirty-scheduler entry points.
  No new NIF/error shape or normal-scheduler animation work was added. Existing
  general NIF/queue/backpressure behavior is not claimed comprehensively audited.
- Existing native receipts cover tree output, not BEAM callback completion,
  renderer installation or display. Removing D23 does not change that contract.

## Validation

Logs: [validation/](validation/), with command order/status in `status.tsv`.
Source-built NIF throughout (`EMERGE_SKIA_BUILD=1`, explicit local Cargo target).

| Check | Result |
|---|---|
| Rust fmt | Pass |
| Standalone cargo test | 1461 unit + 14 integration; 0 failures |
| Clippy, benches/tests + bench-diagnostics, `-D warnings` | Pass |
| Standalone mix test | 520 passed (15 doctests + 505 tests), 9 excluded |
| `./ci-tests.sh all` | 526 Elixir checks, 3 excluded; 1461 + 14 Rust; Dialyzer 0 |
| `mix docs` and internal-docs-enabled build | Pass; 31 documentation screenshots generated |
| `cargo check --tests --features headless-all` | Pass; existing Vulkan unused-method warning, not all-feature strict-Clippy qualification |
| Benchmark runner/summarizer | Existing runner extended with bounded `--closeout` preset, matrix-aware summary and immutable binary copy; Python syntax checked; execution evidence below |
| Diff whitespace | Pass |

No Rust/Elixir code changed after functional validation. Subsequent work is documentation
and benchmark orchestration, not new runtime behavior. Native D23 removal was the only
production change in this closeout. Historical validation archives remain unchanged.

## Performance disclosure

[Full table](performance/table.md), [identity/host](performance/identity.json),
[run log](performance/run.log), [matrix](performance/matrix.json),
[checks](performance/checks.json) and per-process raw `20000-64-*.txt` logs.

AMD Ryzen 9 7950X desktop; 20k nodes / 64 owners (64 looping children under one
finite Content parent for `upward`). Three separate release processes per case,
rotating case order, exclusive performance lock, immutable binary copy. Build
finished before measurements; no concurrent tests/builds. All 18 processes completed
and source/binary hashes stayed unchanged. Each reports 119 ordered warm samples
(2142 total), then finite release and, for mixed/upward, loop cancellation/settle.

| Case | Cold median ms | Warm p50 / p95 ms | Release median [min–max] ms | Settle median ms | Warm RSS MiB |
|---|---:|---:|---:|---:|---:|
| paint | 54.282 | 1.670 / 1.849 | 1.708 [1.647–2.554] | — | 165.7 |
| pixel | 56.937 | 2.121 / 2.348 | 2.107 [1.994–2.138] | — | 165.5 |
| length | 167.401 | 2.422 / 2.604 | 15.888 [14.522–16.952] | — | 316.6 |
| moving | 165.814 | 2.989 / 3.437 | 15.600 [15.292–16.513] | — | 316.4 |
| mixed | 164.543 | 3.978 / 4.561 | 5.164 [4.656–6.116] | 29.455 | 317.0 |
| upward | 200.351 | 71.172 / 86.106 | 76.988 [75.213–96.760] | 32.373 | 378.7 |

Warm columns are medians of per-process quantiles, not pooled distributions. Raw
logs retain maxima (upward worst warm sample 93.619ms) and process RSS, not exact
live heap/allocator/GPU bounds. Native publication timings include synchronous
workspace retirement where it occurs; diagnostics/RSS collection, raster/GPU/BEAM
work and warm-output destruction are outside the warm timing interval. Cold work,
page retention and lifetime disposal are not fully phase-attributed by this probe.
No affinity/frequency isolation or constrained-device cadence claim.

Paint/pixel allocate no dimension workspace. The length and moving workspaces
retire at release; mixed/upward retain real loops until explicit cancellation, then
report no workspace/tracks/receipts. The upward trial records 23,346 cumulative native
queries and one 20k-node model copy; sparse records still entail expensive work.
These observations and existing lifecycle tests are not an arbitrary-duration leak
or exact whole-memory proof.

**Known costs remain significant**, particularly length release, full-model memory,
upward motion and cancellation. Historical baseline03 (~76ms warm/~87ms release
upward) predates D8 and the rebase; it remains historical, not a controlled speedup
comparison. This closes disclosure, not optimization or a desktop ≤12ms/device gate.

Reproduce the bounded preset, using a new output directory:

```sh
./scripts/performance-lock.sh --source-revision snapshot-created-under-lock exclusive \
  python3 plans/artifacts/shared-animation-remaining/performance/run.py NEW_OUTPUT --closeout
python3 plans/artifacts/shared-animation-remaining/performance/summarize.py NEW_OUTPUT
```

The binary also survives outside `/tmp` at the recovery path recorded in
[retained-binary.sha256](performance/retained-binary.sha256). Source archives/hashes
and all raw logs are durable repository artifacts; do not label future builds with
this identity unless their manifest matches.

## Deferred work and closure boundary

[Later plan](../../later-animation-runtime-qualification.md) owns every remaining
P1–P9 category: async input/echo/IME consistency (L1), unapproved pressure policy and
whole-runtime progress (L2), full-model/query/release optimization and exact memory
accounting (L3), broader finite robustness campaigns (L4), physical GPU/platform/
constrained-device cadence and release-artifact qualification (L5).

No physical display/GPU-fault/macOS/constrained-device performance was qualified.
Animation correctness/implementation closure does not imply arbitrary-history proof,
whole-runtime memory bounds, end-to-end input losslessness or target release approval.
