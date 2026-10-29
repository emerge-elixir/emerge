# Handle input

Most UIs need only element events such as `Event.on_press/1`, `Event.on_change/1`
and key bindings. Handle those ordinary messages in `handle_info/2`.
Use `handle_input/2` when you need window-level input, such as a global keyboard
shortcut, rather than events attached to a particular element.

## Element-local mouse positions

Use `Event.on_mouse_move/1`, `on_mouse_down/1`, `on_mouse_up/1`,
`on_mouse_enter/1`, `on_mouse_leave/1`, or `on_click/1` when you need the
position within a particular element:

```elixir
def render(_state) do
  el([width(px(100)), height(px(60)), Event.on_mouse_move(:moved)], none())
end

# With the default payload wrapper:
def handle_info({:moved, %{x: x, y: y}}, state) do
  {:noreply, state |> Map.put(:pointer, {x, y}) |> Viewport.rerender()}
end
```

Positions are always floats in the listener element's logical pixels, measured
from its outer top-left corner, including border/padding. Display and inherited
layout/paint scaling, transforms and ancestor scrolling are undone: at effective
scale 2, 71 physical pixels from the displayed left edge becomes `x: 35.5`. Do not
divide by scale again. Leave positions are not clamped and may be outside the
logical box. Window leave reports the last known pointer position. Down reports
the press position; up/click report the release position. `on_mouse_up` targets
the element under that release, even if another element received the press.

Enter/leave use the transition position, including transitions caused by layout
or transforms under a stationary pointer. Layout revalidation does not synthesize
move callbacks. Events do not bubble; attach each listener to the element whose
coordinate space you need. `Event.on_scroll/1` also uses an x/y map, but its values
are normalized scroll fractions in `[0, 1]`, not pointer positions.

These six events now carry a payload instead of delivering only the stored
message. Change `handle_info(:moved, state)` to the pattern above. Tuple messages
have the map appended, for example `{:selected, id, position}`. External-PID
routes follow the same rules; custom `wrap_payload/3` callbacks receive the map.
`on_press/1` remains payload-free and is still preferred for ordinary actions.
See the [0.6 migration guide](../migrations/0.6.md) for handler and adapter updates
when upgrading from 0.5.

No raw input subscription is needed for element events. The window-level raw
observers below keep their existing surface coordinates and message shapes.

## Subscribe to raw input explicitly

Raw observation defaults to **none**. Return the categories you need in
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

On DRM, raw touch observers receive separate contact lifetimes; up/cancel carry
the contact's last position. They do not receive primary-touch mouse emulation.
Element interactions still use that emulation for ordinary controls and dragging.

## Drag outside a custom control

Ordinary mouse handlers follow hit testing: `on_mouse_up` targets the release
location, not necessarily the element pressed. For a color picker or other custom
control, use `Event.on_drag/1` to retain movement and release outside its bounds:

```elixir
defmodule MyApp.PointerPad do
  use Emerge

  @impl Viewport
  def mount(_opts) do
    {:ok, %{pointer: nil, dragging?: false}, %Emerge.Config{}}
  end

  @impl Viewport
  def render(state) do
    column([spacing(8)], [
      el(
        [
          width(px(180)),
          height(px(120)),
          Background.color(color(:slate, 200)),
          Event.on_drag(:picked)
        ],
        none()
      ),
      text(inspect(state.pointer))
    ])
  end

  @impl Viewport
  def handle_info({:picked, %{phase: phase, x: x, y: y}}, state)
      when phase in [:start, :move, :end] do
    state =
      state
      |> Map.put(:pointer, {x, y})
      |> Map.put(:dragging?, phase != :end)

    {:noreply, Viewport.rerender(state)}
  end

  def handle_info({:picked, %{phase: :cancel}}, state) do
    {:noreply, state |> Map.put(:dragging?, false) |> Viewport.rerender()}
  end
end
```

`:start` is the initial left press inside the pad, `:move` is real captured
movement, and `:end` is release anywhere. Window leave or focus loss emits
`:cancel` at the last known position. This example preserves the last selected
point on cancellation while clearing its drag flag.

Coordinates use current source geometry after patches, not the press-time box.
Clamp a derived selected value, not the received coordinates. Ordinary enter/leave
still follow hit testing during capture. Removing the source/handler or remounting
discards capture without a final callback; reset application state when removing
the control. Keyboard activation does not start dragging.

Use this on custom elements rather than native text inputs or sliders. Do not
combine it on the same element with click/press, swipe or virtual-key handlers;
see `Emerge.UI.Event.on_drag/1` for the full contract.

## Observation is not input control

`events: []` stops raw callbacks; it does **not** disable hit testing, text editing,
scrolling, buttons, keyboard bindings or element callbacks. Observers do not consume
an event or override native dispatch by returning `{:noreply, state}`.

Window close requests go to `handle_close/2` independently of raw subscriptions.
The default closes the viewport normally. A DRM display does not deliver UI input
while off; applications need an independent source, such as a power-button reader,
to turn it back on.

Availability is backend-dependent:

- Headless has no physical input source; a subscription does not create one.
- DRM has no desktop window enter/focus lifecycle.
- IME capabilities vary. The macOS host supports commit/preedit/clear, but does not
  currently emit surrounding-text deletion notifications through its observer path.

For changed callback names and actions, see [Migrate to 0.5.0](../migrations/0.5.md).
For configuration examples, see
[configuration records](https://hexdocs.pm/emerge/Emerge.Config.html).
