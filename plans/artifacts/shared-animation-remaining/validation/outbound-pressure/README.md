# D18 outgoing pressure and shutdown signaling

Base: `33ee559`; D12–D18 uncommitted.
Functional identity: `source-sha256:e98b67da5aa6ddf42f947be3dcef2b2a86fb35336d3e1ed082c77e9dd415032c`.

## Before / fix

- `before.log`: a full tree channel blocks event dispatch before it can read Stop.
- `shutdown-before.log`: after that driver fix, renderer shutdown still withholds
  render/event Stop while waiting on a full tree channel. Both fixtures unblock the
  old sender and join before asserting, rather than leaving hung test threads.
  Before smoke deadlines were 500ms; current deadlines are 5s, not latency claims.

The driver retains complete unsent operation packets in a local FIFO and selects
send readiness with input/control/timers. No packet bypass, envelope stripping,
coalescing or payload cloning during transfer. The immediate send path needs no FIFO
allocation. Pending tree disconnection is terminal; Stop/input disconnection also
release owned state. Full drain releases FIFO capacity synchronously.

The host channel is bounded at 512. Its mutable drain takes channel then outgoing
FIFO packets, at most 512 per call. The eight-round host feedback budget still leaves
subsequent work queued. Renderer shutdown signals render/backend before independently
selecting tree/event Stop sends. Test-harness shutdown uses the same actor helper.

## Nine new directed units

`events/runtime/tests/delayed/outbound.rs` covers:
1. Full outbound channel does not prevent event Stop.
2. 1,281 host operations preserve FIFO despite a freed slot; payload pointers survive
   transfer; earlier receipts drop; drain batches stay bounded, under both policies.
3. An older response cannot acknowledge a packet still awaiting delivery.
4. Real actor recovery preserves X/Y order or rejects the old mount, both policies.
   Raw Y observation proves X dispatch returned while the tree slot was still full.
5. Stop and both peer losses release a pending packet/receipt with 1,024 buffered
   inputs; weak identity is gone before thread completion.
6. 128 non-staling preedits leave the actor FIFO in order, without raw replay copies.
7. A due one-shot virtual-key hold timer and Stop remain selectable while full.
8. 4,097 host edits cross 512-packet/eight-round budgets; the retained last packet
   precedes a newly buffered commit. Completion takes a subsequent pump call.

`lib.rs` adds shutdown signaling with tree/event/both actor channels full. It observes
other Stop signals before freeing the blocked peer; it does not prove all joins can
complete while a peer remains permanently blocked.

`directed.log`: at 1,280 edits, 512 channel + 768 outgoing packets; requested outbox
capacity 1,024 × 64B = **65,536B** on tested x86-64. This excludes nested command/mount/
string/receipt storage, headers, channel/allocator overhead, other queues and RSS.
It is not a total memory charge or a memory-saving claim.

## Validation

- `cargo.log`: **1450 Rust units + 14 integration**.
- `clippy.log`: benches/tests/`bench-diagnostics`, warnings denied; fmt check passes.
- `mix.log`: source-built NIF, **520** tests/doctests, 9 excluded.
- `ci.log`: `./ci-tests.sh all`, **1450 + 14** Rust, **526** Elixir tests/doctests,
  3 excluded, **0** Dialyzer errors; quality/formatting pass.
- `shutdown.log`: directed shutdown tests, including existing join/error coverage.
- `source.sha256`: D17 selected inputs plus outbound module; digest is identity above.
- `changes-from-D17.txt`: changed selected inputs.
- `binaries.sha256`: tested release NIF and default-feature debug unit executable.

Verify manifests from repository root. Historical console EOFs are preserved.

## Still open

Pressure now consumes **unbounded retained memory** rather than blocking dispatch,
and may increase activity/retention in input, callback and BEAM queues. Explicit
backpressure/overflow semantics and whole-runtime accounting remain necessary.
No silent input/operation eviction is introduced.

Signal progress is not a hard shutdown bound: delivery/disconnection and joins still
wait, and asset teardown/callbacks/blocked native driver calls are not preempted.
Already-emitted callbacks cannot be withdrawn. Native Linux helper tests are not
macOS execution, GPU fault/presentation or constrained-device qualification. No new
locked benchmark or P1–P9 closure. No new NIF/atom/wire version, production thread,
resource lock, global cache or animation clock. No commit/push.
