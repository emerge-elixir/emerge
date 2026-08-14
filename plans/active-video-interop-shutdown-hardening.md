# Active Plan: Video Interop Shutdown Hardening

Status: host/mock implementation complete; publication, target rebuild, hardware
acceptance, and rollout remain blocked. The guarded schema was implemented
atomically across `video_interop`, `membrane_video_interop`,
`membrane_libcamera`, Emerge, and the camera application. Do not publish or
deploy a partial protocol.

Last updated: 2026-07-31.

Implemented host gates include authority-verified per-holder guards, explicit
release-dispatcher close/join, exact asynchronous native terminal outcomes,
source/sink/Emerge composite barriers, persistent quarantine, runtime-loss
poisoning, and reusable terminal-draining sinks. Canonical `0.1.0` remains
unpublished and is reserved for the complete core/adapter protocol. Emerge uses
the new `0.4.0` breaking line and incorporates the published 0.3.4 fixes.

Remaining gates are clean registry-ordered publication, rebuilt precompiled NIF
checksums and target release artifacts, native libcamera/hardware execution,
rapid-cycle/FD/RSS acceptance, and the required hardware soaks.

## Objective

Make an acknowledged camera stop mean all of the following:

1. no producer can issue another canonical frame;
2. every Membrane branch has consumed or released every queued holder;
3. every Emerge pending/current/retired claim is retired;
4. the producer `LeaseOwner` has no holders or pending release callbacks;
5. the libcamera service has stopped and dropped, or explicitly quarantined, the
   exact native session;
6. an immediate same-VM reopen is permitted only after all success conditions
   are proven.

Also prevent avoidable lease stranding when an element fails before EOS and add
an eventual per-holder fallback for canonical frames abandoned in a killed
Membrane process or private queue.

This plan fixes the limitations recorded in
`guides/internals/video-interop-architecture.md`:

- `Native.close/1` acknowledges a close request, not necessarily camera
  finalization;
- the camera pipeline currently joins sink EOS acknowledgements but not producer
  or native completion;
- `Camera.VideoSink` raises instead of terminal-draining prefetched holders;
- `Camera.StreamRuntime` may hot-restart after an unacknowledged pipeline exit;
- a plain lease-bearing BEAM term has no destructor when a process and its
  private Membrane queue disappear.

## Non-goals

- Do not make a service panic, VM abort, kernel kill, or quarantined libcamera
  session recoverable in-process.
- Do not treat a finite timeout as proof that ownership was recovered.
- Do not add descriptor conversion, release routing, retries, or queue cleanup to
  application code.
- Do not fork Membrane Core merely to add a callback to its private input queue.
- Do not make ordinary `Membrane.Tee` safe. Every branch still requires a unique
  holder.
- Do not enable PiSP analysis in production as part of this work. It remains
  gated on target ownership and soak acceptance.
- Do not mix this work with explicit-sync, corruption, or dithering changes.

## Current failure boundaries

### Native close is only an admission-close acknowledgement

`MembraneLibcamera.Source` currently performs:

```text
LeaseOwner.close
-> Native.close
-> emit preview/analysis EOS
-> ignore LeaseOwner drained
```

The native service marks the session `closing` and clears delivery tickets, but
when `leased_requests` is nonempty it returns from close while keeping the camera,
allocator, and requests alive. The final `Native.release_frame/1` later invokes
`finalize_session`. A rapid reopen can therefore observe `:camera_busy` after the
current pipeline has reported `:drain` success.

### EOS is necessary but not a native completion barrier

Preview EOS closes the Emerge consumer session, which may be the action that
retires the last displayed holder. Waiting for producer/native finalization
*before* emitting EOS would deadlock. The correct protocol emits EOS first and
then joins independent completion barriers.

### Abrupt queue loss bypasses callbacks

A manual-flow Membrane input queue stores opaque `%Membrane.Buffer{}` terms.
Orderly terminal-drain callbacks can demand and release those buffers, but
`Process.exit(pid, :kill)` and supervisor brutal kill skip callbacks. The queue
cannot be inspected by the producer or application after the element heap is
reclaimed.

The native `FrameLease` protects the private libcamera request when its resource
is eventually dropped, but the public `LeaseOwner` still sees an unreleased
canonical holder. That split can leave drainage permanently unacknowledged.

### Chosen direction and rejected alternatives

Use a per-holder resource guard, conditional on the Milestone 4 prototype gate.
It follows the missing ownership edge because the resource remains reachable
wherever the holder-bearing term is reachable.

