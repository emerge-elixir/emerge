# Active Plan: Relayout / Dependency Boundaries

Last updated: 2026-04-26.

This is the active implementation plan for the next layout-caching slice after
text-flow resolve-cache eligibility. It focuses on making dirty propagation less
global without losing correctness.

Status: implemented and validated for traversal dirtiness plus the first safe
measure boundary (`El`/`None` with child-independent explicit width and height).
Keep this temporary active plan until the user confirms deletion.

## Motivation

Current dirty propagation is intentionally conservative:

```text
changed node -> every ancestor -> root
```

`ElementTree::mark_dirty_ix(...)` marks each ancestor `measure_dirty` for measure
invalidation and always marks each ancestor `resolve_dirty`. This is correct but
expensive when a changed child cannot affect an ancestor's measured size.

The goal is to introduce relayout/dependency boundaries similar in spirit to
Flutter's `parentUsesSize`: if a parent does not depend on a child's measured
size, the child should still be updated, but the parent's own measurement cache
should remain reusable.

Target behavior:

```text
child layout-affecting change
-> dirty the child
-> traverse to the dirty child during layout
-> stop parent measure dirtiness at safe boundaries
-> keep resolve dirtiness where placement/content extents can change
```

Cache stats must remain hit / miss / store only. Boundary observability, if
added, should be separate dirty-propagation stats under the existing unified
stats path, not new cache-bypass counters.

## Current code shape

Relevant files:

- `native/emerge_skia/src/tree/element.rs`
  - `ElementTree::mark_measure_dirty_for_invalidation(...)`
  - `ElementTree::mark_measure_dirty(...)`
  - `ElementTree::mark_resolve_dirty(...)`
  - `ElementTree::mark_dirty_ix(...)`
  - `NodeLayoutState`
- `native/emerge_skia/src/tree/layout.rs`
  - `measure_element(...)`
  - `try_reuse_subtree_measure_cache(...)`
  - `resolve_element(...)`
  - `run_layout_passes(...)`
- `native/emerge_skia/src/tree/patch.rs`
  - patch invalidation calls into dirty marking
- `native/emerge_skia/src/tree/animation.rs`
  - sampled layout effects become ordinary dirty propagation
- tests under `native/emerge_skia/src/tree/layout/tests/cache.rs`

Important caveat: the layout pass starts at the root. Today, if an ancestor's
subtree measurement cache hits, `measure_element(...)` returns without visiting
children. Therefore boundary work cannot simply stop setting `measure_dirty` on
ancestors. It must also preserve a traversal signal so dirty descendants are not
hidden behind a clean ancestor cache.

## Proposed model

Separate two ideas that are currently represented by one boolean:

1. this node's own measured result is invalid
2. some descendant needs measurement traversal

A possible shape:

```rust
measure_dirty: bool,              // this node's own measured result is invalid
measure_descendant_dirty: bool,   // descend during measurement even if this node is reusable
resolve_dirty: bool,
resolve_descendant_dirty: bool,   // optional; only add if needed
```

Names are provisional. The important behavior is:

- a dirty descendant must remain reachable from the root layout pass
- a parent can keep its own measurement cache if it does not depend on the
  changed child measurement
- resolve dirtiness should remain conservative until placement/content dependency
  rules are proven safe

## Slice 1: add traversal dirtiness without changing boundaries — done

Goal: make the state model capable of expressing dirty descendants while keeping
current behavior equivalent.

Tasks:

- add descendant/traversal dirty state to `NodeLayoutState`
- update dirty marking so measure invalidation sets:
  - `measure_dirty` on the changed node
  - traversal dirty on ancestors
  - existing ancestor `measure_dirty` behavior initially preserved
- update `measure_element(...)` so subtree cache reuse is disabled or bypassed
  for traversal when descendants are dirty
- clear descendant/traversal flags at the right point after successful layout
- add tests proving behavior is unchanged for existing patch/animation dirty
  paths

Acceptance:

- all current cache/layout tests still pass
- no dirty descendant is skipped by an ancestor subtree cache
- no cache-stat taxonomy changes

## Slice 2: first safe measure boundary for explicit-size `El`/`None` — done

Goal: stop measure dirtiness at a narrow, easy-to-prove boundary.

Candidate safe boundary:

```text
parent kind: El or None
parent width: child-independent explicit length
parent height: child-independent explicit length
```

A child-independent explicit length means `Px` or `Minimum`/`Maximum` wrappers
whose inner length is also child-independent. Treat `Content`, `None`, `Fill`,
and `FillWeighted` as child-dependent for this slice.

Why start here:

- `El`/`None` measurement resolves to max child size plus insets only for axes
  that ask for content/intrinsic sizing
- if both axes are explicit and child-independent, the parent's own measured
  frame does not need to change when the child text/size changes
- resolve can remain dirty so child placement/alignment/content extent still
  recomputes correctly

Tasks:

