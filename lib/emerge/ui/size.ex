defmodule Emerge.UI.Size do
  @moduledoc "Length and sizing helpers for Emerge UI layouts."

  @type px_length :: {:px, number()}
  @type fill_length :: :fill | {:fill, number()}
  @type content_length :: :content
  @type base_length :: px_length() | fill_length() | content_length()
  @type constrained_length :: {:minimum, number(), length()} | {:maximum, number(), length()}
  @type length :: base_length() | constrained_length()
  @type width_attr :: {:width, length()}
  @type height_attr :: {:height, length()}
  @type t :: width_attr() | height_attr()

  @doc "Set width to a specific pixel value"
  @spec width(length()) :: width_attr()
  def width({:px, _} = val), do: {:width, val}
  def width(:fill), do: {:width, :fill}
  def width({:fill, _} = val), do: {:width, val}
  def width(:content), do: {:width, :content}
  def width({:minimum, _, _} = val), do: {:width, val}
  def width({:maximum, _, _} = val), do: {:width, val}

  @doc "Set height to a specific pixel value"
  @spec height(length()) :: height_attr()
  def height({:px, _} = val), do: {:height, val}
  def height(:fill), do: {:height, :fill}
  def height({:fill, _} = val), do: {:height, val}
  def height(:content), do: {:height, :content}
  def height({:minimum, _, _} = val), do: {:height, val}
  def height({:maximum, _, _} = val), do: {:height, val}

  @doc "Pixel size helper"
  @spec px(number()) :: px_length()
  def px(n) when is_number(n), do: {:px, n}

  @doc "Fill available space. Use `fill(n)` for weighted distribution."
  @spec fill() :: :fill
  def fill, do: :fill

  @spec fill(number()) :: {:fill, number()}
  def fill(weight) when is_number(weight) and weight > 0, do: {:fill, weight}

  def fill(weight) do
    raise ArgumentError, "fill/1 expects a positive number, got: #{inspect(weight)}"
  end

  @doc "Size to content"
  @spec content() :: content_length()
  def content, do: :content

  @doc "Shrink to content"
  @spec shrink() :: content_length()
  def shrink, do: :content

  @doc """
  Minimum size constraint. The resolved length must be at least the given pixel length.
  """
  @spec min(px_length(), length()) :: constrained_length()
  def min({:px, min_px}, length) when is_number(min_px) and min_px >= 0,
    do: {:minimum, min_px, length}

  def min(length_px, _length) do
    raise ArgumentError,
          "min/2 expects the first argument to be px(n) with a non-negative number, got: #{inspect(length_px)}"
  end

  @doc """
  Maximum size constraint. The resolved length must be at most the given pixel length.
  """
  @spec max(px_length(), length()) :: constrained_length()
  def max({:px, max_px}, length) when is_number(max_px) and max_px >= 0,
    do: {:maximum, max_px, length}

  def max(length_px, _length) do
    raise ArgumentError,
          "max/2 expects the first argument to be px(n) with a non-negative number, got: #{inspect(length_px)}"
  end
end
