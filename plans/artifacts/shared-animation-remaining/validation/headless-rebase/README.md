# Animation/headless rebase validation

Source: `source-sha256:8cfca2fac4d83c3573e6496e46aff586cc21f7ee59efd126a37d3e6f52221626`. Base: `39997f0`; rebased branch HEAD: `84906f7`.
Implementation remains uncommitted. Five planning commits replay identically
(`range-diff.txt`). The full pre-rebase work survives in the retained safety stash;
`identity.json` records its identity and the additional patch/index/untracked backup.

- Rust: **1392 units + 14 integration**, debug and CI release, pass.
- Standalone Mix: **520 tests/doctests**, 9 excluded.
- Full CI: **526 tests/doctests**, 3 excluded; Dialyzer **0 errors**.
- Clippy benches/tests/`bench-diagnostics` with denied warnings, fmt and diff checks pass.

Logs: `cargo.log`, `mix.log`, `ci.log`, `clippy.log`; initial compiler collisions in
`compile-before.log`. `restored-work-audit.txt` verifies preservation of all saved
files/symlinks before the final reporting edits. `source.sha256` includes the newly
split Mix helpers and build workflow/support sources as well as native/Elixir/tests.

The added 18-case regression covers paragraph inline decoration under dimension
animation, inherited alignment changes, reparenting, failed publication/retry,
ghost owner remapping before reflow, terminal/cleanup and cached/fresh/old raster
replay. Existing upstream paragraph and animation tests are both retained.

See `plans/animation-headless-rebase.md` for the collision review. No performance,
new release-target or physical presentation claim follows from these results.
