# Scrolling Plan

This document captures the scrolling implementation plan and how it integrates
with the unified event registry.

## Goals

- Keep scrolling state and render loop on the Rust side.
- Use the shared event registry for scroll hit testing.
- Keep Elixir informed later via events (not in MVP).

## Event Registry (Unified)

```
EventNode {
  id: ElementId,
  hit_rect: Rect,        // frame bounds (padding included)
  flags: EventFlags,
}
```

- Nodes are stored in render traversal order.
- Hit testing scans from the end for topmost hits.
- We filter by event flag **before** hit testing so non-listeners do not block.

## Scroll Flow

```
CursorScroll -> hit_test_with_flag(SCROLL) -> update scroll_x/y
             -> render_tree -> request redraw
```

## Runtime State

- `scroll_x` / `scroll_y` live in Rust runtime attrs.
- Layout computes `scroll_x_max` / `scroll_y_max` from content size.
- Scroll values are clamped to max values.

## Rendering

- Background/border are static.
- Children render inside a translate of `(-scroll_x, -scroll_y)`.
- Content is clipped to the padding-aware content rect.

## Click + Scroll Registry Use

- `CLICK` and `SCROLL` flags share the same registry.
- Click hit tests use `CLICK`; scroll hit tests use `SCROLL`.

## Future Extensions

- Scroll offset-aware hit bounds for child elements.
- Optional scroll events back to Elixir for informational updates.
