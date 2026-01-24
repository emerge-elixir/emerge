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
