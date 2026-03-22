defmodule Emerge.UI.ColorTest do
  use ExUnit.Case, async: true

  doctest Emerge.UI.Color

  alias Emerge.UI.Color

  test "color/1 uses shade 400 by default" do
    assert Color.color(:sky) == {:color_rgb, {0, 188, 255}}
  end

  test "color/2 resolves explicit Tailwind shades" do
    assert Color.color(:rose, 300) == {:color_rgb, {255, 161, 173}}
    assert Color.color(:taupe, 950) == {:color_rgb, {12, 10, 9}}
  end

  test "color/3 returns rgba for translucent alpha" do
    assert Color.color(:sky, 200, 0.3) == {:color_rgba, {184, 230, 254, 77}}
  end

  test "color/3 keeps opaque colors as rgb tuples" do
    assert Color.color(:black, 400, 1.0) == {:color_rgb, {0, 0, 0}}
  end

  test "color_rgb/3 returns rgb tuples" do
    assert Color.color_rgb(12, 34, 56) == {:color_rgb, {12, 34, 56}}
  end

  test "color_rgba/4 always returns rgba tuples" do
    assert Color.color_rgba(12, 34, 56, 1.0) == {:color_rgba, {12, 34, 56, 255}}
    assert Color.color_rgba(12, 34, 56, 0.5) == {:color_rgba, {12, 34, 56, 128}}
  end

  test "flat colors accept default shade only" do
    assert Color.color(:white) == {:color_rgb, {255, 255, 255}}

    assert_raise ArgumentError, ~r/does not support shade 500/, fn ->
      Color.color(:white, 500)
    end
  end

  test "unknown color names raise" do
    assert_raise ArgumentError, ~r/unknown color name :banana/, fn ->
      Color.color(:banana)
    end
  end

  test "unknown shades raise" do
    assert_raise ArgumentError, ~r/unknown shade 425 for :sky/, fn ->
      Color.color(:sky, 425)
    end
  end

  test "non-integer shades raise" do
    assert_raise ArgumentError, ~r/shade must be an integer/, fn ->
      Color.color(:sky, 200.0)
    end
  end

  test "invalid alpha raises" do
    assert_raise ArgumentError, ~r/color\/3 expects alpha to be between 0.0 and 1.0/, fn ->
      Color.color(:sky, 200, 1.2)
    end

    assert_raise ArgumentError, ~r/color_rgba\/4 expects alpha to be between 0.0 and 1.0/, fn ->
      Color.color_rgba(12, 34, 56, -0.1)
    end
  end

  test "invalid rgb channels raise" do
    assert_raise ArgumentError,
                 ~r/color_rgb\/3 expects r to be an integer between 0 and 255/,
                 fn ->
                   Color.color_rgb(256, 0, 0)
                 end

    assert_raise ArgumentError,
                 ~r/color_rgba\/4 expects g to be an integer between 0 and 255/,
                 fn ->
                   Color.color_rgba(0, -1, 0, 0.5)
                 end
  end
end
