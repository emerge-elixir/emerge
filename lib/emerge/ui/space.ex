defmodule Emerge.UI.Space do
  @moduledoc "Spacing helpers for padding and child gaps."

  @doc "Uniform padding on all sides"
  def padding(n) when is_number(n), do: {:padding, n}

  @doc "Padding with vertical and horizontal values"
  def padding_xy(x, y), do: {:padding, {y, x, y, x}}

  @doc "Padding with individual values (top, right, bottom, left)"
  def padding_each(top, right, bottom, left), do: {:padding, {top, right, bottom, left}}

  @doc "Space between children in row/column"
  def spacing(n) when is_number(n), do: {:spacing, n}

  @doc "Spacing with horizontal and vertical values"
  def spacing_xy(x, y) when is_number(x) and is_number(y), do: {:spacing_xy, {x, y}}

  @doc "Distribute children with equal gaps between them"
  def space_evenly, do: {:space_evenly, true}
end
