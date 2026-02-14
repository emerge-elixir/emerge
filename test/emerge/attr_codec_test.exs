defmodule Emerge.AttrCodecTest do
  use ExUnit.Case, async: true

  import Emerge.UI

  alias Emerge.AttrCodec

  test "encode/decode round trip for supported attrs" do
    attrs = %{
      width: {:px, 120},
      height: :fill,
      padding: {1, 2, 3, 4},
      spacing: 8,
      align_x: :center,
      align_y: :bottom,
      scrollbar_y: true,
      scrollbar_x: false,
      clip_y: false,
      clip_x: true,
      background: {:gradient, {:color_rgb, {10, 20, 30}}, {:color_rgb, {40, 50, 60}}, 45},
      border_radius: {2, 3, 4, 5},
      border_width: 1,
      border_color: {:color_rgba, {1, 2, 3, 255}},
      font_size: 14,
      font_color: :white,
      font: :roboto,
      font_weight: :bold,
      font_style: :italic,
      font_underline: true,
      font_strike: true,
      font_letter_spacing: 1.25,
      font_word_spacing: 2.5,
      content: "Hello",
      above: el(text("above")),
      below: el(text("below")),
      on_left: el(text("left")),
      on_right: el(text("right")),
      in_front: el(text("front")),
      behind: el(text("behind")),
      snap_layout: true,
      snap_text_metrics: true,
      move_x: 12.5,
      move_y: -8.0,
      rotate: 45.0,
      scale: 1.25,
      alpha: 0.5,
      border_style: :dashed,
      box_shadow: [%{offset_x: 2, offset_y: 3, blur: 8, size: 4, color: :red, inset: false}]
    }

    encoded = AttrCodec.encode_attrs(attrs)
    decoded = AttrCodec.decode_attrs(encoded)

    assert normalize_attrs(decoded) == normalize_attrs(attrs)
  end

  test "runtime attrs are stripped from encoding" do
    attrs = %{width: :fill, scroll_max: 10, scroll_bounds: {0, 0, 0, 0}}
    decoded = attrs |> AttrCodec.encode_attrs() |> AttrCodec.decode_attrs()

    assert decoded == %{width: :fill}
  end

  test "encode/decode transform attrs" do
    attrs = %{move_x: 12.5, move_y: -4.0, rotate: 15.0, scale: 1.2, alpha: 0.0}
    decoded = attrs |> AttrCodec.encode_attrs() |> AttrCodec.decode_attrs()

    assert normalize_attrs(decoded) == normalize_attrs(attrs)
  end

  test "encode/decode length variants" do
    attrs = %{
      width: {:minimum, 80, :content},
      height: {:maximum, 120, {:fill_portion, 2}}
    }

    decoded = attrs |> AttrCodec.encode_attrs() |> AttrCodec.decode_attrs()

    assert normalize_attrs(decoded) == normalize_attrs(attrs)
  end

  test "encode/decode spacing attributes" do
    attrs = %{spacing_xy: {12, 24}, space_evenly: true}
    decoded = attrs |> AttrCodec.encode_attrs() |> AttrCodec.decode_attrs()

    assert normalize_attrs(decoded) == normalize_attrs(attrs)
  end

  test "encode/decode on_click presence" do
    attrs = %{on_click: {self(), :clicked}}
    decoded = attrs |> AttrCodec.encode_attrs() |> AttrCodec.decode_attrs()

    assert normalize_attrs(decoded) == %{on_click: true}
  end

  test "encode/decode mouse event presence" do
    attrs = %{
      on_mouse_down: {self(), :down},
      on_mouse_up: {self(), :up},
      on_mouse_enter: {self(), :enter},
      on_mouse_leave: {self(), :leave},
      on_mouse_move: {self(), :move}
    }

    decoded = attrs |> AttrCodec.encode_attrs() |> AttrCodec.decode_attrs()

    assert normalize_attrs(decoded) == %{
             on_mouse_down: true,
             on_mouse_up: true,
             on_mouse_enter: true,
             on_mouse_leave: true,
             on_mouse_move: true
           }
  end

  test "encode/decode mouse_over decorative attrs" do
    attrs = %{
      mouse_over: %{
        background: {:color_rgb, {20, 30, 40}},
        border_color: {:color_rgba, {10, 20, 30, 255}},
        font_color: :white,
        font_size: 22,
        font_underline: true,
        font_strike: true,
        font_letter_spacing: 1.5,
        font_word_spacing: 2.25,
        move_x: 5,
        move_y: -2,
        rotate: 12,
        scale: 1.1,
        alpha: 0.75
      }
    }

    decoded = attrs |> AttrCodec.encode_attrs() |> AttrCodec.decode_attrs()

    assert normalize_attrs(decoded) == normalize_attrs(attrs)
  end

  # ============================================
  # Per-edge border_width round-trip
  # ============================================

  test "encode/decode per-edge border_width" do
    attrs = %{border_width: {2, 4, 6, 8}}
    decoded = attrs |> AttrCodec.encode_attrs() |> AttrCodec.decode_attrs()

    assert decoded.border_width == {2.0, 4.0, 6.0, 8.0}
  end

  # ============================================
  # border_style round-trip
  # ============================================

  test "encode/decode border_style dashed" do
    attrs = %{border_style: :dashed}
    decoded = attrs |> AttrCodec.encode_attrs() |> AttrCodec.decode_attrs()

    assert decoded.border_style == :dashed
  end

  test "encode/decode border_style dotted" do
    attrs = %{border_style: :dotted}
    decoded = attrs |> AttrCodec.encode_attrs() |> AttrCodec.decode_attrs()

    assert decoded.border_style == :dotted
  end

  test "encode/decode border_style solid" do
    attrs = %{border_style: :solid}
    decoded = attrs |> AttrCodec.encode_attrs() |> AttrCodec.decode_attrs()

    assert decoded.border_style == :solid
  end

  # ============================================
  # box_shadow round-trip
  # ============================================

  test "encode/decode single box_shadow" do
    shadow = %{offset_x: 2, offset_y: 3, blur: 8, size: 4, color: :red, inset: false}
    attrs = %{box_shadow: [shadow]}
    decoded = attrs |> AttrCodec.encode_attrs() |> AttrCodec.decode_attrs()

    assert [decoded_shadow] = decoded.box_shadow
    assert decoded_shadow.offset_x == 2.0
    assert decoded_shadow.offset_y == 3.0
    assert decoded_shadow.blur == 8.0
    assert decoded_shadow.size == 4.0
    assert decoded_shadow.color == :red
    assert decoded_shadow.inset == false
  end

  test "encode/decode multiple box_shadows preserves order" do
    shadow1 = %{offset_x: 1, offset_y: 1, blur: 4, size: 0, color: :black, inset: false}
    shadow2 = %{offset_x: 0, offset_y: 0, blur: 10, size: 5, color: :blue, inset: true}
    attrs = %{box_shadow: [shadow1, shadow2]}
    decoded = attrs |> AttrCodec.encode_attrs() |> AttrCodec.decode_attrs()

    assert [d1, d2] = decoded.box_shadow
    assert d1.color == :black
    assert d1.inset == false
    assert d2.color == :blue
    assert d2.inset == true
  end

  test "mouse_over rejects non-decorative attrs" do
    assert_raise ArgumentError, ~r/mouse_over only supports decorative attributes/, fn ->
      AttrCodec.encode_attrs(%{mouse_over: %{width: :fill}})
    end
  end

  test "mouse_over rejects nested mouse_over" do
    assert_raise ArgumentError, ~r/mouse_over does not support nested mouse_over/, fn ->
      AttrCodec.encode_attrs(%{mouse_over: %{mouse_over: %{alpha: 0.5}}})
    end
  end

  defp normalize_attrs(attrs) do
    attrs
    |> Emerge.Tree.strip_runtime_attrs()
    |> Enum.map(fn {key, value} -> {key, normalize_value(value)} end)
    |> Map.new()
  end

  defp normalize_value(value) when is_number(value), do: value * 1.0

  defp normalize_value(%Emerge.Element{} = element) do
    %{
      element
      | id: nil,
        attrs: normalize_attrs(element.attrs),
        children: Enum.map(element.children, &normalize_value/1)
    }
  end

  defp normalize_value(value) when is_map(value) do
    value
    |> Enum.map(fn {key, val} -> {key, normalize_value(val)} end)
    |> Map.new()
  end

  defp normalize_value(value) when is_list(value), do: Enum.map(value, &normalize_value/1)

  defp normalize_value({a, b, c, d}),
    do: {normalize_value(a), normalize_value(b), normalize_value(c), normalize_value(d)}

  defp normalize_value({a, b, c}),
    do: {normalize_value(a), normalize_value(b), normalize_value(c)}

  defp normalize_value({a, b}), do: {normalize_value(a), normalize_value(b)}

  defp normalize_value(value), do: value
end
