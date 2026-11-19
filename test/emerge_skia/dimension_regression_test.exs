defmodule EmergeSkia.DimensionRegressionTest do
  use ExUnit.Case, async: true
  use Emerge.UI
  alias Emerge.TestSupport.Headless

  defp click(r, {x, y}, who, {lx, ly}) do
    for phase <- [:press, :release] do
      assert :ok = Headless.button(r, phase, x, y)
      assert_receive {:raw, {:pointer_button, {:left, ^phase, [], {^x, ^y}}}}
    end

    assert_receive {:application, {^who, %{x: ^lx, y: ^ly}}}
  end

  test "one width expression changes slope, saturates, then grows again" do
    view = fn w ->
      a = max(min(fill(3), px(30)), fill())

      make = fn key, l, color ->
        el(
          [key(key), width(l), height(px(12)), Background.color(color), Event.on_click(key)],
          none()
        )
      end

      el(
        [width(px(120)), height(px(12)), Background.color(:white)],
        row([width(px(w))], [make.(:a, a, :red), make.(:b, fill(), :blue)])
      )
    end

    {pid, r} = Headless.mount({120, 12}, fn -> view.(20) end, [:pointer_button])

    cases = [
      {20, 15, 5, :b, 2.0},
      {50, 30, 20, :a, 17.0},
      {120, 60, 60, :a, 17.0},
      {20, 15, 5, :b, 2.0}
    ]

    for {{w, a, b, who, local}, i} <- Enum.with_index(cases) do
      if i > 0, do: send(pid, {:scene, fn -> view.(w) end})

      line =
        :binary.copy(<<255, 0, 0, 255>>, a) <>
          :binary.copy(<<0, 0, 255, 255>>, b) <> :binary.copy(<<255, 255, 255, 255>>, 120 - w)

      assert Headless.pixels() == :binary.copy(line, 12)
      click(r, {17.0, 5.0}, who, {local, 5.0})
    end
  end

  test "a mixed height reads the padded wrapped lines before assigning the sibling's height" do
    view = fn w, l ->
      chip = fn -> el([width(px(46)), height(px(20))], none()) end

      a =
        wrapped_row(
          [
            key(:a),
            width(fill()),
            height(l),
            padding(2),
            spacing(4),
            Background.color(:red),
            Event.on_click(:a)
          ],
          [chip.(), chip.()]
        )

      b =
        el(
          [key(:b), width(fill()), height(fill()), Background.color(:blue), Event.on_click(:b)],
          none()
        )

      el(
        [width(px(100)), height(px(120)), Background.color(:white)],
        column([width(px(w)), height(px(120)), spacing(4), Background.color(:black)], [a, b])
      )
    end

    cap = min(shrink(), fill())
    {pid, r} = Headless.mount({100, 120}, fn -> view.(100, cap) end, [:pointer_button])
    # Inner widths 96 and 76; 46 + 4 + 46 fits only in the first. Natural
    # outer heights are 24 and 48. The Column has 116px after its own gap.
    cases = [
      {100, cap, 24, 92, :b, 14.0},
      {80, cap, 48, 68, :a, 42.0},
      {80, max(content(), fill(3)), 87, 29, :a, 42.0},
      {100, cap, 24, 92, :b, 14.0}
    ]

    for {{w, l, a, b, who, local}, i} <- Enum.with_index(cases) do
      if i > 0, do: send(pid, {:scene, fn -> view.(w, l) end})

      expected =
        for {pixel, h} <- [
              {<<255, 0, 0, 255>>, a},
              {<<0, 0, 0, 255>>, 4},
              {<<0, 0, 255, 255>>, b}
            ],
            into: <<>> do
          :binary.copy(:binary.copy(pixel, w) <> :binary.copy(<<255, 255, 255, 255>>, 100 - w), h)
        end

      assert Headless.pixels() == expected
      click(r, {5.0, 42.0}, who, {5.0, local})
    end
  end
end
