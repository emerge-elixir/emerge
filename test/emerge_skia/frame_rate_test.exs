defmodule EmergeSkia.FrameRateTest do
  use ExUnit.Case, async: false
  use Emerge.UI

  alias Emerge.Config
  alias Emerge.Config.Normalize

  test "headless caps tree updates, wakes pending work on live uncap, and stays idle on config-only changes" do
    output = %Config.Headless{
      target: self(),
      size: {2, 2},
      frame_message: "rate_frame",
      frames: %Config.BinaryFrames{pixel_format: :rgb888}
    }

    config =
      Normalize.config!(%Config{
        output: output,
        renderer: [api: :raster, target_fps: 1],
        assets: [otp_app: :emerge]
      })

    {:ok, renderer} = EmergeSkia.start(config)
    on_exit(fn -> EmergeSkia.stop(renderer) end)
    upload_color(renderer, {255, 0, 0})
    await_color(<<255, 0, 0>>, 5_000)
    upload_color(renderer, {0, 0, 255})
    upload_color(renderer, {0, 255, 0})
    refute_receive {"rate_frame", _}, 100
    %Config{renderer: %Config.Renderer{} = rendering} = config
    rendering = %Config.Renderer{rendering | target_fps: nil}
    uncapped = %Config{config | renderer: rendering}
    assert {:ok, token} = EmergeSkia.configure(renderer, config, uncapped)
    assert_receive {:emerge_skia_config, ^token, :ok}, 5_000
    await_color(<<0, 255, 0>>, 750)
    capped = %Config{uncapped | renderer: %Config.Renderer{rendering | target_fps: 1}}
    assert {:ok, token} = EmergeSkia.configure(renderer, uncapped, capped)
    assert_receive {:emerge_skia_config, ^token, :ok}, 5_000
    refute_receive {"rate_frame", _}, 60
    upload_color(renderer, {0, 0, 255})
    stop = Task.async(fn -> EmergeSkia.stop(renderer) end)
    assert :ok = Task.await(stop, 750)
    assert {:ok, %{cleanup_complete: true}} = EmergeSkia.renderer_status(renderer)
    assert {:error, "renderer is stopping"} = EmergeSkia.Native.set_frame_rate(renderer, 30)
  end

  test "equal config snapshots repair partial backend changes instead of skipping replay" do
    config =
      Normalize.config!(%Config{
        output: %Config.Headless{
          target: self(),
          size: {2, 2},
          frame_message: "rate_frame",
          frames: %Config.BinaryFrames{pixel_format: :rgb888}
        },
        renderer: [api: :raster],
        assets: [otp_app: :emerge]
      })

    {:ok, renderer} = EmergeSkia.start(config)
    on_exit(fn -> EmergeSkia.stop(renderer) end)

    # Model a setter that succeeded before a later part of configure failed.
    assert :ok = EmergeSkia.Native.set_frame_rate(renderer, 1)
    upload_color(renderer, {255, 0, 0})
    await_color(<<255, 0, 0>>, 5_000)
    upload_color(renderer, {0, 255, 0})
    refute_receive {"rate_frame", _}, 100

    assert {:ok, token} = EmergeSkia.configure(renderer, config, config)
    assert_receive {:emerge_skia_config, ^token, :ok}, 5_000
    await_color(<<0, 255, 0>>, 750)
    refute_receive {"rate_frame", _}, 60
  end

  test "frame caps validate positive 32-bit values at both boundaries" do
    for fps <- [0, -1, 1.5, 4_294_967_296] do
      assert_raise ArgumentError, ~r/config.renderer.target_fps/, fn ->
        Normalize.config!(%Config{renderer: [target_fps: fps]})
      end
    end

    config = %Config{
      output: %Config.Headless{target: self(), size: {2, 2}},
      renderer: [api: :raster],
      assets: [otp_app: :emerge]
    }

    {:ok, renderer} = EmergeSkia.start(config)

    try do
      assert {:error, "target_fps must be a positive integer or nil"} =
               EmergeSkia.Native.set_frame_rate(renderer, 0)

      assert :ok = EmergeSkia.Native.set_frame_rate(renderer, 30)
      assert :ok = EmergeSkia.Native.set_frame_rate(renderer, nil)
    after
      EmergeSkia.stop(renderer)
    end
  end

  defp upload_color(renderer, {r, g, b}) do
    EmergeSkia.upload_tree(
      renderer,
      el(
        [width(px(2)), height(px(2)), Background.color(Emerge.UI.Color.color_rgb(r, g, b))],
        none()
      )
    )
  end

  defp await_color(pixel, timeout) do
    await_pixels(:binary.copy(pixel, 4), System.monotonic_time(:millisecond) + timeout)
  end

  defp await_pixels(expected, deadline) do
    timeout = Kernel.max(deadline - System.monotonic_time(:millisecond), 0)

    receive do
      {"rate_frame", %VideoInterop.Frame{storage: %VideoInterop.Binary{data: ^expected}}} -> :ok
      {"rate_frame", _other} -> await_pixels(expected, deadline)
    after
      timeout -> flunk("timed out waiting for capped headless output")
    end
  end
end
