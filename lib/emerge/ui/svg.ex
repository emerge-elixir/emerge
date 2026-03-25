defmodule Emerge.UI.Svg do
  @moduledoc "SVG-specific styling attributes"

  @type t :: {:svg_color, Emerge.UI.Color.color() | Emerge.UI.Color.t()}

  @doc "Apply template tinting to all visible SVG pixels"
  @spec color(Emerge.UI.Color.color() | Emerge.UI.Color.t()) :: t()
  def color(c), do: {:svg_color, c}
end
