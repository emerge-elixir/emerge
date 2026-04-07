defmodule Emerge.UI.Interactive do
  @moduledoc """
  Conditional style blocks for interaction states.

  `Emerge.UI.Interactive` lets you change how an element looks when it is
  hovered, focused, or pressed.

  Use:

  - `mouse_over/1` for pointer-over styling
  - `focused/1` for focus styling
  - `mouse_down/1` for pressed styling

  These helpers are purely visual. They do not send messages. Pair them with
  `Emerge.UI.Event` when you want both visual feedback and behavior on the same
  element.

  ## Allowed Attrs

  State blocks accept decorative attrs from these groups:

  - background
  - border and shadow
  - font styling
  - SVG tint
  - transforms

  In practice that means helpers such as `Background.color/1`,
  `Border.rounded/1`, `Border.glow/2`, `Font.color/1`, `Font.underline/0`,
  `Svg.color/1`, and `Transform.move_y/1` are all valid inside interaction
  state blocks.

  ## Not Allowed

  State blocks do not accept layout or behavior attrs such as:

  - `width`, `height`, `padding`, or `spacing`
  - event handlers like `Event.on_press/1`
  - nearby attrs
  - nested `mouse_over/1`, `focused/1`, or `mouse_down/1`

  ## Combining States

  You can attach multiple interaction states to the same element.

  When more than one state is active, their styles combine. If two active states
  set the same attr, `mouse_down` wins over `focused`, and `focused` wins over
  `mouse_over`.

  ## Examples

  A button with hover, focus, and pressed styling:

  The base button is dark slate. Hover lightens the background, focus adds a
  ring, and pressing nudges the button down by 1px.

  ```elixir
  Input.button(
    [
      padding(12),
      Background.color(color(:slate, 700)),
      Border.rounded(8),
      Font.color(color(:white)),
      Event.on_press(:save),
      Interactive.mouse_over([
        Background.color(color(:slate, 600))
      ]),
      Interactive.focused([
        Border.color(color(:sky, 400)),
        Border.glow(color_rgba(56, 189, 248, 0.35), 2)
      ]),
      Interactive.mouse_down([
        Transform.move_y(1)
      ])
    ],
    text("Save")
  )
  ```

  A text input with a focus ring:

  This keeps the field visually quiet until it receives focus, then adds a
  border color and glow.

  ```elixir
  Input.text(
    [
      width(px(260)),
      padding(12),
      Background.color(color(:white)),
      Border.rounded(8),
      Event.on_change(:search_changed),
      Interactive.focused([
        Border.color(color(:sky, 400)),
        Border.glow(color_rgba(56, 189, 248, 0.35), 2)
      ])
    ],
    state.query
  )
  ```
  """

  alias Emerge.UI.Internal.Validation

  @typedoc "Normalized decorative style map applied while an interaction state is active."
  @type state_style :: map()
  @type mouse_over_attr :: {:mouse_over, state_style()}
  @type focused_attr :: {:focused, state_style()}
  @type mouse_down_attr :: {:mouse_down, state_style()}
  @type t :: mouse_over_attr() | focused_attr() | mouse_down_attr()

  @doc """
  Apply decorative styling while the pointer is over the element.

  Use this for hover treatments such as color changes, underline, border color,
  or subtle background shifts.

  ## Example

  This pattern is useful for text actions that should feel interactive without
  turning into fully boxed buttons.

  ```elixir
  Input.button(
    [
      Font.color(color(:slate, 600)),
      Background.color(color_rgba(255, 255, 255, 0.0)),
      Event.on_press(:open_menu),
      Interactive.mouse_over([
        Font.underline(),
        Font.color(color(:slate, 900))
      ])
    ],
    text("Open menu")
  )
  ```
  """
  @spec mouse_over(Emerge.UI.attrs()) :: mouse_over_attr()
  def mouse_over(attrs) when is_list(attrs),
    do: {:mouse_over, Validation.parse_state_style_attrs(attrs, :mouse_over)}

  @doc """
  Apply decorative styling while the element is focused.

  This is commonly used for focus rings on buttons and inputs.

  ## Example

  This is the usual focus-ring pattern for text fields and buttons that need a
  clear keyboard focus treatment.

  ```elixir
  Input.text(
    [
      width(px(260)),
      padding(12),
      Background.color(color(:white)),
      Border.rounded(8),
      Event.on_change(:search_changed),
      Interactive.focused([
        Border.color(color(:sky, 400)),
        Border.glow(color_rgba(56, 189, 248, 0.35), 2)
      ])
    ],
    state.query
  )
  ```
  """
  @spec focused(Emerge.UI.attrs()) :: focused_attr()
  def focused(attrs) when is_list(attrs),
    do: {:focused, Validation.parse_state_style_attrs(attrs, :focused)}

  @doc """
  Apply decorative styling while the left mouse button is pressed on the element.

  This is commonly used for pressed feedback such as a 1px downward movement or
  a small opacity change.

  ## Example

  This gives the button a subtle pressed feel by moving it down slightly and
  lowering the opacity while the mouse is held.

  ```elixir
  Input.button(
    [
      padding(12),
      Background.color(color(:sky, 500)),
      Border.rounded(8),
      Font.color(color(:white)),
      Event.on_press(:save),
      Interactive.mouse_down([
        Transform.move_y(1),
        Transform.alpha(0.92)
      ])
    ],
    text("Save")
  )
  ```
  """
  @spec mouse_down(Emerge.UI.attrs()) :: mouse_down_attr()
  def mouse_down(attrs) when is_list(attrs),
    do: {:mouse_down, Validation.parse_state_style_attrs(attrs, :mouse_down)}
end
