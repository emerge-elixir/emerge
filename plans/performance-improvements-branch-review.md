# performance-improvements branch review

Current review date: 2026-05-04

Current branch head reviewed: `performance-improvements` at `a797532` plus the
branch-fix plan implementation in this worktree.

Comparison base: `main` at merge base
`1ffb362385c184c2794501a3509e199491a3d6d3`.

## Current Summary Verdict

This section supersedes the older 2026-04-26 merge-readiness verdict below. The
current branch is much larger than that earlier review: it is 54 commits ahead
of `main`, with `235 files changed, 43729 insertions(+), 6950 deletions(-)`.
The post-review work added renderer-cache lifecycle improvements, frame-latency
work, scroll viewport culling, renderer stats/code-size cleanup, and additional
benchmarks.

The active branch-fix plan has been implemented in the current worktree:

- benchmark commands now include `--features bench-diagnostics` where required
  by `native/emerge_skia/Cargo.toml`
- `NodeId::from_term_bytes/1` is test-only, so the byte-shaped helper is not
  compiled into release or benchmark code
- mixed tree-actor batches now have regression coverage for animation pulses
  combined with paint patches, active-animation resize, structure upload plus
  registry rebuild, and asset-state invalidation
- the remaining crate-level clippy allowances are explicitly documented as
  test-only fixture-shape exceptions; `too_many_arguments` was removed by
  refactoring the two affected test helpers, and release/benchmark clippy gates
  pass without test allowances
- native diagnostics were classified: hard backend/asset failures stay
  always-on, while queue/backpressure, render debug, hover trace, stats, and
  animation cadence logging remain behind their existing runtime or compile-time
  gates

Current validation:

```text
cargo test --manifest-path native/emerge_skia/Cargo.toml
726 passed, 0 failed

mix test
368 tests, 13 doctests, 0 failures

cargo clippy --manifest-path native/emerge_skia/Cargo.toml -- -D warnings
passed

cargo clippy --manifest-path native/emerge_skia/Cargo.toml --tests -- -D warnings
passed

cargo clippy --manifest-path native/emerge_skia/Cargo.toml --benches --features bench-diagnostics -- -D warnings
passed

cargo bench --manifest-path native/emerge_skia/Cargo.toml --no-run --features bench-diagnostics
passed
```

Remaining worktree hygiene note:

```text
.codex
image_assets_placeholder.png
menu_alpha.png
```

These files are untracked and are not part of the branch diff. They should be
deleted, ignored, or intentionally committed before final merge, but they were
left untouched here because they may be user artifacts.

## Historical 2026-04-26 Review

Date: 2026-04-26

Last revisited: 2026-04-26, after the merge-readiness implementation and
follow-up demo regression fixes in this worktree.

Comparison base: `main` / `origin/main` at `1ffb362385c184c2794501a3509e199491a3d6d3`

Originally reviewed branch: `performance-improvements` at `c3b61ae`

Revisited worktree: `performance-improvements` at `182df18` plus local
review-fix and merge-readiness changes.

## Summary verdict

The branch is merge-ready from this review's perspective.

The original correctness blocker is addressed in the current worktree. Mixed
keyed insert/remove child and nearby updates now emit explicit final ordering,
the failing shapes and broader optimized patch streams have native roundtrip
regression tests, and detached nearby layout-cache restore is scoped to
attachment context.

The remaining review concerns have been closed: full CI is green, retained
layout benchmark smokes have recorded cache counters, topology/registry/
animation-only watch-list paths have targeted hardening, and checked-in
benchmark fixture policy is documented.

Two later manual-demo regressions were also fixed and covered before merging:
`animate_exit` ghosts now remain in active layout with their cloned subtree
topology intact until pruning, and focused single-line text inputs suppress the
follow-up Enter text commit so app-driven clears such as todo creation remain
authoritative.

