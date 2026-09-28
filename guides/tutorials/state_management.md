# Manage state

State management is one of the hard problems in UI applications. In any
non-trivial app, state diverges from the rendering model very quickly.

Take a simple dark and light theme switcher. It looks like one small piece of
state, but it affects rendering across the whole app.

A more complex example is a toolbar whose contents or behavior depend on state
owned somewhere else in the application.

State spreads through a UI much faster than it first appears. Once that
happens, keeping it in the viewport stops being a good default.

## Move state out of the viewport

This is not just an Emerge rule. It is a general UI design rule.

Users do not interact with one component in isolation. They interact with the
application as a whole. A click, a filter change, a draft edit, or a menu
selection is presented in one place, but it often affects several other parts
of the screen.

A search input can affect:

- the input itself
- the visible list
- an empty state
- a result count
- available actions in a toolbar

That is why UI state should not be thought of as "state stored near the
widget". UI state is where the application describes user interactions and
their effects.

The viewport is the bridge between your UI tree and the renderer. Its job is to
configure rendering, describe UI, and rerender when subscribed state changes.
It should not be where application state accumulates.

Once a screen has navigation, filters, form drafts, editable rows, menus, or
background work, that state belongs outside the viewport in dedicated state
processes.

That keeps rendering and application logic as separate concerns.

## Introduce Solve

