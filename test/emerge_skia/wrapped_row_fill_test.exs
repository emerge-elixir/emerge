defmodule EmergeSkia.WrappedRowFillTest do
  use ExUnit.Case, async: true
  use Emerge.UI
  alias Emerge.TestSupport.Headless

  defp chip(w), do: el([width(px(w)), height(px(12))], none())

  defp card(id, size, color, demand, attrs \\ []) do
    el(
      [
        key(id),
        width(size),
        height(content()),
        Background.color(color),
        Event.on_click(id) | attrs
      ],
      chip(demand)
    )
  end

  defp nested_card(policy, children) do
    wrapped_row(
      [
        key(:a),
        width(policy),
        height(content()),
        spacing(4),
        Background.color(:red),
        Event.on_click(:a)
      ],
      children
    )
  end

  defp with_peer(w, a, b) do
    wrapped_row(
      [width(px(w)), height(px(40)), spacing(4), Background.color(:white), Event.on_click(:host)],
      [a, card(:b, b, :blue, 20)]
    )
  end

  defp cards(w, b, a \\ fill(), a_attrs \\ []) do
    with_peer(w, card(:a, a, :red, 20, a_attrs), b)
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

  test "a content-only edit moves the peer's pixels and clicks on the same submission" do
    build = fn n -> with_peer(100, card(:a, min(content(), fill()), :red, n), fill()) end
    {p, r} = Headless.mount({100, 40}, fn -> build.(20) end, [:pointer_button], target_fps: 1)
    before = pixels(100, 20, 24, 0, 76)
    assert Headless.pixels() == before

    send(
      p,
      {:scene,
       fn ->
         send(self(), :built)
         build.(40)
       end}
    )

    assert_receive {:application, :built}
    assert {:ok, ^before} = EmergeSkia.render_to_pixels(r)
    click(r, 32.0, 6.0, :b, 8.0, 6.0)
    after_edit = pixels(100, 40, 44, 0, 56)
    assert Headless.pixels(2500) == after_edit
    click(r, 32.0, 6.0, :a, 32.0, 6.0)
    {_, cold} = Headless.mount({100, 40}, fn -> build.(40) end, [:pointer_button])
    assert Headless.pixels() == after_edit
    click(cold, 32.0, 6.0, :a, 32.0, 6.0)
  end

  test "a Row and one wrapped line split the same constraints alike" do
    for host <- [&row/2, &wrapped_row/2] do
      build = fn size ->
        body = el([width(max(px(0), fill()))], chip(size))
        a = nested_card(min(content(), fill()), [body])

        host.([width(px(100)), height(px(40)), spacing(4), Background.color(:white)], [
          a,
          card(:b, fill(), :blue, 0)
        ])
      end

      {p, r} = Headless.mount({100, 40}, fn -> build.(20) end, [:pointer_button])
      assert Headless.pixels() == pixels(100, 20, 24, 0, 76)
      click(r, 32.0, 6.0, :b, 8.0, 6.0)
      send(p, {:scene, fn -> build.(40) end})
      assert Headless.pixels() == pixels(100, 40, 44, 0, 56)
      click(r, 32.0, 6.0, :a, 32.0, 6.0)
      {_, cold} = Headless.mount({100, 40}, fn -> build.(40) end, [:pointer_button])
      assert Headless.pixels() == pixels(100, 40, 44, 0, 56)
      click(cold, 32.0, 6.0, :a, 32.0, 6.0)
    end
  end

  test "a content floor changes the break before sharing either line" do
    build = fn w, n, floor -> with_peer(w, nested_card(floor, [chip(20), chip(n)]), fill()) end
    floor = max(content(), fill())

    {p, r} =
      Headless.mount({100, 40}, fn -> build.(67, 19, floor) end, [:pointer_button], target_fps: 1)

    before = pixels(67, 43, 47, 0, 20)
    assert Headless.pixels() == before

    send(
      p,
      {:scene,
       fn ->
         send(self(), :built)
         build.(67, 20, floor)
       end}
    )

    assert_receive {:application, :built}
    assert {:ok, ^before} = EmergeSkia.render_to_pixels(r)
    click(r, 50.0, 6.0, :b, 3.0, 6.0)
    assert Headless.pixels(2500) == pixels(67, 67, 0, 16, 67)
    click(r, 50.0, 6.0, :a, 50.0, 6.0)

    for {w, n, policy, aw, bx, by, bw} <- [
          {67, 19, floor, 43, 47, 0, 20},
          {68, 20, floor, 44, 48, 0, 20},
          {100, 20, max(fill(3), shrink()), 72, 76, 0, 24},
          {67, 20, floor, 67, 0, 16, 67}
        ] do
      scene = fn -> build.(w, n, policy) end
      send(p, {:scene, scene})
      expected = pixels(w, aw, bx, by, bw)
      assert Headless.pixels(2500) == expected
      {_, cold} = Headless.mount({100, 40}, scene, [:pointer_button])
      assert Headless.pixels() == expected
      click(cold, bx + 3.0, by + 6.0, :b, 3.0, 6.0)
    end
  end

  test "a nested cap on a content floor moves the peer only on submission" do
    build = fn n ->
      a = nested_card(min(px(50), max(content(), fill())), [chip(20), chip(n)])
      with_peer(64, a, fill())
    end

    {p, r} = Headless.mount({100, 40}, fn -> build.(16) end, [:pointer_button], target_fps: 1)
    before = pixels(64, 40, 44, 0, 20)
    assert Headless.pixels() == before

    send(
      p,
      {:scene,
       fn ->
         send(self(), :built)
         build.(17)
       end}
    )

    assert_receive {:application, :built}
    assert {:ok, ^before} = EmergeSkia.render_to_pixels(r)
    click(r, 46.0, 6.0, :b, 2.0, 6.0)

    wrapped = pixels(64, 50, 0, 16, 64)
    assert Headless.pixels(2500) == wrapped
    click(r, 46.0, 6.0, :a, 46.0, 6.0)
    {_, cold} = Headless.mount({100, 40}, fn -> build.(17) end, [:pointer_button])
    assert Headless.pixels() == wrapped
    click(cold, 46.0, 6.0, :a, 46.0, 6.0)

    send(p, {:scene, fn -> build.(16) end})
    assert Headless.pixels(2500) == before
    click(r, 46.0, 6.0, :b, 2.0, 6.0)
  end

  test "a maximum inside a fill-free operand does not replace the automatic minimum" do
    cap = min(max(shrink(), px(10)), fill(3))
    floor = max(px(10), min(fill(3), shrink()))
    build = fn policy -> with_peer(34, nested_card(policy, [chip(20), chip(17)]), fill()) end
    {p, r} = Headless.mount({100, 40}, fn -> build.(cap) end, [:pointer_button])
    assert Headless.pixels() == pixels(34, 34, 0, 32, 34, 28)
    click(r, 16.0, 6.0, :a, 16.0, 6.0)
    send(p, {:scene, fn -> build.(floor) end})
    assert Headless.pixels() == pixels(34, 10, 14, 0, 20, 28)
    click(r, 16.0, 6.0, :b, 2.0, 6.0)
  end

  test "nested pixel and content bounds share the child's padded outer units at scale" do
    build = fn w ->
      a = card(:a, min(px(30), max(content(), fill(3))), :red, 20, [padding_xy(2, 0), scale(1.5)])
      with_peer(w, a, fill())
    end

    {p, r} = Headless.mount({100, 40}, fn -> build.(60) end, [:pointer_button])
    assert Headless.pixels() == pixels(60, 36, 40, 0, 20, 18)
    click(r, 42.0, 6.0, :b, 2.0, 6.0)
    send(p, {:scene, fn -> build.(59) end})
    assert Headless.pixels() == pixels(59, 45, 0, 22, 59, 18)
    click(r, 42.0, 6.0, :a, 28.0, 4.0)
  end

  test "a nested card breaks at its minimum, but caps its share at its preferred width" do
    build = fn w, chip2, cap, peer ->
      with_peer(w, nested_card(cap, [chip(20), chip(chip2)]), peer)
    end

    cap = min(content(), fill())
    {p, r} = Headless.mount({100, 40}, fn -> build.(68, 20, cap, px(20)) end, [:pointer_button])
    assert Headless.pixels() == pixels(68, 44, 48, 0, 20)

    for {w, chip2, policy, peer, aw, ah, bx, by, bw} <- [
          {67, 20, cap, px(20), 43, 28, 47, 0, 20},
          {44, 20, cap, px(20), 20, 28, 24, 0, 20},
          {43, 20, cap, px(20), 43, 28, 0, 32, 20},
          {68, 20, cap, fill(), 32, 28, 36, 0, 32},
          {68, 20, min(fill(3), shrink()), fill(), 44, 12, 48, 0, 20},
          {100, 20, cap, fill(), 44, 12, 48, 0, 52},
          {100, 10, cap, fill(), 34, 12, 38, 0, 62},
          {100, 20, cap, fill(), 44, 12, 48, 0, 52}
        ] do
      scene = fn -> build.(w, chip2, policy, peer) end
      send(p, {:scene, scene})
      expected = pixels(w, aw, bx, by, bw, ah)
      assert Headless.pixels() == expected
      click(r, bx + 3.0, by + 5.0, :b, 3.0, 5.0)
      {_, cold} = Headless.mount({100, 40}, scene, [:pointer_button])
      assert Headless.pixels() == expected
      click(cold, bx + 3.0, by + 5.0, :b, 3.0, 5.0)
    end
  end

  test "a content cap includes the child's own scaled padding exactly once" do
    cap = min(fill(3), shrink())

    scene = fn n ->
      with_peer(100, card(:a, cap, :red, n, [padding_xy(2, 0), scale(1.5)]), fill())
    end

    {p, r} = Headless.mount({100, 40}, fn -> scene.(0) end, [:pointer_button])
    assert Headless.pixels() == pixels(100, 6, 10, 0, 90, 18)
    send(p, {:scene, fn -> scene.(10) end})
    assert Headless.pixels() == pixels(100, 21, 25, 0, 75, 18)
    click(r, 12.0, 6.0, :a, 8.0, 4.0)
    click(r, 27.0, 6.0, :b, 2.0, 6.0)
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
