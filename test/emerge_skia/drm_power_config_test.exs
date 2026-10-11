defmodule EmergeSkia.DRMPowerConfigTest do
  use ExUnit.Case, async: true
  alias Emerge.Config

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

  test "OpenGL and CPU raster accept brightness and initial-off without opening hardware" do
    for renderer <- [
          [api: :opengl],
          [api: :raster, raster_present: :auto],
          [api: :raster, raster_present: :cpu]
        ],
        display <- [:on, :off],
        brightness <- [nil, 0.0, 0.5, 1.0] do
      config = config(renderer, display, brightness)
      assert :ok = EmergeSkia.validate_config!(config)
      native = config |> Config.Native.options() |> Keyword.fetch!(:drm_power)
      assert native == %{display_on: display == :on, brightness: brightness}
    end
  end

  test "power and brightness are live, but connector and mode changes stay rejected" do
    on = config(api: :opengl)
    off = %{on | output: %{on.output | display: :off}}
    dimmed = %{off | output: %{off.output | backlight: %{off.output.backlight | brightness: 0.2}}}
    resumed = %{dimmed | output: %{dimmed.output | display: :on}}
    assert :ok = EmergeSkia.validate_config_change(on, off)
    assert :ok = EmergeSkia.validate_config_change(off, dimmed)
    assert :ok = EmergeSkia.validate_config_change(dimmed, resumed)

    for changed <- [%{on.output | connector: "DSI-2"}, %{on.output | mode: "1080x2160@60"}] do
      assert {:error, {:unsupported_live_config, :drm_output}} =
               EmergeSkia.validate_config_change(on, %{on | output: changed})
    end
  end

  test "unfinished GPU power paths remain guarded" do
    for renderer <- [
          [api: :auto],
          [api: :vulkan, device: "/dev/dri/renderD128"],
          [api: :raster, raster_present: :gpu_upload]
        ] do
      config = config(renderer)

      assert_raise ArgumentError, ~r/requires OpenGL or CPU raster presentation/, fn ->
        EmergeSkia.validate_config!(config)
      end

      off = %{config | output: %{config.output | display: :off}}

      assert {:error, {:unsupported_live_config, :drm_output}} =
               EmergeSkia.validate_config_change(config, off)
    end
  end
end
