# Emerge


Write native GUI directly from Elixir using declarative API.

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

Emerge is designed with [Solve](https://github.com/emerge-elixir/solve) in mind as state managment solution,
it does not depend on it so you can roll your own solution easily.

For a fuller app example, see [TodoMVC example](https://github.com/emerge-elixir/emerge/tree/main/example)



## Rendering organization

As rendering grows, it is natural to split it into smaller chunks. Emerge
uses plain elixir functions to achieve this. A function only needs to return
`Emerge.tree()`, which every element in Emerge.UI returns.

```elixir
defmodule MyApp.UI do
  use Emerge.UI

  def dashboard(user) do
    column([
      width(fill()),
      padding(20),
      spacing(16),
      Background.color(color(:slate, 900)),
      Border.rounded(12),
      Font.color(color(:white))
    ], [
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

<img src="assets/dashboard-functions.png" alt="Rendered dashboard example" width="760">

### Features

- Backgrounds (solid color, linear gradient, and image cover/contain/repeat)
- Image rendering (`image/2` elements and `Background.image/2`)
- Borders with per-corner rounding
- Font customization (family, weight, style, size, decorations, spacing)
- Scrollable containers with native scrollbars
- Mouse and keyboard input events
- Declarative interaction styling (`mouse_over`, `focused`, `mouse_down`) for background, border, font, SVG tint, and transform attrs
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

Release builds can ship precompiled NIFs for the default backend profiles.
Set `EMERGE_SKIA_BUILD=1` to force a local Rust build instead of downloading a
precompiled artifact.

During the private release phase, precompiled NIF artifacts are downloaded from
private GitHub releases. Consumers need a token with read access to the release
artifacts:

```bash
export EMERGE_SKIA_GITHUB_TOKEN="$(gh auth token)"
gh auth status
```

`gh` is only a convenient way to source the token. The package itself reads
`EMERGE_SKIA_GITHUB_TOKEN` directly and does not require the GitHub CLI at
runtime.

`native/emerge_skia/Cross.toml` is only for `cross` container builds. Its
package installation commands run inside cross's Debian-based image, not on
your local Linux distro.

Nerves builds are currently configured to use a Skia feature set with published
`rust-skia` binary-cache artifacts, so Raspberry Pi 5 builds avoid a full local
Skia source build.

Before publishing to Hex, generate and include the checksum file for the
release assets:

```bash
mix rustler_precompiled.download EmergeSkia.Native --all --print
```

For a private Hex dry run, the recommended release flow is:

1. tag and publish the private GitHub release assets
2. export `EMERGE_SKIA_GITHUB_TOKEN`
3. generate `checksum-Elixir.EmergeSkia.Native.exs`
4. verify package contents with `mix hex.build --unpack`
5. publish with `mix hex.publish --organization YOUR_ORG`

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

The root `demo.exs` script is a renderer feature demo. For a more realistic app built
with `Emerge` and `Solve`, run the example project instead. It uses `Solve` as a path
dependency, so clone `solve` next to this repo first:

```bash
git clone git@github.com:emerge-elixir/solve.git ../solve
cd example
mix deps.get
iex -S mix
```

In dev, the example enables hot-code reloading for files under `example/lib`.

## Local CI Checks

Run the same checks used in CI with:

```bash
./ci-tests.sh
```

You can also run individual groups:

```bash
./ci-tests.sh quality
./ci-tests.sh test
./ci-tests.sh dialyzer
```

## Assets

Emerge supports:

- `image/2`
- `svg/2`
- `Background.image/2`
- startup-configured font assets

Use `~m` for compile-time verified static paths, and configure fonts or runtime
path policy through `EmergeSkia.start/1`.

See:

- [Use assets](guides/tutorials/use_assets.md)
- `EmergeSkia.start/1`

## Documentation

Run `mix docs` for the full API reference.
