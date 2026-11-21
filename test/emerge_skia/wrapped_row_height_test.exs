defmodule EmergeSkia.WrappedRowHeightTest do
  use ExUnit.Case, async: true
  use Emerge.UI
  alias Emerge.TestSupport.Headless

  defp card(id, color, policy, demand) do
    attrs = [key(id), width(px(20)), height(policy), Background.color(color), Event.on_click(id)]

    case demand do
      {:chips, second} ->
        chips = Enum.map([10, second], fn w -> el([width(px(w)), height(px(8))], none()) end)
        wrapped_row([spacing(2) | attrs], chips)

      h ->
        el(attrs, el([height(px(h))], none()))
    end
  end

  defp scene(a, demand \\ 8, host \\ px(64), b \\ px(12), c \\ px(6), w \\ 48) do
    wrapped_row(
      [
        width(px(w)),
        height(host),
        padding(2),
        spacing(4),
        Background.color(:white),
        Event.on_click(:host)
      ],
      [card(:a, :red, a, demand), card(:b, :blue, b, 12), card(:c, color_rgb(0, 255, 0), c, 6)]
    )
  end

  # Literal child boxes, in declaration order. The host clips paint and hits,
  # not the heights used to position the next line.
  defp frame(w, h, boxes) do
    colors = [<<255, 0, 0, 255>>, <<0, 0, 255, 255>>, <<0, 255, 0, 255>>]
    painted = Enum.zip(boxes, colors)

    for y <- 0..79, x <- 0..79, into: <<>> do
      if x < w and y < h do
        Enum.find_value(painted, <<255, 255, 255, 255>>, fn {{cx, cy, ch}, color} ->
          x >= cx and x < cx + 20 and y >= cy and y < cy + ch and color
        end)
      else
        <<0::32>>
      end
    end
  end

  defp click(r, x, y, id, lx, ly) do
    for phase <- [:press, :release] do
      assert :ok = Headless.button(r, phase, x, y)
      assert_receive {:raw, {:pointer_button, {:left, ^phase, [], {^x, ^y}}}}
    end

    assert_receive {:application, {^id, %{x: ^lx, y: ^ly}}}
  end

  test "the next line follows the resolved outer height only when the new frame is submitted" do
    capped = fn n -> scene(min(px(n), fill(3))) end
    {pid, r} = Headless.mount({80, 80}, fn -> capped.(16) end, [:pointer_button], target_fps: 1)
    small = frame(48, 64, [{2, 2, 16}, {26, 2, 12}, {2, 22, 6}])
    large = frame(48, 64, [{2, 2, 32}, {26, 2, 12}, {2, 38, 6}])
    assert Headless.pixels() == small

    send(
      pid,
      {:scene,
       fn ->
         send(self(), :built)
         capped.(32)
       end}
    )

    assert_receive {:application, :built}
    assert {:ok, ^small} = EmergeSkia.render_to_pixels(r)
    click(r, 7.0, 26.0, :c, 5.0, 4.0)
    assert Headless.pixels(2500) == large
    click(r, 7.0, 26.0, :a, 5.0, 24.0)
    click(r, 7.0, 40.0, :c, 5.0, 2.0)
    {_, cold} = Headless.mount({80, 80}, fn -> capped.(32) end, [:pointer_button])
    assert Headless.pixels() == large
    click(cold, 7.0, 26.0, :a, 5.0, 24.0)
    send(pid, {:scene, fn -> capped.(16) end})
    assert Headless.pixels(2500) == small
    click(r, 7.0, 26.0, :c, 5.0, 4.0)
  end

  test "wrapping inside one child changes the natural height offer on the next line" do
    build = fn w -> scene(min(content(), fill(3)), {:chips, w}, content(), px(12), fill()) end
    small = frame(48, 32, [{2, 2, 8}, {26, 2, 12}, {2, 18, 12}])
    large = frame(48, 44, [{2, 2, 18}, {26, 2, 12}, {2, 24, 18}])
    {pid, r} = Headless.mount({80, 80}, fn -> build.(8) end, [:pointer_button])
    assert Headless.pixels() == small
    click(r, 7.0, 19.0, :c, 5.0, 1.0)
    send(pid, {:scene, fn -> build.(9) end})
    assert Headless.pixels() == large
    click(r, 7.0, 19.0, :a, 5.0, 17.0)
    click(r, 7.0, 40.0, :c, 5.0, 16.0)
    Headless.mount({80, 80}, fn -> build.(9) end, [:pointer_button])
    assert Headless.pixels() == large
    send(pid, {:scene, fn -> build.(8) end})
    assert Headless.pixels() == small
  end

  test "all height fills see the same offer; a natural stack does not feed its height back" do
    build = fn -> scene(fill(3), 8, px(64), fill(), fill(0.25), 72) end
    {pid, r} = Headless.mount({80, 80}, build, [:pointer_button])
    assert Headless.pixels() == frame(72, 64, [{2, 2, 60}, {26, 2, 60}, {50, 2, 60}])
    click(r, 51.0, 60.0, :c, 1.0, 58.0)

    for {a, d, host, c, hh, ah, cy, ch, point, target, local} <- [
          {fill(), 8, content(), fill(3), 32, 12, 18, 12, {7, 28}, :c, {5, 10}},
          {fill(), 22, content(), fill(3), 52, 22, 28, 22, {7, 28}, :c, {5, 0}},
          {min(max(fill(2), px(4)), min(shrink(), fill())), 18, px(64), px(6), 64, 18, 24, 6,
           {7, 28}, :c, {5, 4}},
          {max(content(), fill()), 76, px(64), px(6), 64, 76, 82, 6, {7, 63}, :a, {5, 61}},
          {min(px(0), max(fill(3), fill())), 8, px(64), px(6), 64, 0, 18, 6, {7, 15}, :host,
           {7, 15}}
        ] do
      f = fn -> scene(a, d, host, px(12), c) end
      want = frame(48, hh, [{2, 2, ah}, {26, 2, 12}, {2, cy, ch}])
      send(pid, {:scene, f})
      assert Headless.pixels() == want
      {x, y} = point
      {lx, ly} = local
      click(r, x * 1.0, y * 1.0, target, lx * 1.0, ly * 1.0)
      Headless.mount({80, 80}, f, [:pointer_button])
      assert Headless.pixels() == want
    end
  end
end
