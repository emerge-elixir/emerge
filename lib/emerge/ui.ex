defmodule Emerge.UI do
  @moduledoc """
  Elm-UI inspired layout primitives for Emerge.

  ## Example

      import Emerge.UI

      el(
        [width(fill()), height(px(100)), padding(20), Background.color(:navy)],
        text("Hello World")
      )

      row([spacing(20), padding(10)], [
        el([width(fill())], text("Left")),
        el([width(fill())], text("Right"))
      ])

      column([spacing(10), center_x()], [
        text("Centered content")
      ])
  """

  alias Emerge.AttrSchema
  alias Emerge.AttrValidation
  alias Emerge.Element
  alias Emerge.Tree.Attrs, as: TreeAttrs
  alias EmergeSkia.VideoTarget

  @state_style_key_set AttrSchema.state_style_key_set()

  @public_attr_keys MapSet.new([
                      :key,
                      :width,
                      :height,
                      :padding,
                      :spacing,
                      :spacing_xy,
                      :space_evenly,
                      :scrollbar_y,
                      :scrollbar_x,
                      :align_x,
                      :align_y,
                      :background,
                      :border_radius,
                      :border_width,
                      :border_color,
                      :font_size,
                      :font_color,
                      :font,
                      :font_weight,
                      :font_style,
                      :snap_layout,
                      :snap_text_metrics,
                      :text_align,
                      :move_x,
                      :move_y,
                      :rotate,
                      :scale,
                      :alpha,
                      :animate,
                      :animate_enter,
                      :animate_exit,
                      :space_evenly,
                      :on_click,
                      :on_press,
                      :on_mouse_down,
                      :on_mouse_up,
                      :on_mouse_enter,
                      :on_mouse_leave,
                      :on_mouse_move,
                      :mouse_over,
                      :focused,
                      :mouse_down,
                      :font_underline,
                      :font_strike,
                      :font_letter_spacing,
                      :font_word_spacing,
                      :border_style,
                      :box_shadow,
                      :image_fit,
                      :on_change,
                      :on_focus,
                      :on_blur,
                      :above,
                      :below,
                      :on_left,
                      :on_right,
                      :in_front,
                      :behind
                    ])

  @reserved_attr_keys MapSet.new(
                        TreeAttrs.runtime_attrs() ++
                          [:id, :content, :image_src, :image_size, :video_target, :svg_expected]
                      )

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
  def el(attrs, child) do
    {attrs, child} = prepare_single_child!("el/2", attrs, child)
    build_element(attrs, :el, [child])
  end

  @doc """
  A row lays out children horizontally.

  ## Example

      row([spacing(20)], [
        el([], text("A")),
        el([], text("B")),
        el([], text("C"))
      ])
  """
  def row(attrs, children) do
    {attrs, children} = prepare_children!("row/2", attrs, children)
    build_element(attrs, :row, children)
  end

  @doc """
  A wrapped row lays out children horizontally and wraps onto new lines.

  ## Example

      wrapped_row([spacing(12)], [
        el([], text("One")),
        el([], text("Two")),
        el([], text("Three"))
      ])
  """
  def wrapped_row(attrs, children) do
    {attrs, children} = prepare_children!("wrapped_row/2", attrs, children)
    build_element(attrs, :wrapped_row, children)
  end

  @doc """
  A column lays out children vertically.

  ## Example

      column([spacing(10)], [
        text("Line 1"),
        text("Line 2")
      ])
  """
  def column(attrs, children) do
    {attrs, children} = prepare_children!("column/2", attrs, children)
    build_element(attrs, :column, children)
  end

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
  def text_column(attrs, children) do
    {attrs, children} = prepare_children!("text_column/2", attrs, children)

    attrs
    |> Map.put_new(:width, fill())
    |> Map.put_new(:height, content())
    |> build_element(:text_column, children)
  end

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
  def paragraph(attrs, children) do
    {attrs, children} = prepare_children!("paragraph/2", attrs, children)
    build_element(attrs, :paragraph, children)
  end

  @doc """
  A text element.

  It can live on its own as a content leaf, but it does not wrap by default.

  Use `paragraph/2` or `text_column/2` for wrapped text flows.

  To style text, apply Font attributes on the surrounding container:

      el([Font.size(20), Font.color(:white)], text("Hello"))

  ## Example

      text("Hello")
  """
  def text(content) when is_binary(content) do
    build_element(%{content: content}, :text, [])
  end

  def text(other) do
    raise ArgumentError, "text/1 expects a binary string, got: #{inspect(other)}"
  end

  @doc """
  An image element.

  `source` can be a verified `~m"..."` reference, logical asset path,
  runtime file path, or `{:id, image_id}`.
  """
  def image(attrs, source) do
    attrs = prepare_attrs!("image/2", attrs)
    source = validate_image_source!("image/2", source)

    attrs
    |> Map.put(:image_src, source)
    |> build_element(:image, [])
  end

  @doc """
  An SVG element.

  Preserves the SVG's original colors by default. Use `Svg.color/1` to apply
  template tinting to all visible pixels.
  """
  def svg(attrs, source) do
    attrs = prepare_attrs!("svg/2", attrs, extra_public_attr_keys: [:svg_color])
    source = validate_image_source!("svg/2", source)

    attrs
    |> Map.put(:image_src, source)
    |> Map.put(:svg_expected, true)
    |> build_element(:image, [])
  end

  @doc """
  A video element backed by a renderer-owned video target.
  """
  def video(attrs, target) do
    attrs = prepare_attrs!("video/2", attrs)
    target = validate_video_target!("video/2", target)

    attrs
    |> Map.put_new(:image_fit, :contain)
    |> Map.put(:video_target, target.id)
    |> Map.put(:image_size, {target.width, target.height})
    |> build_element(:video, [])
  end

  @doc false
  def __text_input__(attrs, value) do
    attrs = prepare_attrs!("Input.text/2", attrs)
    value = validate_binary_string!("Input.text/2", value)

    attrs
    |> Map.put(:content, value)
    |> build_element(:text_input, [])
  end

  @doc false
  def __input_button__(attrs, child) do
    {attrs, child} = prepare_single_child!("Input.button/2", attrs, child)
    build_element(attrs, :el, [child])
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
  def width({:fill, _} = val), do: {:width, val}
  def width(:content), do: {:width, :content}
  def width({:minimum, _, _} = val), do: {:width, val}
  def width({:maximum, _, _} = val), do: {:width, val}

  @doc "Set height to a specific pixel value"
  def height({:px, _} = val), do: {:height, val}
  def height(:fill), do: {:height, :fill}
  def height({:fill, _} = val), do: {:height, val}
  def height(:content), do: {:height, :content}
  def height({:minimum, _, _} = val), do: {:height, val}
  def height({:maximum, _, _} = val), do: {:height, val}

  @doc "Pixel size helper"
  def px(n) when is_number(n), do: {:px, n}

  @doc "Fill available space. Use `{:fill, n}` for weighted distribution."
  def fill, do: :fill

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

  @doc "Render a vertical scrollbar when content overflows"
  def scrollbar_y, do: {:scrollbar_y, true}

  @doc "Render a horizontal scrollbar when content overflows"
  def scrollbar_x, do: {:scrollbar_x, true}

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
  def on_click(message), do: on_click({self(), message})

  @doc "Register a press handler payload for this element"
  def on_press({pid, _msg} = payload) when is_pid(pid), do: {:on_press, payload}
  def on_press(message), do: on_press({self(), message})

  @doc "Register a mouse down handler payload for this element"
  def on_mouse_down({pid, _msg} = payload) when is_pid(pid), do: {:on_mouse_down, payload}
  def on_mouse_down(message), do: on_mouse_down({self(), message})

  @doc "Register a mouse up handler payload for this element"
  def on_mouse_up({pid, _msg} = payload) when is_pid(pid), do: {:on_mouse_up, payload}
  def on_mouse_up(message), do: on_mouse_up({self(), message})

  @doc "Register a mouse enter handler payload for this element"
  def on_mouse_enter({pid, _msg} = payload) when is_pid(pid), do: {:on_mouse_enter, payload}
  def on_mouse_enter(message), do: on_mouse_enter({self(), message})

  @doc "Register a mouse leave handler payload for this element"
  def on_mouse_leave({pid, _msg} = payload) when is_pid(pid), do: {:on_mouse_leave, payload}
  def on_mouse_leave(message), do: on_mouse_leave({self(), message})

  @doc "Register a mouse move handler payload for this element"
  def on_mouse_move({pid, _msg} = payload) when is_pid(pid), do: {:on_mouse_move, payload}
  def on_mouse_move(message), do: on_mouse_move({self(), message})

  @doc "Register a change handler payload for this input element"
  def on_change({pid, _msg} = payload) when is_pid(pid), do: {:on_change, payload}
  def on_change(message), do: on_change({self(), message})

  @doc "Register a focus handler payload for this input element"
  def on_focus({pid, _msg} = payload) when is_pid(pid), do: {:on_focus, payload}
  def on_focus(message), do: on_focus({self(), message})

  @doc "Register a blur handler payload for this input element"
  def on_blur({pid, _msg} = payload) when is_pid(pid), do: {:on_blur, payload}
  def on_blur(message), do: on_blur({self(), message})

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

  @doc "Animate compatible attrs across keyframes"
  def animate(keyframes, duration, curve, repeat \\ :once) do
    {:animate, %{keyframes: keyframes, duration: duration, curve: curve, repeat: repeat}}
  end

  @doc """
  Animate compatible attrs once when the element is first mounted.

  Unlike `animate/4`, this does not start if it is added later to an existing retained node.
  If both `animate_enter/4` and `animate/4` are present, `animate/4` starts after the
  enter animation completes.

  ## Example

      if open? do
        el(
          [
            key(:shelf),
            width(px(164)),
            alpha(1.0),
            move_x(0),
            animate_enter(
              [
                [width(px(24)), alpha(0.0), move_x(14)],
                [width(px(164)), alpha(1.0), move_x(0)]
              ],
              220,
              :ease_out
            )
          ],
          text("Details")
        )
      end
  """
  def animate_enter(keyframes, duration, curve, repeat \\ :once) do
    {:animate_enter, %{keyframes: keyframes, duration: duration, curve: curve, repeat: repeat}}
  end

  @doc "Animate compatible attrs once when the element is removed from the tree"
  def animate_exit(keyframes, duration, curve, repeat \\ :once) do
    {:animate_exit, %{keyframes: keyframes, duration: duration, curve: curve, repeat: repeat}}
  end

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

  defp parse_attrs(attrs, attrs_owner, opts \\ []) do
    warn_overrides = Keyword.get(opts, :warn_overrides, true)
    extra_public_attr_keys = MapSet.new(Keyword.get(opts, :extra_public_attr_keys, []))

    parsed =
      Enum.reduce(attrs, %{}, fn attr, acc ->
        {key, value} = validate_attr_entry!(attrs_owner, attr, extra_public_attr_keys)

        case key do
          :box_shadow ->
            put_attr(acc, key, value, false)

          _ ->
            put_attr(acc, key, value, warn_overrides)
        end
      end)

    parsed
  end

  defp parse_state_style_attrs(attrs, style_key) do
    attrs = validate_attrs_list!("#{style_key}/1", attrs)
    AttrValidation.normalize_state_style!(style_key, attrs)
  end

  defp put_attr(acc, :box_shadow, value, _warn_overrides) do
    existing = Map.get(acc, :box_shadow, [])
    Map.put(acc, :box_shadow, existing ++ List.wrap(value))
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

  defp validate_attr_entry!(attrs_owner, {key, value}, extra_public_attr_keys)
       when is_atom(key) do
    cond do
      key == :id ->
        raise ArgumentError, "id is not supported; use key instead"

      MapSet.member?(@reserved_attr_keys, key) ->
        raise ArgumentError,
              "#{attrs_owner} does not allow internal attribute #{inspect(key)} in public attrs"

      MapSet.member?(@state_style_key_set, key) ->
        {key, AttrValidation.normalize_state_style!(key, value)}

      key in [:animate, :animate_enter, :animate_exit] ->
        {key, AttrValidation.normalize_animation!(key, value)}

      MapSet.member?(@public_attr_keys, key) or MapSet.member?(extra_public_attr_keys, key) ->
        validate_public_attr_value!(attrs_owner, key, value)
        {key, value}

      true ->
        raise ArgumentError,
              "#{attrs_owner} does not support attribute #{inspect(key)}"
    end
  end

  defp validate_attr_entry!(attrs_owner, other, _extra_public_attr_keys) do
    raise ArgumentError,
          "#{attrs_owner} expects attributes to be {key, value} tuples, got: #{inspect(other)}"
  end

  defp validate_public_attr_value!(_attrs_owner, :key, _value), do: :ok

  defp validate_public_attr_value!(_attrs_owner, :animate, _value), do: :ok

  defp validate_public_attr_value!(_attrs_owner, :animate_enter, _value), do: :ok

  defp validate_public_attr_value!(_attrs_owner, :animate_exit, _value), do: :ok

  defp validate_public_attr_value!(attrs_owner, :width, value),
    do: validate_length!(attrs_owner, :width, value)

  defp validate_public_attr_value!(attrs_owner, :height, value),
    do: validate_length!(attrs_owner, :height, value)

  defp validate_public_attr_value!(attrs_owner, :padding, value),
    do: validate_padding!(attrs_owner, value)

  defp validate_public_attr_value!(attrs_owner, :spacing, value),
    do: validate_number_attr!(attrs_owner, :spacing, value)

  defp validate_public_attr_value!(_attrs_owner, :spacing_xy, {x, y})
       when is_number(x) and is_number(y),
       do: :ok

  defp validate_public_attr_value!(attrs_owner, :spacing_xy, value) do
    raise ArgumentError,
          "#{attrs_owner} expects :spacing_xy to be {x, y} with numeric values, got: #{inspect(value)}"
  end

  defp validate_public_attr_value!(_attrs_owner, key, value)
       when key in [:space_evenly, :scrollbar_y, :scrollbar_x, :snap_layout, :snap_text_metrics] and
              is_boolean(value),
       do: :ok

  defp validate_public_attr_value!(attrs_owner, key, value)
       when key in [:space_evenly, :scrollbar_y, :scrollbar_x, :snap_layout, :snap_text_metrics] do
    raise ArgumentError,
          "#{attrs_owner} expects #{inspect(key)} to be a boolean, got: #{inspect(value)}"
  end

  defp validate_public_attr_value!(_attrs_owner, :align_x, value)
       when value in [:left, :center, :right],
       do: :ok

  defp validate_public_attr_value!(attrs_owner, :align_x, value) do
    raise ArgumentError,
          "#{attrs_owner} expects :align_x to be one of :left, :center, or :right, got: #{inspect(value)}"
  end

  defp validate_public_attr_value!(_attrs_owner, :align_y, value)
       when value in [:top, :center, :bottom],
       do: :ok

  defp validate_public_attr_value!(attrs_owner, :align_y, value) do
    raise ArgumentError,
          "#{attrs_owner} expects :align_y to be one of :top, :center, or :bottom, got: #{inspect(value)}"
  end

  defp validate_public_attr_value!(attrs_owner, :background, value),
    do: AttrValidation.normalize_decorative_value!(attrs_owner, :background, value)

  defp validate_public_attr_value!(attrs_owner, :border_radius, value),
    do: validate_radius!(attrs_owner, :border_radius, value)

  defp validate_public_attr_value!(attrs_owner, :border_width, value),
    do: validate_radius!(attrs_owner, :border_width, value)

  defp validate_public_attr_value!(attrs_owner, key, value)
       when key in [:border_color, :font_color],
       do: AttrValidation.normalize_decorative_value!(attrs_owner, key, value)

  defp validate_public_attr_value!(attrs_owner, :svg_color, value),
    do: AttrValidation.normalize_decorative_value!(attrs_owner, :svg_color, value)

  defp validate_public_attr_value!(attrs_owner, key, value)
       when key in [
              :font_size,
              :move_x,
              :move_y,
              :rotate,
              :scale,
              :alpha,
              :font_letter_spacing,
              :font_word_spacing
            ],
       do: AttrValidation.normalize_decorative_value!(attrs_owner, key, value)

  defp validate_public_attr_value!(_attrs_owner, :font, value)
       when is_atom(value) or is_binary(value),
       do: :ok

  defp validate_public_attr_value!(attrs_owner, :font, value) do
    raise ArgumentError,
          "#{attrs_owner} expects :font to be an atom or binary, got: #{inspect(value)}"
  end

  defp validate_public_attr_value!(_attrs_owner, key, value)
       when key in [:font_weight, :font_style] and is_atom(value),
       do: :ok

  defp validate_public_attr_value!(attrs_owner, key, value)
       when key in [:font_weight, :font_style] do
    raise ArgumentError,
          "#{attrs_owner} expects #{inspect(key)} to be an atom, got: #{inspect(value)}"
  end

  defp validate_public_attr_value!(_attrs_owner, :text_align, value)
       when value in [:left, :center, :right],
       do: :ok

  defp validate_public_attr_value!(attrs_owner, :text_align, value) do
    raise ArgumentError,
          "#{attrs_owner} expects :text_align to be one of :left, :center, or :right, got: #{inspect(value)}"
  end

  defp validate_public_attr_value!(attrs_owner, key, value)
       when key in [
              :on_click,
              :on_press,
              :on_mouse_down,
              :on_mouse_up,
              :on_mouse_enter,
              :on_mouse_leave,
              :on_mouse_move,
              :on_change,
              :on_focus,
              :on_blur
            ],
       do: validate_event_payload!(attrs_owner, key, value)

  defp validate_public_attr_value!(_attrs_owner, key, value)
       when key in [:font_underline, :font_strike] and is_boolean(value),
       do: :ok

  defp validate_public_attr_value!(attrs_owner, key, value)
       when key in [:font_underline, :font_strike] do
    AttrValidation.normalize_decorative_value!(attrs_owner, key, value)
  end

  defp validate_public_attr_value!(_attrs_owner, :border_style, value)
       when value in [:solid, :dashed, :dotted],
       do: :ok

  defp validate_public_attr_value!(attrs_owner, :border_style, value) do
    raise ArgumentError,
          "#{attrs_owner} expects :border_style to be :solid, :dashed, or :dotted, got: #{inspect(value)}"
  end

  defp validate_public_attr_value!(attrs_owner, :box_shadow, value),
    do: AttrValidation.normalize_decorative_value!(attrs_owner, :box_shadow, value)

  defp validate_public_attr_value!(_attrs_owner, :image_fit, value)
       when value in [:contain, :cover],
       do: :ok

  defp validate_public_attr_value!(attrs_owner, :image_fit, value) do
    raise ArgumentError,
          "#{attrs_owner} expects :image_fit to be :contain or :cover, got: #{inspect(value)}"
  end

  defp validate_public_attr_value!(_attrs_owner, key, %Element{} = _value)
       when key in [:above, :below, :on_left, :on_right, :in_front, :behind],
       do: :ok

  defp validate_public_attr_value!(attrs_owner, key, value)
       when key in [:above, :below, :on_left, :on_right, :in_front, :behind] do
    raise ArgumentError,
          "#{attrs_owner} expects #{inspect(key)} to be an Emerge element, got: #{inspect(value)}"
  end

  defp build_element(attrs, type, children) when is_map(attrs) do
    {key, attrs} = Map.pop(attrs, :key)
    attrs = Map.put(attrs, :__attrs_hash, TreeAttrs.attrs_hash(attrs))

    %Element{
      type: type,
      id: key,
      attrs: attrs,
      children: children
    }
  end

  defp prepare_attrs!(function_name, attrs, opts \\ []) do
    attrs = validate_attrs_list!(function_name, attrs)
    parse_attrs(attrs, function_name, opts)
  end

  defp prepare_single_child!(function_name, attrs, child) do
    attrs = validate_attrs_list!(function_name, attrs)
    child = validate_child_element!(function_name, child)

    {parse_attrs(attrs, function_name), child}
  end

  defp prepare_children!(function_name, attrs, children) do
    attrs = validate_attrs_list!(function_name, attrs)
    children = validate_children_list!(function_name, children)

    {parse_attrs(attrs, function_name), children}
  end

  defp validate_attrs_list!(_function_name, attrs) when is_list(attrs), do: attrs

  defp validate_attrs_list!(function_name, other) do
    raise ArgumentError,
          "#{function_name} expects the first argument to be a list of attributes, got: #{inspect(other)}"
  end

  defp validate_child_element!(_function_name, %Element{} = child), do: child

  defp validate_child_element!(function_name, children) when is_list(children) do
    raise ArgumentError,
          "#{function_name} expects the second argument to be a single child element, got a list: #{inspect(children)}. " <>
            "Use row/2, column/2, wrapped_row/2, paragraph/2, or text_column/2 for multiple children."
  end

  defp validate_child_element!(function_name, other) do
    raise ArgumentError,
          "#{function_name} expects the second argument to be a single child element, got: #{inspect(other)}"
  end

  defp validate_children_list!(function_name, children) when is_list(children) do
    Enum.each(children, fn child ->
      case child do
        %Element{} ->
          :ok

        other ->
          raise ArgumentError,
                "#{function_name} expects every child to be an Emerge element, got: #{inspect(other)}"
      end
    end)

    children
  end

  defp validate_children_list!(function_name, other) do
    container_name = function_name |> String.split("/") |> hd()

    raise ArgumentError,
          "#{function_name} expects the second argument to be a list of child elements, got: #{inspect(other)}. " <>
            "Use #{container_name}(attrs, [child]) for a single child."
  end

  defp validate_binary_string!(_function_name, value) when is_binary(value), do: value

  defp validate_binary_string!(function_name, other) do
    raise ArgumentError,
          "#{function_name} expects the second argument to be a binary string, got: #{inspect(other)}"
  end

  defp validate_video_target!(_function_name, %VideoTarget{} = target), do: target

  defp validate_video_target!(function_name, other) do
    raise ArgumentError,
          "#{function_name} expects the second argument to be an EmergeSkia.VideoTarget, got: #{inspect(other)}"
  end

  defp validate_length!(_attrs_owner, _key, :fill), do: :ok
  defp validate_length!(_attrs_owner, _key, :content), do: :ok
  defp validate_length!(_attrs_owner, _key, {:px, value}) when is_number(value), do: :ok
  defp validate_length!(_attrs_owner, _key, {:fill, value}) when is_number(value), do: :ok

  defp validate_length!(attrs_owner, key, {:minimum, min_px, inner}) when is_number(min_px) do
    validate_length!(attrs_owner, key, inner)
  end

  defp validate_length!(attrs_owner, key, {:maximum, max_px, inner}) when is_number(max_px) do
    validate_length!(attrs_owner, key, inner)
  end

  defp validate_length!(attrs_owner, key, value) do
    raise ArgumentError,
          "#{attrs_owner} expects #{inspect(key)} to be a supported length value, got: #{inspect(value)}"
  end

  defp validate_padding!(_attrs_owner, value) when is_number(value), do: :ok

  defp validate_padding!(_attrs_owner, {top, right, bottom, left})
       when is_number(top) and is_number(right) and is_number(bottom) and is_number(left),
       do: :ok

  defp validate_padding!(_attrs_owner, {vertical, horizontal})
       when is_number(vertical) and is_number(horizontal),
       do: :ok

  defp validate_padding!(_attrs_owner, %{top: top, right: right, bottom: bottom, left: left})
       when is_number(top) and is_number(right) and is_number(bottom) and is_number(left),
       do: :ok

  defp validate_padding!(attrs_owner, value) do
    raise ArgumentError,
          "#{attrs_owner} expects :padding to be a number, {vertical, horizontal}, {top, right, bottom, left}, or padding map, got: #{inspect(value)}"
  end

  defp validate_radius!(_attrs_owner, _key, value) when is_number(value), do: :ok

  defp validate_radius!(_attrs_owner, _key, {a, b, c, d})
       when is_number(a) and is_number(b) and is_number(c) and is_number(d),
       do: :ok

  defp validate_radius!(attrs_owner, key, value) do
    raise ArgumentError,
          "#{attrs_owner} expects #{inspect(key)} to be a number or a 4-value tuple, got: #{inspect(value)}"
  end

  defp validate_number_attr!(_attrs_owner, _key, value) when is_number(value), do: :ok

  defp validate_number_attr!(attrs_owner, key, value) do
    raise ArgumentError,
          "#{attrs_owner} expects #{inspect(key)} to be a number, got: #{inspect(value)}"
  end

  defp validate_event_payload!(_attrs_owner, _key, {pid, _message}) when is_pid(pid), do: :ok

  defp validate_event_payload!(attrs_owner, key, value) do
    raise ArgumentError,
          "#{attrs_owner} expects #{inspect(key)} to be a {pid, message} tuple, got: #{inspect(value)}"
  end

  defp validate_image_source!(_attrs_owner, %Emerge.Assets.Ref{path: path} = source)
       when is_binary(path), do: source

  defp validate_image_source!(_attrs_owner, {:id, id}) when is_binary(id), do: {:id, id}
  defp validate_image_source!(_attrs_owner, {:path, path}) when is_binary(path), do: {:path, path}
  defp validate_image_source!(_attrs_owner, path) when is_binary(path), do: path
  defp validate_image_source!(_attrs_owner, path) when is_atom(path), do: path

  defp validate_image_source!(attrs_owner, other) do
    raise ArgumentError,
          "#{attrs_owner} expects an image source to be a binary, atom, ~m reference, {:id, id}, or {:path, path}, got: #{inspect(other)}"
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

  defmodule Svg do
    @moduledoc "SVG-specific styling attributes"

    @doc "Apply template tinting to all visible SVG pixels"
    def color(c), do: {:svg_color, c}
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
    def text(attrs, value) do
      Emerge.UI.__text_input__(attrs, value)
    end

    @doc "Button input with a single child element"
    def button(attrs, child) do
      Emerge.UI.__input_button__(attrs, child)
    end
  end
end
