# Scrolling Behavior

This document describes the current scrolling behavior and runtime flow.

## Overview

- Scroll state is owned in Rust.
- Scroll hit testing uses the same event registry used for click/hover.
- Elixir does not receive per-element scroll updates by default.

## Scroll Input Sources

- Wheel input (`CursorScroll`) produces per-axis scroll requests.
- Content drag (`CursorPos` during active press) produces scroll requests after
  a drag deadzone.
- Arrow keys (`up/down/left/right`) produce registry-targeted scroll requests.
- Scrollbar track and thumb pointer input produce axis-specific thumb-drag
  requests (`ScrollbarThumbDragX/Y`) with snap-to-cursor track behavior.
- Drag follows finger-like direction (pointer movement and content movement are
  aligned by request sign handling).

## Unified Registry Usage

The same registry powers click, hover, and scroll:

- Nodes are traversed in reverse for topmost-hit behavior.
- Scroll hit tests use directional flags (`can scroll +/- on each axis`).
- Flag filtering runs before geometric checks.
- Focusable nodes also carry keyboard scroll matchers and focus-reveal matchers.

## Runtime Flow

```
CursorScroll / drag CursorPos / arrow Key
  -> EventProcessor::scroll_requests
  -> TreeMsg::ScrollRequest {id, dx, dy}
  -> tree.apply_scroll(id, dx, dy)
  -> layout_and_refresh_default(tree, constraint, scale)
  -> EventMsg::RegistryUpdate
  -> redraw

Tab / Shift+Tab focus change
  -> EventProcessor chooses next focus from rendered focusables
  -> EventProcessor emits registry precomputed reveal scroll requests
  -> TreeMsg::ScrollRequest {id, dx, dy}
  -> layout_and_refresh_default(tree, constraint, scale)
  -> EventMsg::RegistryUpdate
  -> redraw

Scrollbar thumb/track input
  -> EventProcessor::scrollbar_thumb_drag_requests
  -> TreeMsg::ScrollbarThumbDragX/Y
  -> tree.apply_scroll_x/y
  -> layout_and_refresh_default(tree, constraint, scale)
  -> EventMsg::RegistryUpdate
  -> redraw
```

This keeps render output and hit bounds synchronized after every scroll change.

When the focused node has no directional keyboard scroll matcher (including
scroll-ancestor boundary cases), arrow scrolling falls back to the first
visible scrollable element (root-first traversal order) that can scroll in
that direction. The same fallback is used when no element is focused.

## Runtime State and Clamping

- Offsets: `scroll_x`, `scroll_y`
- Maxima: `scroll_x_max`, `scroll_y_max` (computed from `content - viewport`)
- Clamping: offsets are always clamped to `[0, max]`

Layout rules:

- If max shrinks, offset clamps toward start.
- If max grows and previous offset was at end, end anchoring is preserved.
- If scrollbar axis is disabled, scroll offset and max for that axis are cleared.

## Rendering

- Child content renders under `Translate(-scroll_x, -scroll_y)` when scrollable.
- Clip rects are padding-aware.
- Scrollbar thumbs render from viewport/content ratio and current offset.
- Thumb hover state is axis-specific (`none | x | y`) and widens thumb thickness
  for hovered axis.

## Current Limits and Next Steps

- No built-in scroll telemetry events back to Elixir.
- No dedicated active/pressed thumb styling beyond hover state.
