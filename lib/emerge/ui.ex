defmodule Emerge.UI do
  @moduledoc """
  Elm-UI inspired layout primitives for Emerge.

  ## Example

      import Emerge.UI

      el([width(fill()), height(px(100)), padding(20), Background.color(:navy)],
        text("Hello World", [Font.size(24), Font.color(:white)])
      )

      row([spacing(20), padding(10)], [
        el([width(fill())], text("Left")),
        el([width(fill())], text("Right"))
      ])

      column([spacing(10), center_x()], [
        text("Centered content")
      ])
  """

  alias Emerge.Element
  alias EmergeSkia.VideoTarget

  @mouse_over_decorative_keys MapSet.new([
                                :background,
                                :border_color,
                                :box_shadow,
                                :font_color,
                                :font_size,
                                :font_underline,
                                :font_strike,
                                :font_letter_spacing,
                                :font_word_spacing,
                                :move_x,
                                :move_y,
                                :rotate,
                                :scale,
                                :alpha
                              ])

  @state_style_keys [:mouse_over, :focused, :mouse_down]

  @override_warning_store_key :emerge_ui_override_warnings

  # ============================================
  # LAYOUT ELEMENTS
  # ============================================

  @doc """
  A container element. The fundamental building block.

  Font styles (size, color) are passed down to text children.

  ## Example

      el([padding(10), Font.size(20), Font.color(:white)], text("Hello"))
  """
  def el(attrs, child) when is_list(attrs) do
    parsed = parse_attrs(attrs)
    {key, parsed} = Map.pop(parsed, :key)
    id = key
    parsed = Map.put(parsed, :__attrs_hash, Emerge.Tree.attrs_hash(parsed))

    %Element{
      type: :el,
      id: id,
      attrs: parsed,
      children: [child]
    }
  end

  def el(child), do: el([], child)

  @doc """
  A row lays out children horizontally.

  ## Example

      row([spacing(20)], [
        el(text("A")),
        el(text("B")),
        el(text("C"))
      ])
  """
  def row(attrs, children) when is_list(attrs) and is_list(children) do
    parsed = parse_attrs(attrs)
    {key, parsed} = Map.pop(parsed, :key)
    id = key
    parsed = Map.put(parsed, :__attrs_hash, Emerge.Tree.attrs_hash(parsed))

    %Element{
      type: :row,
      id: id,
      attrs: parsed,
      children: children
    }
  end

  def row(children) when is_list(children), do: row([], children)

  @doc """
  A wrapped row lays out children horizontally and wraps onto new lines.

  ## Example

      wrapped_row([spacing(12)], [
        el(text("One")),
        el(text("Two")),
        el(text("Three"))
      ])
  """
  def wrapped_row(attrs, children) when is_list(attrs) and is_list(children) do
    parsed = parse_attrs(attrs)
    {key, parsed} = Map.pop(parsed, :key)
    id = key
    parsed = Map.put(parsed, :__attrs_hash, Emerge.Tree.attrs_hash(parsed))

    %Element{
      type: :wrapped_row,
      id: id,
      attrs: parsed,
      children: children
    }
  end

  def wrapped_row(children) when is_list(children), do: wrapped_row([], children)

  @doc """
  A column lays out children vertically.

  ## Example

      column([spacing(10)], [
        text("Line 1"),
        text("Line 2")
      ])
  """
  def column(attrs, children) when is_list(attrs) and is_list(children) do
    parsed = parse_attrs(attrs)
    {key, parsed} = Map.pop(parsed, :key)
    id = key
    parsed = Map.put(parsed, :__attrs_hash, Emerge.Tree.attrs_hash(parsed))

    %Element{
      type: :column,
      id: id,
      attrs: parsed,
      children: children
    }
  end

  def column(children) when is_list(children), do: column([], children)

  @doc """
  A text column lays out paragraph-oriented content vertically.

  It behaves like a column but comes with document-friendly defaults:

  - `width(fill())`
  - `height(content())`

  You can override these by passing explicit width/height attributes.

  ## Example

      text_column([spacing(14)], [
        paragraph([spacing(4)], [text("First paragraph")]),
        paragraph([spacing(4)], [text("Second paragraph")])
      ])
  """
  def text_column(attrs, children) when is_list(attrs) and is_list(children) do
    defaults = [width(fill()), height(content())]

    default_attrs = parse_attrs(defaults, warn_overrides: false)
    user_attrs = parse_attrs(attrs)
    parsed = Map.merge(default_attrs, user_attrs)
    {key, parsed} = Map.pop(parsed, :key)
    id = key
    parsed = Map.put(parsed, :__attrs_hash, Emerge.Tree.attrs_hash(parsed))

    %Element{
      type: :text_column,
      id: id,
      attrs: parsed,
      children: children
    }
  end

  def text_column(children) when is_list(children), do: text_column([], children)

  @doc """
  A paragraph lays out inline text children with word wrapping.

  Children should be `text/1` elements or `el/2`-wrapped text elements.
  Words flow left-to-right and wrap at the container width.

  Font attributes on the paragraph are inherited by text children.
  `el()` wrappers provide inline styling (bold, color, etc.).

  ## Example

      paragraph([spacing(4), Font.size(16)], [
        text("Hello "),
        el([Font.bold()], text("world")),
        text(", this wraps automatically.")
      ])
  """
  def paragraph(attrs, children) when is_list(attrs) and is_list(children) do
    parsed = parse_attrs(attrs)
    {key, parsed} = Map.pop(parsed, :key)
    id = key
    parsed = Map.put(parsed, :__attrs_hash, Emerge.Tree.attrs_hash(parsed))

    %Element{
      type: :paragraph,
      id: id,
      attrs: parsed,
      children: children
    }
  end

  def paragraph(children) when is_list(children), do: paragraph([], children)

  @doc """
  A text element. Can only be used as a child of `el`.

  To style text, apply Font attributes to the parent el:

      el([Font.size(20), Font.color(:white)], text("Hello"))

  ## Example

      text("Hello")
  """
  def text(content) when is_binary(content) do
    attrs = %{content: content}
    attrs = Map.put(attrs, :__attrs_hash, Emerge.Tree.attrs_hash(attrs))

    %Element{
      type: :text,
      attrs: attrs,
      children: []
    }
  end

  @doc """
  An image element.

  `source` can be a verified `~m"..."` reference, logical asset path,
  runtime file path, or `{:id, image_id}`.
  """
  def image(source, attrs \\ []) when is_list(attrs) do
    parsed = parse_attrs([{:image_src, source} | attrs])
    {key, parsed} = Map.pop(parsed, :key)
    id = key
    parsed = Map.put(parsed, :__attrs_hash, Emerge.Tree.attrs_hash(parsed))

    %Element{
      type: :image,
      id: id,
      attrs: parsed,
      children: []
    }
  end

  @doc """
  A video element backed by a renderer-owned video target.
  """
  def video(%VideoTarget{} = target, attrs \\ []) when is_list(attrs) do
    parsed =
      parse_attrs([
        {:video_target, target.id},
        {:image_size, {target.width, target.height}},
        {:image_fit, :contain}
        | attrs
      ])

    {key, parsed} = Map.pop(parsed, :key)
    id = key
    parsed = Map.put(parsed, :__attrs_hash, Emerge.Tree.attrs_hash(parsed))

    %Element{
      type: :video,
      id: id,
      attrs: parsed,
      children: []
    }
  end

  @doc false
  def __text_input__(value, attrs) when is_binary(value) and is_list(attrs) do
    parsed = parse_attrs([{:content, value} | attrs])
    {key, parsed} = Map.pop(parsed, :key)
    id = key
    parsed = Map.put(parsed, :__attrs_hash, Emerge.Tree.attrs_hash(parsed))

    %Element{
      type: :text_input,
      id: id,
      attrs: parsed,
      children: []
    }
  end

  @doc false
  def __input_button__(label, attrs) when is_binary(label) and is_list(attrs) do
    parsed = parse_attrs(attrs)
    {key, parsed} = Map.pop(parsed, :key)
    id = key
    parsed = Map.put(parsed, :__attrs_hash, Emerge.Tree.attrs_hash(parsed))

    %Element{
      type: :el,
      id: id,
      attrs: parsed,
      children: [text(label)]
    }
  end

  @doc """
  An empty element that takes up no space.
  """
  def none do
    %Element{type: :none, attrs: %{}, children: []}
  end

  # ============================================
  # SIZE ATTRIBUTES
  # ============================================

  @doc "Provide a stable key for identity in lists (all siblings must have keys)."
  def key(value), do: {:key, value}

  @doc "Set width to a specific pixel value"
  def width({:px, _} = val), do: {:width, val}
  def width(:fill), do: {:width, :fill}
  def width(:content), do: {:width, :content}
  def width({:fill_portion, _} = val), do: {:width, val}
  def width({:minimum, _, _} = val), do: {:width, val}
  def width({:maximum, _, _} = val), do: {:width, val}

  @doc "Set height to a specific pixel value"
  def height({:px, _} = val), do: {:height, val}
  def height(:fill), do: {:height, :fill}
  def height(:content), do: {:height, :content}
  def height({:fill_portion, _} = val), do: {:height, val}
  def height({:minimum, _, _} = val), do: {:height, val}
  def height({:maximum, _, _} = val), do: {:height, val}

  @doc "Pixel size helper"
  def px(n) when is_number(n), do: {:px, n}

  @doc "Fill available space"
  def fill, do: :fill

  @doc "Fill a portion of available space (for weighted distribution)"
  def fill_portion(n) when is_number(n), do: {:fill_portion, n}

  @doc "Size to content"
  def content, do: :content

  @doc "Shrink to content"
  def shrink, do: :content

  @doc """
  Minimum size constraint. The resolved length must be at least n pixels.

  ## Example

      el([width(minimum(200, fill()))], text("At least 200px wide"))
  """
  def minimum(n, length) when is_number(n), do: {:minimum, n, length}

  @doc """
  Maximum size constraint. The resolved length must be at most n pixels.

  ## Example

      el([width(maximum(400, fill()))], text("At most 400px wide"))
  """
  def maximum(n, length) when is_number(n), do: {:maximum, n, length}

  # ============================================
  # SPACING & PADDING
  # ============================================

  @doc "Uniform padding on all sides"
  def padding(n) when is_number(n), do: {:padding, n}

  @doc "Padding with vertical and horizontal values"
  def padding_xy(x, y), do: {:padding, {y, x, y, x}}

  @doc "Padding with individual values (top, right, bottom, left)"
  def padding_each(top, right, bottom, left), do: {:padding, {top, right, bottom, left}}

  @doc "Space between children in row/column"
  def spacing(n) when is_number(n), do: {:spacing, n}

  @doc "Spacing with horizontal and vertical values"
  def spacing_xy(x, y) when is_number(x) and is_number(y), do: {:spacing_xy, {x, y}}

  @doc "Distribute children with equal gaps between them"
  def space_evenly, do: {:space_evenly, true}

  @doc "Render a vertical scrollbar when content overflows (implies clip_y)"
  def scrollbar_y, do: {:scrollbar_y, true}

  @doc "Render a horizontal scrollbar when content overflows (implies clip_x)"
  def scrollbar_x, do: {:scrollbar_x, true}

  @doc "Clip content on both axes (helper for clip_x + clip_y)"
  def clip, do: %{clip_x: true, clip_y: true}

  @doc "Clip content on the horizontal axis"
  def clip_x, do: {:clip_x, true}

  @doc "Clip content on the vertical axis"
  def clip_y, do: {:clip_y, true}

  # ============================================
  # ALIGNMENT
  # ============================================

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

  # ============================================
  # EVENTS
  # ============================================

  @doc "Register a click handler payload for this element"
  def on_click({pid, _msg} = payload) when is_pid(pid), do: {:on_click, payload}

  @doc "Register a press handler payload for this element"
  def on_press({pid, _msg} = payload) when is_pid(pid), do: {:on_press, payload}

  @doc "Register a mouse down handler payload for this element"
  def on_mouse_down({pid, _msg} = payload) when is_pid(pid), do: {:on_mouse_down, payload}

  @doc "Register a mouse up handler payload for this element"
  def on_mouse_up({pid, _msg} = payload) when is_pid(pid), do: {:on_mouse_up, payload}

  @doc "Register a mouse enter handler payload for this element"
  def on_mouse_enter({pid, _msg} = payload) when is_pid(pid), do: {:on_mouse_enter, payload}

  @doc "Register a mouse leave handler payload for this element"
  def on_mouse_leave({pid, _msg} = payload) when is_pid(pid), do: {:on_mouse_leave, payload}

  @doc "Register a mouse move handler payload for this element"
  def on_mouse_move({pid, _msg} = payload) when is_pid(pid), do: {:on_mouse_move, payload}

  @doc "Register a change handler payload for this input element"
  def on_change({pid, _msg} = payload) when is_pid(pid), do: {:on_change, payload}

  @doc "Register a focus handler payload for this input element"
  def on_focus({pid, _msg} = payload) when is_pid(pid), do: {:on_focus, payload}

  @doc "Register a blur handler payload for this input element"
  def on_blur({pid, _msg} = payload) when is_pid(pid), do: {:on_blur, payload}

  @doc "Apply decorative attributes while pointer is over the element"
  def mouse_over(attrs) when is_list(attrs),
    do: {:mouse_over, parse_state_style_attrs(attrs, :mouse_over)}

  @doc "Apply decorative attributes while this input is focused"
  def focused(attrs) when is_list(attrs), do: {:focused, parse_state_style_attrs(attrs, :focused)}

  @doc "Apply decorative attributes while left mouse button is pressed"
  def mouse_down(attrs) when is_list(attrs),
    do: {:mouse_down, parse_state_style_attrs(attrs, :mouse_down)}

  # ============================================
  # TRANSFORMS
  # ============================================

  @doc "Move element on the X axis (pixels)"
  def move_x(value) when is_number(value), do: {:move_x, value}

  @doc "Move element on the Y axis (pixels)"
  def move_y(value) when is_number(value), do: {:move_y, value}

  @doc "Rotate element in degrees"
  def rotate(value) when is_number(value), do: {:rotate, value}

  @doc "Scale element uniformly"
  def scale(value) when is_number(value), do: {:scale, value}

  @doc "Set element opacity (0.0 - 1.0)"
  def alpha(value) when is_number(value), do: {:alpha, value}

  @doc "Set image fit mode (`:contain` or `:cover`)"
  def image_fit(mode) when mode in [:contain, :cover], do: {:image_fit, mode}

  # ============================================
  # NEARBY POSITIONING
  # ============================================

  @doc "Place an element above the current one without affecting layout flow"
  def above(element), do: {:above, element}

  @doc "Place an element below the current one without affecting layout flow"
  def below(element), do: {:below, element}

  @doc "Place an element on the left of the current one without affecting layout flow"
  def on_left(element), do: {:on_left, element}

  @doc "Place an element on the right of the current one without affecting layout flow"
  def on_right(element), do: {:on_right, element}

  @doc "Render an element in front of the current one"
  def in_front(element), do: {:in_front, element}

  @doc "Render an element behind the current one"
  def behind_content(element), do: {:behind, element}

  # ============================================
  # ATTRIBUTE PARSING
  # ============================================

  defp parse_attrs(attrs, opts \\ []) do
    warn_overrides = Keyword.get(opts, :warn_overrides, true)

    parsed =
      Enum.reduce(attrs, %{}, fn
        {key, value}, acc ->
          put_attr(acc, key, value, warn_overrides)

        other, acc when is_map(other) ->
          Enum.reduce(other, acc, fn
            {key, value}, map_acc -> put_attr(map_acc, key, value, warn_overrides)
          end)

        _, acc ->
          acc
      end)

    validate_scrollbar_clipping!(parsed)
    validate_state_style_payloads!(parsed)
    parsed
  end

  defp parse_state_style_attrs(attrs, style_key) do
    parsed =
      Enum.reduce(attrs, %{}, fn
        {key, value}, acc ->
          put_state_style_attr(acc, key, value)

        other, acc when is_map(other) ->
          Enum.reduce(other, acc, fn {key, value}, map_acc ->
            put_state_style_attr(map_acc, key, value)
          end)

        _, acc ->
          acc
      end)

    validate_decorative_attrs!(parsed, style_key)
    parsed
  end

  defp put_state_style_attr(acc, :box_shadow, value) when is_map(value) do
    existing = Map.get(acc, :box_shadow, [])
    Map.put(acc, :box_shadow, existing ++ [value])
  end

  defp put_state_style_attr(acc, :box_shadow, value) when is_list(value) do
    existing = Map.get(acc, :box_shadow, [])
    Map.put(acc, :box_shadow, existing ++ value)
  end

  defp put_state_style_attr(acc, key, value), do: Map.put(acc, key, value)

  defp put_attr(acc, :box_shadow, value, _warn_overrides) do
    existing = Map.get(acc, :box_shadow, [])
    Map.put(acc, :box_shadow, existing ++ [value])
  end

  defp put_attr(acc, key, value, warn_overrides) do
    if warn_overrides do
      maybe_warn_override(acc, key, value)
    end

    Map.put(acc, key, value)
  end

  defp maybe_warn_override(acc, key, value) do
    case Map.fetch(acc, key) do
      {:ok, prev_value} when prev_value != value ->
        signature = {:attrs, key, prev_value, value}
        warned = Process.get(@override_warning_store_key, MapSet.new())

        if !MapSet.member?(warned, signature) do
          Process.put(@override_warning_store_key, MapSet.put(warned, signature))

          IO.warn(
            "Emerge.UI attribute #{inspect(key)} is set multiple times with different values " <>
              "(#{inspect(prev_value)} -> #{inspect(value)}); last value wins"
          )
        end

      _ ->
        :ok
    end
  end

  defp validate_state_style_payloads!(attrs) do
    Enum.each(@state_style_keys, fn style_key ->
      validate_state_style_payload!(attrs, style_key)
    end)
  end

  defp validate_state_style_payload!(attrs, style_key) do
    case Map.get(attrs, style_key) do
      nil ->
        :ok

      style_attrs when is_map(style_attrs) ->
        validate_decorative_attrs!(style_attrs, style_key)

      other ->
        raise ArgumentError,
              "#{style_key} must be a list/map of decorative attributes, got: #{inspect(other)}"
    end
  end

  defp validate_decorative_attrs!(attrs, style_key) do
    allowed =
      @mouse_over_decorative_keys |> Enum.map(&inspect/1) |> Enum.sort() |> Enum.join(", ")

    Enum.each(attrs, fn {key, _value} ->
      cond do
        key in @state_style_keys ->
          raise ArgumentError, "#{style_key} does not support nested #{key}"

        MapSet.member?(@mouse_over_decorative_keys, key) ->
          :ok

        true ->
          raise ArgumentError,
                "#{style_key} only supports decorative attributes; got #{inspect(key)}. Allowed: #{allowed}"
      end
    end)
  end

  defp validate_scrollbar_clipping!(attrs) do
    if Map.get(attrs, :id) do
      raise ArgumentError, "id is not supported; use key instead"
    end

    if Map.get(attrs, :clip) do
      raise ArgumentError, "clip is not supported; use clip_x and clip_y"
    end

    if Map.get(attrs, :scrollbar_x) && Map.get(attrs, :clip_x) do
      raise ArgumentError, "scrollbar_x implies clip_x; do not set clip_x with scrollbar_x"
    end

    if Map.get(attrs, :scrollbar_y) && Map.get(attrs, :clip_y) do
      raise ArgumentError, "scrollbar_y implies clip_y; do not set clip_y with scrollbar_y"
    end
  end

  # ============================================
  # SUBMODULES
  # ============================================

  defmodule Background do
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

  defmodule Border do
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

    ## Example

        Border.shadow(offset: {2, 2}, blur: 8, color: :black)
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

    ## Example

        Border.glow(:blue, 5)
    """
    def glow(color, size) do
      shadow(offset: {0, 0}, size: size, blur: size * 2, color: color)
    end
  end

  defmodule Font do
    @moduledoc "Font styling attributes"

    @doc "Set font size"
    def size(s), do: {:font_size, s}

    @doc "Set font color"
    def color(c), do: {:font_color, c}

    @doc "Set font family"
    def family(f), do: {:font, f}

    @doc "Bold text"
    def bold, do: {:font_weight, :bold}

    @doc "Italic text"
    def italic, do: {:font_style, :italic}

    @doc "Underline text"
    def underline, do: {:font_underline, true}

    @doc "Strike-through text"
    def strike, do: {:font_strike, true}

    @doc "Extra spacing between letters"
    def letter_spacing(value) when is_number(value), do: {:font_letter_spacing, value}

    @doc "Extra spacing between words"
    def word_spacing(value) when is_number(value), do: {:font_word_spacing, value}

    @doc "Left-align text within element (default)"
    def align_left, do: {:text_align, :left}

    @doc "Right-align text within element"
    def align_right, do: {:text_align, :right}

    @doc "Center text within element"
    def center, do: {:text_align, :center}
  end

  defmodule Input do
    @moduledoc "Input elements"

    @doc "Single-line text input"
    def text(value, attrs \\ []) when is_binary(value) and is_list(attrs) do
      Emerge.UI.__text_input__(value, attrs)
    end

    @doc "Button input with text label"
    def button(label, attrs \\ []) when is_binary(label) and is_list(attrs) do
      Emerge.UI.__input_button__(label, attrs)
    end
  end
end
