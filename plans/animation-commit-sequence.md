# Shared animation commit sequence — completed

Commit the already validated, rebased implementation without changing its source
bytes. Keep dependency-coupled native layout, clocks, assets and actor publication
in one atomic commit rather than manufacturing non-building intermediate layers.
Existing benchmark call-site adaptations belong with that native API change.

1. Headless fence regression reliability (independent test fix).
2. Native shared animation core, frozen inputs, publication and native regressions.
3. Public list-based change API, codec, matching host protocol and ExUnit coverage.
4. Shared-animation benchmark harness, locked baselines and historical probes.
5. Functional validation logs, source manifests and platform inventory.
6. Public/internal documentation, decisions and remaining qualification plans.

The native core adds backward-readable optional tag 84; public emission and both
macOS protocol constants move together in step 3. No push or package completion is
implied. Preserve the pre-rebase safety stash and neighboring worktrees.

Validation: final source bytes must still match the headless-rebase source manifest;
that source passed full CI (1392 Rust units + 14 integration; 526 Elixir tests/
doctests; zero Dialyzer errors). Check intermediate native/public compatibility and
record outcomes here before the final documentation commit.


## Result

- `73925bb` — Fix headless fence test descriptor reuse race.
- `a760420` — Implement shared native animation clocks and transactional publication.
- `53bc9b7` — Expose list-based retained-value animation transitions.
- `b7829b8` — Add reproducible shared animation performance benchmarks.
- `f8dea1a` — Preserve animation validation and platform evidence.
- Final documentation commit — contracts, decisions, rebase review and remaining gates.

Native core alone passed in an isolated worktree: 1392 Rust units + 14 integration,
509 Elixir tests/doctests (9 excluded). Public API commit independently passed 520
Elixir tests/doctests (9 excluded). Fresh combined full CI passed: 1392 Rust units +
14 integration, 526 Elixir tests/doctests (3 excluded), Dialyzer 0; Clippy and fmt pass.
Logs: `plans/artifacts/shared-animation-remaining/validation/commit-sequence/`.
The final source manifest still matches the already validated headless-rebase
identity; no implementation bytes were changed while organizing these commits.
Raw captured logs retain their original trailing blank lines. No benchmark rerun,
platform qualification, push or broad plan closure is implied.