Do not rely on a Membrane queue discard callback: orderly queue cleanup would be
useful but `:kill` skips it. Do not start with an external custodian registry:
process monitoring alone cannot distinguish a frame still queued in Membrane
from the same holder already claimed by native GPU/display code. Correct
custodian handoff would recreate prepare/claim plus another process failure
boundary. If the resource prototype fails, revisit explicit escrow as a
versioned ownership protocol rather than silently weakening the guarantee.

## Required invariants

1. **EOS precedes final join.** Stop producer admission and request native close,
   emit EOS exactly once, then await sink, owner, and native completion.
2. **Independent barriers stay independent.** Sink EOS, `LeaseOwner` drainage,
   and native finalization are not aliases for one another.
3. **No blocking native waiter.** The libcamera service must keep processing
   releases while finalization is pending. Never block its event loop waiting
   for the releases it must itself consume.
4. **Session identity is exact.** Every finalization result carries `session_id`;
   stale results cannot acknowledge a replacement pipeline.
5. **Finalization is exactly once.** Immediate close, deferred last release,
   disconnect, request failure, duplicate close, and resource-drop close all
   converge on one terminal outcome.
6. **Quarantine is a terminal error.** Failed stop or incomplete request
   accounting returns `:quarantined`, suppresses hot reopen, and requires a cold
   VM/firmware restart.
7. **Normal release remains deterministic.** An abandonment guard is a fallback,
   not a replacement for explicit release, consumer close, release retry, or
   producer drain.
8. **Native claims retain the fallback.** A frame fallback must remain live until
   the claimed CPU/GPU/display use retires, not merely until `handle_buffer/4`
   returns.
9. **Timeout means unknown.** A drain timeout removes or abandons only the caller
   wait; it must not kill the pipeline and then authorize a hot restart.
10. **Applications configure policy only.** Reusable libraries implement
    terminal drain, guard creation/retention, native completion, and retries.

## Target shutdown state machine

### Source state

```text
:running
  -- drain/error --> :closing
    mark draining before any close call
    close LeaseOwner issue/retain admission once
    request native close once
    emit EOS once on every configured pad
    release late native frame messages directly

:closing
  + LeaseOwner drained        -> owner_done
  + native terminal outcome   -> native_done
  + both successful           -> notify {:libcamera_source_drained, session_id, outcome}
  + either terminal failure   -> notify {:libcamera_source_drain_failed, session_id, reason}
```

For raw output, the owner barrier starts satisfied. For canonical DMA-BUF output,
both barriers are required.

A late `:libcamera_frame` or `:libcamera_frame_set` delivered after `draining?`
becomes true must release its native keepalive directly. It must never attempt a
new `LeaseOwner.issue/3` against an owner that is already draining.

### Pipeline join

```text
source emits EOS
  -> preview sink closes Emerge session and drains queued input
  -> analysis sink releases queued input and stops detector admission
  -> LeaseOwner completes every final Native.release_frame callback
  -> libcamera service finalizes the exact session
  -> pipeline has all four acknowledgements
       source + preview + optional analysis + no saved terminal error
  -> reply :ok to every drain waiter
  -> orderly pipeline termination is now allowed
```

A saved terminal error is returned only after all still-achievable drain
barriers complete. Returning the error early and immediately tearing down the
pipeline could discard the buffers needed to finish drainage.

### Runtime restart policy

```text
acknowledged :ok drain
  -> terminate empty pipeline
  -> hot reopen allowed

typed drain failure / timeout / abnormal DOWN / unknown receipt
  -> mark cold_restart_required
  -> suppress automatic in-VM pipeline restart
  -> publish fatal diagnostics
  -> platform supervisor performs whole-VM/firmware restart
```

The existing unconditional restart after pipeline `:DOWN` must be removed. A
new pipeline may be started in-process only after the prior pipeline's exact
successful drain result.

## Milestone 0 — Freeze the contract and baseline diagnostics

### Work

- Record exact repository SHAs and current protocol/package versions.
- Verify publication status from Hex/crates registries, not only local locks. If
  no canonical `0.1.0` artifact is public, reserve that version for the complete
  guarded schema. If any artifact is public, reserve `0.2.0` for this breaking
  contract and reject a patch-level schema change.
- Audit Mix/Cargo bounds and lockfiles. During coordinated development pin path
  dependencies to exact SHAs; for a published breaking release update every
  consumer to the new minor line, rebuild Emerge precompiled NIF/checksum
  artifacts, and validate from fresh lockfiles.
- Add lifecycle terminology without changing behavior:
  - `close_requested`;
  - `eos_emitted`;
  - `lease_owner_drained`;
  - `native_finalize_pending`;
  - `native_finalized`;
  - `native_quarantined`;
  - `sink_drained`.
