# Active Performance Branch Fix Plan

Last updated: 2026-05-04.

Status: implemented. The only item not changed in the worktree is deletion of
untracked local artifacts, because those files are outside the branch diff and
should not be destructively removed without an explicit user decision.

Comparison base: `main` at merge base `1ffb362385c184c2794501a3509e199491a3d6d3`.
Current branch head reviewed: `performance-improvements` at `a797532`.

Working tree note: `.codex`, `image_assets_placeholder.png`, and
`menu_alpha.png` are untracked local artifacts. They are not part of this branch
diff and should be deleted, ignored, or intentionally committed before a merge
readiness pass.

## Branch Delta

This branch is 54 commits ahead of `main`.

`git diff --shortstat main...HEAD`:

```text
235 files changed, 43729 insertions(+), 6950 deletions(-)
```

The large insertion count is mostly intentional benchmark and planning material:

- nine checked-in `bench/fixtures/*_500/` fixture families, each roughly 5.5% of
  changed files by directory share
- new Elixir benchmark generators and native Criterion suites
- native tree/layout/render tests for identity, cache, viewport culling, render
  cache, animation, and event registry behavior
- long-lived plans and investigation documents

The release-engine surface changed in these main areas:

- Elixir reconciliation moved from path-derived ids to monotonic numeric
  runtime node ids and added `next_node_id` plumbing.
- Native tree storage moved to dense `NodeIx`-backed storage with id-to-index
  lookup, retained runtime/layout/lifecycle state, host/parent topology, and
  explicit invalidation levels.
- Patch application now preserves retained layout state, handles keyed reorder
  and nearby topology, and restores selected detached nearby layout state.
- Layout now has intrinsic, subtree-measure, resolve, detached-nearby, refresh,
  and registry reuse paths.
- Renderer output now carries cache candidates, viewport-culling participation,
  split timing diagnostics, renderer-cache payloads, and pipeline timing.
- Wayland presentation now uses callback-paced rendering with nonblocking swap
  support, one-shot static late replacement, and callback-anchored animation
  sampling.
- Renderer stats, slow-frame diagnostics, and animation traces were expanded and
  partly split behind separate runtime flags.
- Benchmarks are now a merge requirement for cache, draw-path, culling, and code
  size changes.

## Current Review Assessment

The branch has substantial coverage and the previous known correctness
regressions have dedicated fixes: keyed mixed edits, detached nearby cache
scope, multi-row `animate_exit` topology, text-input Enter reconciliation,
animation cadence, image placeholder invalidation, renderer-cache lifecycle,
and offscreen scroll culling.

At planning time, the branch was not ready to call merge-ready because the
durable review document was stale and several follow-up hygiene issues were
visible from the current diff. The implementation results below record the
branch-fix pass that closed the code and documentation items.

## Implementation Results

Implemented in this pass:

- `bench/README.md` now includes `--features bench-diagnostics` on the
  feature-gated `layout` and `renderer` Criterion commands, plus the broad
  baseline examples that include those benches.
- `NodeId::from_term_bytes/1` is now compiled only for Rust tests. Release and
  benchmark code use the numeric `NodeId` constructors.
- Mixed tree-actor batch coverage was added for:
  - animation pulse plus paint patch
  - animation pulse plus active-animation resize
  - animation pulse plus structure upload and registry rebuild
  - animation pulse plus asset-state invalidation
- The remaining clippy test allowances are test-only and now have an explicit
  crate-level comment. Removing all of them produced hundreds of test
  fixture-shape warnings, mostly mutable `Attrs::default()` builders. The
  `too_many_arguments` allowance was removed by refactoring the two affected
  test helpers; the retained boundary does not affect release or benchmark
  clippy gates.
- Native diagnostics were classified instead of hidden:
  - hard backend/asset failures remain always-on
  - queue/backpressure and render debug logs are already gated by `log_input`,
    `log_render`, or compile-time hover tracing
  - renderer stats and animation cadence logs remain behind
    `renderer_stats_log` and `renderer_animation_log`

Validation completed:

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

Not changed:

- `.codex`, `image_assets_placeholder.png`, and `menu_alpha.png` remain
  untracked. They are not part of the branch diff.

## Fix Plan

### 1. Refresh Branch Review And Merge-Readiness Docs

Finding:
`plans/performance-improvements-branch-review.md` still describes the branch at
an older state and says the merge-readiness checklist was complete before the
later renderer-cache, frame-latency, viewport-culling, and code-bloat work
landed.

Fix:
Update the branch review so it explicitly supersedes the old `182df18`-era
review, includes the current `a797532` head, and records the current validation
state. The updated review should summarize the post-review commits instead of
claiming stale readiness.

Validation:
After the implementation fixes below, record current results for:

```bash
cargo test --manifest-path native/emerge_skia/Cargo.toml
mix test
cargo clippy --manifest-path native/emerge_skia/Cargo.toml -- -D warnings
cargo bench --manifest-path native/emerge_skia/Cargo.toml --no-run --features bench-diagnostics
```

### 2. Repair Benchmark Documentation After Feature Gating

Finding:
`native/emerge_skia/Cargo.toml` now requires `bench-diagnostics` for the
`layout` and `renderer` Criterion benches, but `bench/README.md` still documents
plain `cargo bench --bench layout` and `cargo bench --bench renderer` commands.
Those commands are now misleading for the two suites used most often by this
branch.

