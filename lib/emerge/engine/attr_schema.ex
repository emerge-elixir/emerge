defmodule Emerge.Engine.AttrSchema do
  @moduledoc false

  @decorative_state_keys [
    :background,
    :border_color,
    :box_shadow,
    :font_color,
    :svg_color,
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
  ]

  @decorative_state_key_set MapSet.new(@decorative_state_keys)

  @state_style_keys [:mouse_over, :focused, :mouse_down]
  @state_style_key_set MapSet.new(@state_style_keys)

  @animatable_keys [
    :width,
    :height,
    :padding,
    :spacing,
    :spacing_xy,
    :background,
    :border_radius,
    :border_width,
    :border_color,
    :box_shadow,
    :font_size,
    :font_color,
    :font_letter_spacing,
    :font_word_spacing,
    :svg_color,
    :move_x,
    :move_y,
    :rotate,
    :scale,
    :alpha
  ]

  @animatable_key_set MapSet.new(@animatable_keys)

  def decorative_state_keys, do: @decorative_state_keys
  def decorative_state_key_set, do: @decorative_state_key_set
  def decorative_state_key?(key), do: MapSet.member?(@decorative_state_key_set, key)

  def state_style_keys, do: @state_style_keys
  def state_style_key_set, do: @state_style_key_set
  def state_style_key?(key), do: MapSet.member?(@state_style_key_set, key)

  def animatable_keys, do: @animatable_keys
  def animatable_key_set, do: @animatable_key_set
  def animatable_key?(key), do: MapSet.member?(@animatable_key_set, key)

  def decorative_state_keys_message do
    @decorative_state_keys
    |> Enum.map(&inspect/1)
    |> Enum.sort()
    |> Enum.join(", ")
  end

  def animatable_keys_message do
    @animatable_keys
    |> Enum.map(&inspect/1)
    |> Enum.sort()
    |> Enum.join(", ")
  end
end
