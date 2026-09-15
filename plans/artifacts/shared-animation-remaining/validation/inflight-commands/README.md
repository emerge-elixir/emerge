# D14 in-flight event commands

Base commit: `33ee559`; D12–D14 remain uncommitted.
Functional identity: `source-sha256:c50d357e2dc201db56f860807f24882228a2f6c4edc0788e3a02dbf5372f6f9d`.

`before.log` reproduces queued selection/focus commands mutating a newly uploaded
same-ID text input. The fixture also specifies text-edit and both decode-policy
cases; the before run stops at its first failed assertion.

The shared native driver collects each dispatch's commands and fence, attaching
only sparse target-mount evidence. Tree delivery preserves the envelope until its
consumption point. A stale/missing target rejects the operation's node effects
together, not resize/rebuild/fence/Stop controls. Deferred scroll/style state uses
mount-qualified keys and rechecks them after later edits in the same tree batch.

## Directed qualification

Nine added unit functions:
- Text and selection after replacement under both error policies.
- 32 deferred schedules: eight scroll/thumb/hover/style/focus variants × two
  producer orders within one tree batch × two error policies.
- One stale focus target rejects the whole operation while resize/receipt pass.
- Same-mount requests survive unrelated revision changes.
- Missing/malformed evidence fails closed; Stop remains control.
- Real event actor: queued text packet delivered after replacement, response/Stop.
- Slider value after replacement under both error policies.
- Focused listenerless nodes provide native source evidence for blur.
- Real tree-actor/direct/headless replacement/edit trace: registry/IME parity,
  retained/fresh pixels and unchanged old-scene replay.

Existing D12/D13 tests still pass. Diagnostic taps inspect envelope contents, but
transport helpers never unwrap away authority before tree validation.

## Validation

- `cargo.log`: **1420 Rust units + 14 integration**.
- `clippy.log`: benches/tests/`bench-diagnostics`, warnings denied.
- `mix.log`: source-built release NIF, **520** tests/doctests, 9 excluded.
- `ci.log`: `./ci-tests.sh all`, **1420 + 14** Rust, **526** Elixir tests/doctests,
  3 excluded, **0** Dialyzer errors; formatting/quality pass.
- `source.sha256`: D13 selected functional/build/test inputs plus the new in-flight
  module; its digest is the identity above. `changes-from-D13.txt` names changes.
- `binaries.sha256`: tested release NIF and debug unit-test executable.

Run manifests from the repository root. Historical console EOFs are preserved.

## Limits

No package, performance, memory or platform gate closes. Rejection cannot withdraw
callbacks already sent to Elixir/host. Input is not synchronized to physically
presented pixels. Broader joint input/topology/virtual-key/inertia histories,
renderer failure/recovery and prolonged buffering remain open. Collection, target
sorting/checking, queue retention and synchronous disposal need real accounting and
new locked benchmarks. Sparse evidence does not bound the input queue or establish
an accepted live-memory budget. No new NIF/atom/wire version, production thread,
resource lock, global cache or animation clock. The existing dirty-I/O hover tap
only changes internal observation, not its Rustler contract. No commit or push.
