defmodule Emerge.UI.Border do
  @moduledoc "Border styling attributes"

  @type color_value :: Emerge.UI.Color.color() | Emerge.UI.Color.t()
  @type radius :: number() | {number(), number(), number(), number()}
  @type width_value :: number() | {number(), number(), number(), number()}
  @type style :: :solid | :dashed | :dotted
  @type shadow_options :: keyword()
  @type shadow_value :: %{
          required(:offset_x) => number(),
          required(:offset_y) => number(),
          required(:size) => number(),
          required(:blur) => number(),
          required(:color) => color_value(),
          required(:inset) => boolean()
        }

  @type t ::
          {:border_radius, radius()}
          | {:border_width, width_value()}
          | {:border_color, color_value()}
          | {:border_style, style()}
          | {:box_shadow, shadow_value()}

  @doc "Set border radius (all corners)"
  @spec rounded(radius()) :: {:border_radius, radius()}
  def rounded(r), do: {:border_radius, r}

  @doc "Set individual corner radii"
  @spec rounded_each(number(), number(), number(), number()) :: {:border_radius, radius()}
  def rounded_each(tl, tr, br, bl), do: {:border_radius, {tl, tr, br, bl}}

  @doc "Set border width"
  @spec width(width_value()) :: {:border_width, width_value()}
  def width(w), do: {:border_width, w}

  @doc "Set per-edge border widths (top, right, bottom, left)"
  @spec width_each(number(), number(), number(), number()) :: {:border_width, width_value()}
  def width_each(top, right, bottom, left)
      when top == right and right == bottom and bottom == left,
      do: {:border_width, top}

  def width_each(top, right, bottom, left),
    do: {:border_width, {top, right, bottom, left}}

  @doc "Set border color"
  @spec color(color_value()) :: {:border_color, color_value()}
  def color(c), do: {:border_color, c}

  @doc "Solid border style (default)"
  @spec solid() :: {:border_style, :solid}
  def solid, do: {:border_style, :solid}

  @doc "Dashed border style"
  @spec dashed() :: {:border_style, :dashed}
  def dashed, do: {:border_style, :dashed}

  @doc "Dotted border style"
  @spec dotted() :: {:border_style, :dotted}
  def dotted, do: {:border_style, :dotted}

  @doc """
  Add a box shadow.

  Options:
  - `:offset` - `{x, y}` offset (default `{0, 0}`)
  - `:size` - spread size in pixels (default `0`)
  - `:blur` - blur radius in pixels (default `10`)
  - `:color` - shadow color (default `:black`)
  """
  @spec shadow() :: {:box_shadow, shadow_value()}
  @spec shadow(shadow_options()) :: {:box_shadow, shadow_value()}
  def shadow(opts \\ []) do
    {ox, oy} = Keyword.get(opts, :offset, {0, 0})
    size = Keyword.get(opts, :size, 0)
    blur = Keyword.get(opts, :blur, 10)
    color = Keyword.get(opts, :color, :black)

    {:box_shadow,
     %{offset_x: ox, offset_y: oy, size: size, blur: blur, color: color, inset: false}}
  end

  @doc """
  Add an inner shadow (inset box shadow).

  Same options as `shadow/1` but rendered inside the element.
  """
  @spec inner_shadow() :: {:box_shadow, shadow_value()}
  @spec inner_shadow(shadow_options()) :: {:box_shadow, shadow_value()}
  def inner_shadow(opts \\ []) do
    {ox, oy} = Keyword.get(opts, :offset, {0, 0})
    size = Keyword.get(opts, :size, 0)
    blur = Keyword.get(opts, :blur, 10)
    color = Keyword.get(opts, :color, :black)

    {:box_shadow,
     %{offset_x: ox, offset_y: oy, size: size, blur: blur, color: color, inset: true}}
  end

  @doc """
  Add a uniform glow around the element.

  Sugar for `shadow/1` with zero offset.
  """
  @spec glow(color_value(), number()) :: {:box_shadow, shadow_value()}
  def glow(color, size) do
    shadow(offset: {0, 0}, size: size, blur: size * 2, color: color)
  end
end
