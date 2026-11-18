defmodule EmergeSkia.LayoutScaleTest do
  use ExUnit.Case, async: true
  use Emerge.UI
  alias Emerge.TestSupport.Headless

  defp button(renderer, phase, x, y) do
    assert :ok = Headless.button(renderer, phase, x, y)
    assert_receive {:raw, {:pointer_button, {:left, ^phase, [], {^x, ^y}}}}
  end

  defp edit(pid, scene) do
    send(
      pid,
      {:scene,
       fn ->
         send(self(), :submitted)
         scene.()
       end}
    )

    assert_receive {:application, :submitted}
  end

  test "a reused fill box changes the local divisor and route only at submission" do
    view = fn s, label ->
      el(
        [
          width(fill()),
          height(px(20 / s)),
          scale(s),
          Background.color(:red),
          Event.on_click(label)
        ],
        none()
      )
    end

    {pid, r} =
      Headless.mount({80, 20}, fn -> view.(1, :old) end, [:pointer_button], target_fps: 2)

    red = :binary.copy(<<255, 0, 0, 255>>, 80 * 20)
    assert Headless.pixels() == red
    edit(pid, fn -> view.(2, :new) end)

    for {phase, label, pos} <- [
          {:pending, :old, %{x: 10.0, y: 8.0}},
          {:installed, :new, %{x: 5.0, y: 4.0}}
        ] do
      if phase == :installed, do: assert(Headless.pixels() == red)
      button(r, :press, 10.0, 8.0)
      button(r, :release, 10.0, 8.0)
      assert_receive {:application, {^label, ^pos}}
    end
  end

  test "fractional host clip, child precedence and remaining fill agree cold and after edits" do
    view = fn s ->
      child =
        el(
          [
            width(px(20)),
            height(px(10)),
            scale(s),
            Background.color(:red),
            Event.on_click(:child)
          ],
          none()
        )

      rest = el([width(fill()), height(px(10)), Background.color(:blue)], none())
      row([width(px(120)), Event.on_click(:row)], [child, rest])
    end

    {pid, r} = Headless.mount({120, 20}, fn -> view.(1) end, [:pointer_button])
    band = fn n, pixel -> :binary.copy(pixel, n) end

    expect = fn n ->
      red = band.(n, <<255, 0, 0, 255>>)
      blue = band.(120 - n, <<0, 0, 255, 255>>)

      :binary.copy(red <> blue, 10) <>
        if(n == 40,
          do: :binary.copy(red <> band.(80, <<0::32>>), 10),
          else: band.(1200, <<0::32>>)
        )
    end

    assert Headless.pixels() == expect.(20)
    edit(pid, fn -> view.(2) end)
    assert Headless.pixels() == expect.(40)

    for {x, target, lx, ly} <- [{39.5, :child, 19.75, 2.5}, {41.0, :row, 41.0, 5.0}] do
      button(r, :press, x, 5.0)
      button(r, :release, x, 5.0)
      assert_receive {:application, {^target, %{x: ^lx, y: ^ly}}}
    end

    {_, _} = Headless.mount({120, 20}, fn -> view.(2) end)
    assert Headless.pixels() == expect.(40)

    clipped = fn ->
      wrapped_row([width(px(16.4)), scale(1.25)], [
        el([width(px(28)), height(px(10)), Background.color(:red), Event.on_click(:clip)], none())
      ])
    end

    edit(pid, clipped)
    assert <<255, 0, 0, 255, 0::32>> = binary_part(Headless.pixels(), (5 * 120 + 20) * 4, 8)
    button(r, :press, 20.5, 5.0)
    button(r, :release, 20.5, 5.0)
    assert_receive {:application, {:clip, %{x: 16.4, y: 4.0}}}
    button(r, :press, 21.0, 5.0)
    button(r, :release, 21.0, 5.0)
    refute_receive {:application, _}
  end

  test "a pressed card moves into a differently scaled lane and releases in its new logical box" do
    view = fn side ->
      card =
        el(
          [
            key(:card),
            width(px(20)),
            height(px(10)),
            Background.color(:red),
            Event.on_click(side)
          ],
          none()
        )

      lanes =
        for {key, s} <- [left: 1, right: 2] do
          column([key(key), scale(s), width(px(60 / s))], if(key == side, do: [card], else: []))
        end

      row([width(px(120))], lanes)
    end

    {pid, r} = Headless.mount({120, 20}, fn -> view.(:left) end, [:pointer_button])
    blank = fn n -> :binary.copy(<<0::32>>, n) end
    red = fn n -> :binary.copy(<<255, 0, 0, 255>>, n) end
    assert Headless.pixels() == :binary.copy(red.(20) <> blank.(100), 10) <> blank.(1200)
    button(r, :press, 10.0, 8.0)
    edit(pid, fn -> view.(:right) end)
    assert Headless.pixels() == :binary.copy(blank.(60) <> red.(40) <> blank.(20), 20)
    button(r, :release, 70.0, 8.0)
    assert_receive {:application, {:right, %{x: 5.0, y: 4.0}}}
    button(r, :press, 10.0, 8.0)
    button(r, :release, 10.0, 8.0)
    refute_receive {:application, _}
  end
end