- record or compute whether a parent depends on child measurement
- when dirtying from a child, stop setting ancestor `measure_dirty` at the first
  safe boundary
- keep traversal dirty above the boundary so the child is still measured
- keep resolve dirty conservative for the boundary and ancestors
- add targeted cache-stat tests showing fewer subtree measurement misses

Acceptance:

- child text/font/size changes inside an explicit-size `El` measure the child
  but preserve the parent's own measurement cache
- center/right/bottom alignment after child size changes remains correct because
  resolve still runs
- content-sized `El` remains conservative and dirties parent measurement

## Slice 3: expand only with tests — deferred

The first boundary is implemented. Broader cases remain future work and should
be evaluated one at a time:

- fixed-size `Column` / `Row` where the changed axis cannot affect parent
  measured size
- scrollable fixed-size containers where content extents change but parent frame
  does not
- nearby overlays where overlay measurement should not necessarily dirty host
  measurement

Do not broaden to paragraph/text-flow containers until tests cover wrapping,
floats, content extents, and fragment positions.

## Stats / observability

Do not add layout-cache bypass counters.

If benchmark output needs visibility into boundary behavior, add separate dirty
propagation counters through the existing unified stats path, for example:

```text
layout_dirty_measure_boundary_stops
layout_dirty_measure_descendant_traversals
```

Only add counters if they clarify benchmark results. Keep layout-cache counters
as hit / miss / store.

## Suggested tests

Add focused tests before broadening boundaries:

- fixed-size `El` with changed text child:
  - child is measured again
  - parent measured frame is reused
  - parent resolve still runs and alignment remains correct
- content-sized `El` with changed text child remains conservative and dirties
  parent measurement
- dirty descendant below a clean fixed-size ancestor is not hidden by the
  ancestor subtree-measure cache
- layout-affecting animation inside a fixed-size boundary dirties only the
  affected path plus traversal state
- fixed-size parent with scrollbars updates scroll extents correctly after child
  content grows
- nearby overlay changes remain conservative until a separate nearby dependency
  rule is proven safe

Use cache stats for behavior checks, but assert correctness through frames,
scroll extents, and paragraph fragments where relevant.

## Benchmark/smoke direction

After the first boundary is implemented, run a retained-layout smoke that changes
child layout inside fixed-size shells:

```bash
EMERGE_BENCH_SCENARIOS=layout_matrix,text_rich \
EMERGE_BENCH_SIZES=50 \
EMERGE_BENCH_MUTATIONS=text_content,layout_attr \
EMERGE_BENCH_WARMUP=0.1 \
EMERGE_BENCH_TIME=0.1 \
EMERGE_BENCH_MEMORY_TIME=0 \
mix bench.native.retained_layout
```

Look for fewer subtree-measure misses and unchanged or improved resolve-cache
behavior in relevant cases.

Focused smoke after implementation:

```text
layout_matrix_50 warm_cache: resolve_hits=1 resolve_misses=0 subtree_measure_hits=1 subtree_measure_misses=0
text_rich_50 warm_cache:    resolve_hits=1 resolve_misses=0 subtree_measure_hits=1 subtree_measure_misses=0
layout_matrix_50/layout_attr after_patch: subtree_measure_hits=3 subtree_measure_misses=2 resolve_hits=4 resolve_misses=3
layout_matrix_50/text_content after_patch: subtree_measure_hits=2 subtree_measure_misses=3 resolve_hits=4 resolve_misses=3
text_rich_50/layout_attr after_patch: subtree_measure_hits=3 subtree_measure_misses=2 resolve_hits=4 resolve_misses=3
text_rich_50/text_content after_patch: subtree_measure_hits=2 subtree_measure_misses=3 resolve_hits=4 resolve_misses=3
```

Focused unit tests additionally verify the intended boundary behavior directly:
fixed-size `El` parent measurement stays clean while child text/animation changes
are still traversed and resolved.

## Validation

Run:

```bash
cargo fmt --manifest-path native/emerge_skia/Cargo.toml --check
mix format --check-formatted
git diff --check
cargo test --manifest-path native/emerge_skia/Cargo.toml
mix test
cargo test --manifest-path native/emerge_skia/Cargo.toml --benches --no-run
```

Validation status: all commands above passed, plus the focused retained-layout
benchmark smoke in the benchmark section.

## Non-goals

- Do not implement refresh subtree skipping in this slice.
- Do not add cache-bypass taxonomy.
- Do not make a public API for dependency boundaries.
- Do not make layout work source-specific; patching, animation, runtime state,
  and scroll-derived layout dirtiness should share the same invalidation path.

## Completion protocol

When this slice is implemented and validated:

1. Fold durable notes into `layout-caching-roadmap.md`.
2. Fold implementation lessons into `native-tree-implementation-insights.md`.
3. Update `layout-caching-engine-insights.md` if the design meaningfully changes.
4. Update `plans/README.md` next-step ordering.
5. Ask before deleting temporary active plan files.
