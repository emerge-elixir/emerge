# Commit-sequence validation

The implementation was split without changing the source bytes validated after
rebasing onto headless-backend. Final selected inputs still match
`../headless-rebase/source.sha256`:
`source-sha256:8cfca2fac4d83c3573e6496e46aff586cc21f7ee59efd126a37d3e6f52221626`.

Intermediate commits were tested in an isolated detached worktree, not against
unstaged follow-up changes:

- Native core `a760420`: **1392 Rust units + 14 integration** and **509 Elixir
  tests/doctests** (9 excluded), before introducing the public change builders.
- Public API `53bc9b7`: **520 Elixir tests/doctests** (9 excluded), with both host
  protocol constants advanced together.
- Combined implementation through benchmark commit `b7829b8`: full CI passes,
  **1392 Rust units + 14 integration**, **526 Elixir tests/doctests** (3 excluded),
  Dialyzer **0**. Denied-warning benches/tests Clippy and formatting pass.

Logs in this directory record these runs. Subsequent commits only preserve evidence
and documentation. No benchmark or device qualification was rerun for this commit
organization; existing performance and platform gates remain open.

Raw console logs are preserved verbatim, including Cargo's trailing blank line;
whitespace checks allow blank-at-EOF for these archived outputs only.