Emerge uses [Solve](https://hexdocs.pm/solve) as its state management solution.
Solve is an application framework, not a presentation framework: it models an
application as a graph of reusable state machines without deciding how that
application is rendered or how input reaches it. The same Solve app can drive an
Emerge viewport, another UI, or several presentation processes at once.

Add Solve to your dependencies:

```elixir
def deps do
  [
    {:solve, "~> 0.2.2"}
  ]
end
```

Instead of pushing state into the viewport, model application behavior in
controllers and let rendering subscribe to the exposed state it needs. A
controller owns one slice of interaction and behavior. A Solve app materializes
those controllers, coordinates their lifecycles, and declares how data flows
between them.

This keeps the viewport focused on rendering while state changes are modeled in
dedicated processes.

## Start with one controller

Start with one controller.

A controller is the smallest useful unit in `Solve`. It owns one slice of
state, handles a small set of events, and exposes a plain map for the rest of
the application to read.

```elixir
defmodule MyApp.Screen do
  use Solve.Controller, events: [:set]

  @screens [
    %{id: :tasks, label: "Tasks"},
    %{id: :reports, label: "Reports"}
  ]

  @impl true
  def init(_params, _dependencies), do: %{current: :tasks}

  def set(screen, state) when screen in [:tasks, :reports] do
    %{state | current: screen}
  end

  def set(_screen, state), do: state

  @impl true
  def expose(state, _dependencies, _params) do
    %{current: state.current, screens: @screens}
  end
end
```

This controller models one interaction boundary: screen selection.

That is the right place to begin. Start with one interaction and give it one
controller.

## Run controllers in an app

Controllers do not run on their own. They run inside a `Solve` app.

The app starts the controller graph, keeps it alive, and routes events to the
right controller instance.

```elixir
defmodule MyApp.App do
  use Solve

  @impl Solve
  def controllers do
    [
      controller!(name: :screen, module: MyApp.Screen)
    ]
  end
end
```

Start the app like any other `GenServer`:

```elixir
{:ok, app} = MyApp.App.start_link(name: MyApp.App)
```

At this point:

- `MyApp.Screen` models one interaction boundary
- `MyApp.App` runs that controller
- the app becomes the stable place where rendering, workers, or tests read and
  dispatch state

A running controller exposes a plain map. Without an `expose/3` callback, Solve
exposes the controller's internal state directly. `nil` is reserved to mean that
the controller is not currently running.

At the lowest level, `Solve.subscribe/3` synchronously returns the current
exposed map and sends `%Solve.Message{}` updates to the subscriber as that map
changes:

```elixir
screen = Solve.subscribe(MyApp.App, :screen)
```

Emerge views normally use `Solve.Lookup` instead of handling those messages
manually. The direct subscription API remains useful for workers, tests, and
other presentation processes.

## Describe data flow in the app

The app is not only a runtime container. It also describes how controllers
interact.

That is the main job of the controller graph.

An app defines:

- which controllers exist
- which controllers read from others through dependencies
- which writes cross ownership boundaries through callbacks
- which repeated controller instances are materialized through collections

For example:

```elixir
defmodule MyApp.App do
  use Solve

  @impl Solve
  def controllers do
    [
      controller!(name: :task_list, module: MyApp.TaskList),
      controller!(
        name: :create_task,
        module: MyApp.CreateTask,
        callbacks: %{
          submit: fn title -> dispatch(:task_list, :create_task, title) end
        }
      ),
      controller!(
        name: :filter,
        module: MyApp.Filter,
        dependencies: [:task_list]
      )
    ]
  end
end
```

This graph describes data flow directly:

- `:filter` reads from `:task_list`
- `:create_task` writes back to `:task_list` through a callback
- `:task_list` remains the owner of the canonical data

Controllers implement behavior. The app defines how controllers read from and
write to each other. When `:task_list` exposes a new value, Solve recomputes the
exposed value of `:filter` from that dependency.

`controllers/0` is deliberately static. Solve validates the complete graph at
startup and rejects circular dependencies, so the application module remains a
reliable overview of data flow.

## Use params to control controller lifecycle

Every singleton controller has params. Omitting `params` is equivalent to a
function that always returns `true`, so the controller remains active for the
lifetime of the app.

A params function receives app params and the controller's exposed dependencies:

```elixir
controller!(
  name: :report,
  module: MyApp.Report,
  dependencies: [:screen],
  params: fn %{app_params: app_params, dependencies: %{screen: screen}} ->
    if screen.current == :reports do
      %{account_id: app_params.account_id}
    else
      false
    end
  end
)
```

Pass app-wide params when starting Solve:

```elixir
{:ok, app} = MyApp.App.start_link(params: %{account_id: 42})
```

The params value controls both lifecycle and identity:

| Previous | Current | Result |
| --- | --- | --- |
| false or `nil` | false or `nil` | remain stopped |
| false or `nil` | truthy | start with the new params |
| truthy | false or `nil` | stop and expose `nil` |
| truthy | equal truthy value | keep the existing controller |
| truthy | different truthy value | stop and start a fresh controller |

The truthy params value is passed to `init/2`, event handlers that request it,
and `expose/3`. Use stable values when a controller should preserve its state.
Use a changed value intentionally when the logical identity changed and the
state should restart.

A controller process also restarts from `init/2` after a crash. Subscribers see
`nil` while that controller is unavailable, followed by its newly exposed state.

## Keep the viewport focused on rendering

A good viewport does very little:

- configure renderer options in `mount/1`
- delegate UI construction to a view module
- rerender when subscribed state changes

```elixir
defmodule MyApp.View.Root do
  use Emerge
  use Solve.Lookup

  @impl Viewport
  def mount(opts) do
    {:ok, Keyword.merge([title: "My App"], opts)}
  end

  @impl Viewport
  def render() do
    MyApp.RootView.layout()
  end

  @impl Solve.Lookup
  def handle_solve_updated(_updated, state) do
    {:ok, Viewport.rerender(state)}
  end
end
```

The viewport does not own navigation state, list state, or editor state here.
It only renders and rerenders.

## Decouple concerns into smaller units

State is easier to reason about when each part of the application owns one
concern.

Examples of separate concerns:

- current screen
- current filter
- draft input value
- visible item ids
- open or closed menu state
- one edit session per row

These concerns can all affect the same screen without belonging in the same
state structure.

A simple rule applies here: if two pieces of state can change independently,
they deserve separate ownership.

## Model user interactions in controllers

A controller is not just a place to store values. A controller models how one
slice of user interaction changes the application.

That interaction is often larger than the component where it is presented.

A filter control may be rendered as one small row of buttons, but the
interaction behind it can affect:

- which items are visible
- which counters are shown
- which bulk actions are enabled
- which empty state appears

That interaction should be modeled as one coherent unit.

A small controller looks like this:

```elixir
defmodule MyApp.Screen do
  use Solve.Controller, events: [:toggle_menu, :close_menu, :set_screen]

  @screens [
    %{id: :tasks, label: "Tasks"},
    %{id: :reports, label: "Reports"}
  ]

  @impl true
  def init(_params, _dependencies) do
    %{current: :tasks, menu_open?: false}
  end

  def toggle_menu(_payload, state), do: %{state | menu_open?: !state.menu_open?}
  def close_menu(_payload, state), do: %{state | menu_open?: false}

  def set_screen(screen, state) when screen in [:tasks, :reports] do
    %{state | current: screen, menu_open?: false}
  end

  def set_screen(_screen, state), do: state

  @impl true
  def expose(state, _dependencies, _params) do
    %{
      current: state.current,
      menu_open?: state.menu_open?,
      screens: @screens
    }
  end
end
```

This controller owns one thing: screen selection and its menu state.

Every event named in `events:` must have exactly one implemented arity. Multiple
function clauses at that arity are allowed, as `set_screen/2` demonstrates. An
event handler can request only the leading runtime values it needs:

```elixir
event_name(payload)
event_name(payload, state)
event_name(payload, state, dependencies)
event_name(payload, state, dependencies, callbacks)
event_name(payload, state, dependencies, callbacks, init_params)
```

Each handler returns the controller's next internal state directly. Defining the
same event at more than one arity is a compile error.

A controller is well-shaped when it has:

- one clear interaction boundary
- one small event surface
- one exposed state map
- one reason to change

## Expose render-ready state

Controllers expose the data the UI actually wants to render.

That means exposing values such as:

- selected screen
- available menu items
- visible ids
- active filter
- counts
- status flags

Do not make every view recompute these values from low-level internal state.

For example, a filter controller exposes `visible_ids` directly instead of
forcing the view to filter the full list again during rendering. In this
example, `task_list` exposes ordered `ids` and a `todos` map whose values contain
a `completed?` field:

```elixir
defmodule MyApp.Filter do
  use Solve.Controller, events: [:set]

  @filters [:all, :active, :completed]

  @impl true
  def init(_params, _dependencies), do: %{active: :all}

  def set(filter, _state) when filter in @filters, do: %{active: filter}
  def set(_filter, state), do: state

  @impl true
  def expose(state, _dependencies = %{task_list: task_list}, _params) do
    %{
      filters: @filters,
      active: state.active,
      visible_ids: visible_ids(state.active, task_list)
    }
  end

  defp visible_ids(:all, %{ids: ids}), do: ids

  defp visible_ids(:active, %{ids: ids, todos: todos}),
    do: Enum.reject(ids, fn id -> todos[id].completed? end)

  defp visible_ids(:completed, %{ids: ids, todos: todos}),
    do: Enum.filter(ids, fn id -> todos[id].completed? end)
end
```

This keeps the view declarative. The view maps over `visible_ids` instead of
rebuilding filtering logic itself.

## Read state directly in views

Views read the controller state they need and no more.

With `Solve.Lookup`, views subscribe directly to exposed controller state:

```elixir
defmodule MyApp.RootView do
  use Emerge.UI
  use Solve.Lookup, :helpers

  def layout do
    el(
      [width(fill()), height(fill()), Nearby.in_front(menu_button())],
      active_screen()
    )
  end

  defp menu_button do
    screen = solve(MyApp.App, :screen)

    Input.button(
      [Event.on_press(event(screen, :toggle_menu))],
      text("Menu")
    )
  end
end
```

This is the basic pattern:

- read a singleton controller with `solve/2`
- read a collection source with `collection/2`
- build event tuples with `event/2` or `event/3`
- render directly from exposed state
- keep unrelated state out of the same view function

The first lookup in a process subscribes synchronously and caches the exposed
value in that process. Later lookups read the cache. `use Solve.Lookup` installs
the `%Solve.Message{}` handling that refreshes the cache and invokes
`handle_solve_updated/2`; that is why the viewport rerenders in the earlier
example.

`use Solve.Lookup, :helpers` only imports the lookup helpers. It is appropriate
for a view helper module because its functions execute in the viewport process,
which already owns the subscriptions and message handling. Do not use helper
mode in a separate long-running process unless another module in that same
process handles Solve updates.

This makes it clear which controller drives which part of the UI. It also avoids
prop drilling controller refs through many helper layers just to reach a leaf
widget.

## Keep reusable widgets state-agnostic

Reusable primitives stay state-agnostic.

When you build small shared pieces such as:

- buttons
- chips
- tabs
- toolbars
- nav bars
- cards
- dropdown shells

keep them focused on presentation and generic interaction surfaces. Let them
accept attrs, content, flags, and event tuples from the caller.

Then let domain-specific view helpers build on top of those primitives.

A good split looks like this:

```elixir
def action_button(attrs, content) do
  Input.button(
    [
      padding(10),
      Background.color(color(:slate, 100)),
      Border.rounded(8)
    ] ++ attrs,
    content
  )
end

def delete_button(task_id) do
  task_list = solve(MyApp.App, :task_list)

  action_button(
    [Event.on_press(event(task_list, :delete_task, task_id))],
    text("Delete")
  )
end
```

Here:

- `action_button/2` stays reusable
- `delete_button/1` is intentionally domain-specific
- the domain helper reads its own state with `solve/2`
- no controller ref has to be passed through unrelated helper layers

The thing to avoid is coupling the reusable primitive itself to one controller
graph or one domain concern.

## Dispatch events to the owner

UI events go back to the controller that owns the state being changed.

For example:

```elixir
def screen_tab(screen_id, label) do
  screen = solve(MyApp.App, :screen)

  Input.button(
    [Event.on_press(event(screen, :set_screen, screen_id))],
    text(label)
  )
end

def create_task_input do
  create_task = solve(MyApp.App, :create_task)

  Input.text(
    [Event.on_change(event(create_task, :set_title))],
    create_task.title
  )
end

def comment_body_input do
  comment = solve(MyApp.App, :comment_draft)

  Input.multiline(
    [Event.on_change(event(comment, :set_body))],
    comment.body
  )
end
```

`event/2` reads the direct event tuple attached by `Solve.Lookup`; `event/3`
adds a fixed payload. Code outside the view can dispatch through the app
explicitly:

```elixir
Solve.dispatch(MyApp.App, :screen, :set_screen, :reports)
```

Dispatching through the app keeps event delivery aligned with controller
lifecycle changes.

This keeps ownership obvious:

- screen selection goes to the screen controller
- input changes go to the input controller
- list mutations go to the list controller

The same ownership pattern works for longer drafts such as notes, descriptions,
or comments. `Input.multiline/2` still emits updated values through
`on_change/1`; omitting `height(...)` just means the field auto-grows with its
wrapped content.

The viewport should not become a generic event router for application logic.

## Derive state from dependencies

Not all state should be stored directly. Some state is better derived from other
controllers.

Typical derived values include:

- filtered ids
- grouped sections
- status summaries
- item counts
- selected labels
- enabled actions

A controller depends on the exposed state of another controller to compute
these values in one place. Whenever a dependency exposes a changed map, Solve
refreshes the dependent controller's cached dependencies, reevaluates its
params, and recomputes its exposed map. The dependent controller does not need
to copy derived values into internal state.

Dependencies provide read-only data flow. Declare them in the app rather than
reaching into another controller process directly. This keeps source-of-truth
state smaller, avoids duplicating logic in views, and leaves the full dependency
graph visible in `controllers/0`.

## Use callbacks to cross boundaries

Sometimes one controller owns temporary UI state while another controller owns
the domain data.

For example, an input controller owns the current draft title, while the list
controller owns the actual items.

In that case, use explicit callbacks or app-level dispatch wiring.

App definition:

```elixir
controller!(
  name: :create_task,
  module: MyApp.CreateTask,
  callbacks: %{
    submit: fn title -> dispatch(:task_list, :create_task, title) end
  }
)
```

Controller:

```elixir
defmodule MyApp.CreateTask do
  use Solve.Controller, events: [:set_title, :submit]

  @impl true
  def init(_params, _dependencies), do: %{title: ""}

  def set_title(title) when is_binary(title) do
    %{title: title}
  end

  def submit(_payload, state, _dependencies, _callbacks = %{submit: submit}) do
    case String.trim(state.title) do
      "" ->
        %{title: ""}

      title ->
        submit.(title)
        %{title: ""}
    end
  end
end
```

This keeps ownership explicit:

- one controller owns the draft input
- another controller owns the list
- the handoff is visible in the app graph

Callbacks are plain functions and are passed only to event handlers that request
the callbacks argument. They intentionally cross the read-only dependency graph,
so declare them in the app and avoid callback cycles.

## Handle external process messages in controllers

Controllers are `GenServer` processes and can react to timers, PubSub messages,
monitors, and other BEAM messages with `handle_info`. Like event handlers,
`handle_info` requests a leading subset of controller runtime values and returns
the next internal state directly:

```elixir
def handle_info(message, state)
def handle_info(message, state, dependencies)
def handle_info(message, state, dependencies, callbacks)
def handle_info(message, state, dependencies, callbacks, init_params)
```

For example, a controller can clear a temporary status after a timer:

```elixir
def handle_info(:clear_status, state), do: %{state | status: nil}
```

Use `Solve.dispatch/4` for application events. Reserve `handle_info` for messages
that originate outside Solve's event routing.

## Use collection controllers for repeated local state

When the same behavior repeats across many entities, use collection
controllers.

This fits cases like:

- one edit session per row
- one expanded state per item
- one upload state per file
- one inspector state per node

A collection controller reuses one controller design while keeping each item's
local state separate.

```elixir
controller!(
  name: :task_editor,
  module: MyApp.TaskEditor,
  variant: :collection,
  dependencies: [:task_list],
  collect: fn _context = %{dependencies: %{task_list: task_list}} ->
    Enum.map(task_list.ids, fn id ->
      {id, [params: %{id: id, title: task_list.todos[id].title}]}
    end)
  end
)
```

The `collect` function receives exposed dependencies and app params. It returns
one `{id, options}` pair per active item; options can contain item-specific
`params` and `callbacks`. Removing an id stops that controller and exposes its
removal to collection subscribers. Reintroducing the id starts fresh state.

Views then read one item-specific controller instance directly:

```elixir
def edit_row(task_id) do
  editor = solve(MyApp.App, {:task_editor, task_id})

  Input.text(
    [Event.on_change(event(editor, :set_title))],
    editor.title
  )
end
```

Use `collection(MyApp.App, :task_editor)` when a view needs the complete
collection rather than one item.

Use collection controllers when many entities need the same local behavior and
each entity needs isolated state. Do not use them merely to store a list; a
singleton controller with a map is simpler when items do not need independent
lifecycles. If each item needs a multiline draft, swap `Input.text/2` for
`Input.multiline/2`; the collection ownership pattern stays the same.

## Keep overlays close to the trigger

Menus, popovers, and dropdowns are easier to manage when the trigger renders the
nearby content directly.

A good pattern is:

- a controller owns whether the overlay is open
- the trigger dispatches open and close events
- the view attaches the overlay with nearby helpers
- the menu maps over controller-exposed options

```elixir
defp menu_button do
  screen = solve(MyApp.App, :screen)

  Input.button(
    [
      Event.on_press(event(screen, :toggle_menu)),
      Nearby.below(menu())
    ],
    text("Menu")
  )
end
```

And the menu itself maps exposed options:

```elixir
defp menu do
  screen = solve(MyApp.App, :screen)

  if screen.menu_open? do
    column(
      [spacing(4)],
      Enum.map(screen.screens, fn item ->
        Input.button(
          [Event.on_press(event(screen, :set_screen, item.id))],
          text(item.label)
        )
      end)
    )
  else
    none()
  end
end
```

This keeps overlay behavior local and keeps menu structure driven by state
instead of scattered conditionals.

## Start state before rendering

Start the Solve app before the viewport in your supervision tree.

```elixir
def start(_type, _args) do
  children = [
    MyApp.App.child_spec([]),
    MyApp.View.Root
  ]

  Supervisor.start_link(children, strategy: :one_for_one, name: MyApp.Supervisor)
end
```

This ensures:

- state is ready when the viewport first renders
- subscriptions resolve immediately
- the viewport renders against live application state

The viewport depends on state processes, not bootstrap them during rendering.

## Split independent domains into separate apps

Use separate `Solve` apps when parts of your UI become genuinely independent
domains.

That means they:

- have their own controller graph
- evolve independently
- do not share much internal state ownership
- are composed together at a higher level rather than tightly coordinated

For example, one application may grow into independent domains such as:

- navigation shell
- task management
- reporting
- media library

Separate apps also fit when you need different variants of the same feature or
screen. For example, a regular user app and an admin app can reuse the same
common controllers while the admin app adds extra admin-only state and
behavior.

Controllers stay reusable because the app defines the graph around them. This
works when the reused controllers still receive the dependency keys, params,
and callbacks they expect.

At that point, separate apps are a good fit. Until then, one app with multiple
focused controllers is the simpler design.
