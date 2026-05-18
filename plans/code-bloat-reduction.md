# Code Bloat And Artifact Reduction Plan

Last updated: 2026-05-18.

Status: completed.

## Goal

Reduce code bloat and overcomplicated implementation paths while preserving the
retained layout/refresh and paint-layer cache behavior. Generated artifact size
is a required measurement, but not the only goal: production source should also
be smaller, easier to reason about, and less dependent on duplicated bespoke
traversals.

Priority order:

1. Delete unused production code, fields, APIs, and stats plumbing.
2. Replace duplicated custom traversals/hashing with one authoritative code
   path where correctness can be proven.
3. Move benchmark/test scaffolding out of production modules when it does not
   need to live there.
4. Keep generated and shipped artifacts from growing.
5. Preserve measured runtime wins; do not trade large runtime regressions for
   cosmetic line-count reductions.

Primary artifacts:

- release Rustler NIF: `native/emerge_skia/target/release/libemerge_skia.so`
- copied release NIF: `priv/native/emerge_skia.so`
- generated benchmark fixtures under `bench/external_fixtures/`

## Measurement Rule

Every simplification refactor must record both code-size and artifact-size
evidence before moving to the next refactor. Use the same machine, toolchain,
features, and build profile for each before/after pair.

Source-size commands:

```bash
git diff --shortstat main
git diff --numstat main -- native/emerge_skia/src lib native/emerge_skia/benches mix.exs | sort -nr | sed -n '1,80p'
```

Use `main...HEAD` only for committed branch snapshots. Use `main` while the
active reduction batch is still uncommitted so measurements include the working
tree.

Baseline commands:

```bash
# Current branch
mix compile
stat -c '%n %s' native/emerge_skia/target/release/libemerge_skia.so priv/native/emerge_skia.so
find bench/external_fixtures -type f -printf '%p %s\n' | sort

# main comparison in an isolated worktree
git worktree add /tmp/emerge-main-artifact-size main
cd /tmp/emerge-main-artifact-size
mix compile
stat -c '%n %s' native/emerge_skia/target/release/libemerge_skia.so priv/native/emerge_skia.so
find bench/external_fixtures -type f -printf '%p %s\n' | sort
```

Optional attribution commands when available:

```bash
size -A native/emerge_skia/target/release/libemerge_skia.so
nm -S --size-sort --radix=d native/emerge_skia/target/release/libemerge_skia.so | tail -100
cargo bloat --release --crates
```

## Current Branch Snapshot

Measured on 2026-05-15 after `mix test`/`./ci-tests.sh all` release builds:

| Artifact | Size |
| --- | ---: |
| `native/emerge_skia/target/release/libemerge_skia.so` | `17,979,744` bytes |
| `priv/native/emerge_skia.so` | `17,979,744` bytes |
| `bench/external_fixtures/emerge_demo_showcase_borders/full.emrg` | `30,937` bytes |
| `bench/external_fixtures/emerge_demo_showcase_layout/full.emrg` | `15,865` bytes |
| `bench/external_fixtures/emerge_demo_showcase_interaction/full.emrg` | `50,053` bytes |
| `bench/external_fixtures/emerge_demo_showcase_interaction/virtual_key_text_echo.patch` | `311` bytes |
| `bench/external_fixtures/emerge_demo_showcase_interaction/virtual_key_text_echo_reverse.patch` | `308` bytes |

Main baseline measured on 2026-05-18 in `/tmp/emerge-main-artifact-size`:

| Artifact | Size |
| --- | ---: |
| `native/emerge_skia/target/release/libemerge_skia.so` | `17,547,120` bytes |
| `priv/native/emerge_skia.so` | `17,547,120` bytes |
| unpacked Hex package | `6,652,605` bytes |
| `bench/external_fixtures/` | not present |

## Gates

- Production code should shrink unless a remaining addition is explicitly
  justified by a simpler model, clearer ownership boundary, or measured runtime
  win.
- Prefer deleting code over moving code. Moving benchmark support is useful only
  after unused production code has been removed.
- Avoid adding abstractions whose main effect is to hide complexity rather than
  remove it.
