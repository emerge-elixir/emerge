# D17 bounded replay and lossless host feedback

Base commit: `33ee559`; D12–D17 remain uncommitted.
Functional identity: `source-sha256:a898887d8d19aa26031e1a3e5c804c5990cdf10597e2ba7aa9a7d307380d7bce`.

## Reproductions and change

- `before.log`: 128 non-staling inputs consumed in one registry install; the
  extracted macOS feedback loop drains and drops the ninth request at its cap.
- `drain-before.log`: fresh cursor draining consumes all 129 positions rather than
  yielding after the existing 64-message actor quantum.

Replay/fresh drain now stop after 64 top-level inputs. Remaining replay explicitly
requests a cached response using the existing ordered receipt gate. No second
continuation queue or invented animation pulse. The shared host feedback pump stops
at eight rounds **before** draining the next request; the macOS wrapper delegates to
it, leaving pending work available for the next call.

## Directed qualification

Eight new native unit functions cover:
- Replay and fresh-drain bounds, including preserving Stop/control order.
- The ninth host request surviving the feedback budget.
- 4,096 no-op keys across eight pump calls and 64 cached acknowledgments, no scenes.
- Composition/new input order and a previous receipt not acknowledging its successor.
- Pointer release across a quantum, with same-mount selection versus remount reset.
- Error/Stop paths not draining undelivered host work.
- Real event actor: 2,048-input recovery through 31 continuation responses, or Stop
  after the first quantum without waiting for its receipt.

D16 remount recovery now explicitly processes bounded continuations. Its historical
single-callback behavior/evidence remains tied to the old D16 source identity.

## Validation

- `cargo.log`: **1441 Rust units + 14 integration**.
- `clippy.log`: benches/tests/`bench-diagnostics`, warnings denied.
- `mix.log`: source-built release NIF, **520** tests/doctests, 9 excluded.
- `ci.log`: `./ci-tests.sh all`, **1441 + 14** Rust, **526** Elixir tests/doctests,
  3 excluded, **0** Dialyzer errors; quality/formatting pass.
- `source.sha256`: D16 selected inputs plus host-feedback and replay-yield modules;
  its digest is the identity above. `changes-from-D16.txt` identifies changes.
- `binaries.sha256`: tested release NIF and default-feature debug unit executable.

Run manifests from the repository root. Historical console EOFs are preserved.

## Limits

Shared helper execution on Linux is **not macOS compilation or UI execution**.
Limits count top-level inputs and pump rounds, not milliseconds, callbacks, nested
synthetic work, reconciliation or blocked sends. Yield requests use the existing
channel send fallback: a full outbound tree channel can still block that send; Stop
under that condition is not newly qualified here. Fresh cursor batch boundaries can
change intermediate observation. Extra receipt/feedback costs remain unmeasured.
The lossless input queue is still **unbounded**; a hard cap requires explicit
backpressure/overflow semantics, not silent eviction. No P1–P9 package, memory,
performance, device or physical-presentation gate closes. No new NIF/atom/wire
version, production thread, resource lock, global cache or animation clock.
No commit or push.
