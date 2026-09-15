# Shared animation: minimal closeout

**Status: complete — shared animation implementation closed.**
[Final evidence](artifacts/shared-animation-closeout/README.md) records the scoped
D23 separation, ten-contract audit, full validation and 18-process performance
snapshot. Known performance costs and platform/runtime follow-ups remain in the
later plan. This completed checklist is retained as the bounded closeout record.
Worktree: `/workspace/emerge-animation`, branch `plan/shared-animation-core`.
User direction: finish animation soon; defer everything not necessary to its contract.
This replaces the execution order and completion gates in
[the old P1–P9 plan](shared-animation-remaining-implementation.md) and
[the accumulated checklist](shared-animation-implementation-history.md).
[Later work](later-animation-runtime-qualification.md) owns all deferred items.

## Finish line and scope rule

Close the **shared animation implementation** when the four steps below pass.
Do not wait for exhaustive matrices, async editing, queue budgets, model-storage
redesign, GPU recovery or unavailable hardware. Performance/device qualification
will remain explicitly separate; closing this plan is not a claim that those pass.
No commit, push or release is authorized by this plan.

Keep the implemented contract: pixels/content/fill/weighted fill on both axes;
regular/enter/exit and list-based `Animation.change/3`; independent field clocks;
automatic finite holds/joint release; real reflow; resolved-content detection;
published-source interruption; mount-safe transport; transactional publication;
immutable query/scene inputs; paint/pixel fast paths. Animated min/max stays
unsupported, static min/max stays valid. No second core/workspace, relaxed release
checks, fabricated history/durations, silent cancellation or retry-as-correctness.

**A new blocker must name a supported contract and provide a failing regression,
or identify a concrete unsafe production path.** Missing permutations, optional
optimizations and speculative risks go to later work. Existing tests count; do not
reimplement covered behavior or multiply every fixture by every factor. A reproduced
supported animation bug cannot be relabelled as qualification to close this plan.

## 1. Establish a clean, scoped implementation baseline

- [x] Preserve the complete current dirty state, including untracked D23 sources,
  tests and logs, in a verified recovery artifact outside the source tree. Preserve
  existing safety stashes/backups; do not restore them over current files.
- [x] Defer D23 rather than finish it here: selectively remove its unfinished
  native async-edit authority/publication path and geometry-query policy from the
  closeout source, retaining a reproducible patch/source snapshot for later work.
  Use the archived D22 source and D23-start backup as comparison evidence, **not**
  whole-file restore instructions. Shared files contain earlier valid changes.
- [x] Keep completed D12–D22 fixes and their regressions; do not spend this closeout
  splitting or redesigning validated runtime work. Remove only D23-dependent tests,
  fixtures and declarations with the preserved prototype, not failing core tests.
- [x] Confirm the existing registry-wait contract is unchanged. No latest-snapshot
  dispatch switch, new callback-echo protocol or input overflow policy.
- [x] Record the resulting source identity and scoped diff in one closeout report:
  `artifacts/shared-animation-closeout/README.md`.

**Why:** D22 was validated (1461 Rust units + 14 integration; CI 526 Elixir
checks). D23 is unfinished: its reset/geometry-only-patch test returns empty text
instead of `abcXY`. Neither those historical counts nor D22's archived identity
qualifies the resulting source; step 3 validates it anew.

## 2. One bounded contract audit; fix only demonstrated gaps

For each row, record existing test names and a verdict in the closeout report:
**covered**, **concrete missing assertion**, or **reproduced defect**. Read the tests,
not just old unchecked headings. Add a small targeted regression only when needed.
Both axes/all owners are already covered by base matrices; larger combined cases
need representative adversarial coverage, not another Cartesian expansion.

| Required contract | Existing evidence to reuse |
|---|---|
| Public list policy, empty/no-op validation, ordering/aliases, plain clearing, unsupported bounds | Public API/codec/reconcile and native admission suites |
| Native length pairs, independent clocks, finite holds and jump-free release | 1152-case raster/hit matrix; sibling/group numeric oracles |
| Content changes without owner declaration changes; interruption/no restart | Content watcher and published-source/reversal suites |
| Nested/shared-pool/self-/cross-axis contexts; segments/repeats/skips | D6–D8 continuation plus D11 noncanonical tests |
| Combined model/viewport/scale/font/image/runtime/scroll changes | D8 receipts, D9 joint-input traces, D10/D11 asset snapshots |
| Same-mount reparent/role/unit changes versus remount; first-write retries | D9 transport, D11 multi-edit/orphan/root tests |
| Owner handoffs, cancellation/arrival, ghosts and terminal cleanup | Lifecycle suites, complex ghosts/migrated holds, rebased decoration tests |
| Query/output failure preserves publication and successful patch prefixes | Native failure/corruption tests and both decode policies |
| Actor/direct/public paths, animated hits/clips/scroll and final damage | Existing headless/public integration, D12–D15 delayed/final-output tests |
| Retirement/isolation and unchanged paint/pixel fast paths | Workspace/source/scene ownership, renderer-isolation and zero-query tests |

