defmodule EmergeSkia.WrappedRowFillTest do
  use ExUnit.Case, async: true
  use Emerge.UI
  alias Emerge.TestSupport.Headless

  defp cards(w, b, a \\ fill(), a_attrs \\ []) do
    card = fn id, size, color, attrs ->
      el(
        [
          key(id),
          width(size),
          height(content()),
          Background.color(color),
          Event.on_click(id) | attrs
        ],
        el([width(px(20)), height(px(12))], none())
      )
    end

    wrapped_row(
      [width(px(w)), height(px(40)), spacing(4), Background.color(:white), Event.on_click(:host)],
      [card.(:a, a, :red, a_attrs), card.(:b, b, :blue, [])]
    )
  end

  defp pixels(w, aw, bx, by, bw, ah \\ 12) do
    for y <- 0..39, x <- 0..99, into: <<>> do
      cond do
        x >= w -> <<0::32>>
        x < aw and y < ah -> <<255, 0, 0, 255>>
        x >= bx and x < bx + bw and y >= by and y < by + 12 -> <<0, 0, 255, 255>>
        true -> <<255, 255, 255, 255>>
      end
    end
  end

  defp button(r, phase, x, y) do
    assert :ok = Headless.button(r, phase, x, y)
    assert_receive {:raw, {:pointer_button, {:left, ^phase, [], {^x, ^y}}}}
  end

  defp click(r, x, y, id, lx, ly) do
    for phase <- [:press, :release], do: button(r, phase, x, y)
    assert_receive {:application, {^id, %{x: ^lx, y: ^ly}}}
  end

  test "two plain fills share a wrapped line; an explicit weight is not a full-line width" do
    for {policy, aw, bx, bw} <- [
          {fill(), 48, 52, 48},
          {fill(1), 48, 52, 48},
          {fill(3), 24, 28, 72}
        ] do
      {_, r} = Headless.mount({100, 40}, fn -> cards(100, policy) end, [:pointer_button])
      assert Headless.pixels() == pixels(100, aw, bx, 0, bw)
      click(r, bx + 2.0, 6.0, :b, 2.0, 6.0)
    end
  end

  test "the weight-only edit publishes the new width and click target together" do
    {p, r} =
      Headless.mount({100, 40}, fn -> cards(100, fill()) end, [:pointer_button], target_fps: 1)

    before = pixels(100, 48, 52, 0, 48)
    assert Headless.pixels() == before

    send(
      p,
      {:scene,
       fn ->
         send(self(), :built)
         cards(100, fill(3))
       end}
    )

    assert_receive {:application, :built}
    assert {:ok, ^before} = EmergeSkia.render_to_pixels(r)
    click(r, 50.0, 6.0, :host, 50.0, 6.0)
    assert Headless.pixels(2500) == pixels(100, 24, 28, 0, 72)
    click(r, 50.0, 6.0, :b, 22.0, 6.0)
  end

  test "a pixel cap gives the peer its remainder, but not its clicks until submission" do
    plain = fn -> cards(100, fill()) end
    capped = fn -> cards(100, fill(), min(px(30), fill())) end
    {p, r} = Headless.mount({100, 40}, plain, [:pointer_button], target_fps: 1)
    before = pixels(100, 48, 52, 0, 48)
    assert Headless.pixels() == before

    send(
      p,
      {:scene,
       fn ->
         send(self(), :built)
         capped.()
       end}
    )

    assert_receive {:application, :built}
    assert {:ok, ^before} = EmergeSkia.render_to_pixels(r)
    click(r, 40.0, 6.0, :a, 40.0, 6.0)

    expected = pixels(100, 30, 34, 0, 66)
    assert Headless.pixels(2500) == expected
    click(r, 40.0, 6.0, :b, 6.0, 6.0)
    {_, cold} = Headless.mount({100, 40}, capped, [:pointer_button])
    assert Headless.pixels() == expected
    click(cold, 40.0, 6.0, :b, 6.0, 6.0)
  end

  test "cap below content and explicit floor choose the same breaks for cold and edited cards" do
    {p, r} = Headless.mount({100, 40}, fn -> cards(100, fill()) end, [:pointer_button])
    assert Headless.pixels() == pixels(100, 48, 52, 0, 48)
    cap = min(fill(3), px(10))
    floor = max(px(70), fill(3))

    for {w, a, aw, bx, by, bw} <- [
          {34, cap, 10, 14, 0, 20},
          {33, cap, 10, 0, 16, 33},
          {94, floor, 70, 74, 0, 20},
          {93, floor, 93, 0, 16, 93},
          {24, max(fill(), px(0)), 0, 4, 0, 20},
          {34, min(px(30), max(fill(), px(10))), 10, 14, 0, 20}
        ] do
      build = fn -> cards(w, fill(), a) end
      send(p, {:scene, build})
      expected = pixels(w, aw, bx, by, bw)
      assert Headless.pixels() == expected
      click(r, bx + 3.0, by + 5.0, :b, 3.0, 5.0)
      {_, cold} = Headless.mount({100, 40}, build, [:pointer_button])
      assert Headless.pixels() == expected
      click(cold, bx + 3.0, by + 5.0, :b, 3.0, 5.0)
    end
  end

  test "pixel caps use the child's scale and bound the outer box, including padding" do
    scene = fn w, zoom ->
      cards(w, fill(), min(fill(3), px(10)), [scale(zoom), padding_xy(6, 0)])
    end

    {p, r} = Headless.mount({100, 40}, fn -> scene.(34, 1) end, [:pointer_button])
    assert Headless.pixels() == pixels(34, 10, 14, 0, 20)
    click(r, 12.0, 6.0, :host, 12.0, 6.0)
    send(p, {:scene, fn -> scene.(100, 1.5) end})
    assert Headless.pixels() == pixels(100, 15, 19, 0, 81, 18)
    click(r, 12.0, 6.0, :a, 8.0, 4.0)
    click(r, 21.0, 6.0, :b, 2.0, 6.0)
  end

  test "the minimum decides the break and each new line divides only its own room" do
    {p, r} = Headless.mount({100, 40}, fn -> cards(44, fill(3)) end, [:pointer_button])
    assert Headless.pixels() == pixels(44, 20, 24, 0, 20)
    click(r, 28.0, 6.0, :b, 4.0, 6.0)

    for {w, aw, bx, by, bw} <- [{43, 43, 0, 16, 43}, {100, 24, 28, 0, 72}, {44, 20, 24, 0, 20}] do
      send(p, {:scene, fn -> cards(w, fill(3)) end})
      assert Headless.pixels() == pixels(w, aw, bx, by, bw)
      click(r, bx + 4.0, by + 6.0, :b, 4.0, 6.0)
    end
  end
end
