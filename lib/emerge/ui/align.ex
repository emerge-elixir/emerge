defmodule Emerge.UI.Align do
  @moduledoc "Alignment helpers for positioning within layout parents."

  @type horizontal_alignment :: :left | :center | :right
  @type vertical_alignment :: :top | :center | :bottom
  @type x_attr :: {:align_x, horizontal_alignment()}
  @type y_attr :: {:align_y, vertical_alignment()}
  @type t :: x_attr() | y_attr()

  @doc "Center horizontally within parent"
  @spec center_x() :: x_attr()
  def center_x, do: {:align_x, :center}

  @doc "Center vertically within parent"
  @spec center_y() :: y_attr()
  def center_y, do: {:align_y, :center}

  @doc "Align to the left"
  @spec align_left() :: x_attr()
  def align_left, do: {:align_x, :left}

  @doc "Align to the right"
  @spec align_right() :: x_attr()
  def align_right, do: {:align_x, :right}

  @doc "Align to the top"
  @spec align_top() :: y_attr()
  def align_top, do: {:align_y, :top}

  @doc "Align to the bottom"
  @spec align_bottom() :: y_attr()
  def align_bottom, do: {:align_y, :bottom}
end
