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
    output = %Emerge.Config.Window{
      title: Keyword.get(opts, :title, "Counter"),
      size: Keyword.get(opts, :size, {800, 600})
    }

    {:ok, %{count: 0}, %Emerge.Config{output: output}}
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

## Mount state and configuration

`mount/1` receives your application-defined startup arguments as a keyword list.
It returns state and a typed configuration:

```elixir
output = %Emerge.Config.Window{title: "Counter", size: {800, 600}}
{:ok, %{count: 0}, %Emerge.Config{output: output}}
```

State must be a map and is passed to `render/1` and event callbacks. The
`:__emerge__` key is reserved for Emerge.

The third tuple item must be `%Emerge.Config{}`, not the startup arguments or a
renderer keyword list. The example deliberately translates its `:title` and
`:size` arguments into a window record. Callers cannot override arbitrary renderer
fields unless your `mount/1` chooses to expose them.

A viewport without local state can return `{:ok, %Emerge.Config{}}` and implement
`render/0`. All config groups have defaults. To customize drawing, for example,
add `renderer: %Emerge.Config.Renderer{api: :raster}` to the root config.

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
state. `rerender/1` schedules the UI update; it does not call `render/1` immediately.

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
    size: {1024, 768}
  )
```

Here `title` and `size` are arguments interpreted by this particular module's
`mount/1`, not globally recognized renderer options. GenServer startup options
such as `name: MyApp.Counter` remain separate from the returned configuration.
See [viewport configuration](../reference/viewport-configuration.md) for DRM,
headless output, live fields, assets and diagnostics.

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
    size: {1024, 768}
  )
]
```

## Optional: show the count in the window title

Add this callback to the counter module to make the title follow its state:

```elixir
@impl Viewport
def config(previous, state) do
  output = %{previous.output | title: "Count: #{state.count}"}
  %{previous | output: output}
end
```

It runs before the first render and again when you request a rerender. Updating
`previous.output` preserves the window's size and backend. Supplying a new window
record instead would use defaults for fields you omit.

Use `config(previous)` when no local state is needed; implement only one arity.
Window titles can change while running, but settings such as the renderer API,
assets and initial size require a new viewport. See
[viewport configuration](../reference/viewport-configuration.md) for other settings.

Raw input is opt-in and separate from the buttons above. Continue with
[Observe raw input](handle_input.md) when you need `handle_input/2`.

## Next

Continue with [Describe your UI](describe_ui.md) to learn how to build and style
the tree returned by `render/0` or `render/1`.
