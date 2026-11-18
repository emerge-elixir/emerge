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

  test "scaling default-size text delivers the same glyphs and layout as an explicit 16" do
    view = fn attrs ->
      column([Font.color(:white) | attrs], [text("Move me"), text("Moves: 0")])
    end

    {pid, _} = Headless.mount({180, 100}, fn -> view.([]) end)
    unscaled = Headless.pixels()
    refute unscaled == :binary.copy(<<0::32>>, 180 * 100)

    {_, _} = Headless.mount({180, 100}, fn -> view.([scale(1.5), Font.size(16)]) end)
    explicit = Headless.pixels()
    refute explicit == unscaled

    edit(pid, fn -> view.([scale(1.5)]) end)
    assert Headless.pixels() == explicit
    edit(pid, fn -> view.([]) end)
    assert Headless.pixels() == unscaled
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

  test "shrinking a column leaves the lane gap unpainted and routes its clicks to the row" do
    view = fn w ->
      card =
        el(
          [width(px(104)), height(px(36)), Background.color(:red), Event.on_click(:card)],
          none()
        )

      lane = [width(fill()), height(px(36))]

      row([width(px(w)), spacing(8), scale(1.5), Event.on_click(:row)], [
        column(lane, [card]),
        column([Background.color(:blue) | lane], [])
      ])
    end

    {pid, r} = Headless.mount({324, 54}, fn -> view.(216) end, [:pointer_button])

    for {edit_width, lane_px, target} <- [{nil, 156, :card}, {136, 96, :row}, {216, 156, :card}] do
      if edit_width, do: edit(pid, fn -> view.(edit_width) end)

      line =
        :binary.copy(<<255, 0, 0, 255>>, lane_px) <>
          :binary.copy(<<0::32>>, 12) <>
          :binary.copy(<<0, 0, 255, 255>>, lane_px) <>
          :binary.copy(<<0::32>>, 324 - 2 * lane_px - 12)

      assert Headless.pixels() == :binary.copy(line, 54)

      for {x, owner, lx} <- [{90.0, :card, 60.0}, {102.0, target, 68.0}] do
        button(r, :press, x, 30.0)
        button(r, :release, x, 30.0)
        assert_receive {:application, {^owner, %{x: ^lx, y: 20.0}}}
      end
    end
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
