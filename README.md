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
