defmodule EmergeSkia.ColumnHeightFillTest do
  use ExUnit.Case, async: true
  use Emerge.UI

  alias Emerge.TestSupport.Headless

  defp cold(size, view) do
    Headless.mount(size, view)
    Headless.pixels()
  end

  defp band(key, h, color) do
    el([key(key), width(fill()), height(h), Background.color(color), Event.on_click(key)], none())
  end

  defp stack(header, panels) do
    colors = %{b: color_rgb(0, 255, 0), c: :blue}

    column([width(fill()), height(px(120)), padding(5), spacing(5), Event.on_click(:column)], [
      band(:header, px(header), :red) | Enum.map(panels, &band(&1, fill(), colors[&1]))
    ])
  end

  defp button(r, phase, x, y) do
    assert :ok = Headless.button(r, phase, x, y)
    assert_receive {:raw, {:pointer_button, {:left, ^phase, [], {^x, ^y}}}}
  end

  defp click(r, y, label, local_x, local_y) do
    button(r, :press, 10.0, y)
    button(r, :release, 10.0, y)
    assert_receive {:application, {^label, %{x: ^local_x, y: ^local_y}}}
  end

  test "the submitted split owns the pixels, gap and click, even across a keyed swap" do
    size = {80, 120}
    short = fn -> stack(20, [:b, :c]) end
    tall = fn -> stack(60, [:b, :c]) end
    swap = fn -> stack(20, [:c, :b]) end
    a = cold(size, short)
    b = cold(size, tall)
    c = cold(size, swap)
    refute a == b
    refute a == c
    assert binary_part(a, 4 * (80 * 80 + 10), 4) == <<0, 0, 255, 255>>
    assert binary_part(b, 4 * (80 * 80 + 10), 4) == <<0, 255, 0, 255>>
    assert binary_part(b, 4 * (92 * 80 + 10), 4) == <<0::32>>

    {pid, r} = Headless.mount(size, short, [:pointer_button], target_fps: 1)
    assert Headless.pixels() == a
    # This message sent from render runs after the viewport submits its edit.
    send(
      pid,
      {:scene,
       fn ->
         send(self(), :submitted)
         tall.()
       end}
    )

    assert_receive {:application, :submitted}
    assert {:ok, ^a} = EmergeSkia.render_to_pixels(r)
    click(r, 80.0, :c, 5.0, 5.0)
    assert Headless.pixels(2500) == b
    click(r, 80.0, :b, 5.0, 10.0)
    click(r, 92.0, :column, 10.0, 92.0)
    send(pid, {:scene, short})
    assert Headless.pixels(2500) == a
    click(r, 80.0, :c, 5.0, 5.0)
    button(r, :press, 10.0, 80.0)
    send(pid, {:scene, swap})
    assert Headless.pixels(2500) == c
    button(r, :release, 10.0, 35.0)
    assert_receive {:application, {:c, %{x: 5.0, y: 5.0}}}
    click(r, 80.0, :b, 5.0, 5.0)
  end

  test "a non-fill wrapping sibling changes the fill panel's start and height in both directions" do
    chip = fn ->
      el([width(px(48))], el([height(px(20)), width(fill()), Background.color(:red)], none()))
    end

    view = fn w ->
      column([height(fill()), width(px(w)), spacing(4), Event.on_click(:column)], [
        wrapped_row([key(:chips), width(fill()), spacing(4)], [chip.(), chip.()]),
        band(:panel, fill(), :blue)
      ])
    end

    size = {100, 100}
    wide = cold(size, fn -> view.(100) end)
    narrow = cold(size, fn -> view.(80) end)
    refute wide == narrow
    {pid, r} = Headless.mount(size, fn -> view.(100) end, [:pointer_button])

    for {w, pixels, start} <- [{nil, wide, 24}, {80, narrow, 48}, {100, wide, 24}] do
      if w, do: send(pid, {:scene, fn -> view.(w) end})
      assert Headless.pixels() == pixels
      assert binary_part(pixels, 4 * (100 * (start - 1) + 10), 8) == <<0::64>>
      assert binary_part(pixels, 4 * (100 * start + 10), 4) == <<0, 0, 255, 255>>
      assert binary_part(pixels, 4 * (100 * 99 + 10), 4) == <<0, 0, 255, 255>>
      click(r, start - 1.0, :column, 10.0, start - 1.0)
      click(r, start + 5.0, :panel, 10.0, 5.0)
    end
  end
end
