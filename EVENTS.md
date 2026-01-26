# Events System

This document describes the retained-mode event architecture for EmergeSkia.
The goal is to route input events to the correct element using `element_id`,
while keeping payload mapping on the Elixir side.

## Overview

- Rust owns hit testing and click tracking.
- Elixir owns event payload mapping (`{pid, msg}`) keyed by `element_id`.
- EMRG encodes event flags (presence only), not payloads.

## Event Flow

The renderer already holds the layout tree. On each tree upload/patch, Rust
builds a registry of clickable elements with their hit bounds. Input events are
then routed using this registry.

Sequence (simplified):

```
Wayland/Winit -> Rust InputHandler -> EventRegistry hit test
                                   -> {:emerge_skia_event, {element_id, :click}}
                                   -> Elixir receives -> lookup element_id
                                   -> send {pid, msg}
```

## Click Tracking

`on_click` is generated on press+release inside the same element.
The hit area is the element frame (padding included).

Sequence:

```
CursorDown -> hit test -> pressed_id = element_id
CursorUp   -> hit test -> if element_id == pressed_id -> emit :click
```

## Rust Responsibilities

- Build an event registry after layout:
  - `EventNode { element_id, hit_rect, z_index }`
- Hit test pointer events against `hit_rect`.
- Track `pressed_id` for click detection.
- Emit events to the input target process as:
  `{:emerge_skia_event, {element_id, :click}}`.

## Elixir Responsibilities

- Track `{element_id => {pid, msg}}` for `on_click` attributes.
- Encode `on_click` as a presence flag in EMRG.
- When Rust emits an event, look up `element_id` and dispatch the stored message.

## MVP Scope

- Only `on_click` (no meta, no payloads in Rust).
- No bubbling/propagation.
- No double-click support.

## Future Extensions

- Mouse enter/leave/move tracking.
- Other mouse events (down/up) and hover state.
- Optional meta payloads for pointer position and modifiers.

## Future-Proofing Plan

1) Clip-aware hit testing
- Maintain a clip stack while building the event registry.
- Intersect clickable frames with active clip rects (padding-aware clip, rounded border clip).
- Prevents clicks on visually clipped content.

2) Scroll offset awareness
- When content scrolling is implemented, offset child hit bounds by scroll_x/y.
- Consider inheriting scroll offsets from ancestor scrollable containers.

3) Optional pointer metadata
- Extend event payloads to include `{x, y, button, mods}`.
- Keep backward compatibility by allowing `{id, :click}` or `{id, {:click, meta}}`.

4) Multiple input targets (optional)
- Continue using a single target by default.
- Add support for subtree-specific targets if needed.

5) Hover and move events
- Track `hovered_id` in Rust and emit enter/leave/move for the topmost hit.

6) Multi-touch and gestures
- Add pointer IDs to track multiple touches.
- Leave gesture recognition in Elixir unless performance needs Rust.

7) Registry invalidation (Implemented)

The event registry must be rebuilt whenever scroll positions change, not just on layout changes.
This is handled by `layout::refresh()` which produces both render commands AND event registry:

```rust
// After scroll changes in events.rs
let output = refresh(&tree_guard);
state.commands = output.commands;
processor.rebuild_registry(output.event_registry);
```

Without rebuilding the registry, click positions would mismatch element locations after scrolling.

## Conclusions

- The MVP is fast and deterministic: Rust handles hit testing; Elixir handles payloads.
- Tradeoffs today: no clipping/scroll offset awareness, no metadata, no bubbling.
- Primary risk: scrollable content will need scroll-offset hit testing to stay accurate.
