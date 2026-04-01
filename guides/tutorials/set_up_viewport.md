# Set up a viewport

A viewport is an Elixir process that manages your app's native renderer.

It sends UI definitions to the renderer, requests rerenders when needed, and routes renderer events back to Elixir.

On Wayland, you can think of a viewport as a GUI window. You can start multiple viewports at the same time to open multiple windows.

With the DRM backend on Nerves, the viewport renders to the first connected display.

## A small counter viewport

This example keeps the counter state inside the viewport so you can see the full event flow in one module.

```elixir
defmodule MyApp.View.Counter do
  use Emerge

  @impl Viewport
  def mount(opts) do
    {:ok, %{count: 0}, Keyword.merge([title: "Counter"], opts)}
  end

  @impl Viewport
  def render(%{count: count}) do
    row(
      [
        Background.color(color(:slate, 800)),
        Font.color(color(:white)),
        spacing(12),
        padding(12)
      ],
      [
        button([Event.on_press(:decrement)], text("-")),
        el([padding(10)], text("Count: #{count}")),
        button([Event.on_press(:increment)], text("+"))
      ]
    )
  end

  defp button(attrs, content) do
    Input.button(
      attrs ++ [
        padding(10),
        Background.color(color(:sky, 500)),
        Border.rounded(8)
      ],
      content
    )
  end

  @impl Viewport
  def handle_info(:increment, state) do
    {:noreply, Viewport.rerender(%{state | count: state.count + 1})}
  end

  @impl Viewport
  def handle_info(:decrement, state) do
    {:noreply, Viewport.rerender(%{state | count: state.count - 1})}
  end
end
```

`use Emerge` brings common `Emerge.UI` helpers into scope, so you can call `row/2`, `el/2`, `text/1`, and `Input.button/2` directly.

In this module:

- `mount/1` creates the initial viewport state and returns default viewport options
- `render/1` defines the current UI
- `handle_info/2` receives routed element events and triggers rerenders

If a viewport does not need local state, `mount/1` can return `{:ok, opts}` and `render/0` can be used instead.

## Start a Wayland window

To run this on Wayland, you need Linux, a Wayland session, and a build that includes the Wayland backend.

From `iex -S mix`:

```elixir
{:ok, pid} = MyApp.View.Counter.start_link()
```

This starts the viewport process and opens the counter in a window.

This is the smallest live viewport setup: no explicit backend, window size, or `otp_app`.

## What defaults did that use?

`MyApp.View.Counter.start_link()` uses the options returned by `mount/1`, plus renderer defaults for anything you do not set.

In this case:

- `title` comes from `mount/1`
- `otp_app` is inferred from the viewport module
- `width` defaults to `800`
- `height` defaults to `600`

If `backend` is omitted, Emerge uses the runtime default backend.

On desktop builds, that is `:wayland`.
On Nerves builds, that is `:drm`.

Backend selection also has a compile-time requirement: a backend can start at runtime only if it was compiled into the native code.

To compile Wayland only:

```elixir
config :emerge, compiled_backends: [:wayland]
```

To compile both Wayland and DRM:

```elixir
config :emerge, compiled_backends: [:wayland, :drm]
```

If you leave `compiled_backends` unset, desktop builds default to `[:wayland]` and Nerves builds default to `[:drm]`.

## Change the window settings

You can override any default returned by `mount/1` when the viewport starts.

```elixir
{:ok, pid} =
  MyApp.View.Counter.start_link(
    title: "Counter Large",
    width: 1024,
    height: 768,
    backend: :wayland,
    otp_app: :my_app
  )
```

These options change the viewport itself:

- `title` sets the window title
- `width` sets the initial width
- `height` sets the initial height
- `backend` selects how the viewport is presented
- `otp_app` tells Emerge where to resolve logical assets from `priv/`

For `use Emerge` viewports, Emerge infers `otp_app` from the module when it can. Set it explicitly when that inference is not enough.

These options do not change the UI defined by `render/1`. They change where and how it is displayed.

## How element events reach the viewport

The counter attaches `Event.on_press/1` to each button:

```elixir
button([Event.on_press(:increment)], text("+"))
button([Event.on_press(:decrement)], text("-"))
```

Event helpers accept `{pid, event_message}`.

If you pass only a message, Emerge uses `{self(), event_message}` by default.

So this:

```elixir
Event.on_press(:increment)
```

is shorthand for:

```elixir
Event.on_press({self(), :increment})
```

Because `render/1` runs inside the viewport process, `self()` is the viewport pid at render time.

When the button is pressed, the runtime looks up the handler for that element and sends `event_message` to `pid`.

So:

- `Event.on_press(:increment)` sends `:increment` back to the viewport process
- `Event.on_press({some_pid, :increment})` sends `:increment` to `some_pid`

When the target pid is the viewport, the message arrives in `handle_info/2`.

The counter updates here:

```elixir
@impl Viewport
def handle_info(:increment, state) do
  {:noreply, Viewport.rerender(%{state | count: state.count + 1})}
end

@impl Viewport
def handle_info(:decrement, state) do
  {:noreply, Viewport.rerender(%{state | count: state.count - 1})}
end
```

Payload-carrying element events follow the same routing rule.

For example:

```elixir
Input.text([Event.on_change(:search_changed)], "")
```

With the default routing, a change like `"hello"` becomes:

```elixir
{:search_changed, "hello"}
```

and that message is sent to `self()` unless you provide another pid explicitly.

## Keeping app state in the viewport is an anti-pattern

This tutorial keeps the counter state inside the viewport so you can see the event flow in one module.

That works well for a small example, but it does not scale in real applications. Shared state is harder to coordinate, background work turns into manual message passing, and more application logic ends up in the rendering process.

