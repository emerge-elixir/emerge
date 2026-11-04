# Observe raw input

Most UIs need only element events such as `Event.on_press/1`, `Event.on_change/1`
and key bindings. Handle those ordinary messages in `handle_info/2`.
Use `handle_input/2` when you need window-level input, such as a global keyboard
shortcut, rather than events attached to a particular element.

## Subscribe explicitly

Input observation defaults to **none**. Return the categories you need in
`config.input.events`:

```elixir
defmodule MyApp.KeyboardWindow do
  use Emerge

  @impl Viewport
  def mount(_opts) do
    {:ok,
     %Emerge.Config{
       input: %Emerge.Config.Input{events: [:key, :pointer_button, :resize]}
     }}
  end

  @impl Viewport
  def render(), do: text("Escape closes this window")

  @impl Viewport
  def handle_input({:key, {:escape, :press, _modifiers}}, state) do
    {:stop, :normal, state}
  end

  def handle_input(_event, state), do: {:noreply, state}
end
```

Use `Emerge.Config.Input.all_events/0` for all categories or `[]` for none.
Use category atoms, not a numeric mask or `:all`/`:none`. Duplicates and order
do not affect subscriptions. To change the list while running, update the input
record in a config callback and request a rerender:

```elixir
@impl Viewport
def config(previous, state) do
  input = %{previous.input | events: state.observed_events}
  %{previous | input: input}
end
```

This version assumes a stateful viewport with an `observed_events` list in its
state; the stateless example above needs no config callback. New lists replace
old lists. Messages already in your mailbox are not retracted.

## Exact viewport callback values

The first argument of `handle_input/2` is an `Emerge.Input.event()`:

| Subscription | Callback value |
|---|---|
| `:key` | `{:key, {key, :press \| :release, modifiers}}` |
| `:text` | `{:text_commit, {text, modifiers}}`, `{:text_preedit, {text, cursor_range_or_nil}}`, `:text_preedit_clear`, `{:delete_surrounding, {before_bytes, after_bytes}}` |
| `:pointer_move` | `{:pointer_move, {x, y}}` |
| `:pointer_button` | `{:pointer_button, {button, :press \| :release, modifiers, {x, y}}}` |
| `:pointer_scroll` | `{:pointer_scroll, {{dx, dy}, {x, y}}}` |
| `:pointer_enter` | `{:pointer_enter, entered?}` |
| `:resize` | `{:resize, {width, height, scale}}` |
| `:focus` | `{:focus, focused?}` |
| `:touch` | `{:touch, {contact_id, :down \| :move \| :up \| :cancel, {x, y}}}` |
| `:display_wake` | `{:display_wake, :single_tap \| :double_tap \| :long_press}` |

These are the **viewport** values, not the low-level `{:emerge_skia_event, event}`
envelope sent to `EmergeSkia.set_input_target/2`. The viewport normalizes native
names such as `:cursor_pos` and numeric press/release actions into the values above.
A standalone renderer uses the native payload contract documented by that function.

Keys are canonical identifiers, not inserted characters. For example, Shift+=
can report `:equal` with `[:shift]` while committing `"+"` through `:text`.
Repeated key presses are still `:press`. Use text commits/IME events for text
input: one commit may contain multiple Unicode characters.

Modifiers are `:shift`, `:ctrl`, `:alt` and `:meta`. Pointer buttons are `:left`,
`:right`, `:middle`, `:back`, `:forward` or `:other`. See `Emerge.Input` for the full
closed key union and payload types.

Coordinates are backend surface coordinates. Discrete scroll steps use
`input.scroll_line_pixels` (default `30.0`); precise deltas follow backend normalization.
Resize reports output dimensions and scale. Enter/focus refer to the **window**,
not an individual UI element. IME preedit ranges come from the platform; surrounding
text deletion counts UTF-8 bytes, not characters.

## DRM display wake

An off DRM display can opt into touch wake with
`%Emerge.Config.DRM{wake_on_touch: :double_tap}`. The choices are `:single_tap`,
`:double_tap` and `:long_press`; the default `nil` disables wake. This uses the
existing DRM power controls and requires an explicit OpenGL, Vulkan or raster
renderer and a touchscreen that continues delivering events while the panel is off.

To observe accepted wakes through `handle_input/2`, include `:display_wake` in
`input.events`:

```elixir
%Emerge.Config{
  output: %Emerge.Config.DRM{display: :off, wake_on_touch: :double_tap},
  renderer: [api: :opengl],
  input: [events: [:display_wake]]
}
```

An empty list or `:touch` alone does not deliver wake events. The display still
wakes without an observer subscription; `wake_on_touch: nil` disables waking even
when subscribed. If your config derives display state from application state,
subscribe and update that state here:

```elixir
def handle_input({:display_wake, _gesture}, state) do
  {:noreply, %{state | sleeping?: false}}
end

def handle_input(_event, state), do: {:noreply, state}
```

Desired display state is already `:on` before this event, and config/render
reconciliation follows it. A later explicit off still wins. The event reports
accepted wake intent, not physical presentation. The waking contacts are consumed:
they do not also press an element or appear as raw touch events.

## Observation is not input control

`events: []` stops input callbacks, including wake notifications; it does **not**
disable configured touch wake, hit testing, text editing,
scrolling, buttons, keyboard bindings or element callbacks. Observers do not consume
an event or override native dispatch by returning `{:noreply, state}`.

Window close requests go to `handle_close/2` independently of raw subscriptions.
The default closes the viewport normally. A DRM display does not deliver ordinary
UI input while off. Configured touch wake uses the control notification described
above; otherwise use an independent source, such as a power-button reader, to wake it.

Availability is backend-dependent:

- Headless has no physical input source; a subscription does not create one.
- DRM has no desktop window enter/focus lifecycle.
- IME capabilities vary. The macOS host supports commit/preedit/clear, but does not
  currently emit surrounding-text deletion notifications through its observer path.

For changed callback names and actions, see [Migrate to 0.5.0](../migrations/0.5.md).
For configuration examples, see
[configuration records](https://hexdocs.pm/emerge/Emerge.Config.html).
