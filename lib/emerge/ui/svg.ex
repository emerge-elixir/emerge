defmodule Emerge.UI.Svg do
  @moduledoc "SVG-specific styling attributes"

  @type t :: {:svg_color, Emerge.UI.Color.color() | Emerge.UI.Color.t()}

  @doc """
  Apply a solid or gradient template tint to visible SVG pixels.

  Original RGB is replaced; source alpha is multiplied by tint alpha. Gradients
  span the SVG element's content box, not the cached source raster or each tile.
  """
  @spec color(Emerge.UI.Color.color() | Emerge.UI.Color.t()) :: t()
  def color(c), do: {:svg_color, c}
end
