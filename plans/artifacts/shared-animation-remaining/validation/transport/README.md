# D9 functional validation

Source: `source-sha256:aab1a31e9e1e611b98c2ab057b4dcfc089de2db7c56c428d4edd2ff6b57edf3f`.

- Rust: **1342 unit tests + 14 integration tests** passed.
- Standalone Mix: **494 tests/doctests**, 8 excluded.
- Full CI: **499 tests/doctests**, 3 excluded; Rust release tests pass;
  Dialyzer **0 errors**.
- Clippy benches/tests/`bench-diagnostics` with warnings denied passes.
- Formatting and `git diff --check` pass.

`cargo.log`, `mix.log`, `ci.log`, and `clippy.log` contain actual command output.
`source.sha256` identifies the selected validated inputs; HEAD alone does not
identify this dirty worktree. No commit, benchmark, or platform qualification is
implied. The source manifest includes Elixir tests as well as native tests.

## Scope and remaining gap

18 new directed tests in `lengths/tests/transport.rs` include 270 role/unit/curve
cases and 24 combined asset/runtime/scroll/scale cases. They do not close P2–P5.
The adjacent `same-id-retained-raster-gap.log` records a newly exposed unsupported
rendering case: an old scene's image ID can resolve to a newly registered SVG.
Historical native dimension receipts remain immutable. Font raster replay passes;
immutable same-ID image render bindings and stale-completion/animation integration
remain follow-up work. No tests are ignored to hide this gap.