- Add a single drain correlation value containing camera generation, native
  `session_id`, and pipeline PID/reference.
- Capture baseline counters before implementation:
  - owner active leases/holders, release failures/retries, oldest holder age;
  - native leased requests and lifecycle/release/completion backlog;
  - Emerge pending/current/retired claims and retirement fences;
  - process FD count;
  - camera open/close/reopen outcomes.

### Files

- `/workspace/colibri/membrane_libcamera/lib/membrane_libcamera/source.ex`
- `/workspace/colibri/membrane_libcamera/native/libcamera/src/types.rs`
- `/workspace/colibri/camera/lib/camera/libcamera_pipeline.ex`
- `/workspace/colibri/camera/lib/camera/stream_runtime.ex`
- `guides/internals/video-interop-architecture.md`

### Gate

No code may call a close-request acknowledgement “drained” or “finalized” in a
notification, test, metric, or log.

## Milestone 1 — Add authoritative native finalization acknowledgement

### Native design

Add an asynchronous terminal event from the libcamera service thread:

```elixir
{:libcamera_session_terminal, session_id,
 %MembraneLibcamera.Native.Finalization{
   status: :finalized | :quarantined,
   expected_requests: non_neg_integer(),
   accounted_requests: non_neg_integer(),
   close_requested_at_ns: non_neg_integer(),
   completed_at_ns: non_neg_integer(),
   reason: nil | term()
 }}
```

The exact struct name may follow existing native naming, but the semantic fields
and status distinction are required.

- Register the owner/notification target when the session opens, before close can
  race with a waiter registration.
- On close, mark admission closed and acknowledge the close request before
  beginning potentially multi-second teardown. Only the later terminal event
  proves finalization.
- Replace inline `recv_timeout` finalization with a main-loop-driven
  `:finalizing` substate. Natural quiescence and post-`camera.stop()` callback
  collection have explicit deadlines, but completions and lifecycle commands
  continue through the service `select!` loop between transitions. A synchronous
  libcamera `camera.stop()` call may remain on its required service thread, but
  the service must never sleep or receive-loop waiting for callbacks/releases.
- When the last public lease release moves its request into service-owned
  finalization state, acknowledge that release promptly. `LeaseOwner` drainage
  and native finalization are deliberately separate barriers; the terminal event
  reports later teardown failure.
- Centralize every native terminal path through one `finalize_and_notify`
  function. Do not leave `let _ = finalize_session(...)` sites that discard a
  failure.
- Emit one event after successful callback quiescence, request accounting,
  callback replacement, and session drop.
- On failed stop or accounting mismatch, first register the session/camera in a
  permanent quarantined registry, then retain/forget the session and all already
  returned requests. Every later `Completion::Request` for that session must be
  retained/forgotten by the quarantine path, never dropped by the ordinary
  missing-session branch. Refuse future opens for the quarantined camera.
- Emit `:quarantined` only after that registry is armed, preserving the
  stop/accounting reason.
- Mirror the state machine and injectable failure paths in the mock backend.
- Keep `CaptureHandle` monitor and `Drop` close nonblocking and idempotent.
- Keep the service's cleanup channel unbounded so close/release cannot be lost to
  ordinary command backpressure.
- Do not implement `await_finalized` by waiting inside the service event loop.
  The event loop must continue servicing the final releases.
- Keep a bounded terminal tombstone table keyed by `session_id` for diagnostics,
  duplicate-close behavior, and tests; it supplements rather than replaces the
  push event. Quarantine registry entries themselves are process-lifetime and
  are not evicted.

### NIF boundary

- `Native.close/1` remains a dirty-I/O close-request call and is documented as
  accepted/already-requested, not finalization.
- The final event is sent from the dedicated native service thread using
  `OwnedEnv::send_and_clear`, never from a BEAM scheduler.
- No NIF call waits indefinitely for public holders or callback arrival.
- The event payload must be panic-safe and fully encodable after native session
  state has been removed.
- In this milestone, add a forward-compatible `MembraneLibcamera.Source`
  `handle_info` clause that stashes the matching terminal outcome (including an
  event that races ahead of a later drain request) and safely ignores stale
  sessions. Do not ship a native event to an old source that would crash with a
  callback function-clause error.

### Files

