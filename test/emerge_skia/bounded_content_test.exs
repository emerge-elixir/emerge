defmodule EmergeSkia.BoundedContentTest do
  use ExUnit.Case, async: true
  use Emerge.UI
  alias Emerge.TestSupport.Headless

  defp dims(:row, l), do: [width(l), height(px(12))]
  defp dims(:column, l), do: [height(l), width(px(12))]
  defp group(:row, attrs, children), do: row(attrs, children)
  defp group(:column, attrs, children), do: column(attrs, children)
  defp xy(:row, n, cross), do: {n, cross}
  defp xy(:column, n, cross), do: {cross, n}

  defp scene(axis, policy, demand) do
    a =
      el(
        [key(:a), Event.on_click(:a), Background.color(:red) | dims(axis, policy)],
        el(dims(axis, px(demand)), none())
      )

    b = el([key(:b), Event.on_click(:b), Background.color(:blue) | dims(axis, fill())], none())
    group(axis, dims(axis, px(240)), [a, b])
  end

  defp bands(axis, a) do
    {stride, copies} = xy(axis, 1, 12)

    line =
      :binary.copy(<<255, 0, 0, 255>>, a * stride) <>
        :binary.copy(<<0, 0, 255, 255>>, (240 - a) * stride)

    :binary.copy(line, copies)
  end

  defp click(r, axis, point, k, local) do
    {x, y} = xy(axis, point, 5.0)
    {lx, ly} = xy(axis, local, 5.0)

    for phase <- [:press, :release] do
      assert :ok = Headless.button(r, phase, x, y)
      assert_receive {:raw, {:pointer_button, {:left, ^phase, [], {^x, ^y}}}}
    end

    assert_receive {:application, {^k, %{x: ^lx, y: ^ly}}}
  end

  defp submit(pid, build) do
    send(
      pid,
      {:scene,
       fn ->
         send(self(), :submitted)
         build.()
       end}
    )

    assert_receive {:application, :submitted}
  end

  for axis <- [:row, :column] do
    test "#{axis}: a bounded content box charges its own extent and input follows the submitted split" do
      axis = unquote(axis)
      size = xy(axis, 240, 12)
      floor = max(px(140), shrink())

      {pid, r} =
        Headless.mount(size, fn -> scene(axis, floor, 80) end, [:pointer_button], target_fps: 1)

      initial = bands(axis, 140)
      assert Headless.pixels() == initial
      click(r, axis, 160.0, :b, 20.0)

      Enum.reduce(
        [
          {floor, 180, 180, :b, 20.0, :a, 160.0},
          {min(content(), px(100)), 180, 100, :a, 160.0, :b, 60.0},
          {max(px(120), min(shrink(), px(170))), 180, 170, :b, 60.0, :a, 160.0},
          {floor, 80, 140, :a, 160.0, :b, 20.0}
        ],
        initial,
        fn {policy, demand, extent, old, prior, k, local}, installed ->
          build = fn -> scene(axis, policy, demand) end
          submit(pid, build)
          assert {:ok, ^installed} = EmergeSkia.render_to_pixels(r)
          click(r, axis, 160.0, old, prior)
          expected = bands(axis, extent)
          assert Headless.pixels(2500) == expected
          click(r, axis, 160.0, k, local)
          {_, cold} = Headless.mount(size, build, [:pointer_button])
          assert Headless.pixels() == expected
          click(cold, axis, 160.0, k, local)
          expected
        end
      )
    end
  end

  test "a cap applies to the wrapped height before the Column gives B the remainder" do
    scene = fn w, cap ->
      chip = fn ->
        dims = [width(px(48)), height(px(20))]
        el(dims, el(dims, none()))
      end

      a =
        wrapped_row(
          [
            width(fill()),
            height(min(px(cap), content())),
            spacing(4),
            Background.color(:red),
            Event.on_click(:a)
          ],
          [chip.(), chip.()]
        )

      b = el([width(fill()), height(fill()), Background.color(:blue), Event.on_click(:b)], none())

      el(
        [width(px(100)), height(px(120)), Background.color(:white)],
        column(
          [width(px(w)), height(px(120)), spacing(4), Background.color(color_rgb(0, 255, 0))],
          [a, b]
        )
      )
    end

    expected = fn w, h ->
      for {color, rows} <- [
            {<<255, 0, 0, 255>>, h},
            {<<0, 255, 0, 255>>, 4},
            {<<0, 0, 255, 255>>, 116 - h}
          ],
          into: <<>> do
        line = :binary.copy(color, w) <> :binary.copy(<<255, 255, 255, 255>>, 100 - w)
        :binary.copy(line, rows)
      end
    end

    {pid, r} = Headless.mount({100, 120}, fn -> scene.(100, 36) end, [:pointer_button])
    assert Headless.pixels() == expected.(100, 20)
    click(r, :column, 30.0, :b, 6.0)

    for {w, cap, h, point, k, local} <- [
          {80, 36, 36, 42.0, :b, 2.0},
          {80, 60, 44, 42.0, :a, 42.0},
          {100, 36, 20, 30.0, :b, 6.0}
        ] do
      submit(pid, fn -> scene.(w, cap) end)
      assert Headless.pixels() == expected.(w, h)
      click(r, :column, point, k, local)
    end
  end
end
