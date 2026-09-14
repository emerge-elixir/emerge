# D10 functional validation

Source: `source-sha256:6989691401905f206dc4891207aaccaec917605207e23e19faf5fef09587a6d5`.

- Rust: **1350 unit tests + 14 integration tests** passed.
- Standalone Mix: **494 tests/doctests**, 8 excluded.
- Full CI: **499 tests/doctests**, 3 excluded; Rust release tests pass;
  Dialyzer **0 errors**.
- Clippy benches/tests/`bench-diagnostics` with warnings denied passes.
- Formatting, source manifest verification and `git diff --check` pass.

Actual output: `cargo.log`, `mix.log`, `ci.log`, `clippy.log`.
`source.sha256` covers selected native/Elixir code and tests, configuration and
assets. `changes-from-D9.txt` lists changed selected inputs; many existing scene
fixtures only gain `images: None` to retain their manual live-ID semantics.
HEAD alone does not identify the dirty worktree. No commit or benchmark implied.

## Evidence

Seven new renderer image-snapshot tests plus one loader/tree-update test cover
raster/SVG replacement, fit/profile paths, deferred paint, eviction/reset,
renderer-local generation collisions, cached-version selection, scoped absence,
grayscale policy, no capture rasterization, sparse source pins and disposal.
The loader trace exercises both decode policies and completion after a newer
image publication without restarting the original animation deadline.
The existing 24 D9 combined-input traces now assert old-image raster replay too.
The D9 failure log remains historical evidence, not a current rejection contract.

## Not qualified

Atomic image provenance across concurrent layout and scene assembly; broader
stale-completion/epoch, lifecycle and ghost-baseline matrices; precise live/peak
memory, performance and backend/device presentation. Snapshot charges exclude
parsed SVG heap and globally shared/GPU allocation accounting.
