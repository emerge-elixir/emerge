defmodule Emerge.UI.Input do
  @moduledoc """
  Input element helpers.

  `Emerge.UI.Input` provides the two main interactive element constructors:

  - `text/2` for editable single-line text input
  - `button/2` for button-like interaction around any single child element

  These helpers provide input behavior, not default visuals. Style them with the
  same attrs you would use on other elements, such as `Emerge.UI.Background`,
  `Emerge.UI.Border`, `Emerge.UI.Font`, and `Emerge.UI.Interactive`.

  ## Text Input

  `text/2` builds a single-line text input element. The second argument is the
  current string value.

  Pair it with `Emerge.UI.Event.on_change/1` when you want to receive updated
  values, and with `Emerge.UI.Event.on_focus/1` or `Emerge.UI.Event.on_blur/1`
  when you care about focus changes.

  ## Buttons

  `button/2` wraps exactly one child element and gives it button-like
  interaction behavior.

  The child can be any element tree, not just `Emerge.UI.text/1`. If you need a
  more complex button body, wrap multiple visual parts in `Emerge.UI.row/2`,
  `Emerge.UI.column/2`, or `Emerge.UI.el/2` and pass that as the one child.

  `button/2` does not apply default styling. A typical button composes
  `Emerge.UI.Event.on_press/1` with visual attrs such as padding, background,
  border radius, and interaction styles.

  ## Examples

  This render function builds a common form layout: a search field above a save
  button.

  ```elixir
  def render(state) do
    column([spacing(16)], [
      Input.text(
        [
          key(:search),
          width(px(240)),
          padding(12),
          Background.color(color(:white)),
          Border.rounded(8),
          Event.on_change(:search_changed)
        ],
        state.query
      ),
      Input.button(
        [
          padding(12),
          Background.color(color(:sky, 500)),
          Border.rounded(8),
          Font.color(color(:white)),
          Event.on_press(:save)
        ],
        text("Save")
      )
    ])
  end
  ```
  """

  alias Emerge.UI.Internal.Builder
  alias Emerge.UI.Internal.Validation

  @typedoc "Input element returned by this module."
  @type t :: Emerge.UI.element()

  @doc """
  Build a single-line text input.

  `value` is the current content shown in the field. The element has no
  children.

  Use `Emerge.UI.Event.on_change/1` to receive updated values and treat the
  second argument as the source of truth for the currently rendered content.

  ## Example

  This field keeps its rendered value in `state.query` and emits messages for
  change, focus, and blur.

  ```elixir
  def render(state) do
    Input.text(
      [
        key(:search),
        width(px(240)),
        padding(12),
        Background.color(color(:white)),
        Border.rounded(8),
        Event.on_change(:search_changed),
        Event.on_focus(:search_focused),
        Event.on_blur(:search_blurred)
      ],
      state.query
    )
  end
  ```
  """
  @spec text(Emerge.UI.attrs(), String.t()) :: t()
  def text(attrs, value) do
    {attrs, nearby} = Builder.prepare_attrs!("Input.text/2", attrs)
    value = Validation.validate_binary_string!("Input.text/2", value)

    attrs
    |> Map.put(:content, value)
    |> Builder.build_element(nearby, :text_input, [])
  end

  @doc """
  Build a button-like element with exactly one child.

  `button/2` is unstyled. Compose it with attrs such as
  `Emerge.UI.Event.on_press/1`, `Emerge.UI.Background.color/1`,
  `Emerge.UI.Border.rounded/1`, and `Emerge.UI.Interactive.mouse_down/1` to get
  the behavior and appearance you want.

  The child can be any element tree. If you need multiple visual pieces inside
  the button, wrap them in a layout element and pass that layout as the one
  child.

  ## Examples

  This is a simple styled action button with pressed feedback.

  ```elixir
  Input.button(
    [
      padding(12),
      Background.color(color(:sky, 500)),
      Border.rounded(8),
      Font.color(color(:white)),
      Event.on_press(:save),
      Interactive.mouse_down([Transform.move_y(1)])
    ],
    text("Save")
  )
  ```

  ```elixir
  # Wrap multiple visual parts in a layout element and pass that layout
  # as the button's single child.
  Input.button(
    [
      padding(10),
      Background.color(color(:slate, 100)),
      Border.rounded(8),
      Event.on_press(:open_menu)
    ],
    row([spacing(8)], [
      text("v"),
      text("Actions")
    ])
  )
  ```
  """
  @spec button(Emerge.UI.attrs(), Emerge.UI.child()) :: t()
  def button(attrs, child) do
    {attrs, nearby, child} = Builder.prepare_single_child!("Input.button/2", attrs, child)
    Builder.build_element(attrs, nearby, :el, [child])
  end
end
