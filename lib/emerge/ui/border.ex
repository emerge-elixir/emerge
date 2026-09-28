defmodule Emerge.UI.Border do
  require Emerge.Docs.Examples

  alias Emerge.Docs.Examples

  Examples.external_resources(~w(ui-border-radius-width ui-border-shadows ui-inline-decorations))

  @moduledoc """
  Border styling attributes.

  ## Layout

  Border width participates in layout in Emerge.

  Content and child layout are inset by the sum of padding and border width on
  each side. For an element with `padding(10)` and `Border.width(5)`, content
  starts `15px` in from each edge.

  For fixed-size elements, the outer size stays the same and border width
  reduces the inner content area. For content-sized elements, the element grows
  by the border widths.

  Border width can affect layout even when no border is painted. Layout uses
  `border_width`, while painting also requires `border_color`.

  ## Radius

  Border width defaults to `0`, so `Border.rounded(8)` rounds the element
  corners but does not draw a visible border by itself.

  Rounded corners affect backgrounds and clipping, and also shape the border
  when width and color are present.

  ## Shadows

  `shadow/1` and `glow/2` paint the complete box-shaped shadow behind the
  element's background and content, including behind its text. Transparent
  areas reveal the shadow; opaque backgrounds cover it. Unlike CSS `box-shadow`,
  the casting box's interior is not cut out of the shadow.

  Shadows are decorative and do not affect layout.

  Parent padding does not clip descendant rendering, so outer shadows can bleed
  into a parent's padding.

  Scroll containers clip outer shadows only on active scroll axes: `scroll_y`
  clips top and bottom, `scroll_x` clips left and right. If both axes scroll,
  the full scrollport clips the shadow, including rounded corners.

  `inner_shadow/1` renders inside the element and does not bleed outside it.

  ## Inline paragraph wrappers

  All border styles, independent edge widths/corner radii, shadows and glow work
  on `el(attrs, text(...))` inside a paragraph. Each wrapped line segment gets a
  complete box with its own edges, corners, padding and shadow stack (clone-on-wrap).
  Words on the same line share the box; adjacent wrappers remain separate.

  Border width and padding reserve space and can change wrapping and line height.
  Shadows and glow remain decorative, including when no visible border is set.
  Outer shadows can bleed between lines and into padding; the existing active-axis
  scroll clips still apply. Inset shadows are painted inside the rounded box,
  behind the text. Explicit `Background.color/1` fills paint over outer shadows
  and under inset shadows/text: opaque colors hide the interior shadow while
  transparency reveals it. No implicit background is painted. Borders are
  painted after the text.

  Background and border gradients span each line segment's outer box. Shadow gradients use that
  same casting box before offset, spread and blur. Both restart at a wrapped
  continuation, not at each word. Text gradients still span the paragraph.

  This does not add inline background images, nested wrapper extraction or a
  new multiline hit-testing model. Floating elements retain ordinary box layout.

  #{Examples.code_block!("ui-inline-decorations")}

  #{Examples.image_tag!("ui-inline-decorations", "Wrapped inline borders, shadow and glow")}

  ## Examples

  Rounded corners shape the element even before a visible border is added.
  Adding width and color then paints the border on that same outline.

  #{Examples.code_block!("ui-border-radius-width")}

  #{Examples.image_tag!("ui-border-radius-width", "Rendered border radius and width example")}

  Shadows are easiest to understand side by side: outer shadow bleeds outward,
  glow is a zero-offset shadow, and inner shadow stays inside the element.

  #{Examples.code_block!("ui-border-shadows")}

  #{Examples.image_tag!("ui-border-shadows", "Rendered border shadow comparison")}
  """

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

  @doc "Round the element corners. This does not draw a visible border unless width and color are also set."
  @spec rounded(radius()) :: {:border_radius, radius()}
  def rounded(r), do: {:border_radius, r}

  @doc "Round each element corner individually. This does not draw a visible border unless width and color are also set."
  @spec rounded_each(number(), number(), number(), number()) :: {:border_radius, radius()}
  def rounded_each(tl, tr, br, bl), do: {:border_radius, {tl, tr, br, bl}}

  @doc "Set border width. Border width participates in layout and reduces the content area."
  @spec width(width_value()) :: {:border_width, width_value()}
  def width(w), do: {:border_width, w}

  @doc "Set per-edge border widths (top, right, bottom, left). These widths participate in layout on their respective sides."
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
  Add a box-shaped outer shadow, including behind the element's content.

  Transparent backgrounds reveal the shadow inside the box; opaque backgrounds
  painted over it provide occlusion. This is not a glyph-shaped text shadow.

  Decorative only. Outer shadows do not affect layout.

  Parent padding does not clip descendant rendering, so outer shadows can bleed
  into a parent's padding.

  Scroll containers clip outer shadows only on active scroll axes.

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

  Decorative only. Inner shadows do not affect layout and stay inside the
  element.
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

  Decorative only. `glow/2` follows the same bleed and scroll-axis clipping
  behavior as outer shadows.
  """
  @spec glow(color_value(), number()) :: {:box_shadow, shadow_value()}
  def glow(color, size) do
    shadow(offset: {0, 0}, size: size, blur: size * 2, color: color)
  end
end
