defmodule Emerge.UI.Transform do
  @moduledoc "Geometric transforms and opacity helpers."

  @doc "Move element on the X axis (pixels)"
  def move_x(value) when is_number(value), do: {:move_x, value}

  @doc "Move element on the Y axis (pixels)"
  def move_y(value) when is_number(value), do: {:move_y, value}

  @doc "Rotate element in degrees"
  def rotate(value) when is_number(value), do: {:rotate, value}

  @doc "Scale element uniformly"
  def scale(value) when is_number(value), do: {:scale, value}

  @doc "Set element opacity (0.0 - 1.0)"
  def alpha(value) when is_number(value), do: {:alpha, value}
end
