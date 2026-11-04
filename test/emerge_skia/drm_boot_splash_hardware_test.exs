defmodule EmergeSkia.DrmBootSplashHardwareTest do
  use ExUnit.Case, async: false
  use Emerge.UI

  @moduletag :drm_hardware
  @moduletag timeout: 30_000

  # Run this file alone after an authorized boot with inherited scanout active.
  # Other DRM tests intentionally detach their output and invalidate this setup.
  test "delayed first upload preserves the inherited output and uses a compatible handoff" do
    card = System.fetch_env!("EMERGE_DRM_TEST_CARD")
    connector = System.fetch_env!("EMERGE_DRM_TEST_CONNECTOR")
    mode_id = System.fetch_env!("EMERGE_DRM_TEST_MODE")
    backlight = System.get_env("EMERGE_DRM_TEST_BACKLIGHT")

    renderer =
      case System.get_env("EMERGE_DRM_TEST_API", "opengl") do
        "opengl" -> [api: :opengl]
        "raster" -> [api: :raster, raster_present: :cpu]
        "gpu_upload" -> [api: :raster, raster_present: :gpu_upload]
        "vulkan" -> [api: :vulkan, device: System.fetch_env!("EMERGE_DRM_TEST_VULKAN_NODE")]
        other -> raise ArgumentError, "unsupported DRM handoff renderer: #{inspect(other)}"
      end

    {:ok, outputs} = EmergeSkia.drm_outputs(drm_card: card)
    output = Enum.find(outputs, &(&1.name == connector and &1.connected))
    assert output
    mode = Enum.find(output.modes, &(&1.id == mode_id))
    assert mode, "select the inherited output's exact advertised mode ID"
    {real_card, 0} = System.cmd("readlink", ["-f", card])
    status = "/sys/class/drm/#{Path.basename(String.trim(real_card))}-#{connector}/enabled"
    assert String.trim(File.read!(status)) == "enabled", "boot with the inherited splash active"
    inherited_light = light_state(backlight)

    if inherited_light do
      assert inherited_light.power == "0"
      assert String.to_integer(inherited_light.brightness) > 0
    end

    config = %Emerge.Config{
      output: %Emerge.Config.DRM{
        card: card,
        connector: connector,
        mode: mode.id,
        backlight: [device: backlight, brightness: nil]
      },
      renderer: renderer,
      assets: [otp_app: :emerge],
      diagnostics: [stats: true],
      runtime: [startup_retries: 0]
    }

    {:ok, handle} = EmergeSkia.start(config)

    try do
      # Readiness must return without waiting for this first application upload.
      Process.sleep(1_000)
      assert frames(handle) == 0
      assert String.trim(File.read!(status)) == "enabled"
      assert light_state(backlight) == inherited_light

      EmergeSkia.upload_tree(
        handle,
        el(
          [
            width(px(mode.width)),
            height(px(mode.height)),
            Background.color(Emerge.UI.Color.color_rgb(30, 80, 220))
          ],
          []
        )
      )

      assert eventually(fn -> frames(handle) > 0 end)

      assert_receive {:emerge_skia_log, :info, "drm_handoff",
                      "replacing inherited scanout without modeset"},
                     5_000

      assert String.trim(File.read!(status)) == "enabled"
      assert light_state(backlight) == inherited_light
      assert {:ok, pixels} = EmergeSkia.render_to_pixels(handle)
      assert binary_part(pixels, byte_size(pixels) - 4, 4) == <<30, 80, 220, 255>>
    after
      assert :ok = EmergeSkia.stop(handle, timeout: 5_000)
    end

    assert {:ok, %{cleanup_complete: true}} = EmergeSkia.renderer_status(handle)
    assert String.trim(File.read!(status)) == "disabled"

    IO.puts(
      "Handoff readbacks passed; retain operation traces and optical observations separately."
    )
  end

  defp light_state(nil), do: nil

  defp light_state(path) do
    %{
      brightness: path |> Path.join("brightness") |> File.read!() |> String.trim(),
      power: path |> Path.join("bl_power") |> File.read!() |> String.trim()
    }
  end

  defp frames(handle) do
    {:ok, %{frames: %{frame_count: count}}} = EmergeSkia.stats(handle, :peek)
    count
  end

  defp eventually(check, attempts \\ 200)
  defp eventually(check, 0), do: check.()

  defp eventually(check, attempts) do
    if check.() do
      true
    else
      Process.sleep(10)
      eventually(check, attempts - 1)
    end
  end
end