The broad architecture is coherent: the branch moves the renderer toward retained native tree identity, structured invalidation, layout/render/registry cache reuse, and benchmark coverage. The Rust-side cache design is much more explicit than the previous all-refresh path, and the test volume around layout cache behavior is substantial.

The prior blocking issue was in the Elixir diff optimizer: keyed insert/remove-only updates could emit an insert at a final-list index and then remove an old sibling later. Native patch application applied that stream literally, so the resulting child or nearby order could diverge from the full tree. That issue is now fixed in this worktree, but the review still recommends broadening post-apply equivalence coverage for future optimizer changes.

## Branch shape

- 41 commits ahead of `main` at the revisited `HEAD`.
- 217 files changed.
- `22519` insertions and `4864` deletions in `git diff --stat main...HEAD`.
- Large fixture addition under `bench/fixtures/`.
- Main code changes are concentrated in:
  - Elixir reconciliation, patching, serialization, and numeric node ids.
  - Native retained tree topology, invalidation, layout cache, render cache, registry cache.
  - Native benchmark harnesses and cache regression tests.
  - Planning/docs under `plans/` and `guides/internals/`.

## Verification run

Original standard suites passed before the review fixes:

```text
cargo test
653 passed, 0 failed

mix test
364 tests, 13 doctests, 0 failures
```

Additional ad hoc checks found a failure not covered by the standard suites:

```text
old children: [a, b, c, d]
new children: [a, c, d, x]

patches:
[
  {:insert_subtree, 1, 3, x_subtree},
  {:remove, 4}
]

native patch roundtrip == expected full tree: false
```

The same class reproduces for nearby mounts:

```text
old nearby: [a, b, c, d]
new nearby: [a, c, d, x]

patches:
[
  {:insert_nearby_subtree, host_id, 3, :in_front, x_subtree},
  {:remove, old_b_id}
]

native patch roundtrip == expected full tree: false
```

Revisit verification after implementing the fixes:

```text
mix test test/emerge/patch_test.exs
29 tests, 0 failures

cargo test test_reinserted_nearby_subtree -- --nocapture
3 tests, 0 failures

cargo test --manifest-path native/emerge_skia/Cargo.toml
658 passed, 0 failed

mix test
366 tests, 13 doctests, 0 failures

git diff --check
clean
```

Final merge-readiness verification:

```text
./ci-tests.sh all
passed

mix test
366 tests, 13 doctests, 0 failures

cargo test --release --manifest-path native/emerge_skia/Cargo.toml
662 passed, 0 failed

mix dialyzer
Total errors: 0
```

Focused hardening checks also passed:

```text
mix test test/emerge/patch_test.exs
29 tests, 0 failures

cargo test test_topology_links_stay_consistent_across_representative_mutation_batch
1 passed

cargo test publish_layout_output_preserves_cached_registry_when_output_is_clean
1 passed

cargo test inherited_text_animation_refresh_matches_uncached_render
2 passed
```

Post-demo regression verification:

```text
cargo test --manifest-path native/emerge_skia/Cargo.toml
666 passed, 0 failed

mix test
366 tests, 13 doctests, 0 failures

cargo clippy --manifest-path native/emerge_skia/Cargo.toml -- -D warnings
passed

git diff --check
clean
```

## Findings

### 1. Resolved blocker: keyed insert/remove-only patches could corrupt final child and nearby ordering

Files:

- `lib/emerge/engine/reconcile.ex`
- `native/emerge_skia/src/tree/patch.rs`
- `test/emerge/patch_test.exs`

Status after revisit:

- Fixed in `lib/emerge/engine/reconcile.ex` by emitting explicit final ordering
  when a single sibling group has both inserted and removed ids.
- Covered by native roundtrip tests in `test/emerge/patch_test.exs` for the
  failing child and nearby shapes.
- The old patch-shape expectation that skipped `set_children` for the failing
  mixed edit is replaced with a final-ordering assertion.

