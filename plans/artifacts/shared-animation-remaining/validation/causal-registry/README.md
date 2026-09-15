# D12 causal event-registry installation

Base commit: `33ee559`; implementation remains uncommitted.
Functional identity: `source-sha256:50f673402e7bac56ac463470c4caf0e77095ca6c290c50221496c8bd7102db21`.

## Reproductions and scope

- `before.log`: genuine pre-edit native registry replays buffered input too early
  (`buffered_inputs` becomes 0 instead of 1). Its companion minimal focus fixture
  was underspecified; do not treat that proxy as focus qualification.
- `mount-focus-before.log`: replaces that proxy with native layout-produced focus
  registries, reproducing loss on an unchanged eligible mount. Causal fencing was
  already implemented at this intermediate checkpoint; focus coalescing was not.
- The final suite adds eight units: six backlog/policy keyboard schedules, prior/
  foreign receipts, cached no-op acknowledgment, focus membership/coalescing/defer,
  sharing/drop, bounded draining, real event-actor recovery/Stop and three delayed
  tree-actor/direct/headless schedules. Existing failed-query/ghost/backoff tests
  also assert that the receipt appears on successful recovery, never failed output.
- The headless harness now passes each runtime's own messages to its own engine;
  independent event runtimes cannot share receipt authority. It compares registries,
  messages, keyboard/IME/wheel state, final geometry, fresh/retained pixels and old
  scene replay while draining deferred responses FIFO.

## Validation

- `cargo.log`: **1400 Rust units + 14 integration**, debug.
- `mix.log`: source-built NIF, **520** Elixir tests/doctests, 9 excluded.
- `ci.log`: `./ci-tests.sh all`, **1400 + 14** Rust, **526** Elixir tests/doctests,
  3 excluded, **0** Dialyzer errors; quality and formatting pass.
- `clippy.log`: benches/tests/`bench-diagnostics`, warnings denied.
- `source.sha256`: same selected functional/build/test inputs as headless-rebase,
  plus `events/runtime/tests/delayed.rs`; its digest is the source identity above.
  `binaries.sha256` records the local release NIF and debug unit-test executable.
  Run manifests from the repository root. Raw logs retain their original EOFs.

Commands (repository root):

```sh
cargo test --manifest-path native/emerge_skia/Cargo.toml
cargo clippy --manifest-path native/emerge_skia/Cargo.toml --benches --tests --features bench-diagnostics -- -D warnings
EMERGE_SKIA_BUILD=1 CARGO_TARGET_DIR=/workspace/emerge-animation/native/emerge_skia/target mix test
EMERGE_SKIA_BUILD=1 CARGO_TARGET_DIR=/workspace/emerge-animation/native/emerge_skia/target ./ci-tests.sh all
sha256sum -c plans/artifacts/shared-animation-remaining/validation/causal-registry/source.sha256
```

## Limits

Causal receipts qualify event requests, not renderer installation or compositor
presentation. Obsolete intermediate registries are skipped during a causal wait;
stationary-hover/drag transition semantics across that wait remain unqualified. One pending receipt/target is not a whole-runtime memory bound:
eligible-focus metadata scales with declarations, collection costs work, and the
existing input buffer requires separate long-failure retention qualification.
Sharing/drop tests do not measure exact live/peak heap or disposal time. No current
locked benchmark, GPU/device run, public artifact release or P1–P9 closure is claimed.
No new NIF, wire version, atom, production thread or resource lock. No commit/push.
