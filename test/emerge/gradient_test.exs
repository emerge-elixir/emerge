defmodule Emerge.GradientTest do
  use ExUnit.Case, async: true
  use Emerge.UI

  alias Emerge.Engine.{AttrCodec, Patch, Serialization}

  test "constructs ordered gradients without resolving named atoms or changing alpha" do
    colors = [:red, color(:sky, 200), color_rgba(1, 2, 3, 0.5), :red]
    assert gradient(colors) == {:color_gradient, colors, 0}
    assert gradient(colors, -450) == {:color_gradient, colors, -450}
    assert Background.color(gradient(colors)) == {:background, {:color_gradient, colors, 0}}
    assert Code.ensure_loaded?(Background)
    refute function_exported?(Background, :gradient, 2)
    refute function_exported?(Background, :gradient, 3)
  end

  test "rejects malformed stops and angles at construction and direct encoding" do
    bad_colors = [
      [],
      [:red],
      :red,
      [:red, :blue | :bad],
      [:red, gradient([:black, :white])],
      [:red, {:color_rgb, {256, 0, 0}}],
      [:red, {:color_rgb, {0, 0.5, 0}}],
      [:red, {:color_rgba, {0, 0, 0, -1}}],
      [:red, {:color_rgba, {0, 0, 0, 0.5}}],
      [:red, "blue"]
    ]

    for colors <- bad_colors do
      assert_raise ArgumentError, fn -> gradient(colors) end

      assert_raise ArgumentError, fn ->
        AttrCodec.encode_attrs(%{background: {:color_gradient, colors, 0}})
      end
    end

    for angle <- [:bad, nil, "90", Integer.pow(10, 400)] do
      assert_raise ArgumentError, fn -> gradient([:red, :blue], angle) end

      assert_raise ArgumentError, fn ->
        AttrCodec.encode_attrs(%{background: {:color_gradient, [:red, :blue], angle}})
      end
    end
  end

  test "old raw gradient tuples are rejected, not normalized" do
    old = {:gradient, :black, :white, 0}
    assert_raise ArgumentError, fn -> el([Background.color(old)], none()) end
    assert_raise ArgumentError, fn -> AttrCodec.encode_attrs(%{background: old}) end
    assert_raise ArgumentError, fn -> AttrCodec.decode_attrs(<<0, 1, 12, 1>>) end
  end

  test "gradient wire format is count then ordered solid colors then f64 angle" do
    value = gradient([color_rgb(1, 2, 3), color_rgba(4, 5, 6, 0.5), :red], 90)

    bytes =
      <<0, 1, 12, 0, 3, 3::32, 0, 1, 2, 3, 1, 4, 5, 6, 128, 2, 3::16, "red", 90.0::float-64>>

    assert AttrCodec.encode_attrs(%{background: value}) == bytes
    assert AttrCodec.decode_attrs(bytes) == %{background: value}
  end

  test "stop counts are not truncated to 16 bits" do
    colors = List.duplicate(color_rgb(10, 20, 30), 65_536)
    attrs = %{background: gradient(colors, 1.0e300)}
    encoded = AttrCodec.encode_attrs(attrs)
    assert <<0, 1, 12, 0, 3, 65_536::32, _::binary>> = encoded
    assert AttrCodec.decode_attrs(encoded) == attrs
  end

  test "rejects impossible counts, truncated colors, and nonfinite angles" do
    for count <- [0, 1, 0xFFFFFFFF] do
      assert_raise ArgumentError, fn ->
        AttrCodec.decode_attrs(<<0, 1, 12, 0, 3, count::32, 0::64>>)
      end
    end

    for angle_bits <- [0x7FF0000000000000, 0xFFF0000000000000, 0x7FF8000000000000] do
      assert_raise ArgumentError, fn ->
        AttrCodec.decode_attrs(<<0, 1, 12, 0, 3, 2::32, 0, 1, 2, 3, 0, 4, 5, 6, angle_bits::64>>)
      end
    end

    assert_raise ArgumentError, fn ->
      AttrCodec.decode_attrs(<<0, 1, 12, 0, 3, 2::32, 99, 0::128>>)
    end
  end

  test "every color consumer works directly and in state styles" do
    for stops <- [[:red, :blue], [:red, :green, :blue], List.duplicate(:white, 4)] do
      value = gradient(stops)

      attrs = [
        Font.color(value),
        Border.color(value),
        Background.color(value),
        Border.shadow(color: value),
        Border.inner_shadow(color: value),
        Border.glow(value, 2)
      ]

      for style <- [
            &Function.identity/1,
            &Interactive.mouse_over/1,
            &Interactive.focused/1,
            &Interactive.mouse_down/1
          ] do
        element = el(List.wrap(style.(attrs)), text("gradient"))
        {encoded, _} = Serialization.encode(element)
        assert %Emerge.Engine.Element{} = Serialization.decode(encoded)
        svg = svg(List.wrap(style.([Svg.color(value)])), "sample_assets/tile_quad.svg")
        {encoded, _} = Serialization.encode(svg)
        assert %Emerge.Engine.Element{} = Serialization.decode(encoded)
      end
    end
  end

  test "real svg constructor validates color instead of bypassing it" do
    value = gradient([color_rgba(255, 255, 255, 0), color_rgba(255, 255, 255, 0)])
    assert svg([Svg.color(value)], "sample_assets/tile_quad.svg").attrs.svg_color == value

    for bad <- [
          {:color_gradient, [:red], 0},
          {:color_gradient, [:red, :blue], :bad},
          {:color_rgb, {999, 0, 0}}
        ] do
      assert_raise ArgumentError, fn -> svg([Svg.color(bad)], "sample_assets/tile_quad.svg") end

      for key <- [:background, :font_color, :border_color, :svg_color] do
        assert_raise ArgumentError, fn -> AttrCodec.encode_attrs(%{key => bad}) end
      end
    end
  end

  test "all animation owners require matching gradient stop counts and variants" do
    first = Background.color(gradient([:red, :green, :blue]))
    last = Background.color(gradient([:black, :white, :red], 360))

    for animate <- [&Animation.animate/3, &Animation.animate_enter/3, &Animation.animate_exit/3] do
      attrs =
        el([animate.([[first], [last]], 200, :linear)], none()).attrs
        |> Emerge.Engine.Tree.Attrs.strip_runtime_attrs()

      assert attrs |> AttrCodec.encode_attrs() |> AttrCodec.decode_attrs() == attrs

      for bad <- [Background.color(gradient([:red, :blue]))] do
        assert_raise ArgumentError, ~r/compatible variant/, fn ->
          el([animate.([[first], [bad]], 200, :linear)], none())
        end
      end
    end
  end

  test "shared color animation compatibility includes solid lifting and each shadow" do
    a = gradient([:red, :green, :blue])
    b = gradient([:white, :black, :red], 90)
    bad = gradient([:red, :blue])

    for color <- [
          &Background.color/1,
          &Font.color/1,
          &Border.color/1,
          &Svg.color/1,
          fn c -> Border.shadow(color: c) end,
          fn c -> Border.inner_shadow(color: c) end
        ],
        animate <- [&Animation.animate/3, &Animation.animate_enter/3, &Animation.animate_exit/3] do
      for pair <- [[a, b], [:red, a], [a, :blue]] do
        attr = animate.(Enum.map(pair, fn c -> [color.(c)] end), 200, :linear)

        attrs =
          svg([attr], "test_assets/gradient_mask.svg").attrs
          |> Emerge.Engine.Tree.Attrs.strip_runtime_attrs()

        assert attrs |> AttrCodec.encode_attrs() |> AttrCodec.decode_attrs() == attrs
      end

      for frames <- [
            [[color.(a)], [color.(bad)]],
            [[color.(:red)], [color.(a)], [color.(:blue)], [color.(bad)]]
          ] do
        assert_raise ArgumentError, fn ->
          svg([animate.(frames, 200, :linear)], "test_assets/gradient_mask.svg")
        end
      end
    end
  end

  test "full trees and inserted subtrees use v9, while attr patches carry the new value" do
    tree = el([Background.color(gradient([:red, :green, :blue]))], none())
    {encoded, assigned} = Serialization.encode(tree)
    assert <<"EMRG", 9, _::binary>> = encoded
    assert Serialization.decode(encoded).attrs.background == assigned.attrs.background
    <<"EMRG", 9, body::binary>> = encoded
    assert_raise ArgumentError, fn -> Serialization.decode(<<"EMRG", 7, body::binary>>) end

    patches = [
      {:set_attrs, assigned.id, %{background: gradient([:white, :black])}},
      {:insert_subtree, nil, 0, assigned}
    ]

    decoded = patches |> Patch.encode() |> Patch.decode()

    assert [
             {:set_attrs, _, %{background: {:color_gradient, [:white, :black], +0.0}}},
             {:insert_subtree, nil, 0, inserted}
           ] = decoded

    assert inserted.attrs.background == assigned.attrs.background
  end
end
