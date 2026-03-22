defmodule Emerge.UI.Scroll do
  @moduledoc "Overflow helpers for scrollable layouts."

  @doc "Render a vertical scrollbar when content overflows"
  def scrollbar_y, do: {:scrollbar_y, true}

  @doc "Render a horizontal scrollbar when content overflows"
  def scrollbar_x, do: {:scrollbar_x, true}
end
