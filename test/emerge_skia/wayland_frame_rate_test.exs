defmodule EmergeSkia.WaylandFrameRateTest do
  use ExUnit.Case, async: false
  use Emerge.UI

  alias Emerge.Config
  @moduletag :wayland_hardware

  # Requires a compositor. Override EMERGE_WAYLAND_TEST_API to qualify each route.
  test "Wayland caps redraws, uncaps live, stays idle on config-only changes and stops promptly" do
    {api, raster_present} =
      case System.get_env("EMERGE_WAYLAND_TEST_API", "raster") do
        "raster" -> {:raster, :cpu}
        "gpu_upload" -> {:raster, :gpu_upload}
        "opengl" -> {:opengl, :auto}
        "vulkan" -> {:vulkan, :auto}
      end

    config =
      Config.Normalize.config!(%Config{
        output: %Config.Window{backend: :wayland, title: "Emerge frame-cap test", size: {64, 64}},
        renderer: [api: api, raster_present: raster_present, target_fps: 1],
        diagnostics: [stats: true],
        assets: [otp_app: :emerge]
      })

    assert {:ok, renderer} = EmergeSkia.start(config)
    on_exit(fn -> EmergeSkia.stop(renderer) end)
    upload_color(renderer, {255, 0, 0})
    await_color(renderer, <<255, 0, 0>>, 5_000)
    Process.sleep(30)
    before = frame_count(renderer)
    started = System.monotonic_time(:millisecond)

    for value <- 1..20 do
      upload_color(renderer, {0, value, 0})
      Process.sleep(5)
    end

    elapsed = System.monotonic_time(:millisecond) - started
    assert frame_count(renderer) - before <= div(elapsed, 1_000) + 1
    %Config{renderer: %Config.Renderer{} = rendering} = config
    rendering = %Config.Renderer{rendering | target_fps: nil}
    uncapped = %Config{config | renderer: rendering}
    assert :ok = EmergeSkia.configure(renderer, config, uncapped)
    await_color(renderer, <<0, 20, 0>>, 750)
    Process.sleep(30)
    before = frame_count(renderer)
    assert :ok = EmergeSkia.configure(renderer, uncapped, config)
    Process.sleep(60)
    assert frame_count(renderer) == before
    {:ok, snapshot} = EmergeSkia.stats(renderer)
    assert snapshot.frames.display_frame_ms < 100
    upload_color(renderer, {0, 0, 255})
    stop = Task.async(fn -> EmergeSkia.stop(renderer) end)
    assert :ok = Task.await(stop, 750)
    assert {:error, _} = EmergeSkia.Native.set_frame_rate(renderer, 30)
  end

  defp upload_color(renderer, {r, g, b}) do
    tree =
      el(
        [width(fill()), height(fill()), Background.color(Emerge.UI.Color.color_rgb(r, g, b))],
        none()
      )

    assert {%Emerge.Engine.DiffState{}, %Emerge.Engine.Element{}} =
             EmergeSkia.upload_tree(renderer, tree)
  end

  defp frame_count(renderer) do
    {:ok, snapshot} = EmergeSkia.stats(renderer)
    snapshot.frames.frame_count
  end

  defp await_color(renderer, expected, timeout) do
    deadline = System.monotonic_time(:millisecond) + timeout
    await_pixels(renderer, expected, deadline)
  end

  defp await_pixels(renderer, expected, deadline) do
    remaining = deadline - System.monotonic_time(:millisecond)
    assert remaining > 0, "timed out waiting for Wayland frame"

    case EmergeSkia.render_to_pixels(renderer,
           region: {0, 0, 1, 1},
           pixel_format: :rgb888,
           timeout: remaining
         ) do
      {:ok, ^expected} ->
        :ok

      {:ok, _} ->
        Process.sleep(10)
        await_pixels(renderer, expected, deadline)

      {:error, "no presented frame is available yet"} ->
        Process.sleep(10)
        await_pixels(renderer, expected, deadline)

      other ->
        flunk("Wayland capture failed: #{inspect(other)}")
    end
  end
end
