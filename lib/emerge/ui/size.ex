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

  @doc "Fill available space. Use `{:fill, n}` for weighted distribution."
  def fill, do: :fill

  @doc "Size to content"
  def content, do: :content

  @doc "Shrink to content"
  def shrink, do: :content

  @doc """
  Minimum size constraint. The resolved length must be at least n pixels.
  """
  def minimum(n, length) when is_number(n), do: {:minimum, n, length}

  @doc """
  Maximum size constraint. The resolved length must be at most n pixels.
  """
  def maximum(n, length) when is_number(n), do: {:maximum, n, length}
end
