defmodule Emerge.UI.Interactive do
  @moduledoc "Conditional decorative styles for interaction states."

  alias Emerge.UI.Internal.Validation

  @type state_style :: map()
  @type mouse_over_attr :: {:mouse_over, state_style()}
  @type focused_attr :: {:focused, state_style()}
  @type mouse_down_attr :: {:mouse_down, state_style()}
  @type t :: mouse_over_attr() | focused_attr() | mouse_down_attr()

  @doc "Apply decorative attributes while pointer is over the element"
  @spec mouse_over(Emerge.UI.attrs()) :: mouse_over_attr()
  def mouse_over(attrs) when is_list(attrs),
    do: {:mouse_over, Validation.parse_state_style_attrs(attrs, :mouse_over)}

  @doc "Apply decorative attributes while this input is focused"
  @spec focused(Emerge.UI.attrs()) :: focused_attr()
  def focused(attrs) when is_list(attrs),
    do: {:focused, Validation.parse_state_style_attrs(attrs, :focused)}

  @doc "Apply decorative attributes while left mouse button is pressed"
  @spec mouse_down(Emerge.UI.attrs()) :: mouse_down_attr()
  def mouse_down(attrs) when is_list(attrs),
    do: {:mouse_down, Validation.parse_state_style_attrs(attrs, :mouse_down)}
end
