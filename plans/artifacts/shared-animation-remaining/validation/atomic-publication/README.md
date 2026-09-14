# D11 functional validation

Source: `source-sha256:f60614ecc898495d7e36241764ebeccd3689ea1d49952e02348d1d419106b96a`.

- Rust: **1362 units + 14 integration tests** passed (debug and CI release).
- Standalone Mix: **494 tests/doctests**, 8 excluded.
- Full CI: **499 tests/doctests**, 3 excluded; Dialyzer **0 errors**.
- Clippy benches/tests/`bench-diagnostics`, denied warnings: pass.
- Formatting, source manifest verification and `git diff --check`: pass.

Logs: `cargo.log`, `mix.log`, `ci.log`, `clippy.log`. Selected code, tests,
configuration and fixtures are identified by `source.sha256`; changed selected
inputs since D10 are in `changes-from-D10.txt`. Available validation executables/
NIF artifacts are hashed in `binaries.sha256`. HEAD alone is not this dirty source.
No benchmark, commit or source rollback is implied.

## Directed evidence

15 new unit tests replace three obsolete blocking-only publication tests (net +12):

- 24 atomic image writer/preparation/epoch schedules plus pending readiness,
  paint-only/interaction source-list and frozen lookup/nested preparation tests.
- 144 orphan/multi-edit/wrapper/root schedules and two actual-root regressions.
- 1536 noncanonical self-/cross-axis, repeat/segment, interruption/failure schedules,
  plus an independent fresh-versus-reused Slider native-query oracle.
- 24 complex ghost and 24 migrated hold/arrival/cancellation schedules.
- Actual actor blocked/disconnected consumer, Stop, latest final registry/scene,
  ghost cleanup and pending mount focus tests.
- Direct/actor/native host input-runtime raster traces for focus, IME, selection
  drag, keyboard, wheel, Nearby hover, final damage and retained-scene replay.

`slider-before.log`, `clear-root-before.log`, `frozen-lookup-before.log` preserve
pre-fix diagnostics (not immutable pre-fix builds). The latter also has a targeted
post-fix log. Full current checks above include all promoted regressions.

## Still open

Exhaustive owner/layout/Hz/context cross-products, randomized multi-edit/lifecycle
schedules, delayed registry-install races with concurrent input, exact frame-source/
receipt/live-peak/disposal accounting, new locked performance, constrained/macOS and
device presentation. Enqueue-before-scene is not an event-consumer installation or
compositor acknowledgment. Native headless parity uses CPU raster, not a GPU device.
Bounded queues/reference indices do not demonstrate performance or memory budgets;
baseline 03 predates D8–D11. No P1–P9 package is closed.
