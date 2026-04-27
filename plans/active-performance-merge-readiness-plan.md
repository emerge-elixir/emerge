# Completed Plan: Performance Branch Merge Readiness

Last updated: 2026-04-26.

Status: completed; retained as merge-readiness and post-review regression
evidence.

Source review and resolved concrete fixes:
`plans/performance-improvements-branch-review.md`.

## Goal

Close the remaining review concerns on `performance-improvements` after the
ordering and detached-cache fixes, so the branch has enough validation,
benchmark evidence, and documented tradeoffs to merge.

## Scope

In scope:

- Full CI/quality validation.
- Retained-layout benchmark smoke/regression evidence.
- Broader native post-apply roundtrip coverage for optimized Elixir patch
  streams.
- Focused hardening for remaining topology, registry-refresh, and
  animation-refresh watch-list items.
- Policy for generated benchmark fixture binaries.
- Final review-plan updates before merge.

Out of scope:

- New layout-cache optimizations.
- Rewriting the retained tree topology model.
- Moving benchmark infrastructure unless fixture policy requires it.

## Phase 1: Full Validation

Status: completed.

Purpose:

Prove the current worktree still passes the repository merge gate after the
review fixes, before doing benchmark or additional hardening work.

Preflight:

- Check `git status --short` and record that the validation is against the
  current dirty worktree.
- Do not stash, reset, or revert unrelated work before validation.
- Confirm no long-running local server or previous test session is still active.
- If dependencies are missing, run the normal dependency install command first
  and record it.

Primary command:

```bash
./ci-tests.sh all
```

This expands to:

```bash
mix format --check-formatted
mix credo --strict
cargo clippy --manifest-path native/emerge_skia/Cargo.toml -- -D warnings
mix test
cargo test --release --manifest-path native/emerge_skia/Cargo.toml
mix dialyzer
```

Fallback if `all` is too slow, times out, or fails before enough context is
captured:

```bash
./ci-tests.sh quality
./ci-tests.sh test
./ci-tests.sh dialyzer
```

Failure handling:

- If formatting fails, run `mix format` only on the affected files, review the
  diff, then restart Phase 1 validation.
- If Credo, Clippy, tests, or Dialyzer fail, stop Phase 1 and write the failure
  under "Phase 1 Results" with the command, failing check, and first concrete
  fix task.
- If Dialyzer reports a stale PLT, rely on the script's built-in PLT rebuild
  retry before treating it as a real failure.
- Do not move to Phase 2 benchmarks until Phase 1 is green or the failure is
  explicitly accepted as outside this branch.

Results to record:

- exact command run;
- date;
- pass/fail summary;
- notable warnings or environmental caveats;
- whether the top-level checklist item can be checked off.

Acceptance criteria:

- Full CI passes, or every failure is captured with a concrete fix task.
- The final plan records exact commands, dates, and pass/fail summaries.

Phase 1 Results:

- 2026-04-26: first `./ci-tests.sh all` passed `mix format
  --check-formatted`, `mix credo --strict`, `mix test`, and release
  `cargo test`, but failed before completion on:
  - `cargo clippy --manifest-path native/emerge_skia/Cargo.toml -- -D warnings`
    because `NodeId` is now `Copy` and older code still cloned it in many
    places, plus a few style lints.
  - `mix dialyzer` because the recursive reconciliation `MapSet` accumulator
    produced an opaque-type warning.
- Fixes applied:
  - removed `NodeId`/`Option<NodeId>` clone-on-copy sites and related Clippy
    style lints;
  - refactored the new layout/render helper signatures into argument bundles
    instead of allowing `clippy::too_many_arguments`;
  - changed the reconciliation used-id accumulator to a plain map set
    (`%{id => true}`) to avoid Dialyzer opaque-type flow.
- 2026-04-26: final `./ci-tests.sh all` passed:
  - `mix format --check-formatted`
  - `mix credo --strict`
  - `cargo clippy --manifest-path native/emerge_skia/Cargo.toml -- -D warnings`
  - `mix test` (`366 tests`, `13 doctests`, `0 failures`)
  - `cargo test --release --manifest-path native/emerge_skia/Cargo.toml`
    (`662 passed`, `0 failed`)
  - `mix dialyzer` (`Total errors: 0`)

## Phase 2: Benchmark Evidence

Status: completed.