- `/workspace/colibri/membrane_libcamera/native/libcamera/src/service.rs`
- `/workspace/colibri/membrane_libcamera/native/libcamera/src/backend_libcamera.rs`
- `/workspace/colibri/membrane_libcamera/native/libcamera/src/backend_mock.rs`
- `/workspace/colibri/membrane_libcamera/native/libcamera/src/frame.rs`
- `/workspace/colibri/membrane_libcamera/native/libcamera/src/types.rs`
- `/workspace/colibri/membrane_libcamera/native/libcamera/src/lib.rs`
- `/workspace/colibri/membrane_libcamera/lib/membrane_libcamera/native.ex`
- `/workspace/colibri/membrane_libcamera/lib/membrane_libcamera/source.ex`

### Tests

1. Close with no leased request: one successful final event and immediate reopen.
2. Close with one leased request: close-request returns, no final event, reopen is
   busy; release sends one final event; immediate reopen succeeds without polling.
3. Paired preview/analysis holders in both release orders: no event before the
   last holder/native request retires.
4. Duplicate explicit close and handle drop: one terminal event.
5. Disconnect/request failure: same terminal event path.
6. Inject stop failure and missing-request accounting in the mock: typed
   quarantine, no success event, no in-process reopen; late request completions
   enter the quarantine registry and are never ordinarily dropped.
7. Service panic/channel closure: caller gets service-unavailable/unknown, never a
   fabricated final event.
8. Assert event delivery does not prevent release processing and runs no
   `OwnedEnv::send_and_clear` on a BEAM scheduler.
9. Deliver immediate, duplicate, and stale terminal events to the source before a
   drain request; the matching outcome is stashed and no callback crashes.

### Gate

The existing deferred-close test in
`membrane_libcamera_analysis_test.exs` must become event-driven. A successful
final event must make the next `Native.open/1` succeed on its first attempt.

## Milestone 2 — Join source, owner, native, and sink drainage

### `MembraneLibcamera.Source`

- Replace the boolean `draining?` behavior with an explicit drain record holding:
  - EOS-sent pads;
  - owner status;
  - native status;
  - terminal reason;
  - notification-sent flag.
- Mark draining before calling owner or native close.
- Close `LeaseOwner` issue/retain admission first, then request native close,
  exactly once each. No potentially blocking native call may leave retain
  admission open.
- Emit EOS immediately after close requests, never after finalization.
- Handle late frame/frame-set messages in draining mode by releasing the native
  keepalive directly.
- Before the owner terminates, emit a new immutable final-stats notification and
  retain the existing two-field owner-drained notification for compatibility.
  The source stashes both in either order. The snapshot must show zero active
  holders/leases and retain cumulative issue/release/retry/abandonment counters.
- Consume only the matching native `session_id` terminal event; ignore and count
  stale events.
- Notify the pipeline only when both producer barriers are complete.
- During drain, a release callback failure records diagnostics but does not
  terminate the element; infinite retry remains active. A later successful owner
  drain can still complete.
- Convert recoverable running-state native/source errors into the same
  EOS-first terminal-drain path instead of an immediate `terminate` action. Send
  a typed `{:libcamera_drain_started, session_id, reason}` parent notification
  before returning EOS actions so the pipeline arms its join before sink
  acknowledgements can arrive.
- Preserve a fatal path for service panic/unknown ownership, but report it as an
  unacknowledged drain requiring cold restart.

### `Camera.LibcameraPipeline`

- Replace caller-created `drain_pending` with a persistent pipeline drain state
  that exists independently of callers. Its `begin_drain(reason)` operation
  atomically arms source, preview, and optional analysis barriers before
  notifying the source.
- Start that same state machine from an explicit `:drain` call, a source
  `drain_started` notification, or a sink terminal-transition notification.
  A sink cannot emit upstream EOS itself; the pipeline must idempotently request
  source close/EOS.
- Record acknowledgements even when no call waiter exists. A later `:drain` call
  joins the existing result rather than discarding early acknowledgements or
  returning `:drain_already_started`.
- Support multiple drain callers by storing waiters separately from barrier
  state.
- Include the source barrier alongside preview and optional analysis sinks.
- Correlate source/sink notifications by generation, pipeline, native session,
  and sink stream reference.
- Save the first typed terminal error or unknown-ownership result but continue
  collecting every reachable drain acknowledgement.
- Reply `:ok` only when the source and every configured sink report successful
  drainage.
- Reply `{:error, reason}` only after all reachable barriers complete. A missing
  barrier leaves the operation pending so the caller timeout classifies it as
  unknown; the pipeline must not fabricate a result.

### `Camera.StreamRuntime`

- Handle `Membrane.Pipeline.call/3` exits, timeout, typed error, and pipeline death
  explicitly; remove `:ok = ...` assertions.
- Do not call pipeline terminate after a timeout and then hot-restart. Mark the
  pipeline generation unacknowledged and require a cold restart.
- On successful drain, terminate the now-empty pipeline and verify its monitor
  exits normally before starting a replacement.
