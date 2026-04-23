# Set up a viewport

A viewport is an Elixir process that manages a renderer for Emerge.

Think of it as a gateway to the user. It sends UI definitions to the renderer, requests rerenders when needed, and routes renderer events back to Elixir.

On Wayland and macOS, one viewport is one GUI window. You can start multiple viewports at the same time to open multiple windows.

With the DRM backend (mostly used on Nerves), the viewport renders to the first connected display.

## Example

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
        my_button([Event.on_press(:decrement)], text("-")),
        el([padding(10)], text("Count: #{count}")),
        my_button([Event.on_press(:increment)], text("+"))
      ]
    )
  end

  defp my_button(attrs, content) do
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


### Callbacks

`use Emerge` brings the `Viewport` behaviour and implementation, along with `Emerge.UI` helpers, into scope.

You have to implement the `mount/1` callback and either `render/0` or `render/1`; `handle_info/2`, `handle_input/2`, and `handle_close/2` are optional.

- `mount/1` creates the initial viewport state and returns the default viewport options
- `render/0` or `render/1` returns the UI definition
- `handle_info/2` receives routed element events and triggers rerenders
- `handle_input/2` receives raw renderer input selected by the input mask
- `handle_close/2` handles window close requests

If a viewport does not need local state, `mount/1` can return `{:ok, opts}` and `render/0` can be used instead.

## Start a desktop window

On Linux desktop, this starts a Wayland window.

On macOS, it starts a macOS window through the external `macos_host` runtime.

From `iex -S mix`:

```elixir
{:ok, pid} = MyApp.View.Counter.start_link()
```

You should now see a window with a counter rendering inside of it.


## Mount options

`MyApp.View.Counter.start_link()` uses the options returned by `mount/1`, plus renderer defaults for anything you do not set.

### Options
- `title` comes from `mount/1`. Sets the window title
- `width` and `height` default to `800x600`. This is just the initial window size; you can resize it.
- `otp_app` is inferred from the viewport module. It tells Emerge where to resolve logical assets from `priv/`. Set it explicitly when that inference is not enough.
- `scroll_line_pixels` sets the pixel distance used for each discrete mouse-wheel line step. The default is `30.0`.
- `backend` selects how the viewport is presented. If `backend` is omitted, Emerge uses the runtime default backend.
- `assets` - Asset runtime policy options (optional)


### Backends

Default backends:
- `:macos` on Darwin host builds
- `:wayland` on Linux desktop builds
- `:drm` for Nerves builds

Backend selection also has a compile-time requirement: a backend can start at runtime only if it was compiled into the native code.

To compile Wayland only:

```elixir
config :emerge, compiled_backends: [:wayland]
```

To compile both Wayland and DRM:

```elixir
config :emerge, compiled_backends: [:wayland, :drm]
```

To compile macOS only:

```elixir
config :emerge, compiled_backends: [:macos]
```

To compile all supported runtime backends from one source tree:

```elixir
config :emerge, compiled_backends: [:wayland, :drm, :macos]
```

If you leave `compiled_backends` unset, it defaults to `[:macos]` on macOS, `[:wayland]` on Linux desktop builds, and `[:drm]` on Nerves builds.

Runtime backend options:

- `backend: :macos` starts the macOS backend explicitly
- `macos_backend: :auto | :metal | :raster` selects the macOS surface backend. `:auto` prefers Metal and falls back to raster.

macOS notes:

- `video_target` is not supported on macOS in `0.2.1`
- macOS uses a downloaded and cached `macos_host` runtime binary instead of the in-process Rustler path used by Linux backends

### Assets

`otp_app` tells Emerge where to resolve logical assets from `priv/`.

Renderer asset configuration also lives under the `assets:` start option. That
includes custom fonts and runtime path policy.

This tutorial keeps viewport setup focused on starting the renderer. The next
tutorial covers image, SVG, background, and font assets in detail.

## Event handling

The counter attaches `Event.on_press/1` to each button:

```elixir
Input.button([Event.on_press(:increment)], text("+"))
Input.button([Event.on_press(:decrement)], text("-"))
```

Event helpers accept `{pid, event_message}`.

If you pass only a message, Emerge uses `{self(), event_message}` by default.

```elixir
# This is just a shorthand
Event.on_press(:increment)
# for
Event.on_press({self(), :increment})
```

Because `render/1` runs inside the viewport process, `self()` is the viewport pid at render time.

When the button is pressed, the runtime looks up the handler for that element and sends `event_message` to `pid`.

So:

- `Event.on_press(:increment)` sends `:increment` back to the viewport process
- `Event.on_press({some_pid, :increment})` sends `:increment` to `some_pid`

When the target pid is the viewport, the message arrives in `handle_info/2`.

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

`Viewport.rerender` is not a direct call to `render`. It sets a flag in the state for the viewport to rerender.

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

