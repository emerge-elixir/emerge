defmodule Emerge.ScrollbarVisibilityTest do
  use ExUnit.Case, async: true
  use Emerge.UI

  alias Emerge.TestSupport.Headless

  test "clipped nested scrollbars survive native rebuilds and remain usable when revealed" do
    for factor <- [1.0, 1.6, 2.0] do
      scene = fn position ->
        column(
          [width(px(120)), height(px(80)), scale(factor), scrollbar_y(), scroll_y(position)],
          [
            el([key(:spacer), height(px(160))], none()),
            column(
              [
                key(:inner),
                Background.color(color_rgb(38, 118, 170)),
                width(px(100)),
                height(px(60)),
                scrollbar_x(),
                scrollbar_y(),
                Event.on_scroll(:inner_scrolled)
              ],
              [
                Input.text([width(px(240)), height(px(30))], "retained input"),
                el([width(px(240)), height(px(180))], none())
              ]
            )
          ]
        )
      end

      {pid, r} = Headless.mount({500, 400}, fn -> scene.(0.0) end)
      # Below the text field, above the horizontal bar.
      visible? = fn pixels ->
        binary_part(pixels, (trunc(64 * factor) * 500 + trunc(5 * factor)) * 4, 4) ==
          <<38, 118, 170, 255>>
      end

      refute visible?.(Headless.pixels())

      for position <- [1.0, 0.0, 1.0] do
        send(pid, {:scene, fn -> scene.(position) end})
        Headless.pixels_matching(fn pixels -> visible?.(pixels) == (position == 1.0) end)
      end

      # Use the visible vertical track, above the horizontal scrollbar. The
      # renderer must still accept real native input after the hidden rebuilds.
      # The revealed pane ends at the parent's 80px viewport edge. Its width
      # is smaller than the parent, so the two vertical scrollbars don't overlap.
      {x, y} = {100 * factor - 2, 80 * factor - 10}
      assert :ok = Headless.button(r, :press, x, y)
      assert_receive {:application, {:inner_scrolled, %{y: fraction}}} when fraction > 0.0, 1500
      Headless.pixels()
      assert :ok = Headless.button(r, :release, x, y)
      send(pid, {:scene, fn -> scene.(0.0) end})
      Headless.pixels_matching(fn pixels -> not visible?.(pixels) end)
      GenServer.stop(pid)
    end
  end

  test "collapsed cross-axis scrollbars with retained focus handlers do not kill the renderer" do
    scene = fn w, h, scrollbar ->
      column([width(fill()), height(fill())], [
        el(
          [width(px(w)), height(px(h)), scrollbar, Event.on_focus(:focused)],
          el([width(px(240)), height(px(180))], none())
        )
      ])
    end

    {pid, r} = Headless.mount({300, 200}, fn -> scene.(120, 0, scrollbar_x()) end)
    Headless.pixels()
    send(pid, {:scene, fn -> scene.(0, 60, scrollbar_y()) end})
    Headless.pixels()
    assert EmergeSkia.running?(r)
  end
end
