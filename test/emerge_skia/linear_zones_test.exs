defmodule EmergeSkia.LinearZonesTest do
  use ExUnit.Case, async: true
  use Emerge.UI
  alias Emerge.TestSupport.Headless

  # Each card is {key, main extent, cross extent, main alignment}.
  defp axes(kind, m, c) when kind in [:column, :wrapped_column], do: {c, m}
  defp axes(_, m, c), do: {m, c}
  defp zone(_, :start), do: []
  defp zone(k, :end) when k in [:column, :wrapped_column], do: [align_bottom()]
  defp zone(_, :end), do: [align_right()]
  defp zone(k, :center) when k in [:column, :wrapped_column], do: [center_y()]
  defp zone(_, :center), do: [center_x()]

  defp color_of(:a), do: 0xFF0000FF
  defp color_of(:b), do: 0x0000FFFF
  defp color_of(:c), do: 0x00FF00FF

  defp scene(kind, limit, pad, {gm, gc}, cards, callback \\ :current) do
    {w, h} = axes(kind, limit, 100)
    {gx, gy} = axes(kind, gm, gc)

    children =
      Enum.map(cards, fn {id, m, c, z} ->
        {cw, ch} = axes(kind, m, c)
        <<r, g, b, 255>> = <<color_of(id)::32>>

        el(
          [
            key(id),
            width(px(cw)),
            height(px(ch)),
            Background.color(color_rgb(r, g, b)),
            Event.on_click({callback, id}) | zone(kind, z)
          ],
          none()
        )
      end)

    apply(Emerge.UI, kind, [
      [
        width(px(w)),
        height(px(h)),
        padding(pad),
        spacing_xy(gx, gy),
        Background.color(:white),
        Event.on_click({callback, :host})
      ],
      children
    ])
  end

  # Literal half-open rectangles in declaration order; last painted wins.
  defp frame(kind, limit, boxes) do
    for y <- 0..99, x <- 0..99, into: <<>> do
      {m, c} = axes(kind, x, y)

      color =
        if m < limit do
          Enum.reduce(boxes, 0xFFFFFFFF, fn {id, at, across, w, h}, col ->
            if m >= at and m < at + w and c >= across and c < across + h,
              do: color_of(id),
              else: col
          end)
        else
          0
        end

      <<color::32>>
    end
  end

  defp button(r, phase, x, y) do
    assert :ok = Headless.button(r, phase, x, y)
    assert_receive {:raw, {:pointer_button, {:left, ^phase, [], {^x, ^y}}}}
  end

  defp click(r, x, y, {tag, id}, {lx, ly}) do
    button(r, :press, x, y)
    button(r, :release, x, y)
    assert_receive {:application, {^tag, ^id, %{x: ^lx, y: ^ly}}}
  end

  for kind <- [:row, :column] do
    @kind kind
    test "#{kind} end group publishes its origin and routed click only with the frame" do
      k = @kind
      build = fn z -> scene(k, 100, 4, {4, 4}, [{:a, 20, 20, :start}, {:b, 20, 20, z}]) end
      expected = fn b -> frame(k, 100, [{:a, 4, 4, 20, 20}, {:b, b, 4, 20, 20}]) end

      {p, r} =
        Headless.mount({100, 100}, fn -> build.(:start) end, [:pointer_button], target_fps: 1)

      first = expected.(28)
      assert Headless.pixels() == first

      send(
        p,
        {:scene,
         fn ->
           send(self(), :built)
           build.(:end)
         end}
      )

      assert_receive {:application, :built}
      assert {:ok, ^first} = EmergeSkia.render_to_pixels(r)
      {x, y} = axes(k, 80.0, 10.0)
      click(r, x, y, {:current, :host}, {x, y})
      assert Headless.pixels(2500) == expected.(76)
      click(r, x, y, {:current, :b}, axes(k, 4.0, 6.0))
      Headless.mount({100, 100}, fn -> build.(:end) end)
      assert Headless.pixels() == expected.(76)
      send(p, {:scene, fn -> build.(:start) end})
      assert Headless.pixels(2500) == first
    end
  end

  for kind <- [:row, :column] do
    @kind kind
    test "#{kind} moves its centered card when only the leading card changes size" do
      k = @kind

      build = fn n ->
        scene(k, 100, 4, {4, 2}, [{:a, n, 16, :start}, {:b, 12, 12, :center}, {:c, 20, 16, :end}])
      end

      {p, r} = Headless.mount({100, 100}, fn -> build.(36) end, [:pointer_button])

      for {{n, b}, i} <- Enum.with_index([{36, 52}, {20, 44}, {36, 52}]) do
        if i > 0, do: send(p, {:scene, fn -> build.(n) end})
        expect = frame(k, 100, [{:a, 4, 4, n, 16}, {:b, b, 4, 12, 12}, {:c, 76, 4, 20, 16}])
        assert Headless.pixels() == expect
        {x, y} = axes(k, 54.0, 8.0)
        click(r, x, y, {:current, :b}, axes(k, 54.0 - b, 4.0))
        Headless.mount({100, 100}, fn -> build.(n) end)
        assert Headless.pixels() == expect
      end
    end
  end

  for kind <- [:wrapped_row, :wrapped_column] do
    @kind kind
    test "#{kind} centered singleton uses its own group and publishes the new local route" do
      k = @kind

      build = fn size ->
        scene(k, size, 0, {4, 2}, [{:a, 48, 8, :start}, {:b, 20, 8, :center}])
      end

      {p, r} = Headless.mount({100, 100}, fn -> build.(72) end, [:pointer_button], target_fps: 1)
      together = frame(k, 72, [{:a, 0, 0, 48, 8}, {:b, 50, 0, 20, 8}])
      assert Headless.pixels() == together

      send(
        p,
        {:scene,
         fn ->
           send(self(), :changed)
           build.(60)
         end}
      )

      assert_receive {:application, :changed}
      {x, y} = axes(k, 53.0, 3.0)
      assert {:ok, ^together} = EmergeSkia.render_to_pixels(r)
      click(r, x, y, {:current, :b}, axes(k, 3.0, 3.0))
      separate = frame(k, 60, [{:a, 0, 0, 48, 8}, {:b, 20, 10, 20, 8}])
      assert Headless.pixels(2500) == separate
      click(r, x, y, {:current, :host}, {x, y})
      {tx, ty} = axes(k, 22.0, 13.0)
      click(r, tx, ty, {:current, :b}, axes(k, 2.0, 3.0))
      Headless.mount({100, 100}, fn -> build.(60) end)
      assert Headless.pixels() == separate
      send(p, {:scene, fn -> build.(72) end})
      assert Headless.pixels(2500) == together
    end

    test "#{kind} trailing group keeps measured breaks and uses the padded main extent" do
      k = @kind
      build = fn limit -> scene(k, limit, 0, {4, 2}, [{:a, 48, 8, :start}, {:b, 20, 8, :end}]) end
      {p, r} = Headless.mount({100, 100}, fn -> build.(72) end, [:pointer_button])

      for {{limit, b, line}, i} <-
            Enum.with_index([{72, 52, 0}, {71, 51, 10}, {60, 40, 10}, {72, 52, 0}]) do
        if i > 0, do: send(p, {:scene, fn -> build.(limit) end})
        expected = frame(k, limit, [{:a, 0, 0, 48, 8}, {:b, b, line, 20, 8}])
        assert Headless.pixels() == expected
        {x, y} = axes(k, b + 2.0, line + 3.0)
        click(r, x, y, {:current, :b}, axes(k, 2.0, 3.0))
        Headless.mount({100, 100}, fn -> build.(limit) end)
        assert Headless.pixels() == expected
      end
    end
  end

  test "overlapping zones retain declaration order and a held keyed press across reorder" do
    cards = [{:a, 20, 20, :end}, {:b, 20, 20, :start}]

    {p, r} =
      Headless.mount({100, 100}, fn -> scene(:row, 30, 0, {0, 0}, cards) end, [:pointer_button])

    boxes = [{:a, 10, 0, 20, 20}, {:b, 0, 0, 20, 20}]
    assert Headless.pixels() == frame(:row, 30, boxes)
    click(r, 15.0, 6.0, {:current, :b}, {15.0, 6.0})
    button(r, :press, 15.0, 6.0)
    send(p, {:scene, fn -> scene(:row, 30, 0, {0, 0}, Enum.reverse(cards), :reordered) end})
    assert Headless.pixels() == frame(:row, 30, Enum.reverse(boxes))
    button(r, :release, 5.0, 6.0)
    assert_receive {:application, {:reordered, :b, %{x: 5.0, y: 6.0}}}
    click(r, 15.0, 6.0, {:reordered, :a}, {5.0, 6.0})
  end
end
