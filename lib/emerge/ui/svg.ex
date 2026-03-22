defmodule Emerge.UI.Svg do
  @moduledoc "SVG-specific styling attributes"

  @doc "Apply template tinting to all visible SVG pixels"
  def color(c), do: {:svg_color, c}
end