In practice, keep application state outside the viewport. Later guides will introduce `Solve` as the primary state management library for Emerge apps.

That keeps the viewport focused on rendering and rerendering instead of owning application state and business logic.

## Add the viewport to your application

Start a `use Emerge` module under your application supervisor like any other child.

```elixir
defmodule MyApp.Application do
  use Application

  @impl true
  def start(_type, _args) do
    children = [
      MyApp.View.Counter
    ]

    Supervisor.start_link(children, strategy: :one_for_one, name: MyApp.Supervisor)
  end
end
```

Starting the application also starts the viewport.

For example:

```bash
iex -S mix
```

If you want to override viewport defaults at startup, use `child_spec/1`:

```elixir
children = [
  MyApp.View.Counter.child_spec(
    title: "Counter In App",
    width: 1024,
    height: 768
  )
]
```

## Enable hot code reload in dev

In `example/`, `Emerge.Runtime.CodeReloader` starts only in `:dev`.

```elixir
defmodule MyApp.Application do
  use Application

  @impl true
  def start(_type, _args) do
    Supervisor.start_link(children(), strategy: :one_for_one, name: MyApp.Supervisor)
  end

  def children(env \\ Mix.env())
  def children(:dev), do: base_children() ++ [hot_reload_child()]
  def children(_other), do: base_children()

  defp base_children do
    [
      MyApp.View.Counter
    ]
  end

  defp hot_reload_child do
    {Emerge.Runtime.CodeReloader,
     dirs: [Path.expand("..", __DIR__)],
     reloadable_apps: [:my_app]}
  end
end
```

`dirs` is the list of directories to watch.

`reloadable_apps` is the list of Mix apps to recompile when a file changes.

The code reloader also expects Mix compile notifications. Add this listener in `mix.exs`:

```elixir
def project do
  [
    app: :my_app,
    version: "0.1.0",
    elixir: "~> 1.19",
    start_permanent: Mix.env() == :prod,
    listeners: [Emerge.Runtime.CodeReloader.MixListener],
    deps: deps()
  ]
end
```

The watcher process uses the `:file_system` dependency, so add it in `:dev`:

```elixir
defp deps do
  [
    {:emerge, path: "../.."},
    {:file_system, "~> 1.0", only: :dev}
  ]
end
```

With this in place, `iex -S mix` starts the viewport, watches the configured source directories, recompiles the configured app when files change, and rerenders live viewports after a successful reload.

In this example, `Path.expand("..", __DIR__)` points at `lib/`, so changes under `lib/` are picked up automatically.

## Viewport runtime settings

Most of the options so far configure the rendered surface:

- `title`
- `width`
- `height`
- `backend`
- `drm_card`
- `otp_app`

A viewport can also receive runtime-specific settings under `viewport: [...]`.

Two common runtime settings are:

- `input_mask`
- `renderer_check_interval_ms`

`renderer_check_interval_ms` controls how often the viewport checks whether the renderer is still alive.

`input_mask` controls which raw renderer inputs are delivered to the viewport. It is a bitmask built with helpers from `EmergeSkia`.

Raw renderer input is the low-level stream of backend and renderer notifications. It is separate from events declared on individual elements. It includes resize notifications, focus changes, pointer movement, scrolling, key input, and text input coming directly from the renderer.

A mount function can include viewport runtime settings like this:

```elixir
@impl Viewport
def mount(opts) do
  {:ok,
   %{count: 0},
   Keyword.merge(
     [
       title: "Counter",
       viewport: [
         input_mask: EmergeSkia.input_mask_resize(),
         renderer_check_interval_ms: 500
       ]
     ],
     opts
   )}
end
```

This asks the viewport to receive raw resize input and check renderer liveness every 500 milliseconds.

## Raw renderer input

Handle raw renderer input in `handle_input/2`.

`handle_input/2` is separate from `handle_info/2`:

- `handle_info/2` handles routed element events such as `:increment`
- `handle_input/2` handles raw renderer input selected by the input mask

Representative raw input messages include:

- `{:resized, {width, height, scale}}`
- `{:focused, focused}`
- `{:cursor_pos, {x, y}}`
- `{:cursor_scroll, {{dx, dy}, {x, y}}}`
- `{:key, {key, action, mods}}`
- `{:codepoint, {char, mods}}`
- `{:text_commit, {text, mods}}`

A viewport can handle them directly:

```elixir
@impl Viewport
def handle_input({:resized, {width, height, scale}}, state) do
  IO.inspect({width, height, scale}, label: "resized")
  {:noreply, state}
end
```

Use `handle_input/2` when the viewport needs backend-level input or window and display notifications directly.

## Run the same viewport on DRM

To run the same viewport on DRM, you need Linux, a build that includes the DRM backend, and access to a DRM device such as `/dev/dri/card0`.

Start the counter like this:

```elixir
{:ok, pid} =
  MyApp.View.Counter.start_link(
    backend: :drm,
    drm_card: "/dev/dri/card0"
  )
```

`drm_card` is the additional option.

On DRM, the viewport needs the rendering device explicitly so it knows which display to use.

The viewport module, UI definition, element routing, and rerender flow do not change:

- the same viewport process starts
- the same `render/1` function defines the UI
- the same element events route back into Elixir
- the same rerender flow is used

Only the backend changes.

To always start the application on DRM, pass those options in the child spec:

```elixir
children = [
  MyApp.View.Counter.child_spec(
    backend: :drm,
    drm_card: "/dev/dri/card0"
  )
]
```

Other DRM-specific settings are available when needed:

- `hw_cursor`
- `drm_cursor`
- `input_log`
- `render_log`

`drm_cursor` is only supported with `backend: :drm`.

## Next

In the next section, you will learn how to define your UI using `Emerge.UI` modules.
