# D13 mount-safe pointer recovery

Base commit: `33ee559`; D12 and D13 implementation remains uncommitted.
Functional identity: `source-sha256:662fb39c1eaf3d68334af5a43a1d420bc86f3924cefad1c7db29f2161f8ab64a`.

## Reproductions and qualification

`before.log` reproduces both defects through the real tree-update engine/native
upload decoder and host event driver: buffered release clicks a same-ID replacement,
and a text drag retains its old anchor after remount. The fixed registry carries
native source mounts and discards old-mount local state before accepted replay.

Eleven new unit functions:
- Nine pointer functions cover 17 schedules: both decode policies for remount click;
  text drag/edit reset; same-mount translated selection and release; stationary hover
  recovery with/without a final crossing; remount enter; click release inside/outside
  accepted animated geometry; scroll/thumb and slider retention versus replacement;
  real event-actor release/Stop. Observer input is forwarded once, not on replay.
- One native source-index empty/sharing/drop test.
- One real tree-actor/direct/headless test covers 1/8/32 delayed registries with text
  drag/release/Nearby hover, registry/message/selection/raw-input parity, retained
  versus fresh damage pixels, old-scene replay and final sample removal.

The remount fixtures process the initial action batch before replacement. Reversed
cross-producer ordering—old event-to-tree commands arriving after replacement—is
not qualified by these tests and remains a next P6 target. Obsolete registry
crossings are deliberately skipped, not used to invent historical hover callbacks.
Input still uses installed native registry geometry, not displayed-pixel acknowledgment.

## Validation

- `cargo.log`: **1411 Rust units + 14 integration**, debug.
- `clippy.log`: benches/tests/`bench-diagnostics`, warnings denied.
- `mix.log`: source-built release NIF, **520** tests/doctests, 9 excluded.
- `ci.log`: `./ci-tests.sh all`, **1411 + 14** Rust, **526** Elixir tests/doctests,
  3 excluded, **0** Dialyzer errors; quality/formatting pass.
- `source.sha256`: prior D12 selected functional/build/test inputs plus the new
  pointer module. Its digest is the source identity above. `changes-from-D12.txt`
  lists changed selected inputs; the D12 manifest remains historical evidence.
- `binaries.sha256`: release NIF and debug unit-test executable used locally.
  Run manifests from the repository root. Console logs retain their original EOFs.

```sh
cargo test --manifest-path native/emerge_skia/Cargo.toml
cargo clippy --manifest-path native/emerge_skia/Cargo.toml --benches --tests --features bench-diagnostics -- -D warnings
EMERGE_SKIA_BUILD=1 CARGO_TARGET_DIR=/workspace/emerge-animation/native/emerge_skia/target mix test
EMERGE_SKIA_BUILD=1 CARGO_TARGET_DIR=/workspace/emerge-animation/native/emerge_skia/target ./ci-tests.sh all
sha256sum -c plans/artifacts/shared-animation-remaining/validation/pointer-recovery/source.sha256
```

## Remaining gates

No P1–P9 package closes. Source-index collection/merging, extra state reconciliation,
live/peak retention and synchronous disposal require real accounting and new locked
benchmarks. Shared index references are not exact heap/RSS measurements or a memory
budget. Broader input/topology/ownership/virtual-key/inertia histories, renderer
failure/recovery, prolonged blocked-input retention and actual platform/device
presentation remain open. No animation clock/query model, NIF/atom/wire version,
production thread, global cache or resource lock is added. No commit or push.