- [x] Complete this ten-row audit once. Map D8/D11 evidence onto stale claims about
  self-axis, atomic images and orphan/root behavior; do not reopen their implementation.
- [x] Audit only changed production seams: animation prepare/layout/output/commit
  callers; public encoding/reconciliation; EMRG9/tag84/macOS14 compatibility checks;
  relevant Rustler scheduling, ownership and error shapes. Do not audit the whole NIF
  crate or redesign existing APIs unrelated to animation.
- [x] Fix concrete failures with minimal changes and regression tests. Keep one
  explicit blocker list; do not introduce another P1–P9 expansion. If a fix requires
  a larger contract change, report that blocker before expanding scope.

**Exit:** all ten contracts have concrete evidence and no known unresolved supported
animation correctness defect or unsafe changed entry point. An evidence gap is not
proof of a defect, but a genuinely missing contract assertion still needs a test.

## 3. Validate once at the final source; report performance honestly

- [x] Run Rust tests, source-built Mix tests, full `./ci-tests.sh all`, Rust fmt,
  denied-warning Clippy including benches/tests/`bench-diagnostics`, and `mix docs`.
  Use the commands below. Fix actual regressions; no need to duplicate existing
  passing tests solely to create fresh test counts.
- [x] Confirm existing protocol/version-rejection tests and ordinary available Linux
  build checks. Missing macOS binaries/devices are recorded, not manufactured or
  claimed compatible. Actual release packaging for that target retains its own gate.
- [x] Take one small current-source performance snapshot using the existing benchmark:
  20k nodes / 64 owners, `paint`, `pixel`, `length`, `moving`, `mixed`, `upward`;
  three separate release processes per case (18 measurements), rotating case order.
  Exclusive performance lock; immutable source/build identity and raw output; no
  concurrent builds/tests/measurements. No new benchmark framework or instrumentation.
- [x] Report available cold/warm/release/settle timings, query/model counts and
  retention observations, including synchronous disposal. Missing byte/RSS metrics
  stay missing; requested slots are not heap bounds. Compare only comparable runs.
  Preserve old baseline-03 results as historical, not current qualification.

This snapshot is a disclosure/regression check, **not an optimization campaign**.
Known large-tree/upward cost and the full private-model footprint go to later work.
The inherited device patch target is not a newly mandated desktop length-release
threshold. Do not claim low-resource suitability or a performance pass from this
closeout. Unexpected hangs, unbounded animation-owned lifecycle retention or lost
paint/pixel fast paths are correctness/regression blockers, not deferred speed work.

```sh
# From /workspace/emerge-animation; run performance separately under the lock.
cargo fmt --manifest-path native/emerge_skia/Cargo.toml -- --check
cargo test --manifest-path native/emerge_skia/Cargo.toml
cargo clippy --manifest-path native/emerge_skia/Cargo.toml \
  --benches --tests --features bench-diagnostics -- -D warnings
EMERGE_SKIA_BUILD=1 CARGO_TARGET_DIR=/workspace/emerge-animation/native/emerge_skia/target mix test
EMERGE_SKIA_BUILD=1 CARGO_TARGET_DIR=/workspace/emerge-animation/native/emerge_skia/target ./ci-tests.sh all
EMERGE_SKIA_BUILD=1 CARGO_TARGET_DIR=/workspace/emerge-animation/native/emerge_skia/target mix docs
git diff --check
```

If validation changes code, rerun affected checks and final full CI. Re-measure if
functional/build inputs changed; documentation-only edits do not invalidate timings.

## 4. Close, do not start another implementation phase

- [x] Correct guides/changelog only where behavior or scope is inaccurate: public
  list API, holds, interruption, content changes, remount/transport, ghost cleanup,
  unsupported bounds and explicit performance/platform limitations.
- [x] Finish the single closeout report: ten-row evidence, scoped changes, final
  source identity, validation, small performance snapshot, limitations and links to
  every deferred category. No claim of display acknowledgment from native receipts.
- [x] Mark shared animation **implementation complete** in the plans index. Retire
  active animation implementation checklists into durable history/references; do not
  leave them open merely because later qualification is incomplete. Retain all old
  source archives/results as historical evidence, without rewriting their identities.
- [x] Report completion plus separate outstanding performance/platform/runtime work.

**Stop here.** No async-input implementation, pressure-limit policy, broad GPU fault
work, full-model optimization or exhaustive stress matrix follows automatically.
