defmodule Emerge.UI.Border do
  @moduledoc "Border styling attributes"

  @doc "Set border radius (all corners)"
  def rounded(r), do: {:border_radius, r}

  @doc "Set individual corner radii"
  def rounded_each(tl, tr, br, bl), do: {:border_radius, {tl, tr, br, bl}}

  @doc "Set border width"
  def width(w), do: {:border_width, w}

  @doc "Set per-edge border widths (top, right, bottom, left)"
  def width_each(top, right, bottom, left)
      when top == right and right == bottom and bottom == left,
      do: {:border_width, top}

  def width_each(top, right, bottom, left),
    do: {:border_width, {top, right, bottom, left}}

  @doc "Set border color"
  def color(c), do: {:border_color, c}

  @doc "Solid border style (default)"
  def solid, do: {:border_style, :solid}

  @doc "Dashed border style"
  def dashed, do: {:border_style, :dashed}

  @doc "Dotted border style"
  def dotted, do: {:border_style, :dotted}

  @doc """
  Add a box shadow.

  Options:
  - `:offset` - `{x, y}` offset (default `{0, 0}`)
  - `:size` - spread size in pixels (default `0`)
  - `:blur` - blur radius in pixels (default `10`)
  - `:color` - shadow color (default `:black`)
  """
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
  def glow(color, size) do
    shadow(offset: {0, 0}, size: size, blur: size * 2, color: color)
  end
end
