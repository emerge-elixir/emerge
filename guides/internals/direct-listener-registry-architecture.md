# Direct Listener Registry Architecture

## Purpose

Define the event architecture as a direct listener system:

- `input event -> listener match -> actions`
- no trigger translation layer
- no job compilation layer
- deterministic behavior through listener stack order and deterministic bucket pass order

This document is architecture-only.

## Core Model

Listeners are simple translators:

- `match -> actions`

Behavior differences are represented by different listeners, not by fallback branching in runtime handlers.

## Decisions Locked In

1. Listeners match raw input events directly.
2. Listener matching API is minimal: `match(input) -> bool`.
3. Listeners carry `element_id` for followup/source tracking.
4. Matcher identity for followup source lookup uses matcher enum type only (enum discriminant); matcher payload does not participate in identity.
5. Buckets are deterministic resolution passes, not event categories.
6. Most inputs use one pass; multi-pass is only used when one input must resolve multiple independent behaviors.
7. Listener precedence is bucket stack order (`first matched wins`).
8. Registry is the state tracker for interactive behavior.
9. Tree actor builds a **base listener registry** during the same tree walk that produces draw commands.
10. Tree actor always sends `EventMsg::RegistryUpdate` after every rebuild.
11. Event actor composes an **effective registry** by adding runtime overlay listeners to the top of related base buckets.
12. Overlay listeners are recomputed from runtime state when `RegistryUpdate` is received.
13. Runtime followup listeners are rematerialized from current base listeners on every rebuild.
14. If a tracked source listener (`element_id + matcher enum type`) no longer exists in base registry, that tracker/followup is dropped.
15. Pointer `on_click` starts two trackers on left press: click tracker and drag tracker.
16. Pointer `on_press` uses the same click/drag tracker flow as pointer `on_click`.
17. `on_press` also includes focused keyboard listeners for Enter press.
18. Drag threshold promotion to `Active` drops click/press tracker.
19. No `consumed` flag is used for click/press suppression.
20. If an input results in at least one matched action, registry is stale.
21. While stale, listener lane buffers/coalesces input and does not dispatch.
22. `EventMsg::RegistryUpdate` is the only freshness signal.
23. If matched actions emit zero `TreeMsg`s, event runtime sends `TreeMsg::RebuildRegistry` to force rebuild and update.
24. Event runtime does not classify actions as mutable vs non-mutable.
25. Registry stale affects listener lane only; observer lane continues forwarding normally.
26. Text-input edits are independent of `on_change`; `on_change` gates only `:change` event emission.
27. Hover active tracking applies to both style hover and event-only hover listeners.
28. Focus transitions use runtime requests (`RequestFocusSet`, focused Tab cycle requests) rather than direct per-listener winner branching.

## Listener Definition

A listener is a declarative registry record:

```text
Listener {
  element_id: Option<ElementId>
  matcher: ListenerMatcher
  compute: ListenerCompute
}
```

Notes:

- Listener behavior is still `match -> actions`.
- `element_id` exists for source tracking and followup rebinding.
- Followup source identity is `(element_id, matcher enum type)`.
- Matcher payload is not part of source identity.
- Followup matcher payload is always taken from the current source listener during rebuild.
- Listener order in a bucket determines precedence (`first matched wins`).

## Action Definition

Actions are ordered and run in order when listener matches.

Action classes include:

- tree updates (`TreeMsg`)
- app-facing event emits
- event-runtime transient state mutations

All matched actions are treated equally for staleness.

## Matching Semantics

Within one bucket invocation:

1. Evaluate listeners in bucket stack order.
2. First listener whose `match(input)` returns true wins.
3. Execute that listener's computed actions in order.
4. Stop this bucket invocation.

No fallback-chain logic in handlers.

## Bucket Model

Bucket = deterministic resolution pass.

- Not keyed as an event taxonomy.
- Introduced only when one input must drive multiple independent deterministic resolutions.
- Known case: cursor transition requires two passes:
  - leave pass
  - enter pass

Pass order is deterministic and explicit.

## Registry as State Tracker

Registry rebuild reflects current interaction state and controls listener presence/order.

Example (hover semantics):

- when an element is in hovered state, registry shape includes leave behavior for that state
- when it is not hovered, registry shape includes enter behavior for that state

This is why runtime does not need pointer snapshot hover tracking.

## Registry Ownership and Composition

Tree and event actors have distinct responsibilities:

- **Tree actor** owns base registry construction from tree/layout state.
- **Event actor** owns transient runtime overlay listeners derived from interaction runtime state.
- Dispatch always uses `effective_registry = base_registry + runtime_overlay`.

Composition rules:

- runtime overlay listeners are placed at top precedence in related buckets
- bucket ordering remains deterministic
- composition is deterministic for identical base registry + runtime state
- runtime overlay composition is done when `RegistryUpdate` is received

### Runtime Followup Recomposition

When runtime state requires followup listeners (click/press release, drag threshold, drag active release):

