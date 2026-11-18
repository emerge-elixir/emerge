defmodule EmergeSkia.RetainedRowTest do
  use ExUnit.Case, async: true
  use Emerge.UI
  alias Emerge.TestSupport.Headless

  defp scene(w, order) do
    row(
      [width(px(120)), padding(5), spacing(5)],
      Enum.map(order, fn
        :a ->
          el([key(:a), width(px(w)), height(px(20)), Background.color(:red)], none())

        :b ->
          el(
            [key(:b), width(fill()), height(px(20)), Background.color(:blue), Event.on_click(:b)],
            none()
          )

        :c ->
          el(
            [
              key(:c),
              width(fill()),
              height(px(20)),
              Background.color(:white),
              Event.on_click(:c)
            ],
            none()
          )
      end)
    )
  end

  defp pixels(bands) do
    for y <- 0..29, x <- 0..119, into: <<>> do
      case y in 5..24 && Enum.find(bands, fn {range, _} -> x in range end) do
        {_, :a} -> <<255, 0, 0, 255>>
        {_, :b} -> <<0, 0, 255, 255>>
        {_, :c} -> <<255, 255, 255, 255>>
        _ -> <<0, 0, 0, 0>>
      end
    end
  end

  defp probe(r, x, expected) do
    pos = {x * 1.0, 10.0}

    for event <- [
          {:pointer_move, pos},
          {:pointer_button, {:left, :press, [], pos}},
          {:pointer_button, {:left, :release, [], pos}}
        ] do
      assert :ok = EmergeSkia.send_input(r, event)
      assert_receive {:raw, ^event}
    end

    if expected do
      assert_receive {:application, ^expected}
    else
      refute_receive {:application, _}
    end
  end

  test "a fractional row clips an overflowing child's paint and clicks without wrapping it" do
    scene = fn width ->
      row([width(px(width))], [
        el(
          [width(px(40)), height(px(20)), Background.color(:red), Event.on_click(:child)],
          none()
        )
      ])
    end

    {pid, r} =
      Headless.mount({48, 20}, fn -> scene.(20.5) end, [:pointer_move, :pointer_button])

    stripe = fn n ->
      line = :binary.copy(<<255, 0, 0, 255>>, n) <> :binary.copy(<<0, 0, 0, 0>>, 48 - n)
      :binary.copy(line, 20)
    end

    assert Headless.pixels() == stripe.(21)
    probe(r, 20.5, {:child, %{x: 20.5, y: 10.0}})
    probe(r, 21, nil)
    send(pid, {:scene, fn -> scene.(35) end})
    assert Headless.pixels() == stripe.(35)
    probe(r, 30, {:child, %{x: 30.0, y: 10.0}})
    send(pid, {:scene, fn -> scene.(20.5) end})
    assert Headless.pixels() == stripe.(21)
    probe(r, 30, nil)
  end

  test "a fixed sibling changes fill widths, painted order and click owners on the same row" do
    order = [:a, :b, :c]

    {pid, r} =
      Headless.mount({120, 30}, fn -> scene(20, order) end, [:pointer_move, :pointer_button])

    wide = [{5..24, :a}, {30..69, :b}, {75..114, :c}]
    assert Headless.pixels() == pixels(wide)
    probe(r, 80, {:c, %{x: 5.0, y: 5.0}})
    probe(r, 50, {:b, %{x: 20.0, y: 5.0}})

    send(pid, {:scene, fn -> scene(60, order) end})
    assert Headless.pixels() == pixels([{5..64, :a}, {70..89, :b}, {95..114, :c}])
    probe(r, 80, {:b, %{x: 10.0, y: 5.0}})
    probe(r, 50, nil)

    send(pid, {:scene, fn -> scene(20, order) end})
    assert Headless.pixels() == pixels(wide)
    probe(r, 80, {:c, %{x: 5.0, y: 5.0}})
    send(pid, {:scene, fn -> scene(20, [:a, :c, :b]) end})
    assert Headless.pixels() == pixels([{5..24, :a}, {30..69, :c}, {75..114, :b}])
    probe(r, 80, {:b, %{x: 5.0, y: 5.0}})
  end
end