Fix:
Update `bench/README.md` so every `layout` and `renderer` command includes
`--features bench-diagnostics`. Keep `patch`, `emrg`, and `stats` commands
plain unless their required features change. Add a short note explaining that
the feature keeps benchmark-only counters and helpers out of default release
builds.

Validation:

```bash
cargo bench --manifest-path native/emerge_skia/Cargo.toml --bench layout --features bench-diagnostics --no-run
cargo bench --manifest-path native/emerge_skia/Cargo.toml --bench renderer --features bench-diagnostics --no-run
```

### 3. Remove Release Exposure Of Test-Only NodeId Helper

Finding:
`NodeId::from_term_bytes/1` is documented by its panic message as a test helper,
but it is currently compiled into release code. The helper asserts that input is
at most eight bytes. Most call sites are in inline Rust tests, test-support
modules, and render/layout test helpers.

Fix:
Move the helper behind `#[cfg(test)]` or replace it with a test-support builder
that is not present in release builds. Production code should use
`NodeId::from_u64`, `NodeId::from_wire_u64`, and `to_wire_u64` only.

Validation:

```bash
cargo test --manifest-path native/emerge_skia/Cargo.toml
cargo clippy --manifest-path native/emerge_skia/Cargo.toml -- -D warnings
```

### 4. Revisit Broad Test-Only Clippy Allows

Finding:
At planning time, the crate root still had a broad
`#![cfg_attr(test, allow(..., clippy::too_many_arguments)))]` block. Some of
these allowances were covering inline test helper construction, but the branch
standard has been to fix warnings directly instead of allowing them as the
solution.

Fix:
Classify each crate-root test allowance:

- Keep only allowances that are clearly limited to generated/test-shape code and
  document why the local shape is preferable.
- Prefer moving noisy helper builders into dedicated test-support functions over
  keeping broad crate-level suppressions.
- Do not add new `allow(clippy::...)` items as a fix for release or benchmark
  code.

Validation:

```bash
cargo clippy --manifest-path native/emerge_skia/Cargo.toml --tests -- -D warnings
cargo clippy --manifest-path native/emerge_skia/Cargo.toml --benches --features bench-diagnostics -- -D warnings
```

### 5. Add Mixed Animation And Tree-Message Regression Coverage

Finding:
The tree actor now drains all pending tree messages into one flattened batch.
That is good for throughput, but it is a sensitive boundary because animation
pulses, patch uploads, resize, asset-state changes, and interaction state can
now be processed in the same batch. Previous animation bugs were caused by
sampling cadence and stale state, so this should have explicit coverage before
merge readiness is asserted.

Fix:
Add focused tests around the batch boundary:

- pulse plus paint-only patch keeps the pulse sample monotonic and produces a
  refresh/render scene
- pulse plus structure patch does not reuse stale registry/layout output
- pulse plus resize forces recompute and keeps latest animation sample time
  consistent
- asset-ready change invalidates placeholder cache state and does not preserve a
  stale pending-image subtree

If a direct tree-actor test is too coupled to channels, factor the decision
logic into a small pure helper first. Keep the helper production-relevant; do
not add an artificial test-only scheduler.

Validation:

```bash
cargo test --manifest-path native/emerge_skia/Cargo.toml tree_actor
cargo test --manifest-path native/emerge_skia/Cargo.toml animation
```

### 6. Normalize Native Diagnostics Boundaries

Finding:
The branch added useful diagnostics, but there are still direct `eprintln!`
calls across native tree, render, asset, Wayland, DRM, and macOS host paths.
Some are real errors; some are debug-like queue/backpressure logs. This makes it
harder to reason about what `renderer_stats_log`, `renderer_animation_log`,
`log_render`, and `log_input` control.

Fix:
Classify native diagnostics into:

- always-on user-visible backend failures
- opt-in debug or backpressure logs
- renderer stats / animation cadence logs
- compile-time hover traces

Route opt-in logs through existing flags or `native_log` instead of ad hoc
printing. Do not hide hard failures.

Validation:
Run a stats-enabled demo smoke and confirm normal pages do not emit animation
traces unless `renderer_animation_log: true` is set.

Implementation note:
This pass classified the current native diagnostic paths and kept existing hard
failure logs always-on. The runtime flag split is already covered by
`test/emerge_skia/options_test.exs`; no GUI demo smoke was run in this pass.

### 7. Clean Worktree Artifacts Before Final Review

Finding:
The branch worktree currently has untracked local files:

```text
.codex
image_assets_placeholder.png
menu_alpha.png
```

Fix:
Decide whether they are useful review artifacts. If not, delete them after
explicit approval or leave them untracked but call them out in the final merge
review. If they should become documentation assets, add references and commit
them intentionally.

Validation:

```bash
git status --short
```

## Definition Of Done

- The stale branch review is replaced or updated for current `HEAD`.
- Benchmark commands in docs match the current feature-gated Criterion setup.
- Test-only helper code is not compiled into release code unless it has a real
  production use.
- Clippy allows are either removed or locally justified as test-only shape
  exceptions.
- Mixed animation/tree-message regression coverage exists.
- Native diagnostic flags have a clear boundary.
- Worktree artifacts are either resolved or explicitly left untouched as
  untracked user artifacts.
- `cargo test`, `mix test`, clippy with warnings denied, and benchmark no-run
  smoke all pass and are recorded in the review doc.