- Permit restart only for the exact monitor `:DOWN` expected after a recorded
  successful drain and requested normal termination. Every other pipeline exit,
  including spontaneous `:normal` or `:shutdown`, is unacknowledged, sets
  `cold_restart_required`, and schedules no hot restart.
- Expose `cold_restart_required` and the correlated reason through existing
  diagnostics/UI state.
- For target replacement, defer the caller reply until the old pipeline has
  drained and the new one is started; do not overlap camera generations.

Define an executable restart contract rather than a log-only state:

- add `allow_acknowledged_hot_reopen: false` as the initial release default;
- inject a `cold_restart` MFA/behavior, using a host-safe test implementation and
  the target Nerves reboot implementation in `config/target.exs`;
- invoke cold restart at most once per poisoned generation;
- reject later attach/control/restart calls after poisoning;
- if the restart callback fails, remain fail-closed and keep reporting the
  original ownership reason;
- after a drain timeout, leave the old pipeline intact while requesting cold
  restart; do not perform a destructive local fallback that could discard
  holders.

Update `Camera.Application`, `Camera.StreamRuntime`, target/runtime
configuration, and the preview/UI error projection so the state is supervised,
observable, and testable. Carry the source's final owner/native stats snapshot
into a runtime-owned cumulative diagnostics record that survives pipeline
replacement; soak tests must not query a `LeaseOwner` after it has terminated.

### Tests

- Preview-only and paired pipelines wait for source finalization in addition to
  sink EOS.
- Hold the Emerge current frame until preview EOS; prove no deadlock and correct
  ordering.
- Delay owner-drained and native-finalized independently; neither alone completes
  pipeline drain.
- Duplicate/reordered/stale session events do not unblock a replacement.
- A transient native release error retries and eventually completes without
  destroying the pipeline early.
- Timeout, source death, sink death, and every unexpected pipeline death
  (including `:normal`) set `cold_restart_required` and schedule no hot restart.
- Source- and sink-originated errors arm drainage before any acknowledgement;
  drain callers that arrive later observe the preserved result.
- One thousand acknowledged drain/reopen mock cycles produce zero
  `:camera_busy` results.

## Milestone 3 — Move specialized sink lifecycle into the adapter

`Camera.VideoSink` currently owns useful camera observation but should not own a
second implementation of lease transfer and terminal drain.

### Adapter work

Extend `Membrane.VideoInterop.Sink` with a synchronous observer hook or behavior
that can inspect producer metadata and a borrowed frame before transfer:

```text
observer.handle_frame(buffer, frame, observer_state)
  -> {:ok, observer_state}
  -> {:error, reason, observer_state}
```

Requirements:

- the hook completes before the root holder transfers to the consumer;
- it may create only bounded, synchronously retired child holders;
- asynchronous work receives owned bytes/data, never borrowed FDs;
- hook exceptions and errors before transfer are caller-owned: release the
  current holder and enter the adapter's terminal-drain state;
- `on_error: :stop` closes the consumer once, keeps demanding one buffer at a
  time, releases every canonical holder, waits for EOS, and then reports the
  saved error;
- emit correlated lifecycle notifications:

  ```elixir
  {:membrane_video_interop, sink_pid, stream_ref,
   {:drain_started, :ok | {:error, reason} | {:unknown, reason}}}

  {:membrane_video_interop, sink_pid, stream_ref,
   {:drained, :ok | {:error, reason} | {:unknown, reason}}}
  ```

- add a completion policy: standalone sinks may terminate with the saved error
  after EOS, while the camera mode remains alive after its `:drained`
  notification until the pipeline's composite join authorizes termination;
- a consumer exception/invalid receipt is ownership-unknown, not a resolved
  terminal error. Before guarded leases land it notifies `:unknown` and fails
  closed. After Milestone 4, integration tests may permit continued queue drain
  only because the current term/native claim both retain the abandonment guard;
  neither path may guess an explicit release;
- expose one adapter-owned `discard_buffer/1` helper so application modules do
  not duplicate extraction/release logic.

### Camera work

- Move geometry publication, frame metrics, first-frame notification, and legacy
  bounded thumbnail extraction into a camera observer configured on the reusable
  sink.
- Remove or reduce `Camera.VideoSink` to that observer; it must no longer call
  `VideoInterop.consume/2`, implement terminal drain, or release malformed
  transport itself.
- Keep `Camera.Detection.Sink` synchronous and ownership-safe. Reuse
  `discard_buffer/1` for malformed/quarantined/terminating input.
