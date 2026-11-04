defmodule Emerge.ScrollbarVisibilityTest do
  use ExUnit.Case, async: true
  use Emerge.UI

  alias EmergeSkia.TestHarness

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

      {binary, diff, assigned} =
        Emerge.Engine.encode_full(Emerge.Engine.diff_state_new(), scene.(0.0))

      inner = Enum.find(assigned.children, &(&1.key == :inner))
      id = <<inner.id::64>>
      harness = TestHarness.new(500, 400)
      on_exit(fn -> TestHarness.stop(harness) end)
      assert :ok = TestHarness.set_input_target(harness, self())
      assert :ok = TestHarness.upload_full_bin(harness, binary)
      assert :ok = TestHarness.await_render(harness, 1500)

      diff =
        Enum.reduce([1.0, 0.0, 1.0], diff, fn position, diff ->
          {patch, next, _tree} = Emerge.Engine.diff_state_update(diff, scene.(position))
          assert :ok = TestHarness.apply_patch_bin(harness, patch)
          assert :ok = TestHarness.await_render(harness, 1500)
          next
        end)

      # Use the visible vertical track, above the horizontal scrollbar. The
      # renderer must still accept real native input after the hidden rebuilds.
      # The revealed pane ends at the parent's 80px viewport edge. Its width
      # is smaller than the parent, so the two vertical scrollbars don't overlap.
      {x, y} = {100 * factor - 2, 80 * factor - 10}
      assert :ok = TestHarness.cursor_button(harness, :press, x, y)

      assert_receive {:emerge_skia_event, {^id, :scroll, %{y: fraction}}} when fraction > 0.0,
                     1500

      assert :ok = TestHarness.cursor_button(harness, :release, x, y)
      assert :ok = TestHarness.await_render(harness, 1500)

      {patch, _next, _tree} = Emerge.Engine.diff_state_update(diff, scene.(0.0))
      assert :ok = TestHarness.apply_patch_bin(harness, patch)
      assert :ok = TestHarness.await_render(harness, 1500)
    end
  end

  test "collapsed cross-axis scrollbars with retained focus handlers do not kill the renderer" do
    harness = TestHarness.new(300, 200)
    on_exit(fn -> TestHarness.stop(harness) end)

    for {w, h, scrollbar} <- [{120, 0, scrollbar_x()}, {0, 60, scrollbar_y()}] do
      tree =
        column([width(fill()), height(fill())], [
          el(
            [width(px(w)), height(px(h)), scrollbar, Event.on_focus(:focused)],
            el([width(px(240)), height(px(180))], none())
          )
        ])

      {binary, _diff, _tree} = Emerge.Engine.encode_full(Emerge.Engine.diff_state_new(), tree)
      assert :ok = TestHarness.upload_full_bin(harness, binary)
      assert :ok = TestHarness.await_render(harness, 1500)
    end
  end
end
