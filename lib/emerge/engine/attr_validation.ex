defmodule Emerge.Engine.AttrValidation do
  @moduledoc false

  alias Emerge.Engine.AttrSchema

  @decorative_state_key_set AttrSchema.decorative_state_key_set()
  @state_style_key_set AttrSchema.state_style_key_set()
  @animatable_key_set AttrSchema.animatable_key_set()

  def normalize_state_style!(style_key, attrs) when is_list(attrs) do
    Enum.reduce(attrs, %{}, fn attr, acc ->
      case normalize_state_style_attr!(style_key, attr) do
        :skip -> acc
        {key, value} -> put_decorative_attr(acc, key, value)
      end
    end)
  end

  def normalize_state_style!(style_key, attrs) when is_map(attrs) do
    Enum.reduce(attrs, %{}, fn {key, value}, acc ->
      case normalize_state_style_attr!(style_key, {key, value}) do
        :skip -> acc
        {key, value} -> put_decorative_attr(acc, key, value)
      end
    end)
  end

  def normalize_state_style!(style_key, other) do
    raise ArgumentError,
          "#{style_key} must be a list/map of decorative attributes, got: #{inspect(other)}"
  end

  def normalize_animation!(spec), do: normalize_animation!(:animate, spec)

  def normalize_animation!(owner, %{} = spec)
      when owner in [:animate, :animate_enter, :animate_exit] do
    owner_name = Atom.to_string(owner)
    keyframes = Map.get(spec, :keyframes)
    duration = Map.get(spec, :duration)
    curve = Map.get(spec, :curve)
    repeat = Map.get(spec, :repeat, :once)

    validate_animation_duration!(owner_name, duration)
    validate_animation_curve!(owner_name, curve)
    validate_animation_repeat!(owner_name, repeat)

    normalized_keyframes = normalize_animation_keyframes!(owner_name, keyframes)

    %{
      keyframes: normalized_keyframes,
      duration: duration,
      curve: curve,
      repeat: repeat
    }
  end

  def normalize_animation!(owner, other)
      when owner in [:animate, :animate_enter, :animate_exit] do
    owner_name = Atom.to_string(owner)

    raise ArgumentError,
          "#{owner_name} expects a map with :keyframes, :duration, :curve, and optional :repeat, got: #{inspect(other)}"
  end

  def normalize_decorative_value!(attrs_owner, :background, value) do
    validate_background!(attrs_owner, value)
    value
  end

  def normalize_decorative_value!(attrs_owner, :border_radius, value) do
    validate_radius!(attrs_owner, :border_radius, value)
    value
  end

  def normalize_decorative_value!(attrs_owner, :border_width, value) do
    validate_border_width!(attrs_owner, value)
    value
  end

  def normalize_decorative_value!(attrs_owner, :border_style, value) do
    validate_border_style!(attrs_owner, value)
    value
  end

  def normalize_decorative_value!(attrs_owner, key, value)
      when key in [:border_color, :font_color, :svg_color] do
    validate_color_attr!(attrs_owner, key, value)
    value
  end

  def normalize_decorative_value!(attrs_owner, :font, value) do
    validate_font_attr!(attrs_owner, value)
    value
  end

  def normalize_decorative_value!(attrs_owner, key, value)
      when key in [:font_weight, :font_style] do
    validate_atom_attr!(attrs_owner, key, value)
    value
  end

  def normalize_decorative_value!(attrs_owner, key, value)
      when key in [
             :font_size,
             :move_x,
             :move_y,
             :rotate,
             :scale,
             :alpha,
             :font_letter_spacing,
             :font_word_spacing
           ] do
    validate_number_attr!(attrs_owner, key, value)
    value
  end

  def normalize_decorative_value!(attrs_owner, key, value)
      when key in [:font_underline, :font_strike] do
    validate_boolean_attr!(attrs_owner, key, value)
    value
  end

  def normalize_decorative_value!(attrs_owner, :text_align, value) do
    validate_text_align!(attrs_owner, value)
    value
  end

  def normalize_decorative_value!(attrs_owner, :box_shadow, value) do
    normalize_box_shadow!(attrs_owner, value)
  end

  defp normalize_state_style_attr!(_style_key, nil), do: :skip

  defp normalize_state_style_attr!(style_key, {key, value}) when is_atom(key) do
    cond do
      MapSet.member?(@state_style_key_set, key) ->
        raise ArgumentError, "#{style_key} does not support nested #{key}"

      MapSet.member?(@decorative_state_key_set, key) ->
        skip_nil_or(value, fn -> {key, normalize_decorative_value!(style_key, key, value)} end)

      true ->
        raise ArgumentError,
              "#{style_key} only supports decorative attributes; got #{inspect(key)}. Allowed: #{AttrSchema.decorative_state_keys_message()}"
    end
  end

  defp normalize_state_style_attr!(style_key, other) do
    raise ArgumentError,
          "#{style_key}/1 expects decorative attributes as {key, value} tuples, got: #{inspect(other)}"
  end

  defp normalize_animation_keyframes!(owner_name, keyframes) when is_list(keyframes) do
    normalized =
      keyframes
      |> Enum.with_index(1)
      |> Enum.map(fn {keyframe, index} ->
        normalize_animation_keyframe!(owner_name, keyframe, index)
      end)

    if length(normalized) < 2 do
      raise ArgumentError, "#{owner_name} expects at least 2 keyframes"
    end

    [first | rest] = normalized
    first_keys = first |> Map.keys() |> MapSet.new()

    if MapSet.size(first_keys) == 0 do
      raise ArgumentError, "#{owner_name} keyframes must not be empty"
    end

    Enum.with_index(rest, 2)
    |> Enum.each(fn {keyframe, index} ->
      key_set = keyframe |> Map.keys() |> MapSet.new()

      if key_set != first_keys do
        raise ArgumentError,
              "#{owner_name} keyframe #{index} must use the same attribute set as keyframe 1"
      end

      Enum.each(first, fn {key, first_value} ->
        validate_animation_compatibility!(
          owner_name,
          key,
          first_value,
          Map.fetch!(keyframe, key),
          index
        )
      end)
    end)

    normalized
  end

  defp normalize_animation_keyframes!(owner_name, other) do
    raise ArgumentError,
          "#{owner_name} expects :keyframes to be a list of keyframe attr lists/maps, got: #{inspect(other)}"
  end

  defp normalize_animation_keyframe!(owner_name, attrs, index) when is_list(attrs) do
    Enum.reduce(attrs, %{}, fn attr, acc ->
      case normalize_animation_attr!("#{owner_name} keyframe #{index}", attr) do
        :skip -> acc
        {key, value} -> put_animation_attr(acc, key, value)
      end
    end)
  end

  defp normalize_animation_keyframe!(owner_name, attrs, index) when is_map(attrs) do
    Enum.reduce(attrs, %{}, fn {key, value}, acc ->
      case normalize_animation_attr!("#{owner_name} keyframe #{index}", {key, value}) do
        :skip -> acc
        {key, value} -> put_animation_attr(acc, key, value)
      end
    end)
  end

  defp normalize_animation_keyframe!(owner_name, other, index) do
    raise ArgumentError,
          "#{owner_name} keyframe #{index} must be a list/map of animatable attrs, got: #{inspect(other)}"
  end

  defp normalize_animation_attr!(_attrs_owner, nil), do: :skip

  defp normalize_animation_attr!(attrs_owner, {key, value}) when is_atom(key) do
    if !MapSet.member?(@animatable_key_set, key) do
      raise ArgumentError,
            "#{attrs_owner} only supports animatable attributes; got #{inspect(key)}. Allowed: #{AttrSchema.animatable_keys_message()}"
    end

    skip_nil_or(value, fn ->
      normalized =
        case key do
          :width ->
            validate_length!(attrs_owner, :width, value)
            value

          :height ->
            validate_length!(attrs_owner, :height, value)
            value

          :padding ->
            validate_padding!(attrs_owner, value)
            normalize_padding(value)

          :spacing ->
            validate_number_attr!(attrs_owner, :spacing, value)
            value

          :spacing_xy ->
            validate_spacing_xy!(attrs_owner, value)
            value

          :border_radius ->
            validate_radius!(attrs_owner, :border_radius, value)
            value

          :border_width ->
            validate_border_width!(attrs_owner, value)
            value

          _ ->
            normalize_decorative_value!(attrs_owner, key, value)
        end

      {key, normalized}
    end)
  end

  defp normalize_animation_attr!(attrs_owner, other) do
    raise ArgumentError,
          "#{attrs_owner} expects animatable attributes as {key, value} tuples, got: #{inspect(other)}"
  end

  defp put_animation_attr(acc, :box_shadow, value) do
    existing = Map.get(acc, :box_shadow, [])
    Map.put(acc, :box_shadow, existing ++ value)
  end

  defp put_animation_attr(acc, key, value), do: Map.put(acc, key, value)

  defp skip_nil_or(nil, _fun), do: :skip
  defp skip_nil_or(_value, fun), do: fun.()

  defp validate_animation_compatibility!(owner_name, :width, first, other, index),
    do: validate_length_compatibility!(owner_name, :width, first, other, index)

  defp validate_animation_compatibility!(owner_name, :height, first, other, index),
    do: validate_length_compatibility!(owner_name, :height, first, other, index)

  defp validate_animation_compatibility!(owner_name, :padding, first, other, index) do
    if padding_shape(first) != padding_shape(other) do
      raise ArgumentError,
            "#{owner_name} keyframe #{index} must keep :padding in the same variant as keyframe 1"
    end
  end

  defp validate_animation_compatibility!(
         _owner_name,
         :spacing_xy,
         {_x1, _y1},
         {_x2, _y2},
         _index
       ),
       do: :ok

  defp validate_animation_compatibility!(owner_name, :spacing_xy, _first, _other, index),
    do:
      raise(
        ArgumentError,
        "#{owner_name} keyframe #{index} must keep :spacing_xy as a {x, y} tuple"
      )

  defp validate_animation_compatibility!(owner_name, :border_radius, first, other, index) do
    if radius_shape(first) != radius_shape(other) do
      raise ArgumentError,
            "#{owner_name} keyframe #{index} must keep :border_radius in the same variant as keyframe 1"
    end
  end

  defp validate_animation_compatibility!(owner_name, :border_width, first, other, index) do
    if border_width_shape(first) != border_width_shape(other) do
      raise ArgumentError,
            "#{owner_name} keyframe #{index} must keep :border_width in the same variant as keyframe 1"
    end
  end

  defp validate_animation_compatibility!(owner_name, :background, first, other, index) do
    if !compatible_background?(first, other) do
      raise ArgumentError,
            "#{owner_name} keyframe #{index} must keep :background in a compatible variant with keyframe 1"
    end
  end

  defp validate_animation_compatibility!(owner_name, :box_shadow, first, other, index) do
    if length(first) != length(other) do
      raise ArgumentError,
            "#{owner_name} keyframe #{index} must keep :box_shadow list length the same as keyframe 1"
    end
  end

  defp validate_animation_compatibility!(_owner_name, _key, _first, _other, _index), do: :ok

  defp validate_animation_duration!(_owner_name, duration)
       when is_number(duration) and duration > 0,
       do: :ok

  defp validate_animation_duration!(owner_name, duration) do
    raise ArgumentError,
          "#{owner_name} expects :duration to be a positive number of milliseconds, got: #{inspect(duration)}"
  end

  defp validate_animation_curve!(_owner_name, curve)
       when curve in [:linear, :ease_in, :ease_out, :ease_in_out],
       do: :ok

  defp validate_animation_curve!(owner_name, curve) do
    raise ArgumentError,
          "#{owner_name} expects :curve to be :linear, :ease_in, :ease_out, or :ease_in_out, got: #{inspect(curve)}"
  end

  defp validate_animation_repeat!("animate_exit", :once), do: :ok

  defp validate_animation_repeat!("animate_exit", repeat) do
    raise ArgumentError,
          "animate_exit expects :repeat to be :once, got: #{inspect(repeat)}"
  end

  defp validate_animation_repeat!(_owner_name, :once), do: :ok
  defp validate_animation_repeat!(_owner_name, :loop), do: :ok

  defp validate_animation_repeat!(_owner_name, repeat)
       when is_integer(repeat) and repeat > 0,
       do: :ok

  defp validate_animation_repeat!(owner_name, repeat) do
    raise ArgumentError,
          "#{owner_name} expects :repeat to be :once, :loop, or a positive integer, got: #{inspect(repeat)}"
  end

  defp validate_length_compatibility!(owner_name, key, first, other, index) do
    if !compatible_length?(first, other) do
      raise ArgumentError,
            "#{owner_name} keyframe #{index} must keep #{inspect(key)} in the same length variant as keyframe 1"
    end
  end

  defp compatible_length?(:fill, :fill), do: true
  defp compatible_length?(:content, :content), do: true
  defp compatible_length?({:px, _}, {:px, _}), do: true
  defp compatible_length?({:fill, _}, {:fill, _}), do: true

  defp compatible_length?({:minimum, _min_a, inner_a}, {:minimum, _min_b, inner_b}),
    do: compatible_length?(inner_a, inner_b)

  defp compatible_length?({:maximum, _max_a, inner_a}, {:maximum, _max_b, inner_b}),
    do: compatible_length?(inner_a, inner_b)

  defp compatible_length?(_first, _other), do: false

  defp compatible_background?(
         {:gradient, _from_a, _to_a, _angle_a},
         {:gradient, _from_b, _to_b, _angle_b}
       ),
       do: true

  defp compatible_background?({:image, source_a, fit_a}, {:image, source_b, fit_b}),
    do: source_a == source_b and fit_a == fit_b

  defp compatible_background?(first, other), do: valid_color?(first) and valid_color?(other)

  defp normalize_padding({vertical, horizontal}), do: {vertical, horizontal, vertical, horizontal}
  defp normalize_padding(value), do: value

  defp padding_shape(value) when is_number(value), do: :uniform
  defp padding_shape({_top, _right, _bottom, _left}), do: :sides
  defp padding_shape(%{top: _top, right: _right, bottom: _bottom, left: _left}), do: :map

  defp radius_shape(value) when is_number(value), do: :uniform
  defp radius_shape({_tl, _tr, _br, _bl}), do: :corners

  defp border_width_shape(value) when is_number(value), do: :uniform
  defp border_width_shape({_top, _right, _bottom, _left}), do: :sides

  defp normalize_box_shadow!(attrs_owner, value) when is_map(value) do
    [normalize_box_shadow_item!(attrs_owner, value)]
  end

  defp normalize_box_shadow!(attrs_owner, value) when is_list(value) do
    Enum.map(value, &normalize_box_shadow_item!(attrs_owner, &1))
  end

  defp normalize_box_shadow!(attrs_owner, value) do
    raise ArgumentError,
          "#{attrs_owner} expects :box_shadow to be a shadow map or a list of shadow maps, got: #{inspect(value)}"
  end

  defp put_decorative_attr(acc, :box_shadow, value) do
    existing = Map.get(acc, :box_shadow, [])
    Map.put(acc, :box_shadow, existing ++ value)
  end

  defp put_decorative_attr(acc, key, value), do: Map.put(acc, key, value)

  defp validate_boolean_attr!(_attrs_owner, _key, value) when is_boolean(value), do: :ok

  defp validate_boolean_attr!(attrs_owner, key, value) do
    raise ArgumentError,
          "#{attrs_owner} expects #{inspect(key)} to be a boolean, got: #{inspect(value)}"
  end

  defp validate_number_attr!(_attrs_owner, _key, value) when is_number(value), do: :ok

  defp validate_number_attr!(attrs_owner, key, value) do
    raise ArgumentError,
          "#{attrs_owner} expects #{inspect(key)} to be a number, got: #{inspect(value)}"
  end

  defp validate_color_attr!(attrs_owner, key, value) do
    case valid_color?(value) do
      true ->
        :ok

      false ->
        raise ArgumentError,
              "#{attrs_owner} expects #{inspect(key)} to be a supported color, got: #{inspect(value)}"
    end
  end

  defp validate_font_attr!(_attrs_owner, value) when is_atom(value) or is_binary(value), do: :ok

  defp validate_font_attr!(attrs_owner, value) do
    raise ArgumentError,
          "#{attrs_owner} expects :font to be an atom or binary, got: #{inspect(value)}"
  end

  defp validate_atom_attr!(_attrs_owner, _key, value) when is_atom(value), do: :ok

  defp validate_atom_attr!(attrs_owner, key, value) do
    raise ArgumentError,
          "#{attrs_owner} expects #{inspect(key)} to be an atom, got: #{inspect(value)}"
  end

  defp validate_text_align!(_attrs_owner, value) when value in [:left, :center, :right], do: :ok

  defp validate_text_align!(attrs_owner, value) do
    raise ArgumentError,
          "#{attrs_owner} expects :text_align to be one of :left, :center, or :right, got: #{inspect(value)}"
  end

  defp validate_border_style!(_attrs_owner, value) when value in [:solid, :dashed, :dotted],
    do: :ok

  defp validate_border_style!(attrs_owner, value) do
    raise ArgumentError,
          "#{attrs_owner} expects :border_style to be :solid, :dashed, or :dotted, got: #{inspect(value)}"
  end

  defp validate_background!(attrs_owner, {:gradient, from, to, angle}) when is_number(angle) do
    validate_color_attr!(attrs_owner, :background, from)
    validate_color_attr!(attrs_owner, :background, to)
  end

  defp validate_background!(attrs_owner, {:image, source, fit}) do
    validate_image_source!(attrs_owner, source)
    validate_background_image_fit!(attrs_owner, fit)
  end

  defp validate_background!(attrs_owner, {:image, source}) do
    validate_image_source!(attrs_owner, source)
  end

  defp validate_background!(attrs_owner, value) do
    validate_color_attr!(attrs_owner, :background, value)
  end

  defp validate_background_image_fit!(_attrs_owner, fit)
       when fit in [:contain, :cover, :repeat, :repeat_x, :repeat_y],
       do: :ok

  defp validate_background_image_fit!(attrs_owner, fit) do
    raise ArgumentError,
          "#{attrs_owner} expects background image fit to be :contain, :cover, :repeat, :repeat_x, or :repeat_y, got: #{inspect(fit)}"
  end

  defp normalize_box_shadow_item!(
         attrs_owner,
         %{
           offset_x: ox,
           offset_y: oy,
           size: size,
           blur: blur,
           color: color,
           inset: inset
         } = value
       )
       when is_number(ox) and is_number(oy) and is_number(size) and is_number(blur) and
              is_boolean(inset) do
    validate_color_attr!(attrs_owner, :box_shadow, color)
    value
  end

  defp normalize_box_shadow_item!(attrs_owner, value) do
    raise ArgumentError,
          "#{attrs_owner} expects each :box_shadow entry to include numeric :offset_x, :offset_y, :size, :blur, a valid :color, and boolean :inset, got: #{inspect(value)}"
  end

  defp validate_image_source!(_attrs_owner, %Emerge.Assets.Ref{path: path}) when is_binary(path),
    do: :ok

  defp validate_image_source!(_attrs_owner, {:id, id}) when is_binary(id), do: :ok
  defp validate_image_source!(_attrs_owner, {:path, path}) when is_binary(path), do: :ok
  defp validate_image_source!(_attrs_owner, path) when is_binary(path), do: :ok
  defp validate_image_source!(_attrs_owner, path) when is_atom(path), do: :ok

  defp validate_image_source!(attrs_owner, other) do
    raise ArgumentError,
          "#{attrs_owner} expects an image source to be a binary, atom, ~m reference, {:id, id}, or {:path, path}, got: #{inspect(other)}"
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

  defp validate_border_width!(_attrs_owner, value) when is_number(value), do: :ok

  defp validate_border_width!(_attrs_owner, {top, right, bottom, left})
       when is_number(top) and is_number(right) and is_number(bottom) and is_number(left),
       do: :ok

  defp validate_border_width!(attrs_owner, value) do
    raise ArgumentError,
          "#{attrs_owner} expects :border_width to be a number or a 4-value tuple, got: #{inspect(value)}"
  end

  defp validate_spacing_xy!(_attrs_owner, {x, y}) when is_number(x) and is_number(y), do: :ok

  defp validate_spacing_xy!(attrs_owner, value) do
    raise ArgumentError,
          "#{attrs_owner} expects :spacing_xy to be {x, y} with numeric values, got: #{inspect(value)}"
  end

  defp valid_color?({:color_rgb, {r, g, b}}),
    do: valid_byte?(r) and valid_byte?(g) and valid_byte?(b)

  defp valid_color?({:color_rgba, {r, g, b, a}}),
    do: valid_byte?(r) and valid_byte?(g) and valid_byte?(b) and valid_byte?(a)

  defp valid_color?(color) when is_atom(color), do: true
  defp valid_color?(_other), do: false

  defp valid_byte?(value), do: is_integer(value) and value >= 0 and value <= 255
end
