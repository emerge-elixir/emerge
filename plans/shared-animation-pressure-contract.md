# D19 — interaction-pressure contract proposal

**Status: deferred design/model proposal; not runtime enforcement or an approved default.**
Owned by [later work L2](later-animation-runtime-qualification.md#l2-input-pressure-and-whole-runtime-progress--approval-required),
not the animation closeout. The historical proposal/evidence below is preserved.
This replaces “add a queue cap” with a specific failure/admission proposal. Existing
D12–D18 code remains unchanged and uncommitted. No P1–P9 package closes.

## Decision proposed

Use **explicit terminal renderer failure** when a noncoalescible operation cannot
be admitted within configured interaction limits. Do not silently evict input,
continue after losing an operation, automatically retry a partly observed operation,
or allocate another unlimited staging queue. Restart means a new renderer/mount
lifetime, not resumption of the failed one.

A finite store cannot indefinitely accept lossless input while its consumer makes
no progress. Portable producer backpressure would require separately reachable
control/ack paths and bounded upstream storage at every producer. In particular,
blocking the Wayland compositor callback is not acceptable. Stop-on-overflow is the
recommended portable contract; a future credit-aware producer mode would need its
own capability and integration proof, not a fallback that silently drops input.

**Approval still needed:** terminal failure as public product behavior; target-device
record/capacity/input-size limits and release/default compatibility policy. There is
no accepted whole-renderer budget from which safe numbers can currently be derived.
Do not invent a default, interpret zero as unlimited, or claim legacy configurations
are bounded. This is unrelated to public animation enablement; no animation flag.

## Source audit: why a tail cap is insufficient

| Boundary | Current behavior / required work |
|---|---|
| Wayland `backend/wayland/runtime.rs::try_send_wayland_event` | `Full(_)` discards **any** `EventMsg`, including keys/commits, before the listener FIFO. Existing full-channel test checks nonblocking behavior, not semantic delivery. Fix this ingress before claiming end-to-end losslessness. |
| DRM `drm_input.rs::push_input_blocking` | Retries with 10ms send waits and checks Stop. Waiting preserves that native message, but says nothing about kernel/libinput backlog bounds. Do not confuse droppable DRM presentation timing with physical input. |
| `events/runtime.rs::handle_input_event` | Raw observer forwarding precedes listener buffering. A check in `buffer_input` would be after observation and producer allocation. |
| Driver `dispatch` / `EventCommands` | Local mutation and callbacks can precede packet construction; full strings, command vectors and target evidence can expand from a tiny input. A cap only in `queue_tree_packet` is too late. |
| Host commands / clipboard / synthetic input | Native edit methods bypass the stale raw-input FIFO. Clipboard, IME and virtual-key timers must use the same admission rules, not a second unchecked entrance. |
| Outbox → channel → tree/host batch | Ownership moves, but storage still exists. Dequeue, send success, or receipt arrival must not refund live capacity. |
| macOS `bin/macos_host.rs::read_frame` | Allocates a peer-declared u32 frame length before reading its payload. Frame/class size checks must precede allocation; tree/image/font frames need their own limits, not a blanket small text limit. |
| `events.rs::send_input_event` and element callbacks | Native send is not BEAM/host consumption. Native queue credits cannot bound subscriber mailboxes or retract callbacks. |

Exact source excerpts/hashes: `artifacts/shared-animation-remaining/pressure-contract/audit.json`.
These are source findings, not Wayland/macOS device execution results.

## What is bounded

Proposed configuration names (not implemented): `interaction_limits.max_records`,
`max_owned_capacity_bytes`, and `max_input_bytes`, all finite positive values.

The first enforceable scope is **managed native interaction transport/construction**:
producer-owned admitted messages, stale FIFO storage, dispatch scratch/effect packets,
outbox, event-owned channel messages and receiver-owned batches until disposal or an
explicit accounted handoff. Include nested command vectors, mount evidence, owned
string capacities, spare queue capacity and construction overlap. Count bounded
record/shell overhead as well as bytes. Caller-owned payloads and framework/SDK
allocations already made before admission are outside this claim and need ingress
size checks or bounded conversion paths.

This is not an allocator-usable-size, RSS, BEAM, GPU or whole-renderer cap. Persistent
text/model state, registry snapshots, fonts/assets/scenes and subscriber queues are
separate domains. Moving a string into persistent tree state does not free it: retain
its charge or perform a budgeted handoff into that domain, never relabel a live
allocation as disposal. A whole-runtime bound requires those domains to be bounded
and measured too.

Numerical limits must fit the target's total memory envelope and supported largest
input/effect construction. D18's 65,536 outbox slot bytes exclude nested storage and
cannot establish a safe default. One accepted small keystroke can require copying a
large existing text value; an input-size limit alone is insufficient.

## Admission and ownership rules

1. Reserve record and capacity credit **before** accepting/copying managed input.
   Check wire/SDK lengths before unbounded decode or clipboard conversion. A failed
   charge calculation is overflow, not wrapping arithmetic or a zero charge.
2. Associate credits with storage/last ownership, not enqueue/dequeue counts. Move
   permits across thread/queue boundaries; duplication requires additional credit.
   Spare deque/vector capacity remains charged until its allocation is released.
3. Reserve expansion and old/new coexistence before building effects, growing
   containers, copying payloads or replacing buffers. A final result fitting the cap
   does not prove its construction peak fits. Post-allocation inspection alone is
   not enforcement: use pre-reserved bounded storage or audited capacity-growth
   bounds and fallible allocation paths. Native allocator integration is unproven.
4. Preserve existing adjacent-only cursor/resize/scroll coalescing and semantic
   barriers. No key, commit, composition or button-edge eviction. New packets must
   not bypass old ones, and source mount/receipt envelopes remain intact.
5. Gate all raw, host-command, clipboard, replay and synthetic/timer paths. Do not
   introduce full-tree clones, a second geometry engine or rollback snapshots.
6. Stop/fault signaling must not acquire data credit. Registry acknowledgments need
   an independently progressing, bounded response path; they are not free arbitrary
   “control” payloads. Large registries still need their own retention accounting.
   Pausing reads on a shared FIFO with the needed receipt behind data can deadlock.
7. Native send success/registry acknowledgment does not prove presentation, nor
   does it refund storage still held by another native or BEAM/host owner.

## Terminal semantics and public boundary

- First overflow atomically latches a fixed-size renderer-local reason/stage/limit
  record, closes admission, requests independent stop and preserves the first reason
  through later Stop/disconnection. No offending strings or unbounded error history.
- Pending work may be abandoned **because the renderer has failed**, not silently
  while it remains healthy. In-flight previously admitted work/callbacks can finish
  until quiescence. Already-emitted callbacks and displayed pixels are not reversible;
  this is not global transaction rollback or atomic cancellation of native calls.
- No partial collected native event packet is newly published after construction
  fails. Local changes made before that failure are discarded with the terminal
  runtime, not used as a new live baseline. Already-enqueued/in-flight operations
  retain normal authority checks until workers observe termination.
- Provide an explicit terminal notification **and** queryable latched status. A lone
  log line, dropped notification, `running=false` or thread return is insufficient.
  Notification delivery must not be prerequisite to stopping; querying must not wait
  for the failed data path. Subscriber consumption remains a separate contract.
- Public error shape must distinguish resource exhaustion from ordinary unhandled
  input. In particular, do not turn host overflow into `false` and let Cocoa retry
  through responder fallback. Terminal failure is nonretryable for that renderer.
- Proposed reason: `:interaction_overflow` with bounded numeric/stage metadata;
  exact API/status shape is future work. Use fixed atoms, correct Rustler encodings,
  safe shared-state access and existing appropriate schedulers. Do not change the
  current NIF signatures by implication of this document.
- macOS must negotiate limits and report the same terminal reason. Matching host/
  Elixir protocol artifacts and a coordinated version change are required; do not
  silently enable this against an older host. Partial frame/read/write stalls need
  an independent shutdown route, not a Stop frame behind the stalled payload.
- Preserve D18 signaling independence. Completion still waits for native teardown
  and joins; blocked drivers/callbacks/assets are not made preemptible by a budget.

## Model qualification and implementation sequence

`artifacts/shared-animation-remaining/pressure-contract/model.py` models declared capacity credits, owner transfers,
construction overlap, matching-vs-foreign receipt assumptions, first terminal reason
and delayed disposal. Nine tests and bounded state exploration validate **the model**,
not Rust allocation, causal receipt identities, native FIFO semantics or liveness.
All explored configurations retain frontier states at depth 12; this is not exhaustive
for arbitrary histories. There is no runtime pressure enforcement in this slice.

Next implementation slices, after policy/default approval:

1. Terminal status/error propagation across resource, actor, backend and macOS host;
   prove native fault observation does not rely on saturated data queues.
2. Accounted storage/admission with audited allocation bounds; ingress/frame limits;
   pass a lease through construction/channel/receiver ownership. Close the current
   Wayland silent-drop path using the approved failure contract, not another FIFO.
3. All entry paths and mid-dispatch failures; exact boundary/oversized payload,
   overflow before/after callbacks, remount, Stop/fault races, delayed peers and
   destruction. Exercise native actor + host + public ExUnit paths.
4. Measure all domains, calibration workloads and constrained-device peak/disposal;
   choose defaults and qualify feature/platform artifacts. No memory/performance or
   P1–P9 closure before those gates.
