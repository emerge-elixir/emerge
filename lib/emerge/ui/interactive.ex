defmodule Emerge.UI.Interactive do
  @moduledoc "Conditional decorative styles for interaction states."

  alias Emerge.UI.Internal.Validation

  @doc "Apply decorative attributes while pointer is over the element"
  def mouse_over(attrs) when is_list(attrs),
    do: {:mouse_over, Validation.parse_state_style_attrs(attrs, :mouse_over)}

  @doc "Apply decorative attributes while this input is focused"
  def focused(attrs) when is_list(attrs),
    do: {:focused, Validation.parse_state_style_attrs(attrs, :focused)}

  @doc "Apply decorative attributes while left mouse button is pressed"
  def mouse_down(attrs) when is_list(attrs),
    do: {:mouse_down, Validation.parse_state_style_attrs(attrs, :mouse_down)}
end
