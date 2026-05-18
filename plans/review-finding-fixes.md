# Review Finding Fixes

Last updated: 2026-05-18.

Status: completed.

## Context

Follow-up plan for the local-diff review findings drafted during the code-bloat
reduction branch review:

1. Stats schema fields were removed but `StatsSnapshotNif.version` stayed `14`.
2. A dirty-descendant paint-change regression test was weakened to primitive
   counts only.
3. Several layout benchmarks still say `uncached` while running the retained
   cached/clean-registry implementation.

## Goals

- Preserve the retained layout/refresh and paint-layer cache behavior.
- Fix the review findings without reintroducing the deleted stale benchmark
  wrapper/API plumbing unless required for correctness.
- Keep benchmark names honest so future measurements are not misleading.
- Record validation results before closing this plan.

## Fix Plan

### 1. Stats schema compatibility/versioning

- [x] Bump the stats payload schema version from `14` to a new value after the
      renderer-cache stats fields removal.
- [x] Update the Elixir public type/docs or nearby comments so the current stats
      shape and schema version agree.
- [x] Add or update a test that checks the reported stats version/shape, so a
      future stats-map change cannot land under the same version silently.

Implemented as schema version `15`; the removed renderer-cache stats fields stay
removed. `test/tree_test.exs` now checks the version and representative removed
and retained renderer-cache keys.

### 2. Restore meaningful render-output coverage

- [x] Replace the primitive-count-only assertion in
      `test_child_paint_change_bypasses_dirty_parent_moving_paint_layer` with
      `assert_render_scenes_equivalent` or an equivalent pixel-level comparison.
- [x] If the stronger check fails, fix the renderer/layout behavior or the
      scene-normalization helper rather than falling back to summary counts.

The stronger pixel comparison exposed an ordering bug in paint-layer content
splitting: dirty child paint-layer refs were drawn after later clean sibling own
nodes. `split_paint_layer_content_owned` now keeps only the prefix before the
first child paint-layer ref in `own_nodes` and leaves following content in
ordered child refs, preserving render order while keeping the retained
paint-layer cache model.

### 3. Make layout benchmark labels honest

- [x] Audit all remaining `uncached` labels in `native/emerge_skia/benches/layout.rs`.
- [x] For each label that now runs the retained cached/clean-registry path,
      either remove the duplicate benchmark or rename it to describe what it
      actually measures.
- [x] Do not restore the deleted uncached wrapper functions unless a distinct
      uncached baseline is still needed and can be justified.
- [x] Update any plan notes/measurements that refer to the affected benchmark
      names.

Removed duplicate stale `uncached` benchmark cases and renamed the cold render
refresh case to `cold_layout_refresh`. The deleted uncached wrappers remain
deleted.

## Validation

- [x] `git diff --check` — passed.
- [x] `cargo test --manifest-path native/emerge_skia/Cargo.toml` — passed
      (`856 passed`).
- [x] `mix test` — passed (`13 doctests, 379 tests, 0 failures`).
- [x] `cargo check --manifest-path native/emerge_skia/Cargo.toml --benches` —
      passed.
- [x] `./ci-tests.sh all` — passed.

## Completion Notes

Final measurements after this slice:

- Source diff vs `main`: `72 files changed, 26956 insertions(+), 10322 deletions(-)`.
- Release NIF: `native/emerge_skia/target/release/libemerge_skia.so` =
  `17,680,192` bytes.
- Copied NIF: `priv/native/emerge_skia.so` = `17,680,192` bytes.