Run short retained-layout benchmark smokes for the scenarios most affected by
this branch: large lists, nearby overlays, text flow, scroll-heavy trees, and
animation-heavy trees.

Suggested smoke commands:

```bash
EMERGE_BENCH_SCENARIOS=list_text \
EMERGE_BENCH_SIZES=500 \
EMERGE_BENCH_MUTATIONS=keyed_reorder,layout_attr,text_content \
EMERGE_BENCH_WARMUP=0.2 \
EMERGE_BENCH_TIME=0.3 \
EMERGE_BENCH_MEMORY_TIME=0 \
mix bench.native.retained_layout
```

```bash
EMERGE_BENCH_SCENARIOS=nearby_rich \
EMERGE_BENCH_SIZES=500 \
EMERGE_BENCH_MUTATIONS=nearby_slot_change,nearby_reorder,paint_attr \
EMERGE_BENCH_WARMUP=0.2 \
EMERGE_BENCH_TIME=0.3 \
EMERGE_BENCH_MEMORY_TIME=0 \
mix bench.native.retained_layout
```

```bash
EMERGE_BENCH_SCENARIOS=text_rich,scroll_rich,animation_rich \
EMERGE_BENCH_SIZES=500 \
EMERGE_BENCH_MUTATIONS=layout_attr,paint_attr,animation_attr \
EMERGE_BENCH_WARMUP=0.2 \
EMERGE_BENCH_TIME=0.3 \
EMERGE_BENCH_MEMORY_TIME=0 \
mix bench.native.retained_layout
```

Acceptance criteria:

- Record the grep-friendly `layout_cache_stats` lines or summarize hit/miss
  counters by scenario.
- Record whether the benchmark supports the merge or reveals regressions.
- If benchmark commands need adjustment, update this plan with the working
  commands.

Phase 2 Results:

- Added `:animation_attr` to `bench/native_retained_layout_bench.exs` retained
  mutation coverage. The planned third command previously generated
  `animation_attr` fixture metadata but silently filtered it out of retained
  benchmark inputs.
- 2026-04-26: ran all three benchmark smoke commands with
  `EMERGE_BENCH_WARMUP=0.2`, `EMERGE_BENCH_TIME=0.3`, and
  `EMERGE_BENCH_MEMORY_TIME=0`.
- `list_text_500` counters supported merge:
  - `keyed_reorder`: subtree `501 hits / 2 misses`, resolve
    `501 hits / 2 misses`
  - `layout_attr`: subtree `3 / 2`, resolve `2 / 3`
  - `text_content`: intrinsic `0 / 1`, subtree `2 / 3`, resolve `2 / 3`
- `nearby_rich_500` counters supported merge:
  - `nearby_reorder`: subtree `803 / 0`, resolve `702 / 0`,
    resolve stores `100`
  - `nearby_slot_change`: subtree `5 / 0`, resolve `3 / 0`,
    resolve stores `1`
  - `paint_attr`: subtree `1 / 0`, resolve `1 / 0`
- `animation_rich_500`, `scroll_rich_500`, and `text_rich_500` counters
  supported merge:
  - `animation_attr`: subtree `3 / 2`; resolve `5 / 2` for
    animation/text-rich and `3 / 2` for scroll-rich
  - `layout_attr`: subtree `3 / 2`; resolve `4 / 3` for animation/text-rich
    and `2 / 3` for scroll-rich
  - `paint_attr`: subtree `1 / 0`, resolve `1 / 0`
- No benchmark smoke revealed a correctness failure or obvious cache-regression
  counter shape.

## Phase 3: Broaden Native Patch Roundtrip Coverage

Status: completed.

The review found that patch-shape tests can pass while native post-apply state
is wrong. The fixed mixed insert/remove cases now use
`diff_state_native_patch_roundtrip/2`; broaden that invariant to the rest of
the optimized patch tests.

Files:

- `test/emerge/patch_test.exs`

Target test groups:

- pure keyed insert / append without `set_children`;
- multiple keyed inserts preserving survivor order;
- pure keyed remove without `set_children`;
- keyed reorder with `set_children`;
- nearby slot change;
- adding/removing keyed nearby without final ordering patches;
- nearby keyed reorder.

Acceptance criteria:

- Every test that asserts an optimized patch stream is sufficient also proves
  native patch roundtrip equals a full upload of the new assigned tree.