1. Event runtime checks tracked source identity `(element_id, matcher enum type)` against current base registry.
2. If source exists, runtime rematerializes followup listener from current source matcher payload.
3. If source does not exist, runtime drops that tracker/followup.
4. Effective registry is then composed with deterministic overlay precedence.

## on_click and on_press Behavior

### Shared pointer tracker flow

1. Base listener for left press matches.
2. Matching press emits runtime actions to start:
   - click/press tracker
   - drag tracker (`Candidate`)
3. Click/press tracker stores:
   - `element_id`
   - matcher enum type of source listener

### Overlay behavior for click/press tracker

1. On rebuild, runtime validates source listener via `(element_id, matcher enum type)`.
2. If valid, runtime registers release followup listener using current source matcher payload.
3. If invalid, runtime drops click/press tracker.

### Drag interaction precedence

1. Drag `Candidate` registers threshold listener above click/press release followup.
2. Threshold match promotes drag to `Active` and drops click/press tracker.
3. Drag `Active` registers top-priority left-release listener that clears drag runtime state.

### on_click vs on_press

- `on_click` uses pointer tracker flow only.
- Pointer `on_press` uses the same pointer tracker flow.
- `on_press` additionally registers focused keyboard listeners for Enter press.
- Text-input edit listeners continue to mutate content even when `on_change` is disabled.
- `on_change` controls whether edit listeners include `:change` event emission.
- Text-input command listeners (`cut`/`paste`) issue runtime command requests with
  `on_change`-gated change emission.

## Event Runtime Architecture

Runtime has two lanes:

- **Listener lane**: match listeners and execute actions.
- **Observer lane**: forward input only.

Per incoming input event:

1. Normalize/coalesce for listener lane.
2. Forward observer input immediately (subject to mask), regardless of stale state.
3. If `stale == true`, enqueue/coalesce for listener lane and stop listener dispatch.
4. If `stale == false`, build deterministic bucket invocation list for this input.
5. Execute each required bucket invocation (`first matched wins` per invocation).
6. Listener actions may start/clear runtime trackers (click/press, drag, focus-related transient state).
7. If at least one action matched across the input's invocations:
   - set `stale = true`
   - if zero `TreeMsg` were emitted, send `TreeMsg::RebuildRegistry`
   - pause listener lane until fresh registry arrives
8. On `EventMsg::RegistryUpdate`:
   - replace `base_registry`
   - recompute runtime tracker-derived followups from current base listeners
   - compose `effective_registry`
   - clear `stale`
   - resume listener lane with buffered/coalesced inputs

Observer behavior while stale:

- observer forwarding continues live
- buffered listener-lane inputs are not replayed to observer (no duplicate forwarding)

## Stale/Fresh Protocol

- matched action(s) => stale
- stale => listener lane paused + buffering/coalescing
- rebuild always emits `EventMsg::RegistryUpdate`
- only `EventMsg::RegistryUpdate` => fresh

No additional freshness message type is used.

`TreeMsg::RebuildRegistry` exists only to guarantee a subsequent `RegistryUpdate` when matched actions produced no tree messages.

## Runtime-State Mutation Actions

Some listeners intentionally mutate event runtime transient state (capture/drag lifecycle, etc.).
This is part of the model.

These mutations are still actions, and they participate in stale marking like all other actions.

A runtime-state change is not special-cased:
it follows the same stale -> rebuild -> `RegistryUpdate` -> resume flow.

## Pointer Click/Drag Lifecycle Example

1. Left press on clickable/pressable listener starts click/press tracker and drag tracker (`Candidate`).
2. Overlay rebuild registers click/press release followup, then drag threshold listener above it.
3. Pointer move crossing threshold promotes drag to `Active`.
4. Promotion to `Active` drops click/press tracker.
5. Overlay rebuild registers drag-active left-release listener at top priority.
6. Left release while drag active is handled by drag release listener; click/press followup is no longer present.
7. Drag release listener clears drag runtime state.

Key point:

- suppression comes from deterministic overlay ordering plus tracker transitions
- no separate consumed flag is required

## Guardrails

- No reintroduction of trigger/job translation as core dispatch path.
- No selector-style dispatch logic in runtime handlers.
- Listener logic remains simple (`match -> actions`).
- Deterministic behavior comes from bucket pass order + listener stack order.
- External payload contracts stay stable unless explicitly changed and tested.

## Invariants

- Core path is direct: `InputEvent -> listener match -> actions`.
- `first matched wins` per bucket invocation.
- Buckets are minimal deterministic passes, not event categories.
- Effective dispatch source is always `base_registry + runtime_overlay`.
- Overlay listeners have deterministic top precedence in related buckets.
- Runtime followup source identity is `(element_id, matcher enum type)`.
- Followup matcher payload is always copied from current source listener during rebuild.
- If source listener is missing on rebuild, followup tracker is dropped.
- Drag activation drops click/press tracker.
- Registry stale blocks listener lane only.
- Observer lane remains live while stale.
- Runtime never dispatches listener lane against stale registry.
- Tree actor always sends `RegistryUpdate` after rebuild.
- Only `RegistryUpdate` clears stale.
