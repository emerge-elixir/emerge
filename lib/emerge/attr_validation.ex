defmodule Emerge.AttrValidation do
  @moduledoc false

  alias Emerge.AttrSchema

  @decorative_state_key_set AttrSchema.decorative_state_key_set()
  @state_style_key_set AttrSchema.state_style_key_set()

  def normalize_state_style!(style_key, attrs) when is_list(attrs) do
    Enum.reduce(attrs, %{}, fn attr, acc ->
      {key, value} = normalize_state_style_attr!(style_key, attr)
      put_decorative_attr(acc, key, value)
    end)
  end

  def normalize_state_style!(style_key, attrs) when is_map(attrs) do
    Enum.reduce(attrs, %{}, fn {key, value}, acc ->
      {key, value} = normalize_state_style_attr!(style_key, {key, value})
      put_decorative_attr(acc, key, value)
    end)
  end

  def normalize_state_style!(style_key, other) do
    raise ArgumentError,
          "#{style_key} must be a list/map of decorative attributes, got: #{inspect(other)}"
  end

  def normalize_decorative_value!(attrs_owner, :background, value) do
    validate_background!(attrs_owner, value)
    value
  end

  def normalize_decorative_value!(attrs_owner, key, value)
      when key in [:border_color, :font_color, :svg_color] do
    validate_color_attr!(attrs_owner, key, value)
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

  def normalize_decorative_value!(attrs_owner, :box_shadow, value) do
    normalize_box_shadow!(attrs_owner, value)
  end

  defp normalize_state_style_attr!(style_key, {key, value}) when is_atom(key) do
    cond do
      MapSet.member?(@state_style_key_set, key) ->
        raise ArgumentError, "#{style_key} does not support nested #{key}"

      MapSet.member?(@decorative_state_key_set, key) ->
        {key, normalize_decorative_value!(style_key, key, value)}

      true ->
        raise ArgumentError,
              "#{style_key} only supports decorative attributes; got #{inspect(key)}. Allowed: #{AttrSchema.decorative_state_keys_message()}"
    end
  end

  defp normalize_state_style_attr!(style_key, other) do
    raise ArgumentError,
          "#{style_key}/1 expects decorative attributes as {key, value} tuples, got: #{inspect(other)}"
  end

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

  defp valid_color?({:color_rgb, {r, g, b}}),
    do: valid_byte?(r) and valid_byte?(g) and valid_byte?(b)

  defp valid_color?({:color_rgba, {r, g, b, a}}),
    do: valid_byte?(r) and valid_byte?(g) and valid_byte?(b) and valid_byte?(a)

  defp valid_color?(color) when is_atom(color), do: true
  defp valid_color?(_other), do: false

  defp valid_byte?(value), do: is_integer(value) and value >= 0 and value <= 255
end
