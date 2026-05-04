# Active Release Code Bloat Reduction Plan

Last updated: 2026-04-29.

Status: completed.

## Purpose

The renderer instrumentation refactor reduced duplicated traversal risk, but it
was not a meaningful code-bloat reduction. This plan targets larger release-code
shrinkage by removing product-build surface area or collapsing repeated release
structures that this branch introduced.

The goal is not to hide code in another file. A change counts only if it does at
least one of these:

- removes code from the normal product build through a correct `cfg` boundary
- replaces repeated release structures with one shared representation
- reduces a hot-path implementation without adding measurable overhead

## Completion Summary

Accepted changes:

- benchmark-only Rust entry points are removed from default product builds
  behind `cfg(any(test, feature = "bench-diagnostics"))`
- renderer timing stats now use one typed timing metric matrix
- renderer cache admission checks now share one private admission helper

Rejected changes:

- shared registry rebuild traversal, after Criterion showed full-registry
  rebuild regressions
- text input edit resolver helper extraction, after Criterion showed
  event/registry benchmark regressions
- shared Elixir child/nearby reconciliation, after Benchee showed slower
  list and nearby-heavy diff scenarios

Current decision:

- this code-bloat pass is complete
- keep the explicit duplicated hot-path code where benchmarked abstractions
  were slower
- the next reduction attempt should start from a fresh plan with either a
  release-only boundary or a benchmark-proven generated/declarative shape that
  compiles back to the current fast paths

## Rust Reuse Research Pass

Status: completed.

Sources checked:

- Rust Book, generics chapter: start by extracting duplicated function bodies,
  then use generics only when the remaining difference is type shape.
- Rust Book, monomorphization section: generics have no runtime cost because
  Rust specializes generic code per concrete type, but that also means generic
  helpers can create multiple compiled copies.
- Rust Reference, trait objects: trait objects use a vtable and virtual
  dispatch at runtime; they can reduce monomorphized code shape, but they are
  not free and should not be introduced in hot loops without benchmarks.
- Rust Reference, conditional compilation and Cargo features: `cfg` and Cargo
  features are the correct tools for removing benchmark/test-only code from
  product builds.
- Rust Reference, macros by example: `macro_rules!` repetition is appropriate
  for source-level field lists and repeated item definitions, but macro
  expansion still emits code and should not be treated as a runtime or binary
  shrink mechanism by itself.

Project-specific conclusions:

- Prefer private non-generic helpers for exact repeated logic in hot paths.
  This is why `try_admit_clean_subtree_store` was accepted.
- Prefer finite enums and typed arrays for repeated metric families. This is
  why `RendererTimingMetric` was accepted.
- Prefer `cfg(any(test, feature = "bench-diagnostics"))` for benchmark-only
  APIs. This is already implemented and is the cleanest actual release-code
  reduction.
- Avoid trait-object or closure-policy walkers in traversal hot paths unless a
  benchmark proves neutrality. The registry rebuild traversal experiment
  failed this gate.
- Use macros only when the repetition is a declaration list or stat field list
  where the expanded code would have been identical anyway. Do not use macros
  to hide complex control flow.

## Current Best Targets

### 1. Benchmark-Only Release Entry Points

Status: implemented and validated.

Several `*_for_benchmark` helpers are currently public release functions:

```text
native/emerge_skia/src/tree/layout.rs
native/emerge_skia/src/events/registry_builder.rs
native/emerge_skia/src/renderer.rs
```

These exist for Criterion comparisons such as cached vs uncached layout,
registry rebuild, and render traversal. They are not engine API and should not
be part of the default product build.

Expected fix:

- gate benchmark-only helpers with `#[cfg(any(test, feature = "bench-diagnostics"))]`
- make layout and renderer benchmarks explicitly require or run with
  `bench-diagnostics`
- keep tests that call these helpers working through `cfg(test)`

Why this is a real reduction:

- it removes benchmark-only APIs from the default release crate
- it avoids normal product builds carrying uncached comparison paths whose only
  caller is a benchmark

Validation:

```bash
cargo test --manifest-path native/emerge_skia/Cargo.toml
cargo clippy --manifest-path native/emerge_skia/Cargo.toml --benches --features bench-diagnostics -- -D warnings
cargo bench --manifest-path native/emerge_skia/Cargo.toml --no-run --features bench-diagnostics
mix test
```

Implementation notes:

- gated benchmark-only helpers in layout, registry builder, and renderer code
- marked the `layout` and `renderer` Criterion bench targets as requiring
  `bench-diagnostics`
- normal product builds no longer compile these uncached comparison entry
  points unless tests or benchmark diagnostics explicitly request them

Validation result:

- `cargo check --manifest-path native/emerge_skia/Cargo.toml`: passed
- `cargo check --manifest-path native/emerge_skia/Cargo.toml --features bench-diagnostics`: passed
- `cargo test --manifest-path native/emerge_skia/Cargo.toml`: passed
- `cargo clippy --manifest-path native/emerge_skia/Cargo.toml --benches --features bench-diagnostics -- -D warnings`: passed
- `cargo bench --manifest-path native/emerge_skia/Cargo.toml --no-run --features bench-diagnostics`: passed
- `mix test`: passed

### 2. Stats Timing Matrix Refactor

Status: implemented and validated.

`native/emerge_skia/src/stats.rs` repeats the same timing metric list across:

- `RendererStatsSnapshot`
- `RendererStatsWindow`
- `RendererStatsWindow::new`
- `RendererStatsWindow::snapshot`
- `RendererStatsCollector::record_*`
- renderer stats log formatting
- NIF map conversion in `native/emerge_skia/src/lib.rs`

This is the strongest release-code refactor target. The correct shape is a
single timing metric model:

```rust
enum RendererTimingMetric {
    Render,
    RenderDraw,
    RenderFlush,
    RenderGpuFlush,
    RenderSubmit,
    PresentSubmit,
    Pipeline,
    PipelineSubmitToTreeStart,
    PipelineTree,
    PipelineRenderQueue,
    PipelineSubmitToSwap,
    PipelineSwapToFrameCallback,
    Layout,
    Refresh,
    EventResolve,
    PatchTreeProcess,
}
```

Then store timing windows and snapshots in a typed fixed-size collection, with
the NIF map as the only place that expands metrics into the current external
field names.

Expected fix:

- introduce `RendererTimingMetric` plus a small typed collection
- keep the external NIF map shape unchanged
- make stats log formatting iterate metric descriptors instead of repeating the
  same `format_duration_stat_line` calls
- keep public collector methods as small wrappers so call sites stay readable

Validation:

```bash
cargo test --manifest-path native/emerge_skia/Cargo.toml stats
cargo test --manifest-path native/emerge_skia/Cargo.toml
cargo clippy --manifest-path native/emerge_skia/Cargo.toml --benches --features bench-diagnostics -- -D warnings
mix test
```

Benchmark requirement:

- run an engine/demo benchmark with stats enabled before and after if we change
  the per-record path
- if timing record overhead regresses, keep the existing explicit fields and
  only apply the log/NIF formatting cleanup

Implementation notes:

- added a dedicated Criterion benchmark for stats collector timing overhead
- replaced repeated timing fields with `RendererTimingMetric` plus typed timing
  window/snapshot collections
- kept public collector methods and the NIF timing map field names unchanged
- stats log formatting now iterates metric descriptors instead of spelling out
  every timing line

Benchmark result:

```text
native/stats/collector/record_single_timing: improved about 5.1%
native/stats/collector/record_pipeline_sequence: within noise threshold, about 1.3% faster
native/stats/collector/snapshot_populated: improved about 1.4%
```

Release-code result:

- `native/emerge_skia/src/stats.rs` default-release code drops by about 25
  lines after excluding `cfg(test)`.
- `native/emerge_skia/src/lib.rs` is neutral after the NIF timing conversion
  helper.
- This improves the model and removes repeated timing-field plumbing, but it is
  still not the large code-bloat reduction target.

Validation result:

- `cargo test --manifest-path native/emerge_skia/Cargo.toml stats`: passed
- `cargo bench --manifest-path native/emerge_skia/Cargo.toml --bench stats -- --warm-up-time 0.2 --measurement-time 0.5 --sample-size 20 --baseline stats_timing_matrix_before`: passed benchmark gate
- `cargo test --manifest-path native/emerge_skia/Cargo.toml`: passed
- `cargo clippy --manifest-path native/emerge_skia/Cargo.toml --benches --features bench-diagnostics -- -D warnings`: passed
- `cargo bench --manifest-path native/emerge_skia/Cargo.toml --no-run --features bench-diagnostics`: passed
- `mix test`: passed

### 2.5. Registry Rebuild Traversal Refactor

Status: rejected after benchmark.

After implementing the stats timing matrix, the best non-gating opportunity is
the duplicated event-registry rebuild traversal in
`native/emerge_skia/src/events/registry_builder.rs`.

Current duplication:

- `accumulate_subtree_rebuild_local`
- `drain_deferred_subtrees`
- `accumulate_subtree_rebuild_local_cached`
- `accumulate_subtree_rebuild_local_cached_uncached`
- `drain_deferred_subtrees_cached`

The cached and uncached paths both:

- resolve the same element state
- accumulate the same element listeners
- visit local nearby mounts
- visit retained children with the same viewport-culling decision
- defer escape nearby mounts with the same scene-context calculation

Expected fix:

