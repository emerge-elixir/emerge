# Active Plan: Text-Flow Resolve Cache Reuse

Last updated: 2026-04-26.

This is the active implementation plan for the next layout-caching slice. It
follows the completed origin-agnostic scheduling and targeted layout-affecting
animation invalidation work.

Status: implemented and validated; keep this temporary active plan until the
user confirms deletion.

## Motivation

Current cache counters show measurement caching is healthy, while resolve-cache
misses remain high in text/layout-rich scenes. The next useful slice is to make
more text-flow container kinds safely eligible for coordinate-invariant resolve
cache reuse.

Target behavior:

```text
clean text-flow subtree + same resolve inputs -> resolve cache hit
text/content/constraint/layout-affecting change -> ordinary miss/store
paint-only change -> no layout at all
```

Cache stats should remain hit / miss / store only.

## Current code shape

Relevant files:

- `native/emerge_skia/src/tree/layout.rs`
  - `resolve_element(...)`
  - `resolve_cache_kind_eligible(...)`
  - `can_store_resolve_cache(...)`
  - `try_reuse_resolve_cache(...)`
  - `resolve_multiline_kind(...)`
  - `resolve_wrapped_row_kind(...)`
  - `resolve_text_column_kind(...)`
  - `resolve_paragraph_kind(...)`
- `native/emerge_skia/src/tree/element.rs`
  - `ResolveCache`
  - `ResolveCacheKey`
  - `ResolveExtent`
  - `NodeLayoutState::paragraph_fragments`
- tests under `native/emerge_skia/src/tree/layout/tests/`

Current resolve-cache eligibility is limited to simple kinds:

```rust
Text | TextInput | Image | Video | None | El | Row | Column
```

Currently excluded text-flow kinds:

```text
Multiline
WrappedRow
TextColumn
Paragraph
```

## Implementation order

### Slice 1: make `Multiline` resolve-cache eligible — done

`Multiline` is the lowest-risk candidate. It is leaf-like from a child-layout
perspective, but resolve can produce wrapped text fragments and content extents.

Tasks:

- inspect what `resolve_multiline_kind(...)` mutates
- ensure a cache hit restores all state downstream refresh needs:
  - frame extent
  - content extent
  - paragraph/text fragments if used
  - scroll maxima if relevant
- extend the resolve cache payload if `ResolveExtent` is not enough
- add tests proving repeated resolve hits for unchanged multiline input
- add tests proving width/text/font changes miss and rebuild correctly

Acceptance:

- `Multiline` stores and hits resolve cache when unchanged
- no stale wrapping/fragments after width/content/font changes
- cache hit output matches uncached layout output

### Slice 2: make `TextColumn` resolve-cache eligible — done

`TextColumn` sorts/positions children similarly to a column but has text-flow
semantics in some cases. It should be considered after `Multiline` because it can
contain paragraphs and other text-flow children.

Tasks:

- compare `resolve_text_column_kind(...)` with `resolve_column_kind(...)`
- verify paint-child ordering and content extents are fully restored by existing
  cache hit mechanics
- add tests where a clean text column hits after warm layout
- add tests involving child paragraph/text changes that must miss

Acceptance:

- clean `TextColumn` subtree can hit resolve cache
- child positions, paint order, and scroll extents match uncached layout

### Slice 3: make `WrappedRow` resolve-cache eligible if safe — done

`WrappedRow` has line wrapping and per-line alignment behavior. It can likely be
cached if the key captures child identity/order, measured frame, constraints,
spacing, and alignment.

Tasks:

- inspect `resolve_wrapped_row_kind(...)` and `resolve_wrapped_row_children(...)`
- identify derived state that a cache hit must restore:
  - frame/content extent
  - child frames shifted into the cached relative arrangement
  - paint order
  - scroll maxes, if scrollable
- add cached-vs-uncached tests for common wrapped-row layouts
- test constraints that change wrapping miss and recompute

Acceptance:

- unchanged wrapped rows hit
- width/constraint changes that alter wrapping miss
- mixed alignment/wrapped-line tests still pass

### Slice 4: evaluate `Paragraph` separately — done

`Paragraph` is likely hardest because useful reuse may need cached flow/fragments,
not just an origin-free frame extent. Do not simply add it to
`resolve_cache_kind_eligible(...)` without proving fragment restoration.

Tasks:

- inspect `resolve_paragraph_kind(...)` and paragraph flow helpers
- determine whether a cache hit can safely shift existing fragments, or whether
  `ResolveCache` must store paragraph fragments/child fragment placement
- add tests for floats, inherited font context, wrapping, padding, and fragment
  colors before enabling eligibility

Acceptance:

- `Paragraph` is enabled with tests covering inline fragment storage and
  fragment shifting after a parent alignment change
- inline children owned by paragraph flow do not need independent child resolve
  cache entries for the paragraph cache to be safe

## General implementation guidance

- Add one kind at a time.
- Prefer cached-vs-uncached frame/render-state tests before broad eligibility.
- Do not add bypass counters.
- Keep cache lookup centralized in `resolve_element(...)` and
  `try_reuse_resolve_cache(...)`.
- Extend `ResolveCache` only for state that must be restored on a hit.
- Do not conflate this with refresh subtree skipping.
- Keep normal tests and benchmarks separate.

## Suggested tests

Add focused tests in `native/emerge_skia/src/tree/layout/tests/cache.rs` and/or
existing kind-specific test files:

- `Multiline` warm resolve hit with unchanged constraint
- `Multiline` width/content/font-size change misses and matches uncached
- `TextColumn` warm resolve hit with text children
- `WrappedRow` warm resolve hit with no wrapping change
- `WrappedRow` narrower constraint misses and matches uncached
- paragraph blocker or paragraph cache test, depending on implementation choice

For each enabled kind, assert at least:

```text
resolve_hits > 0
resolve_misses == 0 on unchanged warm layout where appropriate
cached frames == uncached frames
```

## Benchmark/smoke direction

After tests pass, run a retained-layout smoke biased toward text flow:

```bash
EMERGE_BENCH_SCENARIOS=text_rich,layout_matrix \
EMERGE_BENCH_SIZES=50 \
EMERGE_BENCH_MUTATIONS=layout_attr \
EMERGE_BENCH_WARMUP=0.1 \
EMERGE_BENCH_TIME=0.1 \
EMERGE_BENCH_MEMORY_TIME=0 \
mix bench.native.retained_layout
```

Use `layout_cache_stats` lines to confirm resolve misses decrease or resolve hits
increase in relevant scenarios.

Focused smoke after implementation:

```text
layout_cache_stats case=layout_matrix_50 phase=warm_cache resolve_hits=1 resolve_misses=0 resolve_stores=0
layout_cache_stats case=text_rich_50 phase=warm_cache resolve_hits=1 resolve_misses=0 resolve_stores=0
layout_cache_stats case=layout_matrix_50/layout_attr phase=after_patch resolve_hits=4 resolve_misses=3 resolve_stores=3
layout_cache_stats case=text_rich_50/layout_attr phase=after_patch resolve_hits=4 resolve_misses=3 resolve_stores=3
```

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

## Completion protocol

When this slice is implemented and validated:

1. Fold durable notes into `layout-caching-roadmap.md`.
2. Fold implementation lessons into `native-tree-implementation-insights.md`.
3. Update `layout-caching-engine-insights.md` if the final design changes.
4. Update `plans/README.md` next-step ordering.
5. Ask before deleting temporary active plan files.
