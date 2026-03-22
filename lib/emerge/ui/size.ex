defmodule Emerge.UI.Size do
  @moduledoc "Length and sizing helpers for Emerge UI layouts."

  @doc "Set width to a specific pixel value"
  def width({:px, _} = val), do: {:width, val}
  def width(:fill), do: {:width, :fill}
  def width({:fill, _} = val), do: {:width, val}
  def width(:content), do: {:width, :content}
  def width({:minimum, _, _} = val), do: {:width, val}
  def width({:maximum, _, _} = val), do: {:width, val}

  @doc "Set height to a specific pixel value"
  def height({:px, _} = val), do: {:height, val}
  def height(:fill), do: {:height, :fill}
  def height({:fill, _} = val), do: {:height, val}
  def height(:content), do: {:height, :content}
  def height({:minimum, _, _} = val), do: {:height, val}
  def height({:maximum, _, _} = val), do: {:height, val}

  @doc "Pixel size helper"
  def px(n) when is_number(n), do: {:px, n}

  @doc "Fill available space. Use `fill(n)` for weighted distribution."
  def fill, do: :fill

  def fill(weight) when is_number(weight) and weight > 0, do: {:fill, weight}

  def fill(weight) do
    raise ArgumentError, "fill/1 expects a positive number, got: #{inspect(weight)}"
  end

  @doc "Size to content"
  def content, do: :content

  @doc "Shrink to content"
  def shrink, do: :content

  @doc """
  Minimum size constraint. The resolved length must be at least the given pixel length.
  """
  def min({:px, min_px}, length) when is_number(min_px) and min_px >= 0,
    do: {:minimum, min_px, length}

  def min(length_px, _length) do
    raise ArgumentError,
          "min/2 expects the first argument to be px(n) with a non-negative number, got: #{inspect(length_px)}"
  end

  @doc """
  Maximum size constraint. The resolved length must be at most the given pixel length.
  """
  def max({:px, max_px}, length) when is_number(max_px) and max_px >= 0,
    do: {:maximum, max_px, length}

  def max(length_px, _length) do
    raise ArgumentError,
          "max/2 expects the first argument to be px(n) with a non-negative number, got: #{inspect(length_px)}"
  end
end
