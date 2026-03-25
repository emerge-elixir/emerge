defmodule Emerge.UI.Background do
  @moduledoc "Background styling attributes"

  @type color_value :: Emerge.UI.Color.color() | Emerge.UI.Color.t()
  @type fit :: :contain | :cover | :repeat | :repeat_x | :repeat_y
  @type gradient_background :: {:gradient, color_value(), color_value(), number()}
  @type image_background :: {:image, Emerge.UI.image_source(), fit()}
  @type image_options :: keyword()
  @type t :: {:background, color_value() | gradient_background() | image_background()}

  @doc "Set background color"
  @spec color(color_value()) :: t()
  def color(c), do: {:background, c}

  @doc "Set background gradient (linear)"
  @spec gradient(color_value(), color_value()) :: t()
  @spec gradient(color_value(), color_value(), number()) :: t()
  def gradient(from, to, angle \\ 0), do: {:background, {:gradient, from, to, angle}}

  @doc "Set background image (default fit: `:cover`)"
  @spec image(Emerge.UI.image_source()) :: t()
  @spec image(Emerge.UI.image_source(), image_options()) :: t()
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
  @spec uncropped(Emerge.UI.image_source()) :: t()
  def uncropped(source), do: {:background, {:image, source, :contain}}

  @doc "Tile a background image on both axes"
  @spec tiled(Emerge.UI.image_source()) :: t()
  def tiled(source), do: {:background, {:image, source, :repeat}}

  @doc "Tile a background image on the X axis"
  @spec tiled_x(Emerge.UI.image_source()) :: t()
  def tiled_x(source), do: {:background, {:image, source, :repeat_x}}

  @doc "Tile a background image on the Y axis"
  @spec tiled_y(Emerge.UI.image_source()) :: t()
  def tiled_y(source), do: {:background, {:image, source, :repeat_y}}
end
