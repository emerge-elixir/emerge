# Emerge

[![Hex](https://img.shields.io/badge/Hex-emerge-6E4AFF)](https://hex.pm/packages/emerge)
[![HexDocs](https://img.shields.io/badge/HexDocs-emerge-4B9BE0)](https://hexdocs.pm/emerge)
[![CI](https://img.shields.io/badge/CI-GitHub_Actions-2088FF?logo=githubactions&logoColor=white)](https://github.com/emerge-elixir/emerge/actions/workflows/ci.yml)
[![License](https://img.shields.io/github/license/emerge-elixir/emerge.svg)](https://github.com/emerge-elixir/emerge/blob/main/LICENSE)

Write native GUI directly from Elixir using a declarative API.

## Installation

Add `:emerge` to your dependencies:

```elixir
defp deps do
  [
    {:emerge, "~> 0.4.0"}
  ]
end
```

Then run:

```bash
mix deps.get
```

## Quick example

```elixir
defmodule MyApp.View.Counter do
  use Emerge
  use Solve.Lookup

  @impl Viewport
  def mount(opts), do: {:ok, Keyword.merge([title: "Counter"], opts)}

  @impl Viewport
  def render() do
    counter = solve(MyApp.State, :counter)

    row(
      [
        Background.color(color(:slate, 800)),
        Font.color(color(:white)),
        spacing(12),
        padding(12)
      ],
      [
        my_button([Event.on_press(event(counter, :increment))], text("+")),
        el([padding(10)], text("Count: #{counter.count}")),
        my_button([Event.on_press(event(counter, :decrement))], text("-"))
      ]
    )
  end

  # Reusable "component" is just plain elixir function
  def my_button(attrs, content) do
    Input.button(
      attrs ++ [
        padding(10),
        Background.color(color(:sky, 500)),
        Border.rounded(8)
      ],
      content
    )
  end

  @impl Solve.Lookup
  def handle_solve_updated(_updated, state), do: {:ok, Viewport.rerender(state)}
end
```

<img src="assets/counter-basic.png" alt="Rendered counter example" width="272">

## Easy reuse

Reuse in Emerge is just Elixir. Build data, map over it, and extract helpers that return UI trees.

```elixir
defmodule MyApp.UI do
  use Emerge.UI

  def overview do
    column(
      [
        width(fill()),
        padding(20),
        spacing(12),
        Background.color(color(:slate, 900)),
        Border.rounded(12)
      ],
      [
        el([Font.size(22), Font.color(color(:white))], text("Overview")),
        row([spacing(12)], Enum.map(summary_stats(), &stat_card/1))
      ]
    )
  end

  defp summary_stats do
    [
      {"Open", "12"},
      {"Closed", "34"},
      {"Owners", "5"}
    ]
  end

  defp stat_card({label, value}) do
    el(
      [
        width(fill()),
        padding(12),
        Background.color(color(:slate, 800)),
        Border.rounded(8)
      ],
      column([spacing(4)], [
        el([Font.color(color(:slate, 300))], text(label)),
        el([Font.size(20), Font.color(color(:white))], text(value))
      ])
    )
  end
end
```

<img src="assets/dashboard-functions.png" alt="Rendered easy reuse example" width="560">

There is no separate component model to learn. If a function returns UI, you can compose it like any other Elixir function.

## State management

Emerge is designed with [Solve](https://hex.pm/packages/solve) as a state management solution to keep complex UI apps sane. It keeps shared application state and rerender coordination outside the viewport process while Emerge stays focused on rendering.

Emerge does not depend on Solve. You can use another state management approach if it fits your app better.


## Try it out

Take a look at [`emerge_demo`](https://github.com/emerge-elixir/emerge_demo) example repository.
It has https://todomvc.com/ app implentation so you can compare to web stacks and
a showcase app that covers most of the UI features at the basic level.

For nerves example take a look at [`nerves_emerge_demo`](https://github.com/emerge-elixir/nerves_emerge_demo)

## Features

- Build layout and styling in one declarative tree with `el/2`, `row/2`, `column/2`, and related helpers
- Reuse UI with ordinary Elixir functions, data transforms, and `Enum`
- Handle buttons, text input, keyboard, pointer events, and interactive states
- Render images, SVGs, backgrounds, borders, text, and font assets
- Use scroll containers, nearby overlays, paint-time and layout-aware transforms, and animation
- Run the same renderer on macOS, Wayland, DRM, and headless runtimes with Metal, OpenGL, raster, and experimental Vulkan rendering APIs

## Backends and rendering APIs

- **macOS** provides desktop windows through the external `macos_host`, using Metal when available and falling back to raster.
- **Wayland** provides Linux desktop windows with OpenGL or raster presentation.
- **DRM** provides direct embedded, kiosk, and Nerves output with OpenGL ES 2 as the supported minimum or raster GPU upload.
- **Headless** produces retained frame binaries or Linux PRIME/DMA-BUF output.
- **Vulkan** rendering exists for Wayland, DRM, and headless PRIME, but remains experimental.

Raster is a rendering API used by windowed and headless runtimes, not a separate
viewport backend. macOS currently has no video target or retained-frame capture.

Viewport modes and configuration examples are documented by
[`Emerge`](https://hexdocs.pm/emerge/Emerge.html) and
[`Emerge.Runtime.Viewport`](https://hexdocs.pm/emerge/Emerge.Runtime.Viewport.html).
Exact renderer and frame contracts are documented by
[`EmergeSkia`](https://hexdocs.pm/emerge/EmergeSkia.html).

## Requirements

- Elixir 1.19+
- macOS for the `:macos` backend, or Linux with Wayland/DRM for Linux on-screen backends
- OpenGL ES 2 or newer for DRM; no GLES version option is required
- Rust 1.91 or newer when a source build is required

On macOS, Emerge downloads and caches the matching versioned `macos_host` runtime artifact automatically.

On DRM, optional GPU timer profiling and PRIME video import are enabled only when the driver advertises their required GL/EGL extensions. Missing optional capabilities do not prevent ordinary UI rendering; PRIME video targets report an unsupported-capability error when DMA-BUF/external-image import is unavailable.

## Documentation

API reference and tutorials are published on [HexDocs](https://hexdocs.pm/emerge).

The tutorials build on one another:

1. [Set up a viewport](guides/tutorials/set_up_viewport.md)
2. [Describe your UI](guides/tutorials/describe_ui.md)
3. [Use assets](guides/tutorials/use_assets.md)
4. [Manage state](guides/tutorials/state_management.md)

Other documentation:

- [Migrate to Emerge 0.4](guides/migrations/0.4.md)
- [Build the native renderer](guides/reference/native-renderer-builds.md)

Run `mix docs` to build the full docs locally.

## Attribution

Emerge's UI API is heavily inspired by [elm-ui](https://package.elm-lang.org/packages/mdgriffith/elm-ui/latest/) by Matthew Griffith.

## Third-Party Assets

Bundled third-party asset notices are documented in
[THIRD_PARTY_ASSETS.md](https://github.com/emerge-elixir/emerge/blob/main/THIRD_PARTY_ASSETS.md).
Package/runtime-relevant notices are summarized in
[NOTICE](https://github.com/emerge-elixir/emerge/blob/main/NOTICE).

Packaged/runtime-relevant asset groups:

- Inter default fonts in `native/emerge_skia/src/fonts` - SIL Open Font License 1.1
- Mocu DRM cursor SVGs in `native/emerge_skia/src/backend/drm/cursors/mocu_black_right` - CC0 1.0 Universal

If you redistribute Emerge inside an application or firmware image, include the applicable notice files.
