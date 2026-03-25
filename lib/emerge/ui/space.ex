defmodule Emerge.UI.Space do
  @moduledoc "Spacing helpers for padding and child gaps."

  @type edge_values :: {number(), number(), number(), number()}
  @type padding_attr :: {:padding, number() | edge_values()}
  @type spacing_attr :: {:spacing, number()}
  @type spacing_xy_attr :: {:spacing_xy, {number(), number()}}
  @type space_evenly_attr :: {:space_evenly, true}
  @type t :: padding_attr() | spacing_attr() | spacing_xy_attr() | space_evenly_attr()

  @doc "Uniform padding on all sides"
  @spec padding(number()) :: padding_attr()
  def padding(n) when is_number(n), do: {:padding, n}

  @doc "Padding with vertical and horizontal values"
  @spec padding_xy(number(), number()) :: padding_attr()
  def padding_xy(x, y), do: {:padding, {y, x, y, x}}

  @doc "Padding with individual values (top, right, bottom, left)"
  @spec padding_each(number(), number(), number(), number()) :: padding_attr()
  def padding_each(top, right, bottom, left), do: {:padding, {top, right, bottom, left}}

  @doc "Space between children in row/column"
  @spec spacing(number()) :: spacing_attr()
  def spacing(n) when is_number(n), do: {:spacing, n}

  @doc "Spacing with horizontal and vertical values"
  @spec spacing_xy(number(), number()) :: spacing_xy_attr()
  def spacing_xy(x, y) when is_number(x) and is_number(y), do: {:spacing_xy, {x, y}}

  @doc "Distribute children with equal gaps between them"
  @spec space_evenly() :: space_evenly_attr()
  def space_evenly, do: {:space_evenly, true}
end
