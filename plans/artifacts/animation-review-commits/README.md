# Animation review-commit validation

Fresh checks of each reconstructed commit boundary, not reused historical results.
The final implementation remains identical to the closeout source manifest.

| Commit | Scope | Unit tests | Additional integration tests | Clippy |
|---|---|---:|---:|---|
| `4372108` | Keep animation registry replies and input mutations causal and mount-safe | 1420 | 14 | pass |
| `1697927` | Coalesce buffered input incrementally and bound replay work | 1435 | 14 | pass |
| `f23af26` | Keep event delivery and shutdown responsive under backpressure | 1444 | 14 | pass |
| `2ef2721` | Retry terminal headless frames after transient render failures | 1450 | 14 | pass |
| `4c0a89b` | Add an isolated native input-pressure benchmark | 1450 | 14 | pass |
| `0394ff6` | Order host text commands and replacement ranges with raw input | 1461 | 14 | pass |

For each commit, formatted the detached review tree, ran `cargo test`, then
`cargo clippy --tests --benches --features bench-diagnostics -- -D warnings`.
Both use `--manifest-path native/emerge_skia/Cargo.toml`. Every native source
file was compared against its committed blob after validation. The build target
directory was shared serially; no concurrent build/test or measurement ran.

The first boundary intentionally keeps the old buffered-vector replay and
blocking delivery. The second introduces FIFO/work quanta; the third introduces
outbound delivery and its test-only weak barrier helper. Render retry is separate.
The fifth adds test-only pressure instrumentation; the sixth adds ordered host
operations and adapts queue/transport instrumentation without enabling D23.

Historical D12–D22 validation and pressure measurements retain their original
identities. No pressure benchmark or animation performance rerun is claimed here.
Final Mix/CI/docs checks are recorded below.

## Final working-source checks

Standalone Mix: 520 passed / 9 excluded. Full CI: 1461 Rust units plus 14
integration, 526 Elixir checks / 3 excluded, Dialyzer zero. Public and internal
ExDoc builds passed. See `final-*.log` and `final-checks.json`. The full closeout
source manifest still matches; the pre-existing working files are unchanged
except for bookkeeping links in the plan index and commit/closeout records.

## Completed history

The original implementation sequence is `73925bb` (headless fence test), `a760420`
(native core), `53bc9b7` (public API), `b7829b8` (benchmarks), `f8dea1a` (evidence),
and `33ee559` (contracts/docs). The six code commits in the table above were followed
by `03dfe27` (runtime evidence), `5ad077b` (deferred proposals/prototype), `459430a`
(closeout benchmark/evidence), and `6b2c4aa` (closure/docs).

The implementation bytes were preserved through that sequence; all commits were
local, with no push/release performed by that work. Completed root-level checklists
were later removed rather than kept as plans. Their exact text remains in Git at
`6b2c4aa`. Recovery archive/index/patch and detached test snapshots remain at
`/workspace/animation-commits-recovery-20260915-170758`, with starting revision held
by `backup/animation-before-review-commits-20260915-170758`.
