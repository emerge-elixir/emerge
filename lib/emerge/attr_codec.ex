defmodule Emerge.AttrCodec do
  @moduledoc """
  Compact encoding for element attribute maps.
  """

  alias Emerge.Element

  @type_tag %{
    width: 1,
    height: 2,
    padding: 3,
    spacing: 4,
    align_x: 5,
    align_y: 6,
    scrollbar_y: 7,
    scrollbar_x: 8,
    clip: 9,
    clip_y: 10,
    clip_x: 11,
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
    above: 22,
    below: 23,
    on_left: 24,
    on_right: 25,
    in_front: 26,
    behind: 27,
    snap_layout: 28,
    snap_text_metrics: 29,
    text_align: 30,
    move_x: 31,
    move_y: 32,
    rotate: 33,
    scale: 34,
    alpha: 35
  }

  @tag_type Map.new(@type_tag, fn {type, tag} -> {tag, type} end)

  @spec encode_attrs(map()) :: binary()
  def encode_attrs(attrs) when is_map(attrs) do
    attrs
    |> Emerge.Tree.strip_runtime_attrs()
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
  defp encode_value(:align_x, value), do: encode_align_x(value)
  defp encode_value(:align_y, value), do: encode_align_y(value)
  defp encode_value(:scrollbar_y, value), do: encode_bool(value)
  defp encode_value(:scrollbar_x, value), do: encode_bool(value)
  defp encode_value(:clip, value), do: encode_bool(value)
  defp encode_value(:clip_y, value), do: encode_bool(value)
  defp encode_value(:clip_x, value), do: encode_bool(value)
  defp encode_value(:background, value), do: encode_background(value)
  defp encode_value(:border_radius, value), do: encode_radius(value)
  defp encode_value(:border_width, value), do: encode_f64(value)
  defp encode_value(:border_color, value), do: encode_color(value)
  defp encode_value(:font_size, value), do: encode_f64(value)
  defp encode_value(:font_color, value), do: encode_color(value)
  defp encode_value(:font, value), do: encode_font(value)
  defp encode_value(:font_weight, value), do: encode_atom(value)
  defp encode_value(:font_style, value), do: encode_atom(value)
  defp encode_value(:content, value), do: encode_string(value)
  defp encode_value(:above, value), do: encode_element(value)
  defp encode_value(:below, value), do: encode_element(value)
  defp encode_value(:on_left, value), do: encode_element(value)
  defp encode_value(:on_right, value), do: encode_element(value)
  defp encode_value(:in_front, value), do: encode_element(value)
  defp encode_value(:behind, value), do: encode_element(value)
  defp encode_value(:snap_layout, value), do: encode_bool(value)
  defp encode_value(:snap_text_metrics, value), do: encode_bool(value)
  defp encode_value(:text_align, value), do: encode_text_align(value)
  defp encode_value(:move_x, value), do: encode_f64(value)
  defp encode_value(:move_y, value), do: encode_f64(value)
  defp encode_value(:rotate, value), do: encode_f64(value)
  defp encode_value(:scale, value), do: encode_f64(value)
  defp encode_value(:alpha, value), do: encode_f64(value)

  defp decode_value(:width, rest), do: decode_length(rest)
  defp decode_value(:height, rest), do: decode_length(rest)
  defp decode_value(:padding, rest), do: decode_padding(rest)
  defp decode_value(:spacing, rest), do: decode_f64(rest)
  defp decode_value(:align_x, rest), do: decode_align_x(rest)
  defp decode_value(:align_y, rest), do: decode_align_y(rest)
  defp decode_value(:scrollbar_y, rest), do: decode_bool(rest)
  defp decode_value(:scrollbar_x, rest), do: decode_bool(rest)
  defp decode_value(:clip, rest), do: decode_bool(rest)
  defp decode_value(:clip_y, rest), do: decode_bool(rest)
  defp decode_value(:clip_x, rest), do: decode_bool(rest)
  defp decode_value(:background, rest), do: decode_background(rest)
  defp decode_value(:border_radius, rest), do: decode_radius(rest)
  defp decode_value(:border_width, rest), do: decode_f64(rest)
  defp decode_value(:border_color, rest), do: decode_color(rest)
  defp decode_value(:font_size, rest), do: decode_f64(rest)
  defp decode_value(:font_color, rest), do: decode_color(rest)
  defp decode_value(:font, rest), do: decode_font(rest)
  defp decode_value(:font_weight, rest), do: decode_atom(rest)
  defp decode_value(:font_style, rest), do: decode_atom(rest)
  defp decode_value(:content, rest), do: decode_string(rest)
  defp decode_value(:above, rest), do: decode_element(rest)
  defp decode_value(:below, rest), do: decode_element(rest)
  defp decode_value(:on_left, rest), do: decode_element(rest)
  defp decode_value(:on_right, rest), do: decode_element(rest)
  defp decode_value(:in_front, rest), do: decode_element(rest)
  defp decode_value(:behind, rest), do: decode_element(rest)
  defp decode_value(:snap_layout, rest), do: decode_bool(rest)
  defp decode_value(:snap_text_metrics, rest), do: decode_bool(rest)
  defp decode_value(:text_align, rest), do: decode_text_align(rest)
  defp decode_value(:move_x, rest), do: decode_f64(rest)
  defp decode_value(:move_y, rest), do: decode_f64(rest)
  defp decode_value(:rotate, rest), do: decode_f64(rest)
  defp decode_value(:scale, rest), do: decode_f64(rest)
  defp decode_value(:alpha, rest), do: decode_f64(rest)

  defp encode_bool(true), do: <<1>>
  defp encode_bool(false), do: <<0>>
  defp encode_bool(value), do: encode_bool(!!value)

  defp decode_bool(<<1, rest::binary>>), do: {true, rest}
  defp decode_bool(<<0, rest::binary>>), do: {false, rest}

  defp encode_f64(value) when is_integer(value), do: <<value * 1.0::float-64>>
  defp encode_f64(value) when is_float(value), do: <<value::float-64>>

  defp decode_f64(<<value::float-64, rest::binary>>), do: {value, rest}

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
  defp encode_length({:fill_portion, value}), do: <<3, encode_f64(value)::binary>>
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
    {{:fill_portion, value}, rest}
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

  defp encode_element(%Element{} = element) do
    assigned =
      if is_nil(element.id) do
        {_vdom, assigned} = Emerge.Reconcile.assign_ids(element)
        assigned
      else
        element
      end

    encoded = Emerge.Serialization.encode_tree(assigned)
    <<byte_size(encoded)::unsigned-32, encoded::binary>>
  end

  defp decode_element(<<len::unsigned-32, rest::binary>>) do
    <<encoded::binary-size(len), rest::binary>> = rest
    {Emerge.Serialization.decode(encoded), rest}
  end
end
