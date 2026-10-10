defmodule EmergeSkia.DrmRasterHardwareTest do
  use ExUnit.Case, async: false
  use Emerge.UI
  @moduletag :drm_hardware
  @moduletag timeout: 90_000
  test "CPU scanout updates, captures RGBA, and releases a single output for reuse" do
    card = System.fetch_env!("EMERGE_DRM_TEST_CARD")
    {:ok, outputs} = EmergeSkia.drm_outputs(drm_card: card)
    output = Enum.find(outputs, &(&1.connected and &1.modes != []))
    assert output, "the explicitly selected card needs a connected display"
    mode = Enum.find(output.modes, & &1.preferred) || hd(output.modes)
    explicit_apis = [[api: :raster], [api: :raster, raster_present: :cpu]]

    apis =
      if :drm in EmergeSkia.BuildConfig.compiled_opengl_backends() do
        explicit_apis
      else
        [[api: :auto] | explicit_apis]
      end

    for api <- apis do
      {:ok, renderer} =
        EmergeSkia.start_owned(
          %Emerge.Config{
            output: %Emerge.Config.DRM{card: card, connector: output.name, mode: mode},
            renderer: api,
            assets: [otp_app: :emerge],
            diagnostics: [logs: [], stats: true, force_gpu_finish: false],
            runtime: [startup_retries: 0]
          },
          self()
        )

      try do
        assert {:ok,
                %{
                  backend: :drm,
                  rendering_api: %{selected: :raster},
                  capabilities: %{gpu: false, prime_video: false, raster_present: presentations}
                }} = EmergeSkia.renderer_info(renderer)

        assert :cpu in presentations
        # Readiness returns without a tree; cursor wakes cannot present a placeholder.
        Process.sleep(100)
        assert {:ok, %{frames: %{frame_count: 0}}} = EmergeSkia.stats(renderer, :peek)

        Enum.each([{10, 20, 30}, {170, 60, 90}, {20, 160, 50}], fn {r, g, b} ->
          tree =
            el(
              [
                width(px(mode.width)),
                height(px(mode.height)),
                Emerge.UI.Background.color(Emerge.UI.Color.color_rgb(r, g, b))
              ],
              none()
            )

          EmergeSkia.upload_tree(renderer, tree)
          expected = :binary.copy(<<r, g, b, 255>>, mode.width * mode.height)

          assert eventually(fn -> EmergeSkia.render_to_pixels(renderer) == {:ok, expected} end),
                 "last capture: #{inspect(EmergeSkia.render_to_pixels(renderer), limit: 8, printable_limit: 32)}"
        end)

        refute_receive {:emerge_skia_frame, _}, 50
      after
        assert :ok = EmergeSkia.stop(renderer, timeout: 5000)
      end

      assert {:ok, %{cleanup_complete: true}} = EmergeSkia.renderer_status(renderer)
    end
  end

  defp eventually(fun, attempts \\ 50)

  defp eventually(fun, 0) do
    fun.()
  end

  defp eventually(fun, attempts) do
    if fun.() do
      true
    else
      Process.sleep(20)
      eventually(fun, attempts - 1)
    end
  end
end
