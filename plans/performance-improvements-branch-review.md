# performance-improvements branch review

Date: 2026-04-26

Comparison base: `main` / `origin/main` at `1ffb362385c184c2794501a3509e199491a3d6d3`

Reviewed branch: `performance-improvements` at `c3b61ae`

## Summary verdict

This branch is not ready to merge as-is.

The broad architecture is coherent: the branch moves the renderer toward retained native tree identity, structured invalidation, layout/render/registry cache reuse, and benchmark coverage. The Rust-side cache design is much more explicit than the previous all-refresh path, and the test volume around layout cache behavior is substantial.

The blocking issue is in the Elixir diff optimizer: keyed insert/remove-only updates can emit an insert at a final-list index and then remove an old sibling later. Native patch application applies that stream literally, so the resulting child or nearby order can diverge from the full tree. I confirmed this with ad hoc native roundtrip checks even though both standard suites pass.

## Branch shape

- 33 commits ahead of `main`.
- 217 files changed.
- `22298` insertions and `4862` deletions in `git diff --stat main...HEAD`.
- Large fixture addition under `bench/fixtures/`.
- Main code changes are concentrated in:
  - Elixir reconciliation, patching, serialization, and numeric node ids.
  - Native retained tree topology, invalidation, layout cache, render cache, registry cache.
  - Native benchmark harnesses and cache regression tests.
  - Planning/docs under `plans/` and `guides/internals/`.

## Verification run

Standard suites pass:

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

I did not run `./ci-tests.sh`, `cargo clippy`, dialyzer, or benchmarks.

## Findings

### 1. Blocker: keyed insert/remove-only patches can corrupt final child and nearby ordering

Files:

- `lib/emerge/engine/reconcile.ex`
- `native/emerge_skia/src/tree/patch.rs`
- `test/emerge/patch_test.exs`

The optimizer intentionally skips `set_children` or `set_nearby_mounts` when inserted/removed nodes do not reorder the surviving old nodes. That is valid only if the insert indexes are interpreted against the final list or are adjusted for removals that have not happened yet.

The current stream is not adjusted that way.

Relevant Elixir paths:

- Keyed child inserts use the new-list index: `do_reconcile_children_keyed/9`, around `lib/emerge/engine/reconcile.ex:198-210`.
- Keyed nearby inserts do the same: `do_reconcile_nearby_keyed/9`, around `lib/emerge/engine/reconcile.ex:403-415`.
- Removed old children/nearby mounts are prepended into the reversed patch list and then the whole list is reversed, so inserts can be emitted before removes: `prepend_removed_children/3` and `prepend_removed_nearby/3`, around `lib/emerge/engine/reconcile.ex:630`.
- `maybe_set_children/3` and `maybe_set_nearby_mounts/3` skip explicit final ordering when survivors keep the same relative order.

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

This is a correctness issue in normal UI updates, not just a benchmark artifact. Any keyed list or nearby mount update with both deletion before an insertion point and insertion after that deletion can produce a stale sibling order.

Recommended fix:

- Conservative fix: emit `set_children` / `set_nearby_mounts` whenever a sibling update contains both inserted ids and removed ids, unless the inserted indexes are adjusted against the pre-remove list.
- More optimized fix: compute patch indexes against the actual application order. If inserts remain before removes, add the count of old removed siblings before each final insertion point. If removes move before inserts, confirm exit-ghost behavior and remove filtering still hold.
- Add native roundtrip tests for optimized diff scenarios, not only patch-shape assertions.

Minimum regression tests:

- Keyed children: `[a, b, c, d] -> [a, c, d, x]`.
- Keyed children with multiple removals before and between insertions.
- Keyed nearby mounts with the same patterns.
- A test helper that applies `DiffState` output through `EmergeSkia.Native.tree_upload_roundtrip/2` and `tree_patch_roundtrip/2`, then compares to full-tree roundtrip.

The current test at `test/emerge/patch_test.exs:532` asserts that no `set_children` is emitted for this shape, but it does not verify the native post-patch tree.

