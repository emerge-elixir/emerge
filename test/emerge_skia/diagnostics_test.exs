defmodule EmergeSkia.DiagnosticsTest do
  use ExUnit.Case, async: true
  use Emerge.UI

  alias Emerge.Config

  test "stats enables collection without logs, and stats logs enable it without stats" do
    for {stats, logs, enabled} <- [
          {false, [], false},
          {true, [], true},
          {false, [:stats], true},
          {true, [:stats], true},
          {false, [:animation], false}
        ] do
      with_renderer(%Config.Diagnostics{stats: stats, logs: logs}, fn renderer ->
        assert {:ok, snapshot} = EmergeSkia.stats(renderer, :peek)
        assert snapshot.enabled == enabled
        assert snapshot.window.reset_on_read == false
      end)
    end
  end

  test "renderer collection cannot be configured after startup" do
    with_renderer(%Config.Diagnostics{}, fn renderer ->
      assert {:error, "renderer stats are configured when the renderer starts"} =
               EmergeSkia.stats(renderer, {:configure, %{enabled: true}})

      assert {:ok, %{enabled: false}} = EmergeSkia.stats(renderer, :peek)
    end)
  end

  test "native stats logs close a real window, then report idle without requesting a frame" do
    with_renderer(%Config.Diagnostics{logs: [:stats]}, fn renderer ->
      attrs = [width(fill()), height(fill()), Background.color(:red)]
      {state, _tree} = EmergeSkia.upload_tree(renderer, el(attrs, none()))
      assert_receive {:emerge_skia_frame, _}, 2_000
      assert {:ok, %{frames: %{frame_count: 1}}} = EmergeSkia.stats(renderer, :peek)
      EmergeSkia.patch_tree(renderer, state, el([Event.on_click({self(), :hit}) | attrs], none()))
      assert_receive {:emerge_skia_frame, _}, 2_000
      assert :ok = EmergeSkia.send_input(renderer, {:pointer_move, {1, 1}})
      assert {:ok, s} = EmergeSkia.stats(renderer, :peek)
      assert s.version == 25
      assert s.frames.frame_count == 2
      assert s.counters.pipeline.scenes_constructed == 2
      assert s.timings.patch_tree_process.count == 2
      assert s.timings.render.count == 2
      assert s.timings.render_draw.count == 1

      for k <- [
            :render,
            :render_draw,
            :present_submit,
            :pipeline_submit_to_tree_start,
            :pipeline_tree,
            :pipeline_render_queue,
            :pipeline_submit_to_swap,
            :layout,
            :refresh,
            :event_resolve,
            :patch_tree_process
          ] do
        assert %{count: n, min_ms: lo, avg_ms: avg, max_ms: hi} = Map.fetch!(s.timings, k)
        assert n > 0 and 0 <= lo and lo <= avg and avg <= hi
      end

      for k <- [
            :render_flush,
            :render_gpu_flush,
            :render_submit,
            :pipeline_swap_to_frame_callback,
            :pipeline
          ] do
        assert s.timings[k].count == 0
      end

      assert_receive {:emerge_skia_log, :info, "renderer_stats", report}, 7_000

      assert report =~
               "renderer stats\n  window\n    backend: headless\n    rendering_api: raster"

      for label <- [
            "render",
            "render draw",
            "present submit",
            "event resolve",
            "edit decode",
            "edit prepare",
            "edit apply"
          ] do
        assert report =~ "\n    #{label}: avg="
      end

      assert report =~ "\n    render gpu flush: unavailable (raster output)"
      assert report =~ "reused installed drawing targets=1"

      for section <- [
            "layout cache",
            "scene pipeline",
            "video pipeline",
            "renderer cache",
            "asset memory"
          ] do
        assert report =~ "\n  #{section}\n"
      end

      assert report =~ "sized raster sources: encoded_bytes="

      assert {:ok, %{frames: %{frame_count: 0}, timings: %{layout: %{count: 0}}}} =
               EmergeSkia.stats(renderer, :peek)

      recipient =
        Task.async(fn ->
          assert_receive {:emerge_skia_log, :info, "renderer_stats", idle}, 7_000
          idle
        end)

      assert :ok = EmergeSkia.set_log_target(renderer, recipient.pid)
      idle = Task.await(recipient, 8_000)
      refute_receive {:emerge_skia_log, :info, "renderer_stats", _}, 0
      assert idle =~ "\n    frames: 0\n"
      assert idle =~ "\n    render: no samples (count=0)"
      refute_receive {:emerge_skia_frame, _}, 0
    end)
  end

  test "collection-only stays silent across a log interval and peek/take/reset share its counters" do
    with_renderer(%Config.Diagnostics{stats: true}, fn renderer ->
      EmergeSkia.upload_tree(
        renderer,
        el([width(fill()), height(fill()), Background.color(:blue)], none())
      )

      assert_receive {:emerge_skia_frame, _}, 2_000
      refute_receive {:emerge_skia_log, :info, _, _}, 5_250

      assert {:ok, %{frames: %{frame_count: 1}, window: %{reset_on_read: true}}} =
               EmergeSkia.stats(renderer, :take)

      assert {:ok, %{timings: %{render: %{count: 0}}}} = EmergeSkia.stats(renderer, :peek)
      assert :ok = EmergeSkia.send_input(renderer, {:pointer_move, {1, 1}})

      assert {:ok, %{timings: %{event_resolve: %{count: 0}}, window: %{reset_on_read: false}}} =
               EmergeSkia.stats(renderer, :reset)
    end)
  end

  defp with_renderer(diagnostics, run) do
    config = %Config{
      output: %Config.Headless{target: self(), size: {2, 2}},
      renderer: [api: :raster],
      assets: [otp_app: :emerge],
      diagnostics: diagnostics
    }

    {:ok, renderer} = EmergeSkia.start(config)

    try do
      run.(renderer)
    after
      :ok = EmergeSkia.stop(renderer)
    end
  end
end
