# Emerge

GUI framework for Elixir.

Emerge lets you build native graphical interfaces in Elixir using a declarative,
Elm-UI inspired API.

Main inspiration came from needing easy to use GUI framework for Nerves projects
but it should work for general-purpose desktop apps too.

It is backed by Skia for GPU-accelerated rendering.Layout engine, input event processing
and rendering are implemented in rust for performance reasons.

## Quick example

```elixir
import Emerge.UI

# Column element that fills viewport, padding to content of 20px
# elements inside are spaced evenly 
column([width(fill()), height(fill()), space_evenly()), padding(20)], [
  # Header element 
  el([width(fill()), Font.center(), Font.size(24), Font.color(0xFFFFFFFF)],
    text("Hello from Emerge, this a header")),

  # Two content cards where left is bigger to right by 2:1 ratio
  row([spacing(12), padding(20)], [
    el([width(fill_portion(2)), padding(16), Background.color(0x3B82F6FF), Border.rounded(8)],
      text("Left Card")),
    el([width(fill_portion(1), padding(16), Background.color(0x10B981FF), Border.rounded(8)],
      text("Right Card"))
  ])

  # Three element footer each filling equal portion
  row([spacing(12), padding(20)], [
    el([width(fill()), padding(16), Background.color(0x3B82F6FF), Border.rounded(8)],
      text("Left footer side")),
    el([width(fill()), padding(16), Background.color(0x10B981FF), Border.rounded(8)],
      text("Center footer side"))
    el([width(fill()), padding(16), Background.color(0x10B981FF), Border.rounded(8)],
      text("Right footer side"))
  ])
])
```

## Declarative layout

Emerge layout syntax is heavily inspired and influence by `elm-ui` library.
Markup is defined by simple elixir functions, no templating or xml style things.

### Layout elements

Each element is a function accepting
2 arguments, a list of attributes, and a child/list of children. 

Basic building block is `el/2`, `el` only accepts one child,
usually `text/1` which is a content construct
and cannot live on it's own without `el` as it's parent.

If you want element to have multiple children 
you can use `row`, `column` and `wrapped_row`

### Layout sizing

Element can be declared to fixed size by using `width(px)` and `height(px)`
attributes where px is number.

Sizing can also be relative to elements peer element:
- `shrink` attribute will shrink an element to fit it's contents it is also a default.
- `fill` Fill the available space. The available space will be split evenly between elements that have width fill.
- `fill_portion(n)` Fill available space by ratio. `fill` == `fill_portion(1)`
- `min(px)`/`max(px)` Define min/max size of an element, can be used in combination with `shrink`/`fill`

### Spacing

Same as in elm-ui there is no concept of margins.

Padding is the distance between the outer edge and the content, and spacing is the space between children.


## Features

- Backgrounds (solid color, linear gradient, and image cover/contain/repeat)
- Image rendering (`image/2` elements and `Background.image/2`)
- Borders with per-corner rounding
- Font customization (family, weight, style, size, decorations, spacing)
- Scrollable containers with native scrollbars
- Mouse and keyboard input events
- Hover styling (`mouse_over`)
- Transforms (move, rotate, scale, alpha)
- Overlay positioning (`above`, `below`, `in_front`, etc.)
- High-DPI scaling
- Incremental tree patching for efficient updates
- Asset pipeline with compile-time verified media paths (`~m`) and digest manifests

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
mix emerge.assets.digest
mix test
mix run demo.exs
```

## Image Assets

EmergeSkia resolves image **sources** asynchronously in the Rust pipeline after
`EmergeSkia.upload_tree/2` and `EmergeSkia.patch_tree/3`.

Supported source forms:

- `~m"images/logo.png"` (compile-time verified static path)
- `"images/logo.png"` (logical static path, looked up in digest manifest)
- `{:path, "/absolute/or/runtime/path.png"}` (runtime filesystem path)
- `{:id, "img_<sha256>"}` (already-loaded image ID)

Use the `~m` sigil by importing `Emerge.Assets.Path`:

```elixir
defmodule MyApp.UI do
  use Emerge.Assets.Path
  import Emerge.UI

  def view do
    column([spacing(16)], [
      image(~m"images/logo.png", [width(px(120)), height(px(120))]),
      el([
        width(px(320)),
        height(px(180)),
        Background.image(~m"images/hero.jpg", fit: :cover)
      ], none())
    ])
  end
end
```

Background image fit helpers:

- `Background.image/2` defaults to `fit: :cover`
- `Background.uncropped/1` uses `:contain`
- `Background.tiled/1`, `Background.tiled_x/1`, `Background.tiled_y/1` use repeat modes

## Asset Config

Configure global asset behavior under `:emerge_skia, :assets`:

```elixir
config :emerge_skia, :assets,
  sources: ["assets"],
  manifest: [
    path: "priv/static/cache_manifest.json",
    images_meta_path: "priv/static/cache_manifest_images.json"
  ],
  runtime_paths: [
    enabled: false,
    allowlist: [],
    follow_symlinks: false,
    max_file_size: 25_000_000,
    extensions: [".png", ".jpg", ".jpeg", ".webp", ".gif", ".bmp"]
  ]
```

Runtime behavior:

- image loading is async in Rust and does not block rendering
- unresolved sources show a loading placeholder
- failed sources show an `asset_failed` placeholder

Runtime path ingestion is disabled by default for Nerves-friendly security.
Enable `runtime_paths.enabled` only when needed, with an explicit allowlist.

## Documentation

Run `mix docs` for the full API reference.