The optimizer intentionally skips `set_children` or `set_nearby_mounts` when inserted/removed nodes do not reorder the surviving old nodes. That is valid only if the insert indexes are interpreted against the final list or are adjusted for removals that have not happened yet.

The reviewed stream was not adjusted that way.

Original relevant Elixir paths:

- Keyed child inserts use the new-list index: `do_reconcile_children_keyed/9`, around `lib/emerge/engine/reconcile.ex:198-210`.
- Keyed nearby inserts do the same: `do_reconcile_nearby_keyed/9`, around `lib/emerge/engine/reconcile.ex:403-415`.
- Removed old children/nearby mounts are prepended into the reversed patch list and then the whole list is reversed, so inserts can be emitted before removes: `prepend_removed_children/3` and `prepend_removed_nearby/3`, around `lib/emerge/engine/reconcile.ex:630`.
- In the reviewed code, `maybe_set_children/3` and
  `maybe_set_nearby_mounts/3` skipped explicit final ordering when survivors
  kept the same relative order.

Relevant native application paths:

- `InsertSubtree` reads current live children and inserts at the provided index before later patches run: `native/emerge_skia/src/tree/patch.rs:371-379`.
- `InsertNearbySubtree` does the same for current live nearby mounts: `native/emerge_skia/src/tree/patch.rs:425-440`.

Concrete child example:

```elixir
layout1 = [a, b, c, d]
layout2 = [a, c, d, x]
```

The diff emits:

```elixir
[
  {:insert_subtree, parent_id, 3, x},
  {:remove, b_id}
]
```

Native apply does:

```text
[a, b, c, d]
insert x at index 3 -> [a, b, c, x, d]
remove b -> [a, c, x, d]
```

Expected:

```text
[a, c, d, x]
```

This was a correctness issue in normal UI updates, not just a benchmark artifact. Any keyed list or nearby mount update with both deletion before an insertion point and insertion after that deletion could produce a stale sibling order.

Implemented fix:

- Conservative fix: emit `set_children` / `set_nearby_mounts` whenever a sibling update contains both inserted ids and removed ids.
- Native roundtrip tests now cover the known failing scenarios.

Final hardening:

- The native roundtrip helper is now used across the optimized patch-shape tests
  so future optimized streams are checked by post-apply equivalence, not only by
  local patch assertions.

### 2. Resolved high risk: detached nearby layout cache was keyed by subtree shape, not attachment constraint

Files:

- `native/emerge_skia/src/tree/element.rs`
- `native/emerge_skia/src/tree/patch.rs`
- `native/emerge_skia/src/tree/layout/tests/cache.rs`

Status after revisit:

- Fixed in `native/emerge_skia/src/tree/element.rs` by adding attachment
  context to detached layout-cache lookup: host id, slot, host frame, subtree
  signature, and scale.
- Covered in `native/emerge_skia/src/tree/layout/tests/cache.rs` by changed-slot
  and changed-host tests that must take `Resolve` and match uncached layout.
- Existing same-host/same-slot detached cache reuse still passes.

The detached layout cache stores layout state for removed nearby subtrees using:

- subtree signature,
- scale bits,
- cloned `NodeLayoutState`s.

In the reviewed code, the signature excluded the old host, old slot, old host frame, and nearby constraint. Restore happens in `InsertNearbySubtree`, and a restored subtree can downgrade invalidation to `Paint` / `Registry`, allowing refresh without layout.

Relevant paths:

- Store/restore cache: `native/emerge_skia/src/tree/element.rs:1201-1294`.
- Restore and skip-layout decision: `native/emerge_skia/src/tree/patch.rs:441-459`.
- Current test coverage validates same-host/same-slot reinsert reuse: `native/emerge_skia/src/tree/layout/tests/cache.rs:1828`.

This is correct for the intended hover/show-hide case where the same nearby subtree is hidden and restored on the same host with the same slot and stable host frame. The added context key prevents same-shaped nearby content reinserted under a different host, different slot, or host frame with different dimensions from restoring stale absolute frames.

