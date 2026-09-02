# Set up a viewport

A viewport is an Elixir process that displays an Emerge UI and receives its
events. On macOS and Wayland, a viewport opens a window.

In this tutorial you will create a small counter viewport, start it, and add it
to a supervision tree.

## Define the viewport

Create a module that uses `Emerge`:

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
      attrs ++
        [
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

  def handle_info(:decrement, state) do
    {:noreply, Viewport.rerender(%{state | count: state.count - 1})}
  end
end
```

`use Emerge` makes the UI helpers available and aliases `Emerge` as
`Viewport` inside the module.

## Mount state and options

`mount/1` receives the options passed when the viewport starts. This example
returns:

```elixir
{:ok, %{count: 0}, Keyword.merge([title: "Counter"], opts)}
```

The state is passed to `render/1` and callback functions. It must be a map.

The third tuple item contains viewport configuration. Merging `opts` after the
default title lets callers override that title when starting the viewport.

A viewport without local state can instead return `{:ok, viewport_options}` and
implement `render/0`.

## Render UI

`render/1` returns an Emerge UI tree. It runs once when the viewport starts and
again after you request a rerender.

This tutorial uses `row/2`, `text/1`, and `Input.button/2`. The next tutorial,
[Describe your UI](describe_ui.md), explains elements, layout, and styling in
detail.

## Handle events

The buttons declare messages with `Event.on_press/1`:

```elixir
Input.button([Event.on_press(:increment)], text("+"))
```

Passing only a message sends it back to the viewport process. Handle it with
`handle_info/2`:

```elixir
@impl Viewport
def handle_info(:increment, state) do
  {:noreply, Viewport.rerender(%{state | count: state.count + 1})}
end
```

Update the state, pass it to `Viewport.rerender/1`, and return the resulting
state. `rerender/1` schedules another call to `render/1`.

To send an event to another process, pass `{pid, message}`:

```elixir
Event.on_press({worker_pid, :save})
```

## Start the viewport

From `iex -S mix`:

```elixir
{:ok, viewport} = MyApp.View.Counter.start_link()
```

On Linux desktop this opens a Wayland window. On macOS it opens a macOS window.

Override mounted defaults when needed:

```elixir
{:ok, viewport} =
  MyApp.View.Counter.start_link(
    title: "Counter Example",
    width: 1024,
    height: 768
  )
```

The `Emerge.Runtime.Viewport` module documentation lists viewport configuration
for DRM, headless output, raw input, and renderer diagnostics. You do not need
those options for this example.

## Supervise the viewport

A viewport can be supervised like any other child:

```elixir
defmodule MyApp.Application do
  use Application

  @impl true
  def start(_type, _args) do
    children = [
      MyApp.View.Counter
    ]

    Supervisor.start_link(children,
      strategy: :one_for_one,
      name: MyApp.Supervisor
    )
  end
end
```

Pass startup options through `child_spec/1`:

```elixir
children = [
  MyApp.View.Counter.child_spec(
    title: "Counter In App",
    width: 1024,
    height: 768
  )
]
```

## Next

Continue with [Describe your UI](describe_ui.md) to learn how to build and style
the tree returned by `render/0` or `render/1`.
