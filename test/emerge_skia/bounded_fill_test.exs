defmodule EmergeSkia.BoundedFillTest do
  use ExUnit.Case, async: true
  use Emerge.UI

  alias Emerge.TestSupport.Headless

  defp dims(:row, length), do: [width(length), height(px(16))]
  defp dims(:column, length), do: [height(length), width(px(16))]
  defp build(:row, attrs, children), do: row(attrs, children)
  defp build(:column, attrs, children), do: column(attrs, children)
  defp size(:row, n), do: {n, 16}
  defp size(:column, n), do: {16, n}

  defp view(axis, first, opts \\ []) do
    a =
      el(
        [key(:a), Background.color(Keyword.get(opts, :color, :red)), Event.on_click(:a)] ++
          dims(axis, first) ++ Keyword.get(opts, :a, []),
        none()
      )

    b = el([key(:b), Background.color(:blue), Event.on_click(:b) | dims(axis, fill())], none())

    build(
      axis,
      [
        Background.color(color_rgb(0, 255, 0)),
        Event.on_click(:host),
        spacing(Keyword.get(opts, :gap, 0)) | dims(axis, px(120))
      ],
      [a, b]
    )
  end

  defp pos(:row, n, cross), do: {n, cross}
  defp pos(:column, n, cross), do: {cross, n}

  defp click(renderer, axis, point, key, local, local_cross \\ 6.0) do
    {x, y} = pos(axis, point, 6.0)
    {lx, ly} = pos(axis, local, local_cross)

    for phase <- [:press, :release] do
      assert :ok = Headless.button(renderer, phase, x, y)
      assert_receive {:raw, {:pointer_button, {:left, ^phase, [], {^x, ^y}}}}
    end

    assert_receive {:application, {^key, %{x: ^lx, y: ^ly}}}
  end

  defp bands(axis, widths, color \\ :red) do
    a = if color == :red, do: <<255, 0, 0, 255>>, else: <<255, 255, 255, 255>>
    colors = [a, <<0, 0, 255, 255>>, <<0, 255, 0, 255>>, <<255, 255, 255, 255>>]
    {stride, rows} = if axis == :row, do: {1, 16}, else: {16, 1}
    band = for {c, n} <- Enum.zip(colors, widths), into: <<>>, do: :binary.copy(c, n * stride)
    :binary.copy(band, rows)
  end

  for axis <- [:row, :column] do
    test "#{axis}: B receives all space A cannot use, with clicks following the submitted split" do
      axis = unquote(axis)
      size = size(axis, 120)

      {pid, renderer} =
        Headless.mount(size, fn -> view(axis, px(20)) end, [:pointer_button], target_fps: 1)

      fixed = bands(axis, [20, 100, 0])
      assert Headless.pixels() == fixed
      capped = min(px(20), fill())
      # A fixed 20px and a capped 20px draw identical boxes. A color change
      # gives the accepted policy edit its own observable submitted frame.
      Enum.reduce(
        [
          {capped, [20, 100, 0], :white, 90.0, {:b, 70.0}, {:b, 70.0}},
          {min(fill(3), px(50)), [50, 70, 0], :red, 35.0, {:b, 15.0}, {:a, 35.0}},
          {capped, [20, 100, 0], :red, 35.0, {:a, 35.0}, {:b, 15.0}}
        ],
        fixed,
        fn {policy, widths, color, point, {before, old}, {after_key, new}}, installed ->
          scene = fn -> view(axis, policy, color: color) end

          send(
            pid,
            {:scene,
             fn ->
               send(self(), :submitted)
               scene.()
             end}
          )

          assert_receive {:application, :submitted}
          assert {:ok, ^installed} = EmergeSkia.render_to_pixels(renderer)
          click(renderer, axis, point, before, old)
          expected = bands(axis, widths, color)
          assert Headless.pixels(2500) == expected
          click(renderer, axis, point, after_key, new)
          {_, cold} = Headless.mount(size, scene, [:pointer_button])
          assert Headless.pixels() == expected
          click(cold, axis, point, after_key, new)
          expected
        end
      )
    end

    test "#{axis}: fill respects a floor, and an infeasible minimum still clips at the host" do
      axis = unquote(axis)

      scene = fn floor ->
        el(
          [Event.on_click(:outside), Background.color(:white) | dims(axis, px(140))],
          view(axis, max(fill(), px(floor)))
        )
      end

      {pid, r} = Headless.mount(size(axis, 140), fn -> scene.(80) end, [:pointer_button])
      assert Headless.pixels() == bands(axis, [80, 40, 0, 20])
      click(r, axis, 119.5, :b, 39.5)
      send(pid, {:scene, fn -> scene.(135) end})
      assert Headless.pixels() == bands(axis, [120, 0, 0, 20])
      click(r, axis, 119.5, :a, 119.5)
      click(r, axis, 120.5, :outside, 120.5)
    end
  end

  test "a fixed-to-capped policy edit keeps the installed A/B labels and later releases the cap" do
    scene = fn w, a ->
      row([width(px(w)), Event.on_click(:host)], [
        el([key(:a), width(a), Event.on_click(:a), Background.color(:red)], text("A")),
        el([key(:b), width(fill()), Event.on_click(:b), Background.color(:blue)], text("B"))
      ])
    end

    cap = min(px(20), fill())
    size = {120, 24}
    {_, cold} = Headless.mount(size, fn -> scene.(120, cap) end, [:pointer_button])
    wide = Headless.pixels()
    click(cold, :row, 90.0, :b, 70.0)
    Headless.mount(size, fn -> scene.(30, cap) end)
    narrow = Headless.pixels()
    {pid, r} = Headless.mount(size, fn -> scene.(120, px(20)) end, [:pointer_button])
    assert Headless.pixels() == wide
    click(r, :row, 90.0, :b, 70.0)

    send(
      pid,
      {:scene,
       fn ->
         send(self(), :submitted)
         scene.(120, cap)
       end}
    )

    assert_receive {:application, :submitted}
    # Identical visible output is allowed to reuse the installed pixels.
    assert {:ok, ^wide} = EmergeSkia.render_to_pixels(r)
    click(r, :row, 90.0, :b, 70.0)
    send(pid, {:scene, fn -> scene.(30, cap) end})
    assert Headless.pixels_matching(&(&1 == narrow)) == narrow
    click(r, :row, 17.0, :b, 2.0)
    send(pid, {:scene, fn -> scene.(120, cap) end})
    assert Headless.pixels() == wide
    click(r, :row, 90.0, :b, 70.0)
  end

  test "scaled caps keep fractional hit edges and use the child's coordinate system" do
    scene = fn -> view(:column, min(fill(), px(12.25)), a: [scale(1.5)], gap: 1) end
    {_, r} = Headless.mount(size(:column, 120), scene, [:pointer_button])
    pixels = Headless.pixels()
    assert binary_part(pixels, 4 * (16 * 17 + 6), 4) == <<255, 0, 0, 255>>
    click(r, :column, 18.375, :a, 12.25, 4.0)
    click(r, :column, 18.5, :host, 18.5)
    click(r, :column, 20.875, :b, 1.5)
  end
end