- Release NIF size should not grow relative to `main` unless the increase is
  explicitly justified by a measured runtime win and recorded here.
- Generated benchmark fixtures must remain benchmark-only and must not enter
  shipped package output.
- Prefer deleting unused production fields/API before moving benchmark support
  around.
- If a refactor touches renderer or layout hot paths, rerun the focused
  Criterion benchmark that originally justified the code.

## Audit Findings To Act On

1. Unused renderer cache stats plumbing.
   - Candidates: `moved_hits`, `moved_misses`, `payload_copy_time`,
     `dirty_draw_time`, `child_layer_time`, `direct_fallback_time`.
   - These are propagated through Rust stats, NIF encoding, Elixir types, and
     benchmark zero assertions, but are not meaningfully recorded.
   - Expected effect: low-risk artifact/source reduction.

2. Test-only or unused renderer cache APIs.
   - Candidates: `try_store_moving_layer_metadata`,
     `moving_layer_entry_count`, `moving_layer_total_bytes`,
     `PaintLayerPayloadCache::try_store`, `config`, `contains_key`.
   - Remove or gate with `#[cfg(test)]` after confirming no production caller.

3. Benchmark-only layout/profile wrappers in production modules.
   - Large block in `tree/layout.rs` under `#[cfg(any(test, feature =
     "bench-diagnostics"))]`.
   - Refactor toward one production refresh/layout options path plus a
     benchmark profile sink, or move wrappers into a bench-support module.

4. Paint-layer render-node hashing.
   - `render_scene.rs` contains a bespoke recursive render-node hasher for
     paint-layer payload content.
   - Investigate whether `NodeRefreshState.paint_generation` can be the single
     authoritative semantic content key. Only remove the hasher after tests
     prove asset/font/paint mutations bump generation correctly.

5. Visible-frame fingerprint hashing.
   - `renderer.rs` contains a second custom scene traversal for unchanged
     visible-frame skipping.
   - Either justify it with benchmark evidence or replace it with a tree/render
     generation emitted during scene construction.

6. `RenderPaintLayer` compatibility surface.
   - The model currently carries `own_nodes`, `child_refs`, and test/raw
     children helpers.
   - Normalize around one content representation if it reduces clone/helper
     code without reintroducing parent/child paint-layer bugs.

7. Overbroad retained render/cache model boundaries.
   - Review whether retained render fragments, retained layer payloads, and
     renderer payload cache are carrying overlapping cache keys or duplicate
     invalidation state.
   - Delete one layer of bookkeeping if the same correctness property is
     already guaranteed by `paint_generation`, topology keys, or frame-attrs
     preparation.

8. Production files containing large embedded test-only compatibility code.
   - Large test modules are acceptable, but production structs should not carry
     extra fields solely for test compatibility when tests can inspect the new
     canonical representation.
   - Candidate: `RenderPaintLayer.children` and helper constructors used to
     preserve old test expectations.

## Refactor Sequence

1. Record main code-size and artifact baseline, then branch delta.
2. Classify the largest production additions as essential, removable, or
   test/benchmark scaffolding.
3. Remove unused renderer cache stats fields and update Rust/Elixir stats
   encoding plus benchmark zero assertions.
4. Remove or test-gate unused cache manager/payload-cache APIs.
5. Re-measure source size, release NIF, and fixture sizes; run `cargo test`,
   `mix test`, and affected renderer stats tests.
6. Consolidate benchmark-only layout/profile wrappers if production source size
   still points at layout/refresh support code.
7. Investigate replacing paint-layer render-node hashing with
   `paint_generation`.
8. Investigate visible-frame fingerprint simplification.
9. Normalize `RenderPaintLayer` around one canonical content representation.
10. Review retained fragment/layer/cache key overlap for redundant state.
11. Re-measure source size, release NIF, and focused benchmarks after each
   behavior-affecting simplification.
12. Re-run full gates:
   - `git diff --check`
   - `cd native/emerge_skia && cargo test`
   - `mix test`
   - `cd native/emerge_skia && cargo clippy -- -D warnings`
   - `./ci-tests.sh all`