Final hardening:

- Keep future detached-cache optimizations paired with cached-vs-uncached layout
  tests before allowing refresh-only scheduling.

### 3. Resolved medium: optimized patch tests now have broader post-apply equivalence

The new Elixir patch tests use the stronger invariant:

```text
upload old full tree
apply generated patch stream
compare native tree to new full tree
```

This would have caught finding 1. The helper now covers keyed insert/remove,
keyed reorder, insertion-without-final-ordering cases, nearby slot changes,
nearby insert/remove, nearby reorder, no-op, and attr-only small patch streams.
Patch-shape assertions remain as secondary performance-contract checks.

### 4. Resolved demo regression: animate_exit ghosts were not retained in active layout

Files:

- `native/emerge_skia/src/tree/patch.rs`
- `native/emerge_skia/src/tree/layout/tests/row_column.rs`

Status:

- Fixed by preserving cloned ghost subtree topology when a removed subtree is
  converted into exit-animation ghosts.
- The ghost root remains in active child/nearby topology until the animation
  runtime prunes it, so surviving siblings do not jump early and layout is not
  delayed by a paint-only placeholder.
- Covered by topology and active-layout tests for exit ghosts.

The important correction is architectural: exit ghosts are layout participants,
not render-only escape content. When the old subtree is cloned into ghost ids,
child links, paint-child links, and nearby mounts must all be remapped to those
ghost ids before the live node ids are removed from the production topology.

### 5. Resolved demo regression: todo input clear after Enter could be overwritten

Files:

- `native/emerge_skia/src/events/registry_builder.rs`
- `native/emerge_skia/src/events/runtime.rs`

Status:

- Fixed by arming text-commit suppression for Enter key-down bindings on
  focused single-line text inputs.
- Covered by a listener-builder test and an event-runtime regression that
  simulates the stale-listener-lane sequence: Enter key-down, buffered
  `TextCommit("\n")`, focused tree patch with cleared content, and final
  cleared rebuild.

The focused patch reconciliation was already able to accept a non-pending app
value such as `""`. The missing piece was suppressing the backend's follow-up
Enter text commit while the listener lane was stale after forwarding the
Elixir key-down event.

## Merge-readiness closure

The merge-readiness work is complete and the one-off completed plan has been
folded into this review:

1. Full merge validation passed via `./ci-tests.sh all`.
2. Retained-layout benchmark smokes ran for list, nearby, text-rich,
   scroll-rich, and animation-rich scenarios with cache counters recorded during
   the merge-readiness pass.
3. Optimized Elixir patch tests now assert native post-apply equivalence.
4. Watch-list hardening was added for incremental topology links, clean
   registry refresh output, and animation-only inherited text/nearby refresh.
5. Generated benchmark fixtures stay checked in; policy and regeneration
   command are documented in `bench/README.md`.
6. Post-completion demo regressions for `animate_exit` and todo input Enter
   reset were fixed and validated.

## Subsystem review

### Elixir reconciliation and serialization

The branch replaces binary-ish ids with numeric ids and introduces `Emerge.Engine.NodeId` for 64-bit big-endian wire encoding. That simplifies Rust interop and avoids repeated term serialization for ids.

The `VNode` + `DiffState` model is a good direction. It separates semantic identity from serialized tree shape, preserves ids across keyed updates, rejects mixed keyed/unkeyed sibling sets, and scopes key reuse by parent/nearby host while the public docs require global uniqueness.

The main problem is not the identity model; it is patch stream semantics under insert/remove optimization. The optimizer needs to reason about the native apply order, not just final survivor order.

### Native tree identity and topology

The arena-style `ElementTree` with `NodeId -> NodeIx`, free-list reuse, parent links, topology versions, and dirty propagation is a substantial improvement over full-tree replacement. It gives layout, render, and registry builders a stable local identity model.

Strengths:

