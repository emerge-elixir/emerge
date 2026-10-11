# Observe raw input

Most UIs need only element events such as `Event.on_press/1`, `Event.on_change/1`
and key bindings. Handle those ordinary messages in `handle_info/2`.
`handle_input/2` is a separate observer for backend surface/window input.

## Subscribe explicitly

Raw observation defaults to **none**. Return the categories you need in
`config.input.events`:

```elixir
defmodule MyApp.KeyboardWindow do
  use Emerge

  @impl Viewport
  def mount(_opts) do
    {:ok, %Emerge.Config{input: [events: [:key, :pointer_button, :resize]]}}
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
There are no numeric masks or `:all`/`:none` sentinels. Duplicates are deduplicated;
ordering does not affect the subscription set. `input.events` is live: return
updated subscriptions from `config(previous)` or `config(previous, state)` and
request a rerender cycle. New lists replace old lists. Already-forwarded messages
may still be in the mailbox after an update.

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

## Observation is not input control

`events: []` stops raw callbacks; it does **not** disable hit testing, text editing,
scrolling, buttons, keyboard bindings or element callbacks. Observers do not consume
an event or override native dispatch by returning `{:noreply, state}`.

Window close requests go to `handle_close/2` independently of raw subscriptions.
The default closes the viewport normally. On DRM, display-off separately gates
native UI input; an application's independent wake-key reader is outside this
observer subscription mechanism.

Availability is backend-dependent:

- Headless has no physical input source; a subscription does not create one.
- DRM has no desktop window enter/focus lifecycle.
- IME capabilities vary. The macOS host supports commit/preedit/clear, but does not
  currently emit surrounding-text deletion notifications through its observer path.

For the old-to-new tuple mapping see [Migrate to 0.5](../migrations/0.5.md#raw-input).
For subscriptions, replacement and recovery see
[viewport configuration](../reference/viewport-configuration.md).