- extract one shared traversal that takes a small cache policy object
- keep the uncached path as the simplest policy: no lookup, no store, no budget
- keep the cached path responsible only for cache lookup/store/admission
- preserve the current deferred-subtree semantics and mutable borrow boundaries

Benchmark requirement:

```bash
cargo bench --manifest-path native/emerge_skia/Cargo.toml --bench layout --features bench-diagnostics -- registry --save-baseline registry_rebuild_refactor_before
cargo bench --manifest-path native/emerge_skia/Cargo.toml --bench layout --features bench-diagnostics -- registry --baseline registry_rebuild_refactor_before
```

Acceptance rule:

- cached registry rebuild benchmarks must stay neutral or improve
- uncached registry rebuild benchmarks must stay neutral or improve
- if a trait/policy abstraction makes borrow structure complex or slower, reject
  it and keep the explicit duplication

Investigation result:

- implemented a shared visit walker for local nearby mounts, retained children,
  viewport culling, and escape-nearby deferral
- kept the cached path collecting owned visits only where mutable tree borrowing
  required it
- forced the walker/helper to inline and reran the focused benchmark
- restored the original explicit traversal after the benchmark showed a real
  regression

Benchmark result:

```text
native/registry_refresh_cache_regression/interactive_rich_500/event_attr/full_registry_rebuild:
  shared walker: regressed about 6.9% after forcing inline
  restored explicit traversal: within noise threshold, about 1.1% slower
```

Decision:

- do not merge the shared traversal abstraction
- keep the duplicated cached/uncached traversal until there is a design that
  does not add callback or visit-collection overhead to the full rebuild path
- prefer the explicit hot-path code over a cleaner-looking abstraction that
  loses registry rebuild performance

### 3. Renderer Cache Admission Duplication

Status: implemented and validated.

`RendererCacheManager` repeats the same admission checks in multiple clean
subtree store paths:

- max entry size
- visible-count admission threshold
- per-frame payload budget
- store/admit/rejection stats
- eviction accounting

Expected fix:

- extract a single `try_admit_clean_subtree_store` helper after
  size/visibility/budget checks pass
- route metadata store, direct payload store, and reserved payload store through
  that helper
- keep cache semantics and stats exactly equivalent

Validation:

```bash
cargo test --manifest-path native/emerge_skia/Cargo.toml renderer::tests::test_renderer_cache
cargo bench --manifest-path native/emerge_skia/Cargo.toml --bench renderer --features bench-diagnostics -- renderer_cache --save-baseline cache_admission_before
cargo bench --manifest-path native/emerge_skia/Cargo.toml --bench renderer --features bench-diagnostics -- renderer_cache --baseline cache_admission_before
```

Acceptance rule:

- no renderer-cache benchmark regression
- stats counters must match existing tests

Implementation notes:

- `try_store_clean_subtree_metadata`,
  `try_store_clean_subtree_payload`, and
  `reserve_clean_subtree_payload_store` now share one inlined admission helper
- store/eviction accounting and payload write behavior remain in the existing
  store methods
- no new reservation token was added because the current store path does not
  need to carry additional state after admission; adding one would increase
  surface area without reducing behavior

Benchmark result:

```text
native/renderer/cache_candidates/borders_like_static_siblings/picture_miss_store:
  no change detected, about 0.04% faster

native/renderer/cache_candidates_translated/nerves_animated_counter_move_x/picture_miss_store:
  improved about 2.4%
```

Validation result:

- `cargo test --manifest-path native/emerge_skia/Cargo.toml renderer::tests::clean_subtree_cache`: passed
- focused renderer cache admission benchmarks above: passed benchmark gate

### 4. Elixir Reconcile Children/Nearby Duplication

Status: rejected after benchmark.

`lib/emerge/engine/reconcile.ex` duplicates the keyed and unkeyed reconciliation
flow for regular children and nearby mounts. This is release code and could be
collapsed, but it is BEAM hot-path code, so it is lower priority than the Rust
stats and benchmark-boundary work.

Expected fix only if benchmarks allow it:

- introduce a shared sibling reconciliation primitive that preserves linked-list
  traversal and prepend/reverse construction
- keep separate child/nearby patch construction helpers so the abstraction does
  not obscure behavior

Validation:

```bash
mix test
mix run bench/engine_diff_bench.exs
```

Acceptance rule:

- no regression in keyed reorder, insert/remove, and nearby mount benchmark
  cases
- if callback abstraction slows reconciliation, reject this refactor and keep
  the explicit duplicated code

Investigation result:

- tried one shared sibling reconciliation path for both regular children and
  nearby mounts
- kept list building as prepend/reverse and preserved separate patch shapes for
  `:insert_subtree` and `:insert_nearby_subtree`