- Patch-shape assertions remain as performance-contract checks.
- Focused and full test runs pass.

Phase 3 Results:

- `test/emerge/patch_test.exs` now uses
  `diff_state_native_patch_roundtrip/2` for the optimized keyed child and
  nearby patch-shape tests, plus adjacent attr-only and no-op small patch
  streams.
- Patch-shape assertions remain in place for skipped `set_children`, skipped
  `set_nearby_mounts`, insert/remove-only streams, and final ordering streams.
- 2026-04-26: `mix test test/emerge/patch_test.exs` passed
  (`29 tests`, `0 failures`).

## Phase 4: Harden Remaining Watch-List Paths

Status: completed.

Add tests or write down a concrete audit result for each watch-list item from
the revisited review.

### Incremental Topology

Concern:

The production topology is maintained incrementally, while tests can rebuild
topology lazily. A mutation path that forgets to update production topology can
be hard to catch.

Work:

- Audit every topology mutation path touched by this branch:
  `set_children`, `set_paint_children`, `set_nearby_mounts`, insert, remove,
  ghost attach/remove, root replacement, and arena slot reuse.
- Add focused tests or an invariant helper that compares parent/child/nearby
  links after representative mutation sequences.

Acceptance criteria:

- Parent links, nearby host links, and topology version changes are covered for
  representative retained mutations.
- Any intentionally test-only topology behavior is documented.

Result:

- Added
  `tree::patch::tests::test_topology_links_stay_consistent_across_representative_mutation_batch`.
  The test applies remove, insert subtree, insert nearby subtree, nearby reorder,
  and subtree paint-child restoration in one retained mutation sequence, then
  recursively asserts child and nearby parent links from the root.
- Existing coverage also checks topology version behavior and individual
  retained mutation paths:
  - `topology_versions_bump_for_child_order_changes_but_not_noop_writes`
  - `topology_versions_bump_for_nearby_slot_changes_but_not_noop_writes`
  - `test_set_children_reorder_preserves_existing_child_ixs`
  - `test_set_nearby_mounts_reorder_preserves_existing_mount_ixs`
  - `test_set_nearby_mounts_slot_change_preserves_node_ix`
  - insert/remove/arena slot reuse patch tests in
    `native/emerge_skia/src/tree/patch.rs`

### Clean Registry / Refresh Output

Concern:

Cached registry refresh can return an empty `event_rebuild` when
`event_rebuild_changed == false`. That is safe only when every consumer honors
the flag.

Work:

- Audit `LayoutOutput` consumers.
- Add a regression test that exercises clean-registry reuse and proves the tree
  actor does not drop the previously published event registry.

Acceptance criteria:

- No consumer replaces the active registry payload when
  `event_rebuild_changed == false`.
- Regression coverage exists for the main tree actor path.

Result:

- Audited `LayoutOutput` consumers. The main consumer is
  `runtime/tree_actor.rs::publish_layout_output/7`; it replaces the cached
  rebuild and sends an event-actor registry update only when
  `event_rebuild_changed` is true.
- Added
  `runtime::tree_actor::tests::publish_layout_output_preserves_cached_registry_when_output_is_clean`.
  It proves a clean refresh still publishes render output but does not replace
  the cached registry payload or send an empty registry update.

### Animation-Only Refresh Preparation

Concern:

Animation-only refresh can prepare only active nodes once root geometry exists.
Inherited text styles and nearby overlays need cached-vs-uncached coverage.

Work:

- Add cached-vs-uncached tests for paint-only inherited text style updates
  across animation-only refresh.
- Add a nearby overlay variant where inherited font/paint context affects the
  overlay.

Acceptance criteria:

- Cached refresh output matches uncached render/layout output for those cases,
  or the path escalates to a broader preparation/layout step.

Result:

- Added cached-vs-uncached animation-only refresh tests:
  - `test_paint_only_inherited_text_animation_refresh_matches_uncached_render`
  - `test_paint_only_nearby_inherited_text_animation_refresh_matches_uncached_render`
- Both tests warm layout first, then sample a paint-only inherited font-color
  animation through refresh-only scheduling and assert cached render output and
  layout state match the uncached benchmark path.
- 2026-04-26 focused Rust runs passed for the new topology, tree actor, and
  animation-only refresh tests.