- Convert analysis format rejection from a raise into a typed
  `detection_sink` terminal-drain notification. The pipeline requests source
  close/EOS; the detection sink continues discarding/releasing demanded input
  until that EOS arrives.

### Tests

- Queue at least three managed frames, fail the first observer/consumer transfer,
  and assert every holder releases exactly once before terminal EOS.
- Verify no later frame reaches the consumer after terminal failure.
- Verify the consumer session closes once.
- Verify geometry, metrics, and thumbnail behavior remains unchanged.
- Exercise malformed payload, dual metadata, missing camera metadata, observer
  raise/throw, caller-owned rejection, transferred rejection, unknown consumer
  receipt/exception, and EOS loss.
- Verify `drain_started` always precedes `drained`, stream references correlate,
  and camera completion mode does not self-terminate before the composite join.

### Gate

There is one reusable terminal-drain implementation in
`membrane_video_interop`; camera application code contains observation policy but
no lease lifecycle state machine.

## Milestone 4 — Add per-holder abandonment guards

Orderly drain cannot execute callbacks after a hard element kill. Add a
resource-backed fallback carried by each canonical holder.

### Protocol shape

Extend the canonical lease with an opaque field:

```elixir
%VideoInterop.Lease{
  owner: owner,
  token: token,
  holder: holder,
  abandonment_guard: guard | nil
}
```

The guard is one unique Rustler resource per holder. Its destructor queues:

```elixir
{:video_interop_abandoned, token, holder}
```

The `LeaseOwner` handles this as a fallback release. Explicit and fallback
messages may arrive in either order. The first retires the active holder; the
second is silently idempotent, never invokes the backend callback again, and is
not counted as a contract-level duplicate explicit release. Track the number of
fallbacks that retired an active holder as separate abandonment diagnostics.

### Creation and fan-out

- Add a `LeaseOwner` `abandonment_guard_factory` option implemented by the
  producer NIF with this contract:

  ```elixir
  factory.(owner_pid, token, holder)
  #=> {:ok, opaque_guard_resource} | {:error, reason}
  ```

  Invoke it only from an ordinary BEAM/NIF call context, never from the
  libcamera service thread.
- Construct the root guard transactionally after `{token, holder}` allocation and
  before acknowledging `issue/3`.
- Construct a fresh guard for every retained child before acknowledging
  `retain/2`. Change the retain reply to
  `{:video_interop_retained, request_ref, {:ok, child_guard}}`, and build the
  child lease by replacing both `holder` and `abandonment_guard`; `%{lease |
  holder: child}` would incorrectly copy the parent guard.
- If a retain reply arrives after alias cancellation or caller death, dropping
  the undeliverable reply must drop its child guard. Cancellation and fallback
  release may race and must be idempotent. Factory failure registers no child
  holder and returns a retain error.
- Guard creation failure publishes no holder. Root failure keeps the backend token
  owner-transferred and releases it through the existing callback/fallback.
- The owner must not retain a reference to a successfully published guard; the
  frame/consumer claim is what keeps it alive.
- Keep `video_interop` as a pure Elixir package that loads no NIF. Guard creation
  is supplied by the producer NIF using reusable support from the
  `video-interop` Rust crate.

### Rust prepare/claim rule

`video-interop` must save the opaque guard term in the `PreparedLease` owned
environment:

```text
BEAM frame owns guard
  -> prepare saves guard but caller still owns release
  -> claim transfers saved guard with ClaimedLease
  -> native current/pending/retired claim keeps guard alive
  -> native retirement sends explicit release and drops guard
  -> guard's later abandonment message is a silent no-op
```

Dropping an unclaimed prepared frame must not release while the original BEAM
frame is still live. No consumer may decode only `{owner, token, holder}` and
silently discard the guard before asynchronous use.

### Producer integration

- Add reusable guard resource support to the Rust crate's optional `rustler`
  module. Its destructor only enqueues to a lifecycle-owned native dispatcher;
  it never calls `OwnedEnv::send_and_clear` on a BEAM scheduler.
- Replace the existing detached `OnceLock` claimed-release worker at the same
  time. Guard and explicit-release dispatchers need explicit health state,
  resource/NIF lifetime ownership, shutdown, and join semantics so old NIF code
  cannot unload while a worker can execute it. A live guard must keep its
  dispatcher alive; normal producer/consumer drain closes and joins dispatchers
  only after their queues and resources are empty.
- Guard-factory startup failure is an ordinary pre-publication error. After a
  holder is published, dispatcher enqueue/channel failure or worker panic is
  fatal lifecycle corruption: emit the emergency diagnostic through an
  independent path and abort the VM process rather than log and continue with a
  potentially reused live buffer.
