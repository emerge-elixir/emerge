# Emerge

GPU accelerated GUI framework.

Write GUIs directly from Elixir using declarative API
inspired by [Elm-UI](https://package.elm-lang.org/packages/mdgriffith/elm-ui/1.1.8/) library.

It makes writing layouts simple, fun and easy to modify as layout and style are centralized.
UI is tree of elements where each element has attributes and children.
`row(attrs, children)`

## Quick example


```elixir
defmodule MyApp.Counter do
  use Emerge

  @impl Viewport
  def mount(opts) do
    state = %{count: 0}
    {:ok, state, Keyword.merge([title: "Counter"], opts)}
  end

  @impl Viewport
  def render(state) do
    row([spacing(12), padding(12)], [
      el([Font.color(color(:white))], text("Count: #{state.count}")),
      Input.button(
        [
          padding(10),
          Background.color(color(:sky, 500)),
          Border.rounded(8),
          Event.on_press(:increment)
        ],
        text("+")
      )
    ])
  end

  @impl Viewport
  def handle_info(:increment, state) do
    {:noreply, Viewport.rerender(%{state | count: state.count + 1})}
  end
end
```

This example uses internal state similar to LiveView but I would strongly
discourage you from that pattern and encourage you to take a look at [Solve](https://github.com/emerge-elixir/solve)
for state management as by design is no way to create stateful component beyond viewport.
Solve will also save you from prop drilling.


## Rendering organization

As rendering grows, it is natural to split it into smaller chunks. Emerge
uses plain elixir functions to achieve this. A function only needs to return
`Emerge.tree()`, which every element in Emerge.UI returns.

```elixir
defmodule MyApp.UI do
  use Emerge.UI

  def dashboard(user) do
    column([padding(20), spacing(16)], [
      header(user),
      stats(user),
      actions()
    ])
  end

  def header(user) do
    el([Font.size(24), Font.color(color(:white))], text("Welcome #{user.name}"))
  end

  def stats(user) do
    row([spacing(12)], [
      stat("Projects", Integer.to_string(user.project_count)),
      stat("Tasks", Integer.to_string(user.task_count))
    ])
  end

  def stat(label, value) do
    el(
      [
        width(fill()),
        padding(12),
        Background.color(color(:slate, 800)),
        Border.rounded(8)
      ],
      column([spacing(4)], [
        el([Font.color(color(:slate, 300))], text(label)),
        el([Font.size(18), Font.color(color(:white))], text(value))
      ])
    )
  end

  def actions do
    row([spacing(12)], [
      Input.button(
        [
          padding(10),
          Background.color(color(:sky, 500)),
          Border.rounded(8),
          Event.on_press(:save)
        ],
        text("Save")
      )
    ])
  end
end
```


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
- `fill(n)` Fill available space by ratio. `fill()` is equivalent to `fill(1)`
- `min(px(50), fill())` / `max(px(300), shrink())` Clamp a length while still composing with `fill()` or `shrink()`

### Spacing

Spacing in Emerge is achieved by using `padding` and `spacing`.

Padding is the distance between the outer edge and the content, and spacing is the space between children.

### Features

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

- **Wayland** — windowed desktop apps on a Wayland session (default)
- **DRM** — direct framebuffer for Nerves / embedded / kiosk (no window manager needed)
- **Raster** — offscreen CPU rendering for testing and headless use

Compile-time native backends are selected with Elixir config:

```elixir
config :emerge, compiled_backends: [:wayland]
```

If omitted, desktop builds assume `[:wayland]` while Nerves-style builds assume
`[:drm]`. To compile both native window backends:

```elixir
config :emerge, compiled_backends: [:wayland, :drm]
```

Runtime `backend:` options must request a backend that was compiled into the NIF.
For multi-target apps, use target-specific config to choose the compiled backend set
for each build.

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
  use Emerge.UI

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