- Existing node indexes survive attr changes, child reorders, and nearby slot changes.
- Topology versions give cache keys compact invalidation inputs.
- Runtime state moved out of attrs makes text input, hover, focus, and scrollbar state easier to preserve.
- Ghost exit animation handling is integrated with retained topology instead of requiring full replacement.

Areas hardened during merge readiness:

- The non-test topology is maintained incrementally, while test topology is
  rebuilt lazily. A representative retained mutation batch now recursively
  asserts child and nearby parent links after remove, insert, nearby insert,
  nearby reorder, and subtree paint-child restoration.
- Restore paths that skip layout now have changed-slot and changed-host
  cached-vs-uncached tests.

### Layout caching

The layout cache stack is broad:

- leaf intrinsic measurement cache,
- subtree measurement cache,
- resolve cache,
- dirty/descendant-dirty propagation,
- nearby-specific relayout boundaries,
- detached layout subtree cache for nearby hide/show.

The cache keys include the important normal ingredients: kind, layout-relevant attrs, inherited font context, measured frame, constraints, and compact topology versions. The cache tests are extensive and include cached-vs-uncached comparisons for many layout shapes.

The biggest remaining concern is the boundary between "cache is available" and "refresh-only is safe". A restored cache hit is not automatically proof that the previous absolute frames are valid under the new attachment context.

### Render and registry caching

The render cache avoids seeding during cold dirty refreshes and uses bounded subtree cache storage. The registry cache similarly stores clean subtrees and falls back when escape nearby mounts are present. Both systems have explicit dirty flags and cached-vs-uncached regression tests.

The change to return an empty `event_rebuild` when `event_rebuild_changed == false` is reasonable in the tree actor path because `publish_layout_output/7` checks the flag before replacing or sending registry state. Existing other uses of `LayoutOutput` should continue to use only outputs produced by full `refresh/1` or layout paths, not the clean-registry reuse helper, unless they also honor the flag.

### Animation and refresh scheduling

The latest animation path adds active-node-only frame attr preparation for warmed non-transient animations. This is a pragmatic performance optimization:

- transient enter/exit animations still use full preparation,
- dirty tree updates still use full preparation,
- paint-only active animation samples can refresh without layout,
- measure/resolve-affecting samples still escalate to recompute.

The main risk was subtle inherited context behavior when only active nodes are
prepared. Current render/layout contexts recompute inherited font context during
traversal, and merge-readiness hardening added cached-vs-uncached animation
tests for inherited text style and nearby overlays.

### Benchmarks and fixtures

The branch adds a useful benchmark surface:

- Elixir serialization/diff benchmarks,
- native EMRG decode/encode,
- native patch decode/apply,
- native retained layout benchmarks,
- fixture generation across scenario families.

The generated fixture binaries make benchmark runs reproducible but add a large
amount of repository data. The project now keeps them checked in, with
regeneration policy documented in `bench/README.md`.

## Suggested merge checklist

1. [x] Fix the keyed insert/remove patch ordering bug.
2. [x] Add native roundtrip tests for the previously failing child and nearby
   patch streams.
3. [x] Validate detached nearby layout cache restore under changed slot, host,
   and host constraints.
4. [x] Broaden native roundtrip coverage across the remaining optimized patch
   tests.
5. [x] Run `./ci-tests.sh all` or at least `./ci-tests.sh quality test
   dialyzer`.
6. [x] Run the retained-layout benchmark cases that motivated the branch and
   record before/after numbers in the relevant plan.
7. [x] Decide and document the policy for checked-in benchmark fixture binaries.

## Review conclusion

The branch is ready to merge from this review's perspective. The original
blocker and detached-cache risk are fixed, full CI is green, retained-layout
benchmark smokes support the change, broader optimizer roundtrip coverage is in
place, targeted watch-list hardening was added, and benchmark fixture policy is
documented. Follow-up demo regressions found before merge were also fixed and
covered.