- Expose thin producer-NIF constructors in:
  - `membrane_libcamera` for camera frames;
  - Emerge for headless PRIME frames.
- Configure both producer `LeaseOwner`s with the guard factory.
- Require a non-nil guard at the strict Membrane transport boundary. Test/fake
  producers may use a fixture resource, not a bare promise of cleanup.
- Preserve the guard through `Membrane.VideoInterop.put_frame/2`,
  `fetch_frame/1`, Emerge preclaim decoding, retained fan-out, and all schema
  fixtures.

### Prototype gate before full rollout

Before changing every repository, prove in an isolated `video_interop` schema
fixture that:

1. killing a process that holds the sole guarded frame eventually releases one
   holder;
2. killing a process with a guarded frame in a private queue does the same;
3. a claimed native frame remains leased after the original BEAM term dies and
   releases only when the native claim retires;
4. root and retained child guards are distinct;
5. explicit normal release does not increase the existing duplicate-release
   counter when the guard later drops;
6. guard-worker startup/send failure is observable and treated as fatal;
7. resource registration works when the shared Rust crate is linked into both
   Emerge and libcamera NIFs;
8. dispatcher resources shut down and join cleanly, and NIF unload/code-upgrade
   behavior cannot strand a live guard on any supported release target;
9. injected post-publication enqueue failure/worker panic takes the defined fatal
   process-abort path rather than continuing;
10. the core-only Rust crate still builds without Rustler.

If any prototype invariant cannot be proven, stop before schema rollout and
choose an external custodian/escrow design. Do not claim that changes to
Membrane callbacks alone solve `:kill`.

### Compatibility

This is an exact schema change. Update Elixir structs, Rust `NifStruct` decoding,
test fixtures, producer NIFs, consumers, and adapter atomically. If every
canonical `0.1.0` artifact is still unpublished, land this before the first
release. If any canonical artifact has been published, release the breaking
contract as `0.2.0`, update Mix/Cargo constraints and locks together, rebuild
Emerge precompiled NIFs/checksums, and require a cold atomic upgrade. Never place
this schema break in a `0.1.x` patch accepted by old `~> 0.1.0`/Cargo `0.1.0`
consumers.

### Residual semantics

- The guard covers abandoned BEAM terms while the VM and producer release worker
  remain alive.
- It does not make OS/VM kill run destructors.
- It does not make copied same-holder fan-out safe: an explicit release from one
  copy can still retire the holder while another copy exists. Unique holder
  fan-out remains mandatory.
- It does not authorize hot restart after an unacknowledged pipeline loss; the
  runtime still requires the composite drain acknowledgement.

## Milestone 5 — Fault injection and validation

### `video_interop`

Add coverage for:

- transactional root/retain guard construction failure;
- guarded holder abandonment and explicit-release ordering;
- prepare versus claim guard retention;
- release worker unavailable/stopped;
- owner death, waiter timeout, callback retry, and drain completion;
- zero staged host test NIFs after `mix test`.

Run:

```bash
cd /workspace/video_interop
mix format --check-formatted
mix test
mix hex.build
cargo fmt --all -- --check
cargo test --workspace
cargo test -p video-interop --no-default-features
cargo clippy --workspace --all-targets -- -D warnings
cargo clippy -p video-interop --no-default-features --all-targets -- -D warnings
```

### `membrane_video_interop`

Add real-pipeline coverage for:

- terminal observer/consumer error with prefetched holders;
- orderly terminate versus EOS;
- sink process kill with guarded queued frames;
- strict rejection of unguarded canonical Membrane transport;
- no double release on caller-owned/transferred receipts.

Run:

```bash
cd /workspace/membrane_video_interop
mix format --check-formatted
mix test
mix hex.build
```

### Emerge

Validate:

- headless PRIME guard construction/issue failure ownership;
- native pending/current/retired claims preserve the guard;
- target/session close retires the final guard;
- deprecated external-PID recipient death eventually releases guarded frames;
- explicit and implicit sync paths remain unchanged;
- renderer gauges and FDs return to baseline after drain.

Run the repository-required `cargo test`, `mix test`, Clippy, formatting, and
`./ci-tests.sh` gates.

### `membrane_libcamera`

Add mock/native tests for:

- immediate/deferred/quarantined final events;
- late post-close frame messages;
- paired branch release order;
- release retry before finalization;
- source death and guard fallback;
- one thousand mock drain/reopen cycles with first-attempt reopen;
- lifecycle event/counter balance and FD flatness.

Run:

```bash
cd /workspace/colibri/membrane_libcamera
mix format --check-formatted
mix test
cargo fmt --all -- --check
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
```

