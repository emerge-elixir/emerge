defmodule Emerge.UI.Interactive do
  @moduledoc """
  Conditional style blocks for interaction states.

  `mouse_over/1`, `focused/1`, and `mouse_down/1` accept decorative attributes
  from `Background`, `Border`, `Font`, `Svg`, and `Transform`.

  ## Example

      use Emerge.UI

      el(
        [
          Interactive.mouse_over([
            Border.width(2),
            Border.dashed(),
            Font.family(:display),
            Font.bold(),
            Font.center(),
            Transform.alpha(0.9)
          ])
        ],
        text("Hover me")
      )
  """

  alias Emerge.UI.Internal.Validation

  @type state_style :: map()
  @type mouse_over_attr :: {:mouse_over, state_style()}
  @type focused_attr :: {:focused, state_style()}
  @type mouse_down_attr :: {:mouse_down, state_style()}
  @type t :: mouse_over_attr() | focused_attr() | mouse_down_attr()

  @doc "Apply decorative border, font, background, svg, and transform attrs while pointer is over the element"
  @spec mouse_over(Emerge.UI.attrs()) :: mouse_over_attr()
  def mouse_over(attrs) when is_list(attrs),
    do: {:mouse_over, Validation.parse_state_style_attrs(attrs, :mouse_over)}

  @doc "Apply decorative border, font, background, svg, and transform attrs while this input is focused"
  @spec focused(Emerge.UI.attrs()) :: focused_attr()
  def focused(attrs) when is_list(attrs),
    do: {:focused, Validation.parse_state_style_attrs(attrs, :focused)}

  @doc "Apply decorative border, font, background, svg, and transform attrs while left mouse button is pressed"
  @spec mouse_down(Emerge.UI.attrs()) :: mouse_down_attr()
  def mouse_down(attrs) when is_list(attrs),
    do: {:mouse_down, Validation.parse_state_style_attrs(attrs, :mouse_down)}
end
