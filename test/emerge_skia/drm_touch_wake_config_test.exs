defmodule EmergeSkia.DRMTouchWakeConfigTest do
  use ExUnit.Case, async: true
  alias Emerge.Config
  alias Emerge.Config.Normalize

  defp config(wake \\ nil, api \\ :raster) do
    Normalize.config!(%Config{
      output: %Config.DRM{wake_on_touch: wake},
      renderer: [api: api, device: if(api == :vulkan, do: "/dev/dri/renderD128", else: nil)],
      assets: [otp_app: :emerge]
    })
  end

  test "nil is the default and crosses both native adapters unchanged" do
    assert %Config.DRM{}.wake_on_touch == nil
    normalized = config()
    assert normalized.output.wake_on_touch == nil
    native = Config.Native.options(normalized) |> EmergeSkia.Options.build_start_native_opts!()
    assert native.drm_power.wake_on_touch == nil

    assert EmergeSkia.Native.power_defaults(%{display_on: false, brightness: nil}) ==
             %{display_on: false, brightness: nil, wake_on_touch: nil}

    assert EmergeSkia.Native.power_defaults(nil) == nil
  end

  test "all gestures are live with each explicit DRM renderer, including while off" do
    for gesture <- [nil, :single_tap, :double_tap, :long_press],
        api <- [:raster, :opengl, :vulkan] do
      previous = config(nil, api)
      current = config(gesture, api)
      off = %{current | output: %{current.output | display: :off}}
      assert :ok = EmergeSkia.validate_config!(current)
      assert :ok = Config.Changes.validate!(previous, off)
      assert :ok = EmergeSkia.validate_config_change(previous, off)
      native = Config.Native.options(off) |> EmergeSkia.Options.build_start_native_opts!()
      assert native.drm_power.wake_on_touch == gesture
      assert native.drm_power.display_on == false
    end
  end

  test "invalid policies are rejected and automatic API selection cannot enable wake" do
    for invalid <- [false, true, :tap, [], [:single_tap], "double_tap", 1] do
      assert_raise ArgumentError, ~r/wake_on_touch/, fn -> config(invalid) end
    end

    for gesture <- [:single_tap, :double_tap, :long_press] do
      assert_raise ArgumentError, ~r/explicit OpenGL, Vulkan or raster/, fn ->
        EmergeSkia.validate_config!(config(gesture, :auto))
      end
    end

    assert :ok = EmergeSkia.validate_config!(config(nil, :auto))
  end

  test "gesture atoms decode through the NIF and reach backend validation without hardware" do
    {:ok, renderer} =
      EmergeSkia.start(%Config{
        output: %Config.Headless{target: self(), size: {4, 4}},
        renderer: [api: :raster],
        assets: [otp_app: :emerge]
      })

    try do
      for gesture <- [nil, :single_tap, :double_tap, :long_press] do
        assert {:error, "DRM power controls require a DRM owner"} =
                 EmergeSkia.Native.configure(renderer, %{
                   title: nil,
                   target_fps: nil,
                   input_mask: 0,
                   drm_power: %{display_on: false, brightness: nil, wake_on_touch: gesture}
                 })
      end

      assert {:ok, %{desired_revision: 0}} = EmergeSkia.config_status(renderer)
    after
      assert :ok = EmergeSkia.stop(renderer)
    end
  end

  test "wake does not add raw touch subscriptions or alter brightness" do
    current = config(:long_press)
    assert Config.Native.input_mask(current.input) == 0
    assert current.output.backlight.brightness == nil
    assert current.output.display == :on
  end
end