The same payload shape applies to `Input.multiline/2`:

```elixir
Input.multiline([Event.on_change(:notes_changed)], "")
```

If the value becomes `"hello\nworld"`, the delivered message is:

```elixir
{:notes_changed, "hello\nworld"}
```

`Input.multiline/2` is still controlled from your viewport or app state, just
like `Input.text/2`. `Enter` inserts a newline by default unless you intercept
it with `Event.on_key_down(:enter, ...)`.

## Keeping app state in the viewport is an anti-pattern

This tutorial keeps the counter state inside the viewport for demonstration purposes.

That works well for a small example and could be enough for small applications, but it does not scale to complex applications.
Shared state becomes harder to coordinate, background work turns into manual message passing, and more application logic ends up in the rendering process.

It is good practice to keep application state and rendering as separate concerns.
In the "Manage your state" guide, you will be introduced to `Solve` as a state management solution.

That keeps the viewport focused on rendering and rerendering instead of owning application state and business logic.

## Adding the viewport to your supervision tree

You can start a viewport module under your application supervisor like any other child.

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

## Hot code reloading in development

Emerge comes with a `CodeReloader` that works in a similar fashion to the Phoenix code reloader.

```elixir
defmodule MyApp.Application do
  use Application

  @env Mix.env()

  @impl true
  def start(_type, _args) do
    Supervisor.start_link(children(), strategy: :one_for_one, name: MyApp.Supervisor)
  end

  def children(env \\ @env)
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
    version: "0.2.1",
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

## Viewport-specific options

A viewport can also receive runtime-specific settings under `viewport: [...]`.

Two common runtime settings are:

- `renderer_check_interval_ms` controls how often the viewport checks whether the renderer is still alive.
- `input_mask` sets which raw renderer inputs are delivered to the viewport.

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

Renderer-specific input tuning stays under `emerge_skia:`. For example, to make each mouse-wheel line step scroll farther:

```elixir
@impl Viewport
def mount(opts) do
  {:ok,
   %{count: 0},
   Keyword.merge(
     [
       title: "Counter",
       emerge_skia: [scroll_line_pixels: 45]
     ],
     opts
   )}
end
```

## Raw renderer input

Raw renderer input is the low-level stream of backend and renderer notifications. It is separate from events declared on individual elements. It includes resize notifications, focus changes, pointer movement, scrolling, key input, and text input coming directly from the renderer.

To handle raw renderer input, implement the `handle_input/2` callback.

`handle_input/2` is separate from `handle_info/2`:

- `handle_info/2` handles processed element events
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

def handle_input(event, state), do: super(event, state)
```

Handling raw cursor input can cause a lot of messages, so be aware of how you handle it.
The most common use case for raw input is listening to resize events if you want to change the UI depending on window size.

## Window close

On Wayland, close requests use the dedicated `handle_close/2` callback instead of `handle_input/2`.

`use Emerge` stops the viewport by default when it receives `:window_close_requested`.

If you want custom close behavior, implement `handle_close/2`:

```elixir
@impl Viewport
def handle_close(:window_close_requested, state) do
  # Notify other processes or persist state here before stopping.
  {:stop, :normal, state}
end
```

Close requests bypass the input mask so they still arrive when you only listen
to a narrow set of raw input events.

## Run the same viewport on Nerves (DRM)

To run the same viewport directly render using linux libdrm, you need a build that includes the DRM backend, libdrm on your system, and access to a DRM device such as `/dev/dri/card0`, for nerves_system_rpi5 that will be `/dev/dri/card1`.

Start the counter like this:

```elixir
{:ok, pid} =
  MyApp.View.Counter.start_link(
    backend: :drm,
    drm_card: "/dev/dri/card0"
  )
```

On DRM, the viewport needs the rendering device explicitly so it knows which display to use.

The viewport module, UI definition, element routing, and rerender flow do not change.

To always start the application on DRM, pass those options in the child spec:

```elixir
children = [
  MyApp.View.Counter.child_spec(
    backend: :drm,
    drm_card: "/dev/dri/card0"
  )
]
```

Other DRM-specific settings:

- `hw_cursor` - Enable hardware cursor when available (default: true). If the device has a cursor plane, it will draw the cursor independently from the rest of the UI.
- `drm_cursor` - Optional DRM-only cursor overrides for `default`, `text`, and `pointer`
- `input_log` - Log DRM input devices on startup (default: false)
- `render_log` - Log DRM render/present diagnostics (default: false)
- `renderer_stats_log` - Log renderer timing stats every 5 seconds, including render and present submit timings (default: false)

Each `drm_cursor` entry supports:
- `source` (required, `.png` or `.svg`; logical path under `<otp_app>/priv`, `%Emerge.Assets.Ref{}`, or an absolute runtime path allowed by `assets.runtime_paths`)
- `hotspot` (required `{x, y}` tuple; integers and floats are allowed)