### 2. High risk: detached nearby layout cache is keyed by subtree shape, not attachment constraint

Files:

- `native/emerge_skia/src/tree/element.rs`
- `native/emerge_skia/src/tree/patch.rs`
- `native/emerge_skia/src/tree/layout/tests/cache.rs`

The detached layout cache stores layout state for removed nearby subtrees using:

- subtree signature,
- scale bits,
- cloned `NodeLayoutState`s.

The signature excludes the old host, old slot, old host frame, and nearby constraint. Restore happens in `InsertNearbySubtree`, and a restored subtree can downgrade invalidation to `Paint` / `Registry`, allowing refresh without layout.

Relevant paths:

- Store/restore cache: `native/emerge_skia/src/tree/element.rs:1201-1294`.
- Restore and skip-layout decision: `native/emerge_skia/src/tree/patch.rs:441-459`.
- Current test coverage validates same-host/same-slot reinsert reuse: `native/emerge_skia/src/tree/layout/tests/cache.rs:1828`.

This is probably correct for the intended hover/show-hide case where the same nearby subtree is hidden and restored on the same host with the same slot and stable host frame. It is risky for same-shaped nearby content reinserted under a different host, different slot, or host frame with different dimensions. In those cases, the restored absolute frames and resolve caches may represent the previous attachment constraint.

Recommended follow-up:

- Add tests that remove a nearby subtree, reinsert a same-signature subtree under a different slot and a different-sized host, then compare refresh-only output against an uncached layout.
- Either include attachment constraint data in the detached cache key or degrade restored reinsertions to `Resolve` unless the host id, slot, and host frame/constraint match.

### 3. Medium: optimized patch tests assert patch shape more than post-apply equivalence

The new Elixir patch tests are valuable, but several optimizer tests stop at assertions like "no `set_children` was emitted". For a diff optimizer, the invariant should be stronger:

```text
upload old full tree
apply generated patch stream
compare native tree to new full tree
```

This would have caught finding 1. The existing single stateful roundtrip test uses a demo tree that does not cover the failing list/nearby shapes.

Recommended follow-up:

- Add a reusable assertion helper for `DiffState`-generated patches.
- Use it in every test that claims an optimized patch stream is sufficient.
- Keep patch-shape assertions as secondary checks.

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

Areas to watch:

- The non-test topology is maintained incrementally, while test topology is rebuilt lazily. The test path can mask incremental topology maintenance bugs if a mutation path forgets to update production topology.
- Restore paths that skip layout need extra tests against changed host constraints.

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

The main risk is subtle inherited context behavior when only active nodes are prepared. Current render/layout contexts appear to recompute inherited font context during traversal, so paint-only inherited font changes should flow through parent cache misses. Keep this area covered with cached-vs-uncached animation tests for inherited text style and nearby overlays.

### Benchmarks and fixtures

The branch adds a useful benchmark surface:

- Elixir serialization/diff benchmarks,
- native EMRG decode/encode,
- native patch decode/apply,
- native retained layout benchmarks,
- fixture generation across scenario families.

The generated fixture binaries make benchmark runs reproducible but add a large amount of repository data. That is acceptable if the project wants checked-in perf fixtures; otherwise move generated fixtures to CI artifacts and keep only manifests/seeds.

## Suggested merge checklist

1. Fix the keyed insert/remove patch ordering bug.
2. Add native roundtrip tests for optimized child and nearby patch streams.
3. Validate detached nearby layout cache restore under changed slot, host, and host constraints.
4. Run `./ci-tests.sh all` or at least `./ci-tests.sh quality test dialyzer`.
5. Run at least the retained layout benchmark cases that motivated the branch and record before/after numbers in the relevant plan.

## Review conclusion

The branch is directionally strong and most of the Rust cache architecture looks deliberate. The current blocker is narrow but serious because it can produce an incorrect native tree from a valid Elixir update while all standard tests still pass. Fix that first, then harden the cache restore boundary tests before merging.
