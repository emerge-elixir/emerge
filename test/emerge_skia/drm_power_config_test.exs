defmodule EmergeSkia.DRMPowerConfigTest do
  use ExUnit.Case, async: true
  alias Emerge.Config

  @renderers [
    [api: :opengl],
    [api: :vulkan, device: "/dev/dri/renderD128"],
    [api: :raster, raster_present: :auto],
    [api: :raster, raster_present: :cpu],
    [api: :raster, raster_present: :gpu_upload]
  ]

  defp config(renderer, display \\ :on, brightness \\ 0.5) do
    Config.Normalize.config!(%Config{
      output: %Config.DRM{
        card: "/dev/dri/card0",
        connector: "DSI-1",
        display: display,
        backlight: [device: "/sys/class/backlight/1a94000.dsi.0", brightness: brightness]
      },
      renderer: renderer,
      assets: [otp_app: :emerge]
    })
  end

  test "explicit renderers accept brightness and initial-off without hardware" do
    for renderer <- @renderers,
        display <- [:on, :off],
        brightness <- [nil, 0.0, 0.5, 1.0] do
      config = config(renderer, display, brightness)
      assert :ok = EmergeSkia.validate_config!(config)
      native = config |> Config.Native.options() |> Keyword.fetch!(:drm_power)
      assert native == %{display_on: display == :on, brightness: brightness}
    end
  end

  test "power and brightness are live, but connector and mode changes stay rejected" do
    for renderer <- @renderers do
      on = config(renderer)
      off = %{on | output: %{on.output | display: :off}}

      dimmed = %{
        off
        | output: %{off.output | backlight: %{off.output.backlight | brightness: 0.2}}
      }

      resumed = %{dimmed | output: %{dimmed.output | display: :on}}

      restored = %{
        resumed
        | output: %{resumed.output | backlight: %{resumed.output.backlight | brightness: nil}}
      }

      assert :ok = EmergeSkia.validate_config_change(on, off)
      assert :ok = EmergeSkia.validate_config_change(off, dimmed)
      assert :ok = EmergeSkia.validate_config_change(dimmed, resumed)
      assert :ok = EmergeSkia.validate_config_change(resumed, restored)

      for changed <- [%{on.output | connector: "DSI-2"}, %{on.output | mode: "1080x2160@60"}] do
        assert {:error, {:unsupported_live_config, :drm_output}} =
                 EmergeSkia.validate_config_change(on, %{on | output: changed})
      end
    end
  end

  test "Vulkan accepts automatic and unmanaged backlights but still requires a device" do
    config = config([api: :vulkan, device: "/dev/dri/renderD128"], :off, nil)

    for device <- [:auto, nil] do
      config = %{
        config
        | output: %{config.output | backlight: %{config.output.backlight | device: device}}
      }

      assert :ok = EmergeSkia.validate_config!(config)
    end

    assert_raise ArgumentError, ~r/config.renderer.device/, fn ->
      config(api: :vulkan)
    end

    assert_raise ArgumentError, ~r/a managed backlight device/, fn ->
      Config.Normalize.config!(%{
        config
        | output: %{config.output | backlight: [device: nil, brightness: 0.5]}
      })
    end
  end

  test "Vulkan brightness remains bounded and typed" do
    for invalid <- [-0.1, 1.1, :nan, :infinity] do
      assert_raise ArgumentError, ~r/config.output.backlight.brightness/, fn ->
        config([api: :vulkan, device: "/dev/dri/renderD128"], :on, invalid)
      end
    end
  end

  if :drm in EmergeSkia.BuildConfig.compiled_backends() and
       :drm not in EmergeSkia.BuildConfig.compiled_opengl_backends() do
    @tag :linux_only
    test "GPU-upload power still requires the compiled OpenGL presenter" do
      config = config([api: :raster, raster_present: :gpu_upload], :off, nil)
      config = %{config | output: %{config.output | card: "/dev/dri/not-a-test-card"}}
      assert :ok = EmergeSkia.validate_config!(config)

      assert {:error, {:error, "OpenGL DRM rendering support is not available in this build"}} =
               EmergeSkia.start(config)
    end
  end

  test "automatic API selection remains guarded for every presentation choice" do
    for renderer <- [
          [api: :auto],
          [api: :auto, raster_present: :cpu],
          [api: :auto, raster_present: :gpu_upload]
        ] do
      config = config(renderer)

      assert_raise ArgumentError,
                   ~r/requires an explicit OpenGL, Vulkan or raster renderer/,
                   fn ->
                     EmergeSkia.validate_config!(config)
                   end

      off = %{config | output: %{config.output | display: :off}}

      assert {:error, {:unsupported_live_config, :drm_output}} =
               EmergeSkia.validate_config_change(config, off)
    end
  end
end