## Phase 5: Benchmark Fixture Policy

Status: completed.

The branch checks in generated benchmark fixture binaries. That can be useful
for reproducible benchmark runs, but it adds repository weight.

Work:

- Decide whether fixture binaries stay tracked.
- If they stay, document the rationale and regeneration command in
  `bench/README.md`.
- If they move out of git, update `.gitignore`, generator docs, and any CI or
  benchmark command that expects them.

Acceptance criteria:

- `bench/README.md` explains the fixture policy.
- The repository does not have unexplained generated binary data.

Phase 5 Results:

- Kept `bench/fixtures/` tracked.
- Documented the rationale, regeneration command (`mix bench.fixtures`), update
  triggers, and valid empty no-op patch binaries in `bench/README.md`.

## Phase 6: Final Review Update

Status: completed.

Update review and plan status once the remaining items are complete.

Files:

- `plans/performance-improvements-branch-review.md`
- `plans/README.md`
- this plan

Acceptance criteria:

- Review verdict says whether the branch is ready to merge.
- Done checklist reflects actual validation and benchmark results.
- `plans/README.md` points to the right active plan, or says no active plan is
  open after completion.

Phase 6 Results:

- Updated this plan with validation, benchmark, and hardening results.
- Updated `plans/performance-improvements-branch-review.md` with the final
  merge-readiness verdict.
- Updated `plans/README.md` to say no active plan remains open.

## Post-Completion Regression Follow-Up

Status: completed.

After the merge-readiness plan was completed, manual checks in
`../emerge_demo` found two regressions that were fixed in the same worktree.
They are recorded here so the completed plan remains accurate as merge
evidence.

### Animate Exit Ghost Layout

Issue:

- `animate_exit` in the todo demo disappeared immediately and delayed sibling
  layout until the animation duration elapsed.
- The failed approach treated the exit ghost as paint-only escape content. That
  was wrong because the ghost must remain part of active layout until it is
  pruned.

Fix:

- Exit ghost creation now preserves cloned subtree topology for production
  builds as well as tests: child links, paint-child links, and nearby mounts are
  remapped to cloned ghost ids.
- Removed nodes with `animate_exit` now leave an active-layout ghost subtree in
  place until the exit animation finishes and pruning removes it.

Coverage:

- `tree::patch::tests::test_remove_with_animate_exit_preserves_ghost_subtree_topology`
- `tree::layout::tests::row_column::test_exit_ghost_stays_in_active_layout_until_pruned`

### Todo Input Enter Reset

Issue:

- Pressing Enter in the todo app input moved the cursor to the front, but the
  text remained visible.
- The focused tree patch carried the cleared app value, but a follow-up text
  commit for Enter could be buffered while the listener registry was stale and
  replay after the reset.

Fix:

- Focused single-line text inputs now arm text-commit suppression for Enter
  key-down bindings, matching the existing suppression model for character keys
  and multiline Enter.
- The event runtime keeps the app-driven reset authoritative when the buffered
  follow-up commit is replayed.

Coverage:

- `events::registry_builder::tests::listeners_for_focused_text_input_enter_key_down_arms_text_commit_suppression`
- `events::runtime::tests::direct_runtime_single_line_enter_binding_suppresses_buffered_text_commit_after_reset`

Validation:

- 2026-04-26: `cargo test --manifest-path native/emerge_skia/Cargo.toml`
  passed (`666 passed`, `0 failed`).
- 2026-04-26: `mix test` passed (`366 tests`, `13 doctests`, `0 failures`).
- 2026-04-26:
  `cargo clippy --manifest-path native/emerge_skia/Cargo.toml -- -D warnings`
  passed.
- 2026-04-26: `git diff --check` passed.

## Done Checklist

- [x] Run `./ci-tests.sh all` or documented fallback CI commands.
- [x] Run retained-layout benchmark smokes and record counters/results.
- [x] Extend native patch roundtrip assertions to remaining optimized patch
      tests.
- [x] Add or document incremental topology hardening.
- [x] Add or document clean-registry refresh-output hardening.
- [x] Add or document animation-only refresh inherited-context hardening.
- [x] Decide and document benchmark fixture policy.
- [x] Update branch review verdict after remaining work.
- [x] Update `plans/README.md` when this plan is complete.
- [x] Record and validate post-completion demo regression fixes.