Native-libcamera validation remains target/pkg-config dependent; mock success is
not hardware acceptance.

### Camera application

Add tests for:

- preview-only and PiSP topology composite drain;
- observer terminal drain and saved error;
- drain timeout/exit handling;
- no hot restart after unacknowledged `:DOWN`;
- stale generation/session event rejection;
- target replacement waits for successful old-generation drain;
- release build contains only target-architecture production NIFs.

Run `mix test`, formatting, target compile, and RPi5 release construction. Clean
and rebuild path dependencies before inspecting release ELF architectures.

## Milestone 6 — Hardware acceptance and rollout

### Matrix

Test every supported camera/sensor mode and both preview-only and paired
preview/analysis topologies with:

- minimum, default, and maximum supported buffer counts;
- 30 and 60 fps;
- Wayland and DRM presentation;
- explicit sync and
  `EMERGE_SKIA_HEADLESS_PRIME_FORCE_IMPLICIT_SYNC=1`;
- repeated target hide/show and target recreation;
- controls/autofocus updates during capture;
- orderly stop/reopen every one to five minutes.

### Soak

- 24-hour preview-only soak per hardware class.
- 48-hour paired PiSP/preview soak per hardware class.
- 10,000 rapid acknowledged stop/reopen cycles.
- 100 injected abrupt pipeline deaths, each followed by a cold VM/firmware
  restart; these test containment, not hot recovery.

### Pass criteria

For every acknowledged stop:

- owner active leases and holders are zero;
- native leased requests are zero;
- native status is `:finalized`, never `:quarantined`;
- Emerge pending/current/retired claims are zero;
- submitted and released canonical holder totals balance over an unreset
  observation window;
- created/released retirement fences balance for paths that use them;
- process FD count returns to baseline tolerance (`baseline + 2` maximum);
- immediate reopen succeeds on the first call;
- no release-worker error, service panic, duplicate final event, stale-event
  acceptance, request-accounting alarm, or monotonic RSS/FD/lease-age growth.

PiSP remains disabled in release configuration until the paired soak passes.

## Deployment sequence

1. Land the native finalization event plus the forward-compatible source stash
   handler; no emitted event may crash the old source state machine.
2. Land camera runtime restart lockout and cold-restart policy first. From this
   point every unacknowledged pipeline exit fails closed.
3. Land the pipeline's persistent source barrier next. With an older source it
   must time out/cold-restart, never fall back to sink-only success.
4. Land the new source composite acknowledgement and enable first-attempt reopen
   only in tests; keep production hot reopen disabled.
5. Land adapter observer/typed terminal-drain reuse and migrate camera off its
   custom lifecycle path.
6. Pass the abandonment-guard prototype gate.
7. Atomically update `video_interop`, Rust `video-interop`, Emerge,
   `membrane_video_interop`, `membrane_libcamera`, demos, and camera for the new
   lease schema and version line.
8. Re-run all host suites and target release hygiene checks from clean lockfiles
   and dependency builds, including rebuilt precompiled NIF checksums.
9. Deploy canary firmware with PiSP and acknowledged hot reopen disabled.
10. Enable acknowledged hot reopen after soak. Enable PiSP separately and last.
11. Publish immutable packages only after sibling path dependencies and Cargo
    patches are removed, in dependency order.

Do not hot-upgrade one NIF, schema, adapter, producer, or application. Drain the
old stack completely or cold restart into the new complete artifact set.

## Rollback gates

Immediately disable hot reopen, restore the last complete artifact set, and cold
restart if any of the following occurs:

- acknowledged drain followed by `:camera_busy`;
- drain success with any nonzero owner/native/Emerge retirement gauge;
- native final event for the wrong session/generation;
- duplicate or missing terminal event;
- frame/backend reuse before final holder retirement;
- release worker or libcamera service panic;
- quarantine/request-accounting failure;
- drain success after injected EOS/finalization loss;
- monotonic FD, RSS, holder-age, or native-request growth.

Never roll back only the camera application or only a NIF.

## Completion criteria

This plan is complete only when:

1. camera pipeline `:drain` success proves sink EOS, owner drainage, and exact
   native session finalization;
2. same-process immediate reopen succeeds without retry after every acknowledged
   stop;
3. application sink code no longer owns canonical consume/release/drain logic;
4. killed Membrane queues containing guarded canonical frames retire their
   holders in integration tests while the VM remains alive;
5. abnormal or unknown shutdown never schedules a hot restart;
6. all cross-repository host/target gates and hardware soak criteria pass;
7. the architecture guide is updated to remove the resolved limitations and
   retain only the true VM/service-failure residual risks.
