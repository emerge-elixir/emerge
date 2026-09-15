# Isolated native event-pressure experiment

## Bottom line

**On this desktop, ordinary-rate input did not come close to filling the event
channel.** A separate event thread helps substantially. It does not remove the
causal wait for tree responses, or protect against a stopped event thread/a producer
sending tens of thousands of records in milliseconds.

Completed evidence: `run-02/` — **90 separate processes**, three repeats for 30 cases,
release build under the exclusive performance lock. Ryzen 9 7950X, 16 cores / 32
threads. Source identity:
`source-sha256:3bdf8fb7db17961302c44398ab2b8dc270e4257b8e8526288aae0f4638b011da`.
`run-01/` is a build-only attempt; unavailable `/usr/bin/time` prevented measurement.
No RSS numbers are claimed.

## What actually filled?

- The production event channel has **4,096 slots**. Its `Full` condition is distinct
  from the actor's unbounded listener FIFO and the outgoing FIFO.
- Both 1-node and 20,000-node fixtures handled **30 and 125 text edits/sec** without
  a listener backlog. Incoming channel peak: **1**; no full-channel rejection.
- Both handled **8,000 pointer updates/sec** without backlog or channel rejection.
  This is synthetic pointer traffic, not a measured physical device poll rate.
- The 1-node fixture also kept up with **8,000 edits/sec**: incoming peak 1–7 and
  listener peak 0–5. This is a tested rate, **not a measured maximum throughput**.
- With 20,000 nodes and **1,000 edits/sec**, listener backlog peaked at **736–812**,
  while incoming channel peak stayed **1**. After two more seconds, **225–480 edits**
  remained buffered. Native tree-response throughput, not ingress reception, limited
  this case. At 8,000 edits/sec, backlog was approximately 7,800.
- Pausing tree forwarding for one second at 30 edits/sec left **29 buffered inputs**
  (1,280 requested FIFO slot bytes); at 1,000/sec it left **999** (40,960 slot bytes).
  The incoming channel did not fill. 8,000 pointer updates coalesced to **1** retained
  position during a genuine causal wait.
- 1,000 composition updates during a one-second tree pause used the **512 tree
  channel slots plus 488 outgoing packets**. Composition is noncoalescible here but
  does not stale the listener lane like a content edit.
- Pausing the **event thread itself** for one second at 8,000 inputs/sec filled all
  **4,096 slots** and rejected **3,904** further inputs, for both edits and pointers.
  With no consumer, that rate fills the channel in **0.512 seconds**. Coalescing
  cannot help until the event thread runs again.
- Unpaced 20,000-edit bursts (about **1–4ms**, depending on case/process) also filled
  the incoming channel. Rejections varied with scheduling; these were millions of
  offered records/sec, not human typing. The 100,000-pointer burst also filled it.

If the event thread were completely unable to consume, the arithmetic is:

| Offered records/sec | Time for 4,096 records |
|---:|---:|
| 30 | 136.5 seconds |
| 125 | 32.8 seconds |
| 1,000 | 4.10 seconds |
| 8,000 | 0.512 seconds |

These are **zero-consumption calculations**, not normal processing latency.
Thirty/125 *edit operations* per second are deliberately aggressive typing proxies;
physical typing also includes key down/up records. A key record, a text mutation,
a pointer move and a paste are not equal-cost operations.

## Is this realistic?

Normal typing and even mouse-like 1k/8k input rates did not overload this isolated
native path. The observed overflow required a synthetic flood or a deliberate long
pause of the event thread. Large-tree editing at 1k/sec produced a substantial
internal backlog without filling the incoming channel—plausible for automation or
an event storm, not ordinary typing.

This supports treating overflow handling as a **defensive fault policy**, not normal
flow control. It does not establish a need for an arbitrary tight default cap.
A slow/hung native callback, clipboard operation, slower target CPU, long text or
large payload changes the result; a single huge paste can consume memory without
thousands of events. No target-device memory default is justified by these counts.

## Harness and limitations

- Real production event loop (`run_event_actor`) and real tree actor, separate
  threads. Native event/tree capacities 4096/512. A forwarding gate adds one thread
  and another 512-slot tree-side channel, present in **all** cases. It pauses before
  reading the event driver's outbound channel, not inside the layout implementation.
- Static fixtures: one focused text input; large fixture adds fixed-size elements
  with mouse-move listeners in a column. Initial native layout and 20 editing warmup
  events occur before counters/timing reset. Not a general 20k-node app benchmark.
- `edit`: alternating one-character insertion/backspace, keeping text short.
  `pointer`: movement within the input with a mouse-move callback.
  `ime`: changing 32-byte composition text, not growing committed content.
- Producer uses `try_send` and counts full-channel failures, matching the current
  Wayland helper's full/drop policy. This is not compositor/kernel/device execution.
- Raw/callback sinks increment atomics; **no BEAM application callbacks**, actual
  clipboard I/O, reconciliation triggered by user code, GPU drawing or presentation.
  Render scenes use the normal latest-scene sender but are not rasterized/displayed.
- Test-only loop-boundary counters and pause polling add work. Peaks exclude
  intra-dispatch transient allocations. Only FIFO requested slot bytes are recorded;
  strings, packet vectors, maps, other channels/queues and allocator overhead are not.
- Paced runs offer one nominal second of input (first record at time zero). A final
  FIFO control marks completion; recovery is observed for at most two seconds after
  production. Nonsettled runs are then explicitly stopped, abandoning their remaining
  work. `settle_seconds` is not per-event latency and has roughly 1ms polling resolution.
- The stalled-tree pointer case sends one extra commit to establish a genuine
  registry wait; its raw/callback count includes that bootstrap, offered count does not.
- `shutdown_seconds` includes actor/tree joins and synchronous disposal in this
  harness, not a general shutdown bound. No CPU affinity/frequency isolation or
  application-load stress. Three repetitions are not long-duration/TTL qualification.

## Reproduce / evidence

```sh
./scripts/performance-lock.sh --source-revision snapshot-created-under-lock exclusive \
  python3 plans/artifacts/event-pressure-probe/run.py \
  plans/artifacts/event-pressure-probe/new-run
python3 plans/artifacts/event-pressure-probe/summarize.py \
  plans/artifacts/event-pressure-probe/new-run
```

`run-02/` contains source archive/hash, host/compiler/environment identity, build
logs, immutable binary hash/path, case list, all raw logs, `results.json` and
`table.md`. Sources are checked before/after measurement; no tests/builds run during
measurement processes. The binary is retained outside the repository at the recorded
temporary path. Old source identities intentionally describe their own snapshots.

No pressure policy, API/default or production behavior changed. Instrumentation is
compiled only for tests with `bench-diagnostics`; the probe is ignored by default.
No P1–P9 closure, physical platform qualification or new animation performance gate.