- avoided ETS, arrays, and random-access list operations
- focused reconcile tests passed
- the BEAM benchmark showed slower results in several representative scenarios,
  especially list and nearby-heavy cases

Benchmark result:

```text
mix run bench/engine_diff_bench.exs, warmup=0.2s, time=0.5s:
  list_text_500 nearby_slot_change: about 1.04 ms before, about 1.38 ms after
  nearby_rich_500 nearby_reorder: about 1.59 ms before, about 1.80 ms after
  scroll_rich_500 event_attr: about 1.98 ms before, about 2.17 ms after
```

Decision:

- do not merge the shared sibling reconciliation path
- keep the explicit child/nearby functions because the duplicated code is
  faster on the BEAM hot path
- future source-reduction work should target generated/declarative code that
  compiles back to the explicit fast shape, or non-hot release-only code

### 5. Text Input Edit Resolution Duplication

Status: rejected after benchmark.

`native/emerge_skia/src/events/registry_builder.rs` repeats the same text-input
snapshot/action boilerplate across many `TextInputEditRequest` variants:

- get the runtime snapshot or return no actions
- compute a target cursor or content edit
- apply cursor/content mutation
- build `text_runtime_actions` or `content_change_actions`
- optionally write primary selection

This looks like a better fit for Rust's simple function-extraction guidance
than the rejected registry traversal refactor because the repeated code is
local state-machine logic, not a recursive tree walk with borrow-sensitive hot
paths.

Expected fix:

- add small private helpers for:
  - cursor movement edits that return runtime actions plus optional primary
    selection
  - content mutation edits that return content-change actions
- keep each `TextInputEditRequest` arm responsible only for choosing the target
  movement/delete operation
- avoid trait objects, boxed closures, or broad policy objects
- if closures are used, keep them non-capturing or small and benchmark against
  the current implementation

Benchmark requirement:

```bash
cargo bench --manifest-path native/emerge_skia/Cargo.toml --bench layout --features bench-diagnostics -- native/registry_refresh_cache_regression/interactive_rich_500/event_attr --save-baseline text_edit_refactor_before
cargo bench --manifest-path native/emerge_skia/Cargo.toml --bench layout --features bench-diagnostics -- native/registry_refresh_cache_regression/interactive_rich_500/event_attr --baseline text_edit_refactor_before
```

Acceptance rule:

- event/registry benchmarks must stay neutral
- existing text input runtime tests must pass
- if helper extraction makes the match arms harder to audit or slows the
  event path, reject it and keep the explicit arms

Investigation result:

- tried a finite-operation helper shape instead of boxed callbacks or trait
  objects
- represented cursor edits and content edits as private enums so the match arms
  only chose the operation
- added shared resolver helpers for snapshot lookup, mutation application, and
  listener-action construction
- reran the focused text-input tests successfully
- reran the required event/registry benchmark before and after adding `#[inline]`
  to the helper path

Benchmark result:

```text
native/registry_refresh_cache_regression/interactive_rich_500/event_attr:
  finite helper extraction: regressed several event/registry cases
  finite helper extraction with inline hints: still regressed several cases
```

Decision:

- do not merge the text edit helper extraction
- keep the explicit text edit arms because the benchmark gate did not confirm
  the abstraction
- a future attempt needs either a text-edit-specific benchmark that proves the
  edited path improves without harming the registry benchmark, or a simpler
  change that removes code from release builds rather than rearranging hot-path
  source

## Implementation Order

1. Gate benchmark-only Rust entry points behind `bench-diagnostics`.
2. Refactor stats timing metrics into a single typed timing matrix.
3. Reject registry rebuild traversal dedupe after benchmark regression.
4. Refactor renderer cache admission duplication.
5. Reject Elixir child/nearby reconcile dedupe after benchmark regression.
6. Reject text input edit resolution helper extraction after benchmark
   regression.
7. Re-run broad branch code-count comparison.

Broad branch diff check:

```text
git diff --shortstat main --:
  232 files changed, 42739 insertions(+), 6950 deletions(-)

git diff --numstat main -- native/emerge_skia/src lib mix.exs:
  native/lib/mix diff added=25683 deleted=5900

git diff --numstat main -- native/emerge_skia/benches bench test plans:
  tests/benches/plans diff added=16435 deleted=1045
```

This is a broad diff category check, not a `cfg(test)`-stripped release-code
counter. The accepted reductions are the benchmark-only `cfg` boundary, the
stats timing matrix, and the renderer-cache admission helper. The larger
source-level hot-path refactors were rejected because the benchmarks did not
support them.

## Definition Of Done

- default release-code count drops materially, not just `renderer.rs` line count
- current external APIs and stats NIF map shape remain compatible
- all tests pass
- benchmarks that exercise changed hot paths are neutral or improved
- rejected refactors are documented with benchmark results
