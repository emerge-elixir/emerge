# Events System

This document describes the current retained-mode event architecture for
EmergeSkia.

## Overview

- Rust owns hit testing, pointer state, hover state, click detection, and scroll
  request generation.
- Elixir owns payload routing (`{pid, msg}`) keyed by encoded `element_id`.
- EMRG encodes event attributes as presence flags only (no payloads).
- Scrollbar-specific hit testing and interaction state live in
  `native/emerge_skia/src/events/scrollbar.rs` and are coordinated by
  `EventProcessor`.

## End-to-End Event Flow

```
Backend input (Wayland/DRM)
  -> InputEvent
  -> Event actor
       -> sends raw input event to target pid
       -> EventProcessor uses registry for hit testing
            -> emits element event {:emerge_skia_event, {element_id_bin, event_atom}}
  -> Elixir looks up element_id_bin + event_atom
  -> Elixir dispatches stored {pid, msg}
```

Notes:

- Raw input events and element events are both delivered as
  `{:emerge_skia_event, ...}`.
- `element_id_bin` is the `term_to_binary` payload for the element id.

## Event Registry

After each tree upload, patch, or scroll-driven update, Rust rebuilds the event
registry from the current tree.

Each node stores:

- target id
- hit rectangle
- event flags
- self rounded-corner data
- active clip rectangle and clip rounded-corner data

Registry order follows render traversal order. Hit testing scans in reverse, so
topmost elements win.

## Hit Testing Behavior

Current hit testing is:

- clip-aware (including inherited clip intersections)
- padding-aware for clip regions
- rounded-corner-aware (self and active clip)
- scroll-offset-aware for descendants of scrollable containers

Flag filtering happens before geometric checks, so non-listener nodes do not
block listeners behind them.

## Click, Hover, and Button Behavior

- `on_click` is emitted on left-button press+release on the same element.
- `on_mouse_down` and `on_mouse_up` are emitted for left button only.
- Hover state tracks topmost hit and emits `:mouse_enter`, `:mouse_leave`, and
  `:mouse_move` based on listener flags.
- A drag deadzone suppresses click when pointer movement exceeds the threshold
  during a press.

## Scroll-Related Event Behavior

- Wheel and drag scrolling both use the same registry.
- Directional scroll flags are computed from current offset vs max offset.
- EventProcessor converts pointer movement/wheel deltas into scroll requests to
  the tree actor.
- Scrollbar track/thumb hit testing and thumb drag are implemented (track click
  snaps thumb to cursor, then drag continues from that point).
- Scrollbar hover emits axis-specific hover updates for thumb styling.
- After scroll changes, layout/render output and event registry are refreshed to
  keep hit testing aligned with visible content.

## Elixir Responsibilities

- Build and maintain `%{element_id_bin => %{event => {pid, msg}}}` in diff state.
- Encode event attrs as presence flags in EMRG (`on_click`, `on_mouse_*`).
- On Rust element events, resolve and forward stored payloads.

## Supported Element Events

- `:click`
- `:mouse_down`
- `:mouse_up`
- `:mouse_enter`
- `:mouse_leave`
- `:mouse_move`

## Current Limits

- No bubbling/capture propagation.
- No double-click semantics.
- Element events do not include pointer metadata payloads.
- Right/middle buttons are not mapped to element-level down/up events.
- No distinct scrollbar active/pressed visual state beyond hover width changes.

## Possible Extensions

- Optional metadata payloads for element events (position/modifiers/button).
- Optional bubbling/capture model.
- Optional multiple input targets.
- Multi-touch pointer ids and gesture hooks.
