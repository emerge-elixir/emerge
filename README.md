# Emerge

[![Hex](https://img.shields.io/badge/Hex-emerge-6E4AFF)](https://hex.pm/packages/emerge)
[![HexDocs](https://img.shields.io/badge/HexDocs-emerge-4B9BE0)](https://hexdocs.pm/emerge)
[![CI](https://img.shields.io/badge/CI-GitHub_Actions-2088FF?logo=githubactions&logoColor=white)](https://github.com/emerge-elixir/emerge/actions/workflows/ci.yml)
[![License](https://img.shields.io/github/license/emerge-elixir/emerge.svg)](https://github.com/emerge-elixir/emerge/blob/main/LICENSE)

Write native GUI directly from Elixir using declarative API.

## Installation

Add `:emerge` to your dependencies:

```elixir
defp deps do
  [
    {:emerge, "~> 0.1.0"}
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

## State management

Emerge works well with [Solve](https://github.com/emerge-elixir/solve) for powerful state management in larger apps. It keeps shared application state and rerender coordination outside the viewport process while Emerge stays focused on rendering.

Emerge does not depend on Solve. You can use another state management approach if it fits your app better.

For a fuller app example that uses `Emerge` and `Solve`, see [`example/`](https://github.com/emerge-elixir/emerge/tree/main/example).

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

## Features

- Build layout and styling in one declarative tree with `el/2`, `row/2`, `column/2`, and related helpers
- Reuse UI with ordinary Elixir functions, data transforms, and `Enum`
- Handle buttons, text input, keyboard, pointer events, and interactive states
- Render images, SVGs, backgrounds, borders, text, and font assets
- Use scroll containers, nearby overlays, paint-time transforms, and animation
- Run the same renderer on Wayland, DRM, and raster backends with high-DPI rendering and efficient tree updates

## Backends

- **Wayland** for desktop Linux windows
- **DRM** for embedded, kiosk, and Nerves deployments
- **Raster** for offscreen rendering and tests

Compile the backends you need:

```elixir
config :emerge, compiled_backends: [:wayland]
```

For runtime backend selection and multi-backend setup, see [Set up a viewport](guides/tutorials/set_up_viewport.md).

## Requirements

- Elixir 1.19+
- Rust toolchain
- Linux (Wayland or DRM for on-screen backends)

## Documentation

API reference and tutorials are published on [HexDocs](https://hexdocs.pm/emerge).

Key guides:

- [Set up a viewport](guides/tutorials/set_up_viewport.md)
- [Describe your UI](guides/tutorials/describe_ui.md)
- [Use assets](guides/tutorials/use_assets.md)
- [Manage state](guides/tutorials/state_management.md)

Run `mix docs` to build the full docs locally.

## Attribution

Emerge's UI API is heavily inspired by [elm-ui](https://package.elm-lang.org/packages/mdgriffith/elm-ui/latest/) by Matthew Griffith.
