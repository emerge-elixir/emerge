# Emerge

A simple native GUI toolkit for Elixir.

Emerge lets you build native graphical interfaces in Elixir using a declarative,
Elm-UI inspired API. It targets Nerves projects (embedded displays, kiosks,
dashboards) but works for general-purpose desktop apps too. Backed by Skia for
GPU-accelerated rendering.

## Quick example

```elixir
import Emerge.UI

column([width(fill()), spacing(16), padding(20)], [
  el([Font.size(24), Font.color(0xFFFFFFFF)],
    text("Hello from Emerge")),

  row([spacing(12)], [
    el([width(fill()), padding(16), Background.color(0x3B82F6FF), Border.rounded(8)],
      text("Card A")),
    el([width(fill()), padding(16), Background.color(0x10B981FF), Border.rounded(8)],
      text("Card B"))
  ])
])
```

## Features

- Declarative layout (`row`, `column`, `el`, `wrapped_row`, `text`)
- Elm-UI sizing model (`fill`, `shrink`, `px`, `fill_portion`, `min`/`max`)
- Padding, spacing, alignment
- Backgrounds (solid color, linear gradient)
- Borders with per-corner rounding
- Font customization (family, weight, style, size, decorations, spacing)
- Scrollable containers with native scrollbars
- Mouse and keyboard input events
- Hover styling (`mouse_over`)
- Transforms (move, rotate, scale, alpha)
- Overlay positioning (`above`, `below`, `in_front`, etc.)
- High-DPI scaling
- Incremental tree patching for efficient updates

## Backends

- **Wayland** — windowed desktop apps (default)
- **DRM** — direct framebuffer for Nerves / embedded / kiosk (no window manager needed)
- **Raster** — offscreen CPU rendering for testing and headless use

The Wayland backend uses winit, which can fall back to X11 automatically if no
Wayland display is available — but X11 is not a first-class supported target.

## Requirements

- Elixir 1.19+
- Rust toolchain
- Linux (Wayland or DRM)

## Getting started

```bash
mix deps.get
mix compile
mix test
mix run demo.exs
```

## Documentation

Run `mix docs` for the full API reference.
