defmodule EmergeSkia.WaylandVulkanSyncTest do
  use ExUnit.Case, async: false
  use Emerge.UI

  alias Emerge.Config
  @moduletag :wayland_hardware

  test "Vulkan reuses swapchain images without capture or CPU readback" do
    renderer = start_renderer()
    state = render_burst(renderer)

    # Stop with new work outstanding, not after a synchronous screenshot has drained the GPU.
    update_color(renderer, state, {0, 0, 255})
    stop = Task.async(fn -> EmergeSkia.stop(renderer) end)
    assert :ok = Task.await(stop, 750)
  end

  test "Vulkan capture's second flush preserves the one-shot acquisition dependency" do
    renderer = start_renderer()

    for _ <- 1..4,
        {rgb, expected} <- [{{255, 0, 0}, <<255, 0, 0>>}, {{0, 255, 0}, <<0, 255, 0>>}],
        reduce: nil do
      state ->
        state = update_color(renderer, state, rgb)
        await_color(renderer, expected, System.monotonic_time(:millisecond) + 5_000)
        state
    end
  end

  defp start_renderer do
    config =
      Config.Normalize.config!(%Config{
        output: %Config.Window{
          backend: :wayland,
          title: "Emerge Vulkan sync test",
          size: {64, 64}
        },
        renderer: [api: :vulkan],
        diagnostics: [stats: true],
        assets: [otp_app: :emerge]
      })

    assert {:ok, renderer} = EmergeSkia.start(config)
    on_exit(fn -> EmergeSkia.stop(renderer) end)

    assert {:ok,
            %{rendering_api: %{selected: :vulkan}, vulkan_device: %{software: false} = device}} =
             EmergeSkia.renderer_info(renderer)

    IO.puts("Wayland Vulkan hardware confirmed: #{inspect(device)}")
    renderer
  end

  defp render_burst(renderer) do
    # Wait only for successful submissions, never read pixels during the burst.
    Enum.reduce(1..48, nil, fn value, state ->
      before = frame_count(renderer)
      state = update_color(renderer, state, {value, 255 - value, 0})
      await_frame(renderer, before, System.monotonic_time(:millisecond) + 5_000)
      state
    end)
  end

  defp await_frame(renderer, before, deadline) do
    if frame_count(renderer) <= before do
      assert System.monotonic_time(:millisecond) < deadline, "Vulkan presentation stalled"
      Process.sleep(5)
      await_frame(renderer, before, deadline)
    end
  end

  defp frame_count(renderer) do
    assert {:ok, snapshot} = EmergeSkia.stats(renderer)
    snapshot.frames.frame_count
  end

  defp update_color(renderer, state, {r, g, b}) do
    tree =
      el(
        [width(fill()), height(px(64)), Background.color(Emerge.UI.Color.color_rgb(r, g, b))],
        none()
      )

    {state, _} =
      if state,
        do: EmergeSkia.patch_tree(renderer, state, tree),
        else: EmergeSkia.upload_tree(renderer, tree)

    state
  end

  defp await_color(renderer, expected, deadline) do
    remaining = deadline - System.monotonic_time(:millisecond)
    assert remaining > 0, "timed out waiting for Vulkan capture"

    case EmergeSkia.render_to_pixels(renderer,
           region: {0, 0, 1, 1},
           pixel_format: :rgb888,
           timeout: remaining
         ) do
      {:ok, ^expected} ->
        :ok

      {:ok, _other} ->
        Process.sleep(10)
        await_color(renderer, expected, deadline)

      {:error, "no presented frame is available yet"} ->
        Process.sleep(10)
        await_color(renderer, expected, deadline)

      other ->
        flunk("Vulkan capture failed: #{inspect(other)}")
    end
  end
end
