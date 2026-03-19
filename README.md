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
alias Emerge.Color

# Column element that fills viewport, padding to content of 20px
# elements inside are spaced evenly 
column([width(fill()), height(fill()), space_evenly(), padding(20)], [
  # Header element 
  el([width(fill()), Font.center(), Font.size(24), Font.color(Color.color(:white))],
    text("Hello from Emerge, this a header")),

  # Two content cards where left is bigger to right by 2:1 ratio
  row([spacing(12), padding(20)], [
    el([width({:fill, 2}), padding(16), Background.color(Color.color(:sky, 500)), Border.rounded(8)],
      text("Left Card")),
    el([width({:fill, 1}), padding(16), Background.color(Color.color(:emerald, 500)), Border.rounded(8)],
      text("Right Card"))
  ]),

  # Three element footer each filling equal portion
  row([spacing(12), padding(20)], [
    el([width(fill()), padding(16), Background.color(Color.color(:sky, 500)), Border.rounded(8)],
      text("Left footer side")),
    el([width(fill()), padding(16), Background.color(Color.color(:emerald, 500)), Border.rounded(8)],
      text("Center footer side")),
    el([width(fill()), padding(16), Background.color(Color.color(:emerald, 500)), Border.rounded(8)],
      text("Right footer side"))
  ])
])
```

## Declarative layout

Emerge layout syntax is heavily inspired and influence by `elm-ui` library.
Markup is defined by simple elixir functions, no templating or xml style things.

### Layout elements

Each container element takes 2 arguments: a list of attributes and its child or children.

The basic building block is `el/2`, which always accepts exactly one child element.
Use `el([], none())` for an empty element.

`text/1` is a standalone content element. It does not wrap by default.
Use `paragraph/2` and `text_column/2` for wrapped text flows.

If you want an element to have multiple children, use `row/2`, `column/2`, `wrapped_row/2`,
`paragraph/2`, or `text_column/2`.

### Layout sizing

Element can be declared to fixed size by using `width(px)` and `height(px)`
attributes where px is number.

Sizing can also be relative to elements peer element:
- `shrink` attribute will shrink an element to fit it's contents it is also a default.
- `fill` Fill the available space. The available space will be split evenly between elements that have width fill.
- `{:fill, n}` Fill available space by ratio. `fill` is equivalent to `{:fill, 1}`
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
- Declarative interaction styling (`mouse_over`, `focused`, `mouse_down`)
- Transforms (move, rotate, scale, alpha)
- Overlay positioning (`above`, `below`, `in_front`, etc.)
- High-DPI scaling
- Incremental tree patching for efficient updates
- Source-based asset loading with compile-time verified media paths (`~m`)

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

## Image Assets

EmergeSkia resolves image **sources** asynchronously in the Rust pipeline after
`EmergeSkia.upload_tree/2` and `EmergeSkia.patch_tree/3`.

`image/2` and `Background.image/2` support raster formats plus self-contained SVGs.
SVG text uses system font matching; relative subresources and external SVG fonts are not loaded.

Supported source forms:

- `~m"images/logo.png"` (compile-time verified static path)
- `"images/logo.png"` (logical static path, resolved from `<otp_app>/priv/images/logo.png`)
- `{:path, "/absolute/or/runtime/path.png"}` (runtime filesystem path)
- `{:id, "img_<sha256>"}` (already-loaded image ID)

Use the `~m` sigil by importing `Emerge.Assets.Path`:

```elixir
defmodule MyApp.UI do
  use Emerge.Assets.Path, otp_app: :my_app
  import Emerge.UI

  def view do
    column([spacing(16)], [
      image([width(px(120)), height(px(120))], ~m"images/logo.png"),
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

## Asset Startup Options

Configure assets when starting the renderer. `otp_app` is required:

```elixir
{:ok, renderer} =
  EmergeSkia.start(
    otp_app: :my_app,
    title: "My App",
    assets: [
      fonts: [
        [family: "my-font", source: "fonts/MyFont-Regular.ttf", weight: 400],
        [family: "my-font", source: "fonts/MyFont-Bold.ttf", weight: 700],
        [family: "my-font", source: "fonts/MyFont-Italic.ttf", weight: 400, italic: true]
      ],
      runtime_paths: [
        enabled: false,
        allowlist: [],
        follow_symlinks: false,
        max_file_size: 25_000_000,
        extensions: [".png", ".jpg", ".jpeg", ".webp", ".gif", ".bmp", ".svg"]
      ]
    ]
  )
```

Logical paths are always resolved from the provided app's `priv` directory.

Font assets are loaded at startup from logical `priv` paths and registered by
`family` + `weight` + `italic`.

Runtime behavior:

- image loading is async in Rust and does not block rendering
- unresolved sources show a loading placeholder
- failed sources show an `asset_failed` placeholder

Runtime path ingestion is disabled by default for Nerves-friendly security.
Enable `runtime_paths.enabled` only when needed, with an explicit allowlist.

## Documentation

Run `mix docs` for the full API reference.
