defmodule Emerge.UI.Background do
  @moduledoc "Background styling attributes"

  @doc "Set background color"
  def color(c), do: {:background, c}

  @doc "Set background gradient (linear)"
  def gradient(from, to, angle \\ 0), do: {:background, {:gradient, from, to, angle}}

  @doc "Set background image (default fit: `:cover`)"
  def image(source, opts \\ []) do
    fit =
      case Keyword.get(opts, :fit, :cover) do
        :contain -> :contain
        :cover -> :cover
        :repeat -> :repeat
        :repeat_x -> :repeat_x
        :repeat_y -> :repeat_y
        _ -> :cover
      end

    {:background, {:image, source, fit}}
  end

  @doc "Set a background image that fits without cropping (`:contain`)"
  def uncropped(source), do: {:background, {:image, source, :contain}}

  @doc "Tile a background image on both axes"
  def tiled(source), do: {:background, {:image, source, :repeat}}

  @doc "Tile a background image on the X axis"
  def tiled_x(source), do: {:background, {:image, source, :repeat_x}}

  @doc "Tile a background image on the Y axis"
  def tiled_y(source), do: {:background, {:image, source, :repeat_y}}
end
