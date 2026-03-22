defmodule EmergeSkia.LiveAnimationHoverTest do
  use ExUnit.Case, async: false

  alias EmergeSkia.Native
  alias EmergeSkia.TestHarness
  alias EmergeSkia.TestSupport.AnimatedHitCase

  defp run_static_cursor_sequence(harness, probe_label, target_id_bin) do
    {x, y} = AnimatedHitCase.probe(probe_label)

    assert :ok == TestHarness.reset_clock(harness)
    assert :ok == TestHarness.cursor_pos(harness, x, y)
    assert [] == TestHarness.drain_mouse_over_msgs(harness, 30)

    Enum.find_value(AnimatedHitCase.sample_times_ms(), fn sample_ms ->
      assert :ok == TestHarness.animation_pulse(harness, sample_ms, sample_ms)
      assert :ok == TestHarness.await_render(harness, 250)

      TestHarness.drain_mouse_over_msgs(harness, 30)
      |> Enum.find_value(fn
        {^target_id_bin, true} -> sample_ms
        _ -> nil
      end)
    end)
  end

  defp run_static_cursor_sequence_with_samples(harness, {x, y}, target_id_bin, sample_times_ms) do
    assert :ok == TestHarness.reset_clock(harness)
    assert :ok == TestHarness.cursor_pos(harness, x, y)
    assert [] == TestHarness.drain_mouse_over_msgs(harness, 30)

    Enum.find_value(sample_times_ms, fn sample_ms ->
      assert :ok == TestHarness.animation_pulse(harness, sample_ms, sample_ms)
      assert :ok == TestHarness.await_render(harness, 250)

      TestHarness.drain_mouse_over_msgs(harness, 30)
      |> Enum.find_value(fn
        {^target_id_bin, true} -> sample_ms
        _ -> nil
      end)
    end)
  end

  defp run_static_cursor_sequence_at_point(harness, {x, y}, target_id_bin) do
    assert :ok == TestHarness.reset_clock(harness)
    assert :ok == TestHarness.cursor_pos(harness, x, y)
    assert [] == TestHarness.drain_mouse_over_msgs(harness, 30)

    Enum.find_value(AnimatedHitCase.sample_times_ms(), fn sample_ms ->
      assert :ok == TestHarness.animation_pulse(harness, sample_ms, sample_ms)
      assert :ok == TestHarness.await_render(harness, 250)

      TestHarness.drain_mouse_over_msgs(harness, 30)
      |> Enum.find_value(fn
        {^target_id_bin, true} -> sample_ms
        _ -> nil
      end)
    end)
  end

  defp world_probe_for_initial_visual_origin(tree, probe_label, width, height) do
    state = Emerge.Engine.diff_state_new()
    {full_bin, next_state, _assigned_tree} = Emerge.Engine.encode_full(state, tree)
    target_id_bin = AnimatedHitCase.target_id_bin_from_state(next_state)
    tree_res = Native.tree_new()
    assert match?(:ok, unwrap_ok(Native.tree_upload(tree_res, full_bin)))
    {:ok, frames} = Native.tree_layout(tree_res, width, height, 1.0)

    {_id_bin, target_x, target_y, _target_width, _target_height} =
      Enum.find(frames, fn {id_bin, _x, _y, _w, _h} -> id_bin == target_id_bin end)

    {local_x, local_y} = AnimatedHitCase.probe(probe_label)
    initial_move_x = -16.0
    {target_x + initial_move_x + local_x, target_y + local_y}
  end

  defp unwrap_ok(:ok), do: :ok
  defp unwrap_ok({:ok, :ok}), do: :ok
  defp unwrap_ok(other), do: other

  defp drain_probe_events(acc \\ []) do
    receive do
      {:probe, label} -> drain_probe_events([label | acc])
    after
      0 -> Enum.reverse(acc)
    end
  end

  test "full upload path activates hover when animated target reaches static cursor" do
    state = Emerge.Engine.diff_state_new()
    {full_bin, next_state, _assigned_tree} = Emerge.Engine.encode_full(state, AnimatedHitCase.tree())
    target_id_bin = AnimatedHitCase.target_id_bin_from_state(next_state)
    harness = TestHarness.new(128, 82)
    on_exit(fn -> :ok = TestHarness.stop(harness) end)

    assert :ok == TestHarness.upload_full_bin(harness, full_bin)
    assert :ok == TestHarness.await_render(harness, 250)
    assert [] == TestHarness.drain_mouse_over_msgs(harness, 30)

    activation_sample =
      run_static_cursor_sequence(harness, :newly_occupied_outside_host, target_id_bin)

    assert activation_sample ==
             AnimatedHitCase.expected_first_activation_ms(:newly_occupied_outside_host)
  end

  test "patch path activates hover when animated target reaches static cursor" do
    state = Emerge.Engine.diff_state_new()

    {full_bin, state, _assigned_placeholder} =
      Emerge.Engine.encode_full(state, AnimatedHitCase.placeholder_tree())

    {patch_bin, next_state, _assigned_tree} =
      Emerge.Engine.diff_state_update(state, AnimatedHitCase.tree())

    target_id_bin = AnimatedHitCase.target_id_bin_from_state(next_state)
    harness = TestHarness.new(128, 82)
    on_exit(fn -> :ok = TestHarness.stop(harness) end)

    assert :ok == TestHarness.upload_full_bin(harness, full_bin)
    assert :ok == TestHarness.await_render(harness, 250)
    assert [] == TestHarness.drain_mouse_over_msgs(harness, 30)

    assert :ok == TestHarness.apply_patch_bin(harness, patch_bin)
    assert :ok == TestHarness.await_render(harness, 250)
    assert [] == TestHarness.drain_mouse_over_msgs(harness, 30)

    activation_sample =
      run_static_cursor_sequence(harness, :newly_occupied_outside_host, target_id_bin)

    assert activation_sample ==
             AnimatedHitCase.expected_first_activation_ms(:newly_occupied_outside_host)
  end

  test "page-switch patch path activates hover when animated target reaches static cursor" do
    state = Emerge.Engine.diff_state_new()

    {full_bin, state, _assigned_placeholder} =
      Emerge.Engine.encode_full(state, AnimatedHitCase.page_switch_placeholder_tree())

    {patch_bin, next_state, _assigned_tree} =
      Emerge.Engine.diff_state_update(state, AnimatedHitCase.page_switch_tree())

    target_id_bin = AnimatedHitCase.target_id_bin_from_state(next_state)
    harness = TestHarness.new(128, 82)
    on_exit(fn -> :ok = TestHarness.stop(harness) end)

    assert :ok == TestHarness.upload_full_bin(harness, full_bin)
    assert :ok == TestHarness.await_render(harness, 250)
    assert [] == TestHarness.drain_mouse_over_msgs(harness, 30)

    assert :ok == TestHarness.apply_patch_bin(harness, patch_bin)
    assert :ok == TestHarness.await_render(harness, 250)
    assert [] == TestHarness.drain_mouse_over_msgs(harness, 30)

    activation_sample =
      run_static_cursor_sequence(harness, :newly_occupied_outside_host, target_id_bin)

    assert activation_sample ==
             AnimatedHitCase.expected_first_activation_ms(:newly_occupied_outside_host)
  end

  test "demo-like page-switch patch path activates hover when animated target reaches static cursor" do
    state = Emerge.Engine.diff_state_new()

    {full_bin, state, _assigned_placeholder} =
      Emerge.Engine.encode_full(state, AnimatedHitCase.demo_like_page_switch_placeholder_tree())

    {patch_bin, next_state, _assigned_tree} =
      Emerge.Engine.diff_state_update(state, AnimatedHitCase.demo_like_tree())

    target_id_bin = AnimatedHitCase.target_id_bin_from_state(next_state)
    harness = TestHarness.new(320, 260)
    on_exit(fn -> :ok = TestHarness.stop(harness) end)

    probe_point =
      world_probe_for_initial_visual_origin(
        AnimatedHitCase.demo_like_tree(),
        :newly_occupied_outside_host,
        320,
        260
      )

    assert :ok == TestHarness.upload_full_bin(harness, full_bin)
    assert :ok == TestHarness.await_render(harness, 250)
    assert [] == TestHarness.drain_mouse_over_msgs(harness, 30)

    assert :ok == TestHarness.apply_patch_bin(harness, patch_bin)
    assert :ok == TestHarness.await_render(harness, 250)
    assert [] == TestHarness.drain_mouse_over_msgs(harness, 30)

    activation_sample =
      run_static_cursor_sequence_at_point(harness, probe_point, target_id_bin)

    assert activation_sample ==
             AnimatedHitCase.expected_first_activation_ms(:newly_occupied_outside_host)
  end

  test "full-row demo-like patch path activates hover when first animated target reaches static cursor" do
    state = Emerge.Engine.diff_state_new()

    {full_bin, state, _assigned_placeholder} =
      Emerge.Engine.encode_full(state, AnimatedHitCase.demo_like_full_row_placeholder_tree())

    {patch_bin, next_state, _assigned_tree} =
      Emerge.Engine.diff_state_update(state, AnimatedHitCase.demo_like_full_row_tree())

    target_id_bin = AnimatedHitCase.target_id_bin_from_state(next_state)
    harness = TestHarness.new(760, 260)
    on_exit(fn -> :ok = TestHarness.stop(harness) end)

    probe_point =
      world_probe_for_initial_visual_origin(
        AnimatedHitCase.demo_like_full_row_tree(),
        :newly_occupied_outside_host,
        760,
        260
      )

    assert :ok == TestHarness.upload_full_bin(harness, full_bin)
    assert :ok == TestHarness.await_render(harness, 250)
    assert [] == TestHarness.drain_mouse_over_msgs(harness, 30)

    assert :ok == TestHarness.apply_patch_bin(harness, patch_bin)
    assert :ok == TestHarness.await_render(harness, 250)
    assert [] == TestHarness.drain_mouse_over_msgs(harness, 30)

    activation_sample =
      run_static_cursor_sequence_at_point(harness, probe_point, target_id_bin)

    assert activation_sample ==
             AnimatedHitCase.expected_first_activation_ms(:newly_occupied_outside_host)
  end

  test "demo-like page-switch patch path does not delay first hover activation to a later loop" do
    state = Emerge.Engine.diff_state_new()

    {full_bin, state, _assigned_placeholder} =
      Emerge.Engine.encode_full(state, AnimatedHitCase.demo_like_page_switch_placeholder_tree())

    {patch_bin, next_state, _assigned_tree} =
      Emerge.Engine.diff_state_update(state, AnimatedHitCase.demo_like_tree())

    target_id_bin = AnimatedHitCase.target_id_bin_from_state(next_state)
    harness = TestHarness.new(320, 260)
    on_exit(fn -> :ok = TestHarness.stop(harness) end)

    probe_point =
      world_probe_for_initial_visual_origin(
        AnimatedHitCase.demo_like_tree(),
        :newly_occupied_outside_host,
        320,
        260
      )

    assert :ok == TestHarness.upload_full_bin(harness, full_bin)
    assert :ok == TestHarness.await_render(harness, 250)
    assert [] == TestHarness.drain_mouse_over_msgs(harness, 30)

    assert :ok == TestHarness.apply_patch_bin(harness, patch_bin)
    assert :ok == TestHarness.await_render(harness, 250)
    assert [] == TestHarness.drain_mouse_over_msgs(harness, 30)

    activation_sample =
      run_static_cursor_sequence_with_samples(
        harness,
        probe_point,
        target_id_bin,
        AnimatedHitCase.loop_sample_times_ms(3)
      )

    assert activation_sample ==
             AnimatedHitCase.expected_first_activation_ms(:newly_occupied_outside_host)
  end

  test "demo-like page-switch patch path with move-triggered repatches activates hover on time" do
    state = Emerge.Engine.diff_state_new()

    {full_bin, state, _assigned_placeholder} =
      Emerge.Engine.encode_full(state, AnimatedHitCase.demo_like_page_switch_placeholder_tree())

    {patch_bin, state, _assigned_tree} =
      Emerge.Engine.diff_state_update(state, AnimatedHitCase.demo_like_tree_with_log("Animation 0"))

    target_id_bin = AnimatedHitCase.target_id_bin_from_state(state)
    harness = TestHarness.new(320, 260)
    on_exit(fn -> :ok = TestHarness.stop(harness) end)

    probe_point =
      world_probe_for_initial_visual_origin(
        AnimatedHitCase.demo_like_tree_with_log("Animation 0"),
        :newly_occupied_outside_host,
        320,
        260
      )

    assert :ok == TestHarness.upload_full_bin(harness, full_bin)
    assert :ok == TestHarness.await_render(harness, 250)
    assert [] == TestHarness.drain_mouse_over_msgs(harness, 30)

    assert :ok == TestHarness.apply_patch_bin(harness, patch_bin)
    assert :ok == TestHarness.await_render(harness, 250)
    assert [] == TestHarness.drain_mouse_over_msgs(harness, 30)

    assert :ok == TestHarness.reset_clock(harness)
    assert :ok == TestHarness.cursor_pos(harness, elem(probe_point, 0), elem(probe_point, 1))
    assert [] == TestHarness.drain_mouse_over_msgs(harness, 30)

    {activation_sample, _state, _count} =
      Enum.reduce_while(AnimatedHitCase.sample_times_ms(), {nil, state, 0}, fn sample_ms,
                                                                               {_activation,
                                                                                state_acc,
                                                                                count_acc} ->
        assert :ok == TestHarness.animation_pulse(harness, sample_ms, sample_ms)
        assert :ok == TestHarness.await_render(harness, 250)

        probe_events = drain_probe_events()

        {next_state, next_count} =
          Enum.reduce(probe_events, {state_acc, count_acc}, fn _event, {state_acc, count_acc} ->
            next_count = count_acc + 1

            {next_patch_bin, next_state, _assigned_tree} =
              Emerge.Engine.diff_state_update(
                state_acc,
                AnimatedHitCase.demo_like_tree_with_log("Animation #{next_count}")
              )

            assert :ok == TestHarness.apply_patch_bin(harness, next_patch_bin)
            assert :ok == TestHarness.await_render(harness, 250)
            {next_state, next_count}
          end)

        case Enum.find_value(TestHarness.drain_mouse_over_msgs(harness, 30), fn
               {^target_id_bin, true} -> sample_ms
               _ -> nil
             end) do
          nil -> {:cont, {nil, next_state, next_count}}
          activation_sample -> {:halt, {activation_sample, next_state, next_count}}
        end
      end)

    assert activation_sample ==
             AnimatedHitCase.expected_first_activation_ms(:newly_occupied_outside_host)
  end

  test "demo-like cursor-position repatches while moving to the right still activates hover on time" do
    state = Emerge.Engine.diff_state_new()

    {full_bin, state, _assigned_placeholder} =
      Emerge.Engine.encode_full(state, AnimatedHitCase.demo_like_page_switch_placeholder_tree())

    {patch_bin, state, _assigned_tree} =
      Emerge.Engine.diff_state_update(state, AnimatedHitCase.demo_like_tree_with_log("Mouse 0"))

    target_id_bin = AnimatedHitCase.target_id_bin_from_state(state)
    harness = TestHarness.new(320, 260)
    on_exit(fn -> :ok = TestHarness.stop(harness) end)

    final_probe =
      world_probe_for_initial_visual_origin(
        AnimatedHitCase.demo_like_tree_with_log("Mouse 0"),
        :newly_occupied_outside_host,
        320,
        260
      )

    move_points = [
      {elem(final_probe, 0) - 60.0, elem(final_probe, 1)},
      {elem(final_probe, 0) - 40.0, elem(final_probe, 1)},
      {elem(final_probe, 0) - 20.0, elem(final_probe, 1)},
      final_probe
    ]

    assert :ok == TestHarness.upload_full_bin(harness, full_bin)
    assert :ok == TestHarness.await_render(harness, 250)
    assert :ok == TestHarness.apply_patch_bin(harness, patch_bin)
    assert :ok == TestHarness.await_render(harness, 250)
    assert [] == TestHarness.drain_mouse_over_msgs(harness, 30)
    assert :ok == TestHarness.reset_clock(harness)

    move_schedule = Enum.zip([0, 100, 200, 300], move_points)

    {state, _} =
      Enum.reduce(move_schedule, {state, 0}, fn {sample_ms, {x, y}}, {state_acc, index_acc} ->
        assert :ok == TestHarness.cursor_pos(harness, x, y)
        assert :ok == TestHarness.animation_pulse(harness, sample_ms, sample_ms)
        assert :ok == TestHarness.await_render(harness, 250)

        {next_patch_bin, next_state, _assigned_tree} =
          Emerge.Engine.diff_state_update(
            state_acc,
            AnimatedHitCase.demo_like_tree_with_log("Mouse #{index_acc + 1}")
          )

        assert :ok == TestHarness.apply_patch_bin(harness, next_patch_bin)
        assert :ok == TestHarness.await_render(harness, 250)
        _ = TestHarness.drain_mouse_over_msgs(harness, 10)
        {next_state, index_acc + 1}
      end)

    activation_sample =
      Enum.find_value(AnimatedHitCase.loop_sample_times_ms(2), fn sample_ms ->
        if sample_ms <= 300 do
          nil
        else
          assert :ok == TestHarness.animation_pulse(harness, sample_ms, sample_ms)
          assert :ok == TestHarness.await_render(harness, 250)

          probe_events = drain_probe_events()

          {_state, _count} =
            Enum.reduce(probe_events, {state, 4}, fn _event, {state_acc, count_acc} ->
              next_count = count_acc + 1

              {next_patch_bin, next_state, _assigned_tree} =
                Emerge.Engine.diff_state_update(
                  state_acc,
                  AnimatedHitCase.demo_like_tree_with_log("Mouse #{next_count}")
                )

              assert :ok == TestHarness.apply_patch_bin(harness, next_patch_bin)
              assert :ok == TestHarness.await_render(harness, 250)
              {next_state, next_count}
            end)

          TestHarness.drain_mouse_over_msgs(harness, 30)
          |> Enum.find_value(fn
            {^target_id_bin, true} -> sample_ms
            _ -> nil
          end)
        end
      end)

    assert activation_sample ==
             AnimatedHitCase.expected_first_activation_ms(:newly_occupied_outside_host)
  end

  test "demo-like page-switch patch path activates hover within one loop for many cursor placement phases" do
    probe_point =
      world_probe_for_initial_visual_origin(
        AnimatedHitCase.demo_like_tree(),
        :newly_occupied_outside_host,
        320,
        260
      )

    failures =
      for phase_ms <- 0..1350//50, reduce: [] do
        failures ->
          state = Emerge.Engine.diff_state_new()

          {full_bin, state, _assigned_placeholder} =
            Emerge.Engine.encode_full(state, AnimatedHitCase.demo_like_page_switch_placeholder_tree())

          {patch_bin, next_state, _assigned_tree} =
            Emerge.Engine.diff_state_update(state, AnimatedHitCase.demo_like_tree())

          target_id_bin = AnimatedHitCase.target_id_bin_from_state(next_state)
          harness = TestHarness.new(320, 260)

          try do
            :ok = TestHarness.upload_full_bin(harness, full_bin)
            :ok = TestHarness.await_render(harness, 250)
            _ = TestHarness.drain_mouse_over_msgs(harness, 30)

            :ok = TestHarness.apply_patch_bin(harness, patch_bin)
            :ok = TestHarness.await_render(harness, 250)
            _ = TestHarness.drain_mouse_over_msgs(harness, 30)

            :ok = TestHarness.reset_clock(harness)

            Enum.each(0..phase_ms//50, fn sample_ms ->
              :ok = TestHarness.animation_pulse(harness, sample_ms, sample_ms)
              :ok = TestHarness.await_render(harness, 250)
              _ = TestHarness.drain_mouse_over_msgs(harness, 10)
            end)

            :ok = TestHarness.cursor_pos(harness, elem(probe_point, 0), elem(probe_point, 1))

            activated =
              Enum.any?(phase_ms..(phase_ms + 1400)//50, fn sample_ms ->
                :ok = TestHarness.animation_pulse(harness, sample_ms, sample_ms)
                :ok = TestHarness.await_render(harness, 250)

                Enum.any?(TestHarness.drain_mouse_over_msgs(harness, 10), fn
                  {^target_id_bin, true} -> true
                  _ -> false
                end)
              end)

            if activated, do: failures, else: [phase_ms | failures]
          after
            :ok = TestHarness.stop(harness)
          end
      end

    assert failures == []
  end

  test "demo-like page-switch with predicted-next-frame pulses does not clear hover while target stays under static cursor" do
    state = Emerge.Engine.diff_state_new()

    {full_bin, state, _assigned_placeholder} =
      Emerge.Engine.encode_full(state, AnimatedHitCase.demo_like_page_switch_placeholder_tree())

    {patch_bin, next_state, _assigned_tree} =
      Emerge.Engine.diff_state_update(state, AnimatedHitCase.demo_like_tree())

    target_id_bin = AnimatedHitCase.target_id_bin_from_state(next_state)
    harness = TestHarness.new(320, 260)
    on_exit(fn -> :ok = TestHarness.stop(harness) end)

    probe_point =
      world_probe_for_initial_visual_origin(
        AnimatedHitCase.demo_like_tree(),
        :newly_occupied_outside_host,
        320,
        260
      )

    assert :ok == TestHarness.upload_full_bin(harness, full_bin)
    assert :ok == TestHarness.await_render(harness, 250)
    assert :ok == TestHarness.apply_patch_bin(harness, patch_bin)
    assert :ok == TestHarness.await_render(harness, 250)
    assert [] == TestHarness.drain_mouse_over_msgs(harness, 30)

    assert :ok == TestHarness.reset_clock(harness)
    assert :ok == TestHarness.cursor_pos(harness, elem(probe_point, 0), elem(probe_point, 1))
    assert [] == TestHarness.drain_mouse_over_msgs(harness, 30)

    tracked_samples = Enum.filter(AnimatedHitCase.sample_times_ms(), &(&1 < 1400))

    {first_activation_sample, false_samples_after_activation} =
      Enum.reduce(tracked_samples, {nil, []}, fn sample_ms, {activation_sample, false_samples} ->
        assert :ok == TestHarness.animation_pulse(harness, sample_ms, sample_ms + 16)
        assert :ok == TestHarness.await_render(harness, 250)

        msgs = TestHarness.drain_mouse_over_msgs(harness, 10)

        activated_now =
          Enum.any?(msgs, fn
            {^target_id_bin, true} -> true
            _ -> false
          end)

        cleared_now =
          Enum.any?(msgs, fn
            {^target_id_bin, false} -> true
            _ -> false
          end)

        activation_sample = activation_sample || if(activated_now, do: sample_ms)

        false_samples =
          if activation_sample && cleared_now do
            [sample_ms | false_samples]
          else
            false_samples
          end

        {activation_sample, false_samples}
      end)

    assert first_activation_sample in (AnimatedHitCase.expected_first_activation_ms(
                                         :newly_occupied_outside_host
                                       ) - 50)..AnimatedHitCase.expected_first_activation_ms(
             :newly_occupied_outside_host
           )

    assert false_samples_after_activation == []
  end
end
