defmodule Emerge.UI.Transform do
  @moduledoc "Geometric transforms and opacity helpers."

  @type numeric_attr ::
          {:move_x, number()}
          | {:move_y, number()}
          | {:rotate, number()}
          | {:scale, number()}
          | {:alpha, number()}

  @type t :: numeric_attr()

  @doc "Move element on the X axis (pixels)"
  @spec move_x(number()) :: {:move_x, number()}
  def move_x(value) when is_number(value), do: {:move_x, value}

  @doc "Move element on the Y axis (pixels)"
  @spec move_y(number()) :: {:move_y, number()}
  def move_y(value) when is_number(value), do: {:move_y, value}

  @doc "Rotate element in degrees"
  @spec rotate(number()) :: {:rotate, number()}
  def rotate(value) when is_number(value), do: {:rotate, value}

  @doc "Scale element uniformly"
  @spec scale(number()) :: {:scale, number()}
  def scale(value) when is_number(value), do: {:scale, value}

  @doc "Set element opacity (0.0 - 1.0)"
  @spec alpha(number()) :: {:alpha, number()}
  def alpha(value) when is_number(value), do: {:alpha, value}
end
