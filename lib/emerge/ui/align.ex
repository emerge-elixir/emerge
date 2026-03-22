defmodule Emerge.UI.Align do
  @moduledoc "Alignment helpers for positioning within layout parents."

  @doc "Center horizontally within parent"
  def center_x, do: {:align_x, :center}

  @doc "Center vertically within parent"
  def center_y, do: {:align_y, :center}

  @doc "Align to the left"
  def align_left, do: {:align_x, :left}

  @doc "Align to the right"
  def align_right, do: {:align_x, :right}

  @doc "Align to the top"
  def align_top, do: {:align_y, :top}

  @doc "Align to the bottom"
  def align_bottom, do: {:align_y, :bottom}
end