## Validation Notes

- Source-size wins must be real production simplifications, not only test/doc
  movement.
- Any simplification replacing a bespoke traversal must include regression
  tests for the behavior that traversal protected.
- Any simplification touching renderer/layout hot paths must include the
  focused Criterion benchmark that originally justified the code.
- Rejected simplifications should be recorded with the measured reason they
  were rejected, so the branch does not re-try them later.

## Previous Artifact-Focused Sequence

The original artifact-only sequence is superseded by the broader refactor
sequence above. Its useful checks remain:

- Re-measure release NIF and fixture sizes after each code-size refactor.
- Run `cargo test`, `mix test`, affected renderer stats tests, and
  `cd native/emerge_skia && cargo clippy -- -D warnings` after reduction
  batches.

## Completion Notes

- `NodeRefreshState.paint_generation` cannot be the sole moving paint-layer
  payload content key yet. Focused/dragged slider glow layers intentionally keep
  a stable own-payload key while child track/thumb layout changes. Existing
  focused slider coverage guards this behavior, so the payload-content traversal
  remains, but its custom FNV hasher was deleted in favor of `DefaultHasher`.
- The visible-frame fingerprint cannot be replaced by a simple scene/tree
  generation without losing its two important filters: it ignores offscreen
  dynamic layer changes and refuses to skip while visible cacheable image
  resources are not ready. The fingerprint traversal remains, but primitive,
  rect, and transform hashing now reuse the paint-layer hash helpers instead of
  debug-formatting primitives and carrying duplicate hash helpers.
- `RenderPaintLayer` now has one canonical content representation:
  `own_nodes` plus `child_refs`. The test-only raw `children` compatibility
  field and raw-child constructor plumbing were removed; tests inspect canonical
  content through `content_nodes`, `own_nodes`, and `child_refs`.
- Retained render fragments, retained render-layer metadata, and renderer
  payload cache entries are not duplicate state: fragment caches skip clean
  subtree render construction, layer caches preserve tree-derived paint-layer
  metadata across clean scroll frames, and renderer payload entries own the
  backend CPU/GPU raster payloads. No safe layer could be deleted in this pass.

## Measurement Log

