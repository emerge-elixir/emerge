defmodule EmergeSkia.LiveLengthChangeTest do
  use ExUnit.Case, async: false
  use Emerge.UI

  alias EmergeSkia.TestHarness

  defp tree(length) do
    row([key(:root), width(px(600)), height(px(60))], [
      el(
        [
          key(:target),
          Animation.change([width(length)], 1000, :linear),
          height(fill()),
          Interactive.mouse_over([Background.color(:blue)])
        ],
        none()
      ),
      el([key(:peer), width(fill()), height(fill())], none())
    ])
  end

  test "a public pixel-to-fill change advances real actor hit geometry" do
    {full, state, assigned} =
      Emerge.Engine.encode_full(Emerge.Engine.diff_state_new(), tree(px(40)))

    [target | _] = assigned.children
    target_id = Emerge.Engine.NodeId.encode(target.id)
    {patch, _state, _assigned} = Emerge.Engine.diff_state_update(state, tree(fill()))
    harness = TestHarness.new(600, 60)
    on_exit(fn -> TestHarness.stop(harness) end)

    assert :ok = TestHarness.upload_full_bin(harness, full)
    assert :ok = TestHarness.await_render(harness)
    assert :ok = TestHarness.cursor_pos(harness, 200, 30)
    refute {target_id, true} in TestHarness.drain_mouse_over_msgs(harness)
    assert :ok = TestHarness.reset_clock(harness)
    assert :ok = TestHarness.apply_patch_bin(harness, patch)
    assert :ok = TestHarness.await_render(harness)

    activation =
      Enum.find_value(0..1200//50, fn ms ->
        assert :ok = TestHarness.animation_pulse(harness, ms, ms)
        assert :ok = TestHarness.await_render(harness)
        if {target_id, true} in TestHarness.drain_mouse_over_msgs(harness, 10), do: ms
      end)

    # Width 40→300 reaches x=200 at ~615ms, not an immediate symbolic switch.
    assert activation in 600..750
  end

  defp content_tree(string) do
    row([key(:root), width(px(600)), height(px(60))], [
      el(
        [
          key(:target),
          Animation.change([width(content())], 1000, :linear),
          height(fill()),
          Font.size(20),
          Interactive.mouse_over([Background.color(:blue)])
        ],
        text(string)
      ),
      el([key(:peer), width(fill()), height(fill())], none())
    ])
  end

  test "unchanged content width policy animates a descendant-only text update" do
    {full, state, assigned} =
      Emerge.Engine.encode_full(Emerge.Engine.diff_state_new(), content_tree("S"))

    [target | _] = assigned.children
    target_id = Emerge.Engine.NodeId.encode(target.id)
    {patch, _state, _assigned} = Emerge.Engine.diff_state_update(state, content_tree("Something"))
    harness = TestHarness.new(600, 60)
    on_exit(fn -> TestHarness.stop(harness) end)
    assert :ok = TestHarness.upload_full_bin(harness, full)
    assert :ok = TestHarness.await_render(harness)
    assert :ok = TestHarness.cursor_pos(harness, 50, 30)
    refute {target_id, true} in TestHarness.drain_mouse_over_msgs(harness)
    assert :ok = TestHarness.reset_clock(harness)
    assert :ok = TestHarness.apply_patch_bin(harness, patch)
    assert :ok = TestHarness.await_render(harness)
    refute {target_id, true} in TestHarness.drain_mouse_over_msgs(harness)

    activation =
      Enum.find_value(0..1200//50, fn ms ->
        assert :ok = TestHarness.animation_pulse(harness, ms, ms)
        assert :ok = TestHarness.await_render(harness)
        if {target_id, true} in TestHarness.drain_mouse_over_msgs(harness, 10), do: ms
      end)

    assert activation in 200..900
  end
end
