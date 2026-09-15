# Animation closeout commit sequence

Status: completed on `plan/shared-animation-core`.
Preserve existing six implementation/API/benchmark/evidence/docs commits through
`33ee559`; do not rewrite history or push. Preserve all current source bytes and
historical evidence. The unfinished D23 code remains a deferred artifact only.

## Sequence

1. Causal registry installation and mount-safe state/commands (D12–D14).
2. Incremental input FIFO, bounded replay and lossless host feedback (D16–D17).
3. Selectable outbound delivery and independent shutdown signaling (D18).
4. Binary headless terminal-frame retry and recovery regressions (D15).
5. Test-only input-pressure instrumentation (D20).
6. Ordered raw/host command/edit/range admission (D22).
7. Durable runtime validation plus the isolated pressure runner/results (historical, not relabelled).
8. Unapproved pressure proposal/model and deferred async-input work.
9. Bounded final benchmark preset and closeout measurements/evidence.
10. User/internal docs, implementation closure and later-work ownership.

Use a verified recovery archive and detached review worktree. Build intermediate
source views there, stage their blobs without overwriting the final working files,
and test each code commit. D20/D22 archived sources are comparison material; any
reconstructed earlier boundary must pass its own tests, not borrow a historical
result. Finish by verifying the entire final functional manifest and the original
working-file hashes; new bookkeeping documents are the only additional content.
Run final Rust/Mix validation. No benchmark rerun is necessary for unchanged code.

## Results

The existing six commits through `33ee559` are unchanged. New sequence:

| Commit | Review scope |
|---|---|
| `4372108` | Causal registry receipts, mount-safe interaction state and command packets |
| `1697927` | Incremental FIFO, bounded replay, host feedback batching |
| `f23af26` | Selectable outbound delivery and independent Stop signaling |
| `2ef2721` | Binary terminal-frame retry and render recovery regressions |
| `4c0a89b` | Test-only native pressure instrumentation |
| `0394ff6` | Ordered host commands/edits and mount-scoped range generations |
| `03dfe27` | Historical runtime/pressure evidence and fresh per-commit checks |
| `5ad077b` | Unapproved pressure policy, deferred async prototype and later-work ownership |
| `459430a` | Bounded benchmark preset, measurements and closeout audit evidence |
| This documentation commit | Contracts, closure, history rename and commit records |

Each code commit was tested independently in a detached review worktree. Unit
counts were 1420, 1435, 1444, 1450, 1450 and 1461; each also passed 14 integration
tests and denied-warning Clippy with tests, benches and `bench-diagnostics`.
The first boundary's unused weak-identity test helper was assigned to the outbound
commit where its tests need it, rather than suppressing the warning.

Final standalone Mix: **520 passed**, 9 excluded. Full CI: **1461 Rust units +
14 integration**, **526 Elixir checks**, 3 excluded, **Dialyzer 0**. Public and
internal docs passed. [Fresh logs and per-commit results](artifacts/animation-review-commits/README.md)
are separate from unchanged historical evidence.

Final native/Elixir/build/benchmark inputs match the complete closeout manifest
`bf4c131c8da7882991cdfcbd4058102476abcd36435041c951e5c17a4e66fdff`.
No runtime implementation was changed while making these commits. Only commit
bookkeeping links/records were added to the existing documentation. The full
original working-file archive was compared before completion.

Recovery archive, original index/patch and detached review snapshots remain in
`/workspace/animation-commits-recovery-20260915-170758`; the starting revision is
also retained by `backup/animation-before-review-commits-20260915-170758`.
Earlier safety branches/stash and neighboring worktrees were not altered.

Source/documentation range whitespace checks pass. Raw logs and saved patch/diff
artifacts intentionally retain their original blank EOF and patch-context lines.

No push or release. D23 remains a preserved, failing prototype outside production;
D19 policy/defaults and later qualification remain unapproved/deferred.
