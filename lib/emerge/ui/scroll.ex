defmodule Emerge.UI.Scroll do
  @moduledoc "Overflow helpers for scrollable layouts."

  @type scrollbar_y_attr :: {:scrollbar_y, true}
  @type scrollbar_x_attr :: {:scrollbar_x, true}
  @type t :: scrollbar_y_attr() | scrollbar_x_attr()

  @doc "Render a vertical scrollbar when content overflows"
  @spec scrollbar_y() :: scrollbar_y_attr()
  def scrollbar_y, do: {:scrollbar_y, true}

  @doc "Render a horizontal scrollbar when content overflows"
  @spec scrollbar_x() :: scrollbar_x_attr()
  def scrollbar_x, do: {:scrollbar_x, true}
end