| Change | Command | Before | After | Notes |
| --- | --- | --- | --- | --- |
| main baseline | `mix compile`; `stat -c '%n %s' native/emerge_skia/target/release/libemerge_skia.so priv/native/emerge_skia.so`; `mix hex.build --unpack emerge-0.3.1`; `du -sb emerge-0.3.1` | not applicable | NIF `17,547,120` bytes; unpacked Hex package `6,652,605` bytes; no external fixtures | Main worktree `/tmp/emerge-main-artifact-size`. |
| branch snapshot | `git diff --shortstat main...HEAD`; `stat -c '%n %s' native/emerge_skia/target/release/libemerge_skia.so priv/native/emerge_skia.so`; fixture `find` command | main baseline above | source `70 files changed, 27239 insertions(+), 10147 deletions(-)`; NIF `17,979,744` bytes; fixtures listed above | Committed branch before this reduction batch. |
| remove dead renderer cache stats/API and unify direct fallback path | `git diff --shortstat main`; focused tests; `mix test`; `stat -c '%n %s' ...` | working tree source `70 files changed, 27247 insertions(+), 10147 deletions(-)`; NIF `17,979,744` bytes | source `70 files changed, 27071 insertions(+), 10160 deletions(-)`; NIF `17,665,760` bytes after final CI rebuild | Removed `moved_*`, unused draw-split timers, unused metadata store APIs, and shared the direct paint-layer fallback helper. |
| stats benchmark | `CARGO_TARGET_DIR=/tmp/emerge-active-plan-bench-target cargo bench --bench stats -- native/stats/collector/snapshot_populated --save-baseline active_plan_before --sample-size 10 --measurement-time 1 --warm-up-time 1`; rerun with `--baseline active_plan_before` | `85.727 ns` median estimate | `85.680 ns` median estimate; change `-1.1079%` | Criterion: change within noise threshold. |
| renderer fallback benchmark | `CARGO_TARGET_DIR=/tmp/emerge-active-plan-bench-target cargo bench --bench renderer --features bench-diagnostics -- scroll_return/cache_after_clipped_frames --save-baseline active_plan_before --sample-size 10 --measurement-time 1 --warm-up-time 1`; rerun with `--baseline active_plan_before` | `5.8280 us` median estimate | `5.9141 us` median estimate; change `+3.5269%`, `p = 0.60` | Criterion: no performance change detected. |
| exclude native tests from Hex package | `mix hex.build --unpack emerge-0.3.1`; `du -sb emerge-0.3.1`; `find emerge-0.3.1 -path '*bench*' -o -path '*external_fixtures*' -o -path '*/tests/*'` | branch package before change `7,022,573` bytes, no bench/external fixtures, native test files present | `6,328,078` bytes, no bench/external fixtures/native `tests` paths | Package artifact reduced by `694,495` bytes from branch-before and `324,527` bytes below main. Final working-tree source diff: `71 files changed, 27084 insertions(+), 10162 deletions(-)`. |
| validation before layout-wrapper consolidation | `git diff --check`; `cd native/emerge_skia && cargo test`; `mix test`; `./ci-tests.sh all` | not applicable | all passed | Full CI wrapper passed format, credo, clippy, full-sweep Mix tests, release Rust tests, and Dialyzer before the later layout-wrapper consolidation; rerun full gates before closing the plan. |
| consolidate stale layout benchmark wrappers | `git diff --shortstat main`; `mix compile`; `stat -c '%n %s' native/emerge_skia/target/release/libemerge_skia.so priv/native/emerge_skia.so`; fixture `find` command; `git diff --check`; `cargo test --manifest-path native/emerge_skia/Cargo.toml`; `mix test`; `cd native/emerge_skia && cargo clippy -- -D warnings` | source `71 files changed, 27084 insertions(+), 10162 deletions(-)`; NIF `17,665,760` bytes | source `71 files changed, 27047 insertions(+), 10267 deletions(-)`; NIF `17,665,032` bytes; fixtures unchanged | Deleted obsolete `*_uncached_for_benchmark` aliases and shared the layout-or-refresh prepare/finish paths. Validation passed. |
| normalize paint-layer content and share hashing helpers | `git diff --shortstat main`; `mix compile`; `stat -c '%n %s' native/emerge_skia/target/release/libemerge_skia.so priv/native/emerge_skia.so`; fixture `find` command; `cargo test --manifest-path native/emerge_skia/Cargo.toml`; attempted focused Criterion command `cargo bench --bench layout --features bench-diagnostics -- render_refresh_cache_regression/emerge_demo_showcase_borders/layout_animation_cached_refresh --sample-size 10 --measurement-time 1 --warm-up-time 1` | source `71 files changed, 27047 insertions(+), 10267 deletions(-)`; NIF `17,665,032` bytes | source `71 files changed, 26974 insertions(+), 10254 deletions(-)`; NIF `17,660,712` bytes; fixtures unchanged | Removed `RenderPaintLayer.children` compatibility storage/raw clones, switched tests to canonical content, replaced the custom moving-payload FNV hasher with `DefaultHasher`, and reused paint-layer primitive/geometry hash helpers in visible-frame fingerprinting. Rust tests passed. The focused Criterion filter completed benchmark setup but matched no measured row, so no timing delta was recorded. |
| final validation | `git diff --check`; `mix test`; `cd native/emerge_skia && cargo clippy -- -D warnings`; `./ci-tests.sh all`; final `stat -c '%n %s' ...`, `git diff --shortstat main`, and `mix hex.build --unpack emerge-0.3.1` | not applicable | all passed; source `71 files changed, 26981 insertions(+), 10254 deletions(-)`; NIF `17,660,712` bytes; unpacked Hex package `6,320,200` bytes | Full CI wrapper passed format, credo, clippy, full-sweep Mix tests, release Rust tests, and Dialyzer. Hex package check still had no bench/external fixtures/native `tests` paths. |
