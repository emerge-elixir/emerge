# Platform Runtime Architecture Differences

Status: explanatory note, 2026-05-20

## Why this note exists

The intended architecture is that Linux/Wayland and macOS share the same input,
tree update, and retained-rendering semantics. They do share substantial code,
but they do **not** currently run it through the same orchestration topology.
This note explains the differences so future fixes can converge the platforms
instead of adding backend-specific behavior.

## Shared core

Both Linux/Wayland and macOS use the same core Rust modules for most framework
semantics:

- `events/runtime.rs`
  - `DirectEventRuntime` owns listener dispatch, hover/focus/text/slider state,
    drag-scroll state, inertia, stale listener-lane buffering, and runtime
    overlay listeners.
  - `EventRuntimeDriver` wraps `DirectEventRuntime` and emits `TreeMsg`s.
- `runtime/tree_update.rs`
  - `TreeUpdateEngine` applies `TreeMsg`s to the retained tree and returns a
    `TreeUpdateEffect` (`Skip`, `RegistryUpdate`, or `Layout`).
- `events/registry_builder.rs`
  - Builds and refreshes event registries/listeners.
- layout/render modules under `tree/` and renderer scene generation.

So the matching logic, drag-scroll activation, listener lane behavior, tree
mutation semantics, and registry rebuild payload format are intended to be
shared.

## Linux / Wayland runtime shape

Linux/Wayland is actor/channel backed:

```text
Wayland backend/input thread
  -> EventMsg channel
  -> event actor thread (`spawn_event_actor`)
       EventRuntimeDriver / DirectEventRuntime
       emits TreeMsg
  -> TreeMsg channel
  -> tree actor thread (`spawn_tree_actor`)
       TreeUpdateEngine
       emits RegistryUpdate EventMsg and RenderMsg
  -> event actor / render thread
```

Key files/functions:

- `lib.rs`
  - creates `tree_tx/tree_rx`, `event_tx/event_rx`, `render_tx/render_rx`
  - starts `spawn_event_actor(...)`
  - starts `runtime::tree_actor::spawn_tree_actor(...)`
- `events/runtime.rs`
  - `spawn_event_actor(...)`
  - `EventRuntimeDriver::handle_actor_message(...)`
- `runtime/tree_actor.rs`
  - `spawn_tree_actor(...)`
  - `send_registry_update(...)`
  - `publish_layout_output(...)`

Consequences:

- Event input, tree updates, registry updates, and render scene publication are
  asynchronous relative to each other.
- Listener freshness crosses thread/channel boundaries.
- Registry updates are synchronization messages from tree actor back to event
  actor.
- If a registry update is delayed or dropped, the event runtime can remain on a
  stale listener lane while more input accumulates.
- Channel capacity/backpressure behavior matters for correctness, not only
  throughput.

## macOS host runtime shape

macOS does not currently start the same `spawn_event_actor` +
`spawn_tree_actor` pair for each session. It uses the shared runtime and tree
engine directly inside the AppKit host session flow:

```text
AppKit/native host callback
  -> HostEventRuntime
       EventRuntimeDriver / DirectEventRuntime
       queues TreeMsg in an internal channel
  -> macOS host drains TreeMsg synchronously-ish
  -> TreeUpdateEngine::process_messages(...)
  -> install registry/layout output directly into HostEventRuntime/session
  -> mark session dirty for draw
```

Key files/functions:

- `events/runtime.rs`
  - `HostEventRuntime`
  - contains an `EventRuntimeDriver`
  - owns a local `tree_rx` used to drain runtime-produced `TreeMsg`s
- `bin/macos_host.rs`
  - `handle_runtime_input(...)`
  - `process_runtime_messages(...)`
  - `process_tree_messages_with_policy(...)`
  - `install_layout_output(...)`

Consequences:

- macOS shares `DirectEventRuntime`, `EventRuntimeDriver`, and
  `TreeUpdateEngine`, but not the Linux actor topology.
- Registry rebuilds are installed directly via
  `session.event_runtime.install_rebuild(...)`, not sent through the Linux
  `EventMsg::RegistryUpdate` channel path.
- There is still internal queuing (`HostEventRuntime` has a bounded `tree_tx` /
  `tree_rx` pair), but there is no separate tree actor sending registry updates
  back through a potentially full event channel.
- Many correctness bugs in shared listener/tree logic reproduce on both
  platforms; bugs caused by actor channel backpressure or dropped registry
  updates are Linux/Wayland-specific unless macOS grows the same actor topology.

## Current divergence summary

| Concern | Linux/Wayland | macOS host |
| --- | --- | --- |
| Event runtime semantics | Shared `DirectEventRuntime` | Shared `DirectEventRuntime` |
| Event driver wrapper | `EventRuntimeDriver` in event actor | `EventRuntimeDriver` inside `HostEventRuntime` |
| Tree update semantics | Shared `TreeUpdateEngine` in tree actor | Shared `TreeUpdateEngine` called by host session |
| Event input delivery | `EventMsg` channel to event actor | AppKit callback into host runtime |
| Runtime-to-tree delivery | `TreeMsg` channel to tree actor | local `HostEventRuntime` `TreeMsg` queue drained by host |
| Tree-to-event registry delivery | `EventMsg::RegistryUpdate` channel | direct `install_rebuild(...)` call |
| Render publication | `RenderMsg` to render thread | direct session render state update / dirty flag |
| Backpressure risk | event/tree/render channels | local queue + host loop; no tree->event registry channel |

## Why this mattered for the scroll-stuck bug

The Linux Wayland report said:

- drag-scroll canvas
- move into nested vertical scroll
- canvas/nested scroll get stuck
- wheel also stops on stuck targets
- scrolling another container unsticks them

That pattern strongly suggests stale listener registry state. Because it is
reported on Linux Wayland, the actor/channel path is especially relevant:

- event runtime marks listener input stale after scroll-like tree messages
- tree actor must publish a registry update to make the event runtime fresh
- if that registry update is not sent/received, wheel and drag can both stop
  matching the expected target until some later unrelated scroll publishes a
  newer registry update

On macOS, the equivalent shared stale-lane logic exists, but tree-to-event
registry installation is direct in the host loop. Therefore a dropped
`EventMsg::RegistryUpdate` cannot be the macOS failure mode in the current
architecture.

## Desired convergence direction

The long-term goal should be one orchestration model, not merely shared helper
modules. Options:

1. Move macOS sessions onto the same event actor + tree actor topology as
   Linux, with AppKit constrained to backend/window/render responsibilities.
2. Or extract a single platform-neutral runtime pump that both Linux actors and
   macOS host call, with identical backpressure/registry synchronization
   semantics and tests.

Either way, correctness-sensitive behavior should be specified at the shared
boundary:

- every `TreeMsg` that makes listener data stale must guarantee a corresponding
  registry response or explicit stale-lane cancellation
- registry updates must be reliable synchronization messages
- direct host and actor-backed paths must run the same stale-lane, registry
  install, and buffered-input replay test scenarios

## Immediate implication

The Linux Wayland stuck-scroll fix needed both sides of registry freshness:
actor-channel registry updates must be reliable, and refreshed registries must
be requested for every interaction layer change. Overlay nearby roots (`above`,
`below`, `on_left`, `on_right`, `in_front`) emit blockers even without explicit
listeners and therefore need registry invalidation when mounted, unmounted, or
moved. Still add shared tests at the `DirectEventRuntime` / `TreeUpdateEngine`
boundary when possible so macOS remains covered for the shared parts of the
behavior.
