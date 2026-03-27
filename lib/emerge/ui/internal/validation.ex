defmodule Emerge.UI.Internal.Validation do
  @moduledoc false

  alias Emerge.Engine.AttrSchema
  alias Emerge.Engine.AttrValidation
  alias Emerge.Engine.Element
  alias Emerge.Engine.Tree.Attrs, as: TreeAttrs
  alias Emerge.UI.Event
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
                      :on_click,
                      :on_press,
                      :on_swipe_up,
                      :on_swipe_down,
                      :on_swipe_left,
                      :on_swipe_right,
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
                      :on_key_down,
                      :on_key_up,
                      :on_key_press,
                      :virtual_key,
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

  @type attrs_owner :: String.t()
  @type attrs_options :: keyword()
  @type attrs_map :: map()
  @type attrs_list :: Emerge.UI.attrs()
  @type state_style_key :: :mouse_over | :focused | :mouse_down
  @type image_source :: Emerge.UI.image_source()

  @spec parse_attrs(attrs_list(), attrs_owner()) :: attrs_map()
  @spec parse_attrs(attrs_list(), attrs_owner(), attrs_options()) :: attrs_map()
  def parse_attrs(attrs, attrs_owner, opts \\ []) do
    warn_overrides = Keyword.get(opts, :warn_overrides, true)
    extra_public_attr_keys = MapSet.new(Keyword.get(opts, :extra_public_attr_keys, []))

    attrs
    |> Enum.reduce(%{}, fn attr, acc ->
      {key, value} = validate_attr_entry!(attrs_owner, attr, extra_public_attr_keys)

      case key do
        :box_shadow ->
          put_attr(acc, key, value, false)

        _ ->
          put_attr(acc, key, value, warn_overrides)
      end
    end)
    |> validate_attr_conflicts!(attrs_owner)
  end

  @spec parse_state_style_attrs(attrs_list(), state_style_key()) :: attrs_map()
  def parse_state_style_attrs(attrs, style_key) do
    attrs = validate_attrs_list!("#{style_key}/1", attrs)
    AttrValidation.normalize_state_style!(style_key, attrs)
  end

  @spec validate_attrs_list!(attrs_owner(), term()) :: attrs_list()
  def validate_attrs_list!(_function_name, attrs) when is_list(attrs), do: attrs

  def validate_attrs_list!(function_name, other) do
    raise ArgumentError,
          "#{function_name} expects the first argument to be a list of attributes, got: #{inspect(other)}"
  end

  @spec validate_child_element!(attrs_owner(), term()) :: Element.t()
  def validate_child_element!(_function_name, %Element{} = child), do: child

  def validate_child_element!(function_name, children) when is_list(children) do
    raise ArgumentError,
          "#{function_name} expects the second argument to be a single child element, got a list: #{inspect(children)}. " <>
            "Use row/2, column/2, wrapped_row/2, paragraph/2, or text_column/2 for multiple children."
  end

  def validate_child_element!(function_name, other) do
    raise ArgumentError,
          "#{function_name} expects the second argument to be a single child element, got: #{inspect(other)}"
  end

  @spec validate_children_list!(attrs_owner(), term()) :: [Element.t()]
  def validate_children_list!(function_name, children) when is_list(children) do
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

  def validate_children_list!(function_name, other) do
    container_name = function_name |> String.split("/") |> hd()

    raise ArgumentError,
          "#{function_name} expects the second argument to be a list of child elements, got: #{inspect(other)}. " <>
            "Use #{container_name}(attrs, [child]) for a single child."
  end

  @spec validate_binary_string!(attrs_owner(), term()) :: binary()
  def validate_binary_string!(_function_name, value) when is_binary(value), do: value

  def validate_binary_string!(function_name, other) do
    raise ArgumentError,
          "#{function_name} expects the second argument to be a binary string, got: #{inspect(other)}"
  end

  @spec validate_video_target!(attrs_owner(), term()) :: VideoTarget.t()
  def validate_video_target!(_function_name, %VideoTarget{} = target), do: target

  def validate_video_target!(function_name, other) do
    raise ArgumentError,
          "#{function_name} expects the second argument to be an EmergeSkia.VideoTarget, got: #{inspect(other)}"
  end

  @spec validate_image_source!(attrs_owner(), image_source()) :: image_source()
  def validate_image_source!(_attrs_owner, %Emerge.Assets.Ref{path: path} = source)
      when is_binary(path),
      do: source

  def validate_image_source!(_attrs_owner, {:id, id}) when is_binary(id), do: {:id, id}
  def validate_image_source!(_attrs_owner, {:path, path}) when is_binary(path), do: {:path, path}
  def validate_image_source!(_attrs_owner, path) when is_binary(path), do: path
  def validate_image_source!(_attrs_owner, path) when is_atom(path), do: path

  def validate_image_source!(attrs_owner, other) do
    raise ArgumentError,
          "#{attrs_owner} expects an image source to be a binary, atom, ~m reference, {:id, id}, or {:path, path}, got: #{inspect(other)}"
  end

  defp put_attr(acc, :box_shadow, value, _warn_overrides) do
    existing = Map.get(acc, :box_shadow, [])
    Map.put(acc, :box_shadow, existing ++ List.wrap(value))
  end

  defp put_attr(acc, key, value, _warn_overrides)
       when key in [:on_key_down, :on_key_up, :on_key_press] do
    existing = Map.get(acc, key, [])
    Map.put(acc, key, existing ++ List.wrap(value))
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

      key in [:on_key_down, :on_key_up, :on_key_press] ->
        {key, Event.normalize_key_listener_bindings!(key, value)}

      key == :virtual_key ->
        {key, Event.normalize_virtual_key!(value)}

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
  defp validate_public_attr_value!(_attrs_owner, :virtual_key, _value), do: :ok

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
              :on_swipe_up,
              :on_swipe_down,
              :on_swipe_left,
              :on_swipe_right,
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

  defp validate_length!(_attrs_owner, _key, :fill), do: :ok
  defp validate_length!(_attrs_owner, _key, :content), do: :ok
  defp validate_length!(_attrs_owner, _key, {:px, value}) when is_number(value), do: :ok

  defp validate_length!(_attrs_owner, _key, {:fill, value}) when is_number(value) and value > 0,
    do: :ok

  defp validate_length!(attrs_owner, key, {:fill, value}) when is_number(value) do
    raise ArgumentError,
          "#{attrs_owner} expects #{inspect(key)} fill weight to be a positive number, got: #{inspect(value)}"
  end

  defp validate_length!(attrs_owner, key, {:minimum, min_px, inner})
       when is_number(min_px) and min_px >= 0 do
    validate_length!(attrs_owner, key, inner)
  end

  defp validate_length!(attrs_owner, key, {:minimum, min_px, _inner}) when is_number(min_px) do
    raise ArgumentError,
          "#{attrs_owner} expects #{inspect(key)} min length to be non-negative, got: #{inspect(min_px)}"
  end

  defp validate_length!(attrs_owner, key, {:maximum, max_px, inner})
       when is_number(max_px) and max_px >= 0 do
    validate_length!(attrs_owner, key, inner)
  end

  defp validate_length!(attrs_owner, key, {:maximum, max_px, _inner}) when is_number(max_px) do
    raise ArgumentError,
          "#{attrs_owner} expects #{inspect(key)} max length to be non-negative, got: #{inspect(max_px)}"
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

  defp validate_attr_conflicts!(attrs, attrs_owner) do
    if Map.has_key?(attrs, :virtual_key) and
         (Map.has_key?(attrs, :on_click) or Map.has_key?(attrs, :on_press)) do
      raise ArgumentError,
            "#{attrs_owner} does not allow :virtual_key together with :on_click or :on_press"
    end

    attrs
  end
end
