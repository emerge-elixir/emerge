defmodule Emerge.Engine.AttrCodec do
  @moduledoc """
  Compact encoding for element attribute maps.
  """

  alias Emerge.Engine.AttrValidation
  alias Emerge.Engine.Tree.Attrs, as: TreeAttrs
  alias Emerge.Engine.Tree.Nearby

  @type_tag %{
    width: 1,
    height: 2,
    padding: 3,
    spacing: 4,
    align_x: 5,
    align_y: 6,
    scrollbar_y: 7,
    scrollbar_x: 8,
    background: 12,
    border_radius: 13,
    border_width: 14,
    border_color: 15,
    font_size: 16,
    font_color: 17,
    font: 18,
    font_weight: 19,
    font_style: 20,
    content: 21,
    snap_layout: 28,
    snap_text_metrics: 29,
    text_align: 30,
    move_x: 31,
    move_y: 32,
    rotate: 33,
    scale: 34,
    alpha: 35,
    spacing_xy: 36,
    space_evenly: 37,
    scroll_x: 38,
    scroll_y: 39,
    on_click: 40,
    on_mouse_down: 41,
    on_mouse_up: 42,
    on_mouse_enter: 43,
    on_mouse_leave: 44,
    on_mouse_move: 45,
    mouse_over: 46,
    font_underline: 47,
    font_strike: 48,
    font_letter_spacing: 49,
    font_word_spacing: 50,
    border_style: 51,
    box_shadow: 52,
    image_src: 53,
    image_fit: 54,
    image_size: 55,
    on_change: 56,
    on_focus: 57,
    on_blur: 58,
    focused: 59,
    mouse_down: 60,
    on_press: 61,
    video_target: 62,
    svg_color: 63,
    svg_expected: 64,
    animate: 65,
    animate_enter: 66,
    animate_exit: 67
  }

  @tag_type Map.new(@type_tag, fn {type, tag} -> {tag, type} end)

  @spec encode_attrs(map()) :: binary()
  def encode_attrs(attrs) when is_map(attrs) do
    attrs
    |> TreeAttrs.strip_runtime_attrs()
    |> Nearby.strip_nearby_attrs()
    |> Map.to_list()
    |> Enum.map(fn {key, value} ->
      tag = Map.fetch!(@type_tag, key)
      {tag, key, value}
    end)
    |> Enum.sort_by(fn {tag, _key, _value} -> tag end)
    |> Enum.map(fn {tag, key, value} ->
      [<<tag::unsigned-8>>, encode_value(key, value)]
    end)
    |> then(fn encoded ->
      count = length(encoded)
      [<<count::unsigned-16>> | encoded] |> IO.iodata_to_binary()
    end)
  end

  @spec decode_attrs(binary()) :: map()
  def decode_attrs(<<count::unsigned-16, rest::binary>>) do
    {attrs, <<>>} = decode_pairs(rest, count, [])
    Map.new(attrs)
  end

  defp decode_pairs(rest, 0, acc), do: {Enum.reverse(acc), rest}

  defp decode_pairs(<<tag::unsigned-8, rest::binary>>, count, acc) do
    key = Map.fetch!(@tag_type, tag)
    {value, rest} = decode_value(key, rest)
    decode_pairs(rest, count - 1, [{key, value} | acc])
  end

  defp encode_value(:width, value), do: encode_length(value)
  defp encode_value(:height, value), do: encode_length(value)
  defp encode_value(:padding, value), do: encode_padding(value)
  defp encode_value(:spacing, value), do: encode_f64(value)
  defp encode_value(:spacing_xy, value), do: encode_spacing_xy(value)
  defp encode_value(:align_x, value), do: encode_align_x(value)
  defp encode_value(:align_y, value), do: encode_align_y(value)
  defp encode_value(:scrollbar_y, value), do: encode_bool(value)
  defp encode_value(:scrollbar_x, value), do: encode_bool(value)
  defp encode_value(:background, value), do: encode_background(value)
  defp encode_value(:border_radius, value), do: encode_radius(value)
  defp encode_value(:border_width, value), do: encode_border_width(value)
  defp encode_value(:border_color, value), do: encode_color(value)
  defp encode_value(:font_size, value), do: encode_f64(value)
  defp encode_value(:font_color, value), do: encode_color(value)
  defp encode_value(:font, value), do: encode_font(value)
  defp encode_value(:font_weight, value), do: encode_atom(value)
  defp encode_value(:font_style, value), do: encode_atom(value)
  defp encode_value(:content, value), do: encode_string(value)
  defp encode_value(:snap_layout, value), do: encode_bool(value)
  defp encode_value(:snap_text_metrics, value), do: encode_bool(value)
  defp encode_value(:text_align, value), do: encode_text_align(value)
  defp encode_value(:move_x, value), do: encode_f64(value)
  defp encode_value(:move_y, value), do: encode_f64(value)
  defp encode_value(:rotate, value), do: encode_f64(value)
  defp encode_value(:scale, value), do: encode_f64(value)
  defp encode_value(:alpha, value), do: encode_f64(value)
  defp encode_value(:space_evenly, value), do: encode_bool(value)
  defp encode_value(:scroll_x, value), do: encode_f64(value)
  defp encode_value(:scroll_y, value), do: encode_f64(value)
  defp encode_value(:on_click, _value), do: encode_bool(true)
  defp encode_value(:on_press, _value), do: encode_bool(true)
  defp encode_value(:on_mouse_down, _value), do: encode_bool(true)
  defp encode_value(:on_mouse_up, _value), do: encode_bool(true)
  defp encode_value(:on_mouse_enter, _value), do: encode_bool(true)
  defp encode_value(:on_mouse_leave, _value), do: encode_bool(true)
  defp encode_value(:on_mouse_move, _value), do: encode_bool(true)
  defp encode_value(:mouse_over, value), do: encode_mouse_over(value)
  defp encode_value(:focused, value), do: encode_focused(value)
  defp encode_value(:mouse_down, value), do: encode_mouse_down_style(value)
  defp encode_value(:font_underline, value), do: encode_bool(value)
  defp encode_value(:font_strike, value), do: encode_bool(value)
  defp encode_value(:font_letter_spacing, value), do: encode_f64(value)
  defp encode_value(:font_word_spacing, value), do: encode_f64(value)
  defp encode_value(:border_style, value), do: encode_border_style(value)
  defp encode_value(:box_shadow, value), do: encode_box_shadow(value)
  defp encode_value(:image_src, value), do: encode_image_src(value)
  defp encode_value(:image_fit, value), do: encode_image_fit(value)
  defp encode_value(:image_size, value), do: encode_image_size(value)
  defp encode_value(:svg_color, value), do: encode_color(value)
  defp encode_value(:svg_expected, value), do: encode_bool(value)
  defp encode_value(:video_target, value), do: encode_string(value)
  defp encode_value(:animate, value), do: encode_animation(value, :animate)
  defp encode_value(:animate_enter, value), do: encode_animation(value, :animate_enter)
  defp encode_value(:animate_exit, value), do: encode_animation(value, :animate_exit)
  defp encode_value(:on_change, _value), do: encode_bool(true)
  defp encode_value(:on_focus, _value), do: encode_bool(true)
  defp encode_value(:on_blur, _value), do: encode_bool(true)

  defp decode_value(:width, rest), do: decode_length(rest)
  defp decode_value(:height, rest), do: decode_length(rest)
  defp decode_value(:padding, rest), do: decode_padding(rest)
  defp decode_value(:spacing, rest), do: decode_f64(rest)
  defp decode_value(:spacing_xy, rest), do: decode_spacing_xy(rest)
  defp decode_value(:align_x, rest), do: decode_align_x(rest)
  defp decode_value(:align_y, rest), do: decode_align_y(rest)
  defp decode_value(:scrollbar_y, rest), do: decode_bool(rest)
  defp decode_value(:scrollbar_x, rest), do: decode_bool(rest)
  defp decode_value(:background, rest), do: decode_background(rest)
  defp decode_value(:border_radius, rest), do: decode_radius(rest)
  defp decode_value(:border_width, rest), do: decode_border_width(rest)
  defp decode_value(:border_color, rest), do: decode_color(rest)
  defp decode_value(:font_size, rest), do: decode_f64(rest)
  defp decode_value(:font_color, rest), do: decode_color(rest)
  defp decode_value(:font, rest), do: decode_font(rest)
  defp decode_value(:font_weight, rest), do: decode_atom(rest)
  defp decode_value(:font_style, rest), do: decode_atom(rest)
  defp decode_value(:content, rest), do: decode_string(rest)
  defp decode_value(:snap_layout, rest), do: decode_bool(rest)
  defp decode_value(:snap_text_metrics, rest), do: decode_bool(rest)
  defp decode_value(:text_align, rest), do: decode_text_align(rest)
  defp decode_value(:move_x, rest), do: decode_f64(rest)
  defp decode_value(:move_y, rest), do: decode_f64(rest)
  defp decode_value(:rotate, rest), do: decode_f64(rest)
  defp decode_value(:scale, rest), do: decode_f64(rest)
  defp decode_value(:alpha, rest), do: decode_f64(rest)
  defp decode_value(:space_evenly, rest), do: decode_bool(rest)
  defp decode_value(:scroll_x, rest), do: decode_f64(rest)
  defp decode_value(:scroll_y, rest), do: decode_f64(rest)
  defp decode_value(:on_click, rest), do: decode_bool(rest)
  defp decode_value(:on_press, rest), do: decode_bool(rest)
  defp decode_value(:on_mouse_down, rest), do: decode_bool(rest)
  defp decode_value(:on_mouse_up, rest), do: decode_bool(rest)
  defp decode_value(:on_mouse_enter, rest), do: decode_bool(rest)
  defp decode_value(:on_mouse_leave, rest), do: decode_bool(rest)
  defp decode_value(:on_mouse_move, rest), do: decode_bool(rest)
  defp decode_value(:mouse_over, rest), do: decode_mouse_over(rest)
  defp decode_value(:focused, rest), do: decode_focused(rest)
  defp decode_value(:mouse_down, rest), do: decode_mouse_down_style(rest)
  defp decode_value(:font_underline, rest), do: decode_bool(rest)
  defp decode_value(:font_strike, rest), do: decode_bool(rest)
  defp decode_value(:font_letter_spacing, rest), do: decode_f64(rest)
  defp decode_value(:font_word_spacing, rest), do: decode_f64(rest)
  defp decode_value(:border_style, rest), do: decode_border_style(rest)
  defp decode_value(:box_shadow, rest), do: decode_box_shadow(rest)
  defp decode_value(:image_src, rest), do: decode_image_src(rest)
  defp decode_value(:image_fit, rest), do: decode_image_fit(rest)
  defp decode_value(:image_size, rest), do: decode_image_size(rest)
  defp decode_value(:svg_color, rest), do: decode_color(rest)
  defp decode_value(:svg_expected, rest), do: decode_bool(rest)
  defp decode_value(:video_target, rest), do: decode_string(rest)
  defp decode_value(:animate, rest), do: decode_animation(rest, :animate)
  defp decode_value(:animate_enter, rest), do: decode_animation(rest, :animate_enter)
  defp decode_value(:animate_exit, rest), do: decode_animation(rest, :animate_exit)
  defp decode_value(:on_change, rest), do: decode_bool(rest)
  defp decode_value(:on_focus, rest), do: decode_bool(rest)
  defp decode_value(:on_blur, rest), do: decode_bool(rest)

  defp encode_mouse_over(value), do: encode_state_style(value, :mouse_over)

  defp encode_focused(value), do: encode_state_style(value, :focused)

  defp encode_mouse_down_style(value), do: encode_state_style(value, :mouse_down)

  defp encode_state_style(value, style_key) do
    encoded = style_key |> AttrValidation.normalize_state_style!(value) |> encode_attrs()
    <<byte_size(encoded)::unsigned-32, encoded::binary>>
  end

  defp decode_mouse_over(rest), do: decode_state_style(rest)

  defp decode_focused(rest), do: decode_state_style(rest)

  defp decode_mouse_down_style(rest), do: decode_state_style(rest)

  defp decode_state_style(<<len::unsigned-32, rest::binary>>) do
    <<attrs_bin::binary-size(len), rest::binary>> = rest
    {decode_attrs(attrs_bin), rest}
  end

  defp encode_animation(value, owner) do
    spec = AttrValidation.normalize_animation!(owner, value)

    keyframes =
      spec.keyframes
      |> Enum.map(fn keyframe ->
        encoded = encode_attrs(keyframe)
        <<byte_size(encoded)::unsigned-32, encoded::binary>>
      end)

    payload = [
      <<length(spec.keyframes)::unsigned-16>>,
      keyframes,
      encode_f64(spec.duration),
      encode_atom(spec.curve),
      encode_animation_repeat(spec.repeat)
    ]

    payload = IO.iodata_to_binary(payload)
    <<byte_size(payload)::unsigned-32, payload::binary>>
  end

  defp decode_animation(<<len::unsigned-32, rest::binary>>, owner) do
    <<payload::binary-size(len), rest::binary>> = rest
    {value, <<>>} = decode_animation_payload(payload, owner)
    {value, rest}
  end

  defp decode_animation_payload(<<count::unsigned-16, rest::binary>>, owner) do
    {keyframes, rest} = decode_animation_keyframes(rest, count, [])
    {duration, rest} = decode_f64(rest)
    {curve, rest} = decode_atom(rest)
    {repeat, rest} = decode_animation_repeat(rest)

    value =
      AttrValidation.normalize_animation!(owner, %{
        keyframes: keyframes,
        duration: duration,
        curve: curve,
        repeat: repeat
      })

    {value, rest}
  end

  defp decode_animation_keyframes(rest, 0, acc), do: {Enum.reverse(acc), rest}

  defp decode_animation_keyframes(<<len::unsigned-32, rest::binary>>, count, acc) do
    <<attrs_bin::binary-size(len), rest::binary>> = rest
    decode_animation_keyframes(rest, count - 1, [decode_attrs(attrs_bin) | acc])
  end

  defp encode_animation_repeat(:once), do: <<0>>
  defp encode_animation_repeat(:loop), do: <<1>>
  defp encode_animation_repeat({:times, count}), do: <<2, count::unsigned-32>>

  defp decode_animation_repeat(<<0, rest::binary>>), do: {:once, rest}
  defp decode_animation_repeat(<<1, rest::binary>>), do: {:loop, rest}

  defp decode_animation_repeat(<<2, count::unsigned-32, rest::binary>>),
    do: {{:times, count}, rest}

  defp encode_bool(true), do: <<1>>
  defp encode_bool(false), do: <<0>>
  defp encode_bool(value), do: encode_bool(!!value)

  defp decode_bool(<<1, rest::binary>>), do: {true, rest}
  defp decode_bool(<<0, rest::binary>>), do: {false, rest}

  defp encode_f64(value) when is_integer(value), do: <<value * 1.0::float-64>>
  defp encode_f64(value) when is_float(value), do: <<value::float-64>>

  defp decode_f64(<<value::float-64, rest::binary>>), do: {value, rest}

  defp encode_spacing_xy({x, y}) do
    <<encode_f64(x)::binary, encode_f64(y)::binary>>
  end

  defp decode_spacing_xy(rest) do
    {x, rest} = decode_f64(rest)
    {y, rest} = decode_f64(rest)
    {{x, y}, rest}
  end

  defp encode_string(value) when is_binary(value) do
    <<byte_size(value)::unsigned-16, value::binary>>
  end

  defp decode_string(<<len::unsigned-16, rest::binary>>) do
    <<value::binary-size(len), rest::binary>> = rest
    {value, rest}
  end

  defp encode_atom(value) when is_atom(value) do
    encoded = Atom.to_string(value)
    <<byte_size(encoded)::unsigned-16, encoded::binary>>
  end

  defp decode_atom(<<len::unsigned-16, rest::binary>>) do
    <<value::binary-size(len), rest::binary>> = rest
    {String.to_atom(value), rest}
  end

  defp encode_length(:fill), do: <<0>>
  defp encode_length(:content), do: <<1>>
  defp encode_length({:px, value}), do: <<2, encode_f64(value)::binary>>
  defp encode_length({:fill, value}), do: <<3, encode_f64(value)::binary>>

  defp encode_length({:minimum, min_px, inner}),
    do: <<4, encode_f64(min_px)::binary, encode_length(inner)::binary>>

  defp encode_length({:maximum, max_px, inner}),
    do: <<5, encode_f64(max_px)::binary, encode_length(inner)::binary>>

  defp decode_length(<<0, rest::binary>>), do: {:fill, rest}
  defp decode_length(<<1, rest::binary>>), do: {:content, rest}

  defp decode_length(<<2, rest::binary>>) do
    {value, rest} = decode_f64(rest)
    {{:px, value}, rest}
  end

  defp decode_length(<<3, rest::binary>>) do
    {value, rest} = decode_f64(rest)
    {{:fill, value}, rest}
  end

  defp decode_length(<<4, rest::binary>>) do
    {min_px, rest} = decode_f64(rest)
    {inner, rest} = decode_length(rest)
    {{:minimum, min_px, inner}, rest}
  end

  defp decode_length(<<5, rest::binary>>) do
    {max_px, rest} = decode_f64(rest)
    {inner, rest} = decode_length(rest)
    {{:maximum, max_px, inner}, rest}
  end

  defp encode_padding(value) when is_number(value) do
    <<0, encode_f64(value)::binary>>
  end

  defp encode_padding({top, right, bottom, left}) do
    <<1, encode_f64(top)::binary, encode_f64(right)::binary, encode_f64(bottom)::binary,
      encode_f64(left)::binary>>
  end

  defp encode_padding({vertical, horizontal}) do
    <<1, encode_f64(vertical)::binary, encode_f64(horizontal)::binary,
      encode_f64(vertical)::binary, encode_f64(horizontal)::binary>>
  end

  defp encode_padding(%{top: top, right: right, bottom: bottom, left: left}) do
    <<2, encode_f64(top)::binary, encode_f64(right)::binary, encode_f64(bottom)::binary,
      encode_f64(left)::binary>>
  end

  defp decode_padding(<<0, rest::binary>>) do
    {value, rest} = decode_f64(rest)
    {value, rest}
  end

  defp decode_padding(<<1, rest::binary>>) do
    {top, rest} = decode_f64(rest)
    {right, rest} = decode_f64(rest)
    {bottom, rest} = decode_f64(rest)
    {left, rest} = decode_f64(rest)
    {{top, right, bottom, left}, rest}
  end

  defp decode_padding(<<2, rest::binary>>) do
    {top, rest} = decode_f64(rest)
    {right, rest} = decode_f64(rest)
    {bottom, rest} = decode_f64(rest)
    {left, rest} = decode_f64(rest)
    {%{top: top, right: right, bottom: bottom, left: left}, rest}
  end

  defp encode_radius(value) when is_number(value) do
    <<0, encode_f64(value)::binary>>
  end

  defp encode_radius({tl, tr, br, bl}) do
    <<1, encode_f64(tl)::binary, encode_f64(tr)::binary, encode_f64(br)::binary,
      encode_f64(bl)::binary>>
  end

  defp decode_radius(<<0, rest::binary>>) do
    {value, rest} = decode_f64(rest)
    {value, rest}
  end

  defp decode_radius(<<1, rest::binary>>) do
    {tl, rest} = decode_f64(rest)
    {tr, rest} = decode_f64(rest)
    {br, rest} = decode_f64(rest)
    {bl, rest} = decode_f64(rest)
    {{tl, tr, br, bl}, rest}
  end

  defp encode_border_width(value) when is_number(value) do
    <<0, encode_f64(value)::binary>>
  end

  defp encode_border_width({top, right, bottom, left}) do
    <<1, encode_f64(top)::binary, encode_f64(right)::binary, encode_f64(bottom)::binary,
      encode_f64(left)::binary>>
  end

  defp decode_border_width(<<0, rest::binary>>) do
    decode_f64(rest)
  end

  defp decode_border_width(<<1, rest::binary>>) do
    {top, rest} = decode_f64(rest)
    {right, rest} = decode_f64(rest)
    {bottom, rest} = decode_f64(rest)
    {left, rest} = decode_f64(rest)
    {{top, right, bottom, left}, rest}
  end

  defp encode_border_style(:solid), do: <<0>>
  defp encode_border_style(:dashed), do: <<1>>
  defp encode_border_style(:dotted), do: <<2>>

  defp decode_border_style(<<0, rest::binary>>), do: {:solid, rest}
  defp decode_border_style(<<1, rest::binary>>), do: {:dashed, rest}
  defp decode_border_style(<<2, rest::binary>>), do: {:dotted, rest}

  defp encode_box_shadow(shadows) do
    shadows = AttrValidation.normalize_decorative_value!("box_shadow", :box_shadow, shadows)
    count = length(shadows)

    encoded =
      Enum.map(shadows, fn %{
                             offset_x: ox,
                             offset_y: oy,
                             size: size,
                             blur: blur,
                             color: color,
                             inset: inset
                           } ->
        <<encode_f64(ox)::binary, encode_f64(oy)::binary, encode_f64(blur)::binary,
          encode_f64(size)::binary, encode_color(color)::binary, encode_bool(inset)::binary>>
      end)

    [<<count::unsigned-8>> | encoded] |> IO.iodata_to_binary()
  end

  defp decode_box_shadow(<<count::unsigned-8, rest::binary>>) do
    decode_box_shadow_items(rest, count, [])
  end

  defp decode_box_shadow_items(rest, 0, acc), do: {Enum.reverse(acc), rest}

  defp decode_box_shadow_items(rest, count, acc) do
    {ox, rest} = decode_f64(rest)
    {oy, rest} = decode_f64(rest)
    {blur, rest} = decode_f64(rest)
    {size, rest} = decode_f64(rest)
    {color, rest} = decode_color(rest)
    {inset, rest} = decode_bool(rest)

    shadow = %{offset_x: ox, offset_y: oy, blur: blur, size: size, color: color, inset: inset}
    decode_box_shadow_items(rest, count - 1, [shadow | acc])
  end

  defp encode_image_src(source), do: encode_image_source(source)

  defp decode_image_src(rest), do: decode_image_source(rest)

  defp encode_image_fit(:contain), do: <<0>>
  defp encode_image_fit(:cover), do: <<1>>
  defp encode_image_fit(:repeat), do: <<2>>
  defp encode_image_fit(:repeat_x), do: <<3>>
  defp encode_image_fit(:repeat_y), do: <<4>>
  defp encode_image_fit(_other), do: <<0>>

  defp decode_image_fit(<<0, rest::binary>>), do: {:contain, rest}
  defp decode_image_fit(<<1, rest::binary>>), do: {:cover, rest}
  defp decode_image_fit(<<2, rest::binary>>), do: {:repeat, rest}
  defp decode_image_fit(<<3, rest::binary>>), do: {:repeat_x, rest}
  defp decode_image_fit(<<4, rest::binary>>), do: {:repeat_y, rest}

  defp encode_image_size({w, h}) when is_number(w) and is_number(h) do
    <<encode_f64(w)::binary, encode_f64(h)::binary>>
  end

  defp decode_image_size(rest) do
    {w, rest} = decode_f64(rest)
    {h, rest} = decode_f64(rest)
    {{w, h}, rest}
  end

  defp encode_align_x(:left), do: <<0>>
  defp encode_align_x(:center), do: <<1>>
  defp encode_align_x(:right), do: <<2>>

  defp decode_align_x(<<0, rest::binary>>), do: {:left, rest}
  defp decode_align_x(<<1, rest::binary>>), do: {:center, rest}
  defp decode_align_x(<<2, rest::binary>>), do: {:right, rest}

  defp encode_align_y(:top), do: <<0>>
  defp encode_align_y(:center), do: <<1>>
  defp encode_align_y(:bottom), do: <<2>>

  defp decode_align_y(<<0, rest::binary>>), do: {:top, rest}
  defp decode_align_y(<<1, rest::binary>>), do: {:center, rest}
  defp decode_align_y(<<2, rest::binary>>), do: {:bottom, rest}

  defp encode_text_align(:left), do: <<0>>
  defp encode_text_align(:center), do: <<1>>
  defp encode_text_align(:right), do: <<2>>

  defp decode_text_align(<<0, rest::binary>>), do: {:left, rest}
  defp decode_text_align(<<1, rest::binary>>), do: {:center, rest}
  defp decode_text_align(<<2, rest::binary>>), do: {:right, rest}

  defp encode_color({:color_rgb, {r, g, b}}),
    do: <<0, r::unsigned-8, g::unsigned-8, b::unsigned-8>>

  defp encode_color({:color_rgba, {r, g, b, a}}),
    do: <<1, r::unsigned-8, g::unsigned-8, b::unsigned-8, a::unsigned-8>>

  defp encode_color(color) when is_atom(color), do: <<2, encode_atom(color)::binary>>

  defp decode_color(<<0, r::unsigned-8, g::unsigned-8, b::unsigned-8, rest::binary>>),
    do: {{:color_rgb, {r, g, b}}, rest}

  defp decode_color(
         <<1, r::unsigned-8, g::unsigned-8, b::unsigned-8, a::unsigned-8, rest::binary>>
       ),
       do: {{:color_rgba, {r, g, b, a}}, rest}

  defp decode_color(<<2, rest::binary>>) do
    {atom, rest} = decode_atom(rest)
    {atom, rest}
  end

  defp encode_background({:gradient, from, to, angle}) do
    <<1, encode_color(from)::binary, encode_color(to)::binary, encode_f64(angle)::binary>>
  end

  defp encode_background({:image, source, fit}) do
    <<2, encode_image_source(source)::binary, encode_image_fit(fit)::binary>>
  end

  defp encode_background({:image, source}) do
    encode_background({:image, source, :cover})
  end

  defp encode_background(color) do
    <<0, encode_color(color)::binary>>
  end

  defp decode_background(<<0, rest::binary>>) do
    {color, rest} = decode_color(rest)
    {color, rest}
  end

  defp decode_background(<<1, rest::binary>>) do
    {from, rest} = decode_color(rest)
    {to, rest} = decode_color(rest)
    {angle, rest} = decode_f64(rest)
    {{:gradient, from, to, angle}, rest}
  end

  defp decode_background(<<2, rest::binary>>) do
    {source, rest} = decode_image_source(rest)
    {fit, rest} = decode_image_fit(rest)
    {{:image, source, fit}, rest}
  end

  defp encode_image_source({:id, id}) when is_binary(id), do: <<0, encode_string(id)::binary>>

  defp encode_image_source(%Emerge.Assets.Ref{path: path}) when is_binary(path),
    do: <<1, encode_string(path)::binary>>

  defp encode_image_source({:path, path}) when is_binary(path),
    do: <<2, encode_string(path)::binary>>

  defp encode_image_source(path) when is_binary(path),
    do: <<1, encode_string(path)::binary>>

  defp encode_image_source(path) when is_atom(path),
    do: <<1, encode_string(Atom.to_string(path))::binary>>

  defp encode_image_source(other) do
    raise ArgumentError,
          "unsupported image source #{inspect(other)} (expected binary/atom/~m/{:id,id}/{:path,path})"
  end

  defp decode_image_source(<<0, rest::binary>>) do
    {id, rest} = decode_string(rest)
    {{:id, id}, rest}
  end

  defp decode_image_source(<<1, rest::binary>>) do
    decode_string(rest)
  end

  defp decode_image_source(<<2, rest::binary>>) do
    {path, rest} = decode_string(rest)
    {{:path, path}, rest}
  end

  defp decode_image_source(<<variant, _rest::binary>>) do
    raise ArgumentError, "unknown image source variant: #{variant}"
  end

  defp encode_font(value) when is_atom(value), do: <<0, encode_atom(value)::binary>>
  defp encode_font(value) when is_binary(value), do: <<1, encode_string(value)::binary>>

  defp decode_font(<<0, rest::binary>>) do
    {atom, rest} = decode_atom(rest)
    {atom, rest}
  end

  defp decode_font(<<1, rest::binary>>) do
    {value, rest} = decode_string(rest)
    {value, rest}
  end
end
