defmodule Emerge.ConfigTest do
  use ExUnit.Case, async: true
  alias Emerge.Config
  alias Emerge.Config.Normalize

  test "defaults normalize recursively and idempotently" do
    config = Normalize.config!(%Config{}, Emerge)
    assert %Config.Window{} = config.output

    assert %Config.Backlight{} =
             Normalize.config!(%Config{output: %Config.DRM{}}).output.backlight

    assert config.assets.otp_app == :emerge
    assert config.input.events == []
    assert Normalize.config!(config) == config
  end

  test "nested shorthand and explicit structs have the same result" do
    short = %Config{
      output: %Config.DRM{connector: "DSI-1", backlight: [brightness: 0.7]},
      input: [events: [:resize, :key, :key]]
    }

    explicit = %Config{
      output: %Config.DRM{connector: "DSI-1", backlight: %Config.Backlight{brightness: 0.7}},
      input: %Config.Input{events: [:key, :resize]}
    }

    assert Normalize.config!(short) == Normalize.config!(explicit)
  end

  test "typed updates preserve device and other records" do
    config =
      Normalize.config!(%Config{
        output: %Config.DRM{
          connector: "DSI-1",
          backlight: [device: "/sys/class/backlight/panel", brightness: 0.7]
        }
      })

    %Config{output: %Config.DRM{backlight: %Config.Backlight{} = backlight} = output} = config
    backlight = %Config.Backlight{backlight | brightness: 0.2}
    output = %Config.DRM{output | backlight: backlight, display: :off}
    next = Normalize.config!(%Config{config | output: output})
    assert next.output.backlight.device == config.output.backlight.device
    assert next.output.connector == "DSI-1"
    assert config.output.backlight.brightness == 0.7
  end

  test "new records use defaults instead of a hidden merge" do
    previous =
      Normalize.config!(%Config{
        output: %Config.DRM{backlight: [device: "/sys/class/backlight/panel"]}
      })

    %Config{output: %Config.DRM{} = output} = previous
    output = %Config.DRM{output | backlight: [brightness: 0.2]}
    assert Normalize.config!(%Config{previous | output: output}).output.backlight.device == :auto
  end

  test "unknown fields, wrong records and invalid enum values include a path" do
    assert_raise ArgumentError, ~r/config.output.backlight.brightnes/, fn ->
      Normalize.config!(%Config{output: %Config.DRM{backlight: [brightnes: 0.5]}})
    end

    assert_raise ArgumentError, ~r/config.renderer/, fn ->
      Normalize.config!(%Config{renderer: %Config.Input{}})
    end

    assert_raise ArgumentError, ~r/config.input.events.*one of/, fn ->
      Normalize.config!(%Config{input: [events: [:keys]]})
    end

    assert_raise ArgumentError, ~r/config.output.backlight.brightness/, fn ->
      Normalize.config!(%Config{output: %Config.DRM{backlight: [brightness: 2]}})
    end

    assert_raise ArgumentError, ~r/removed in 0.5/, fn -> Normalize.config!([]) end
  end

  test "unfinished native controls fail explicitly instead of silently ignoring intent" do
    for output <- [
          %Config.DRM{display: :off},
          %Config.DRM{backlight: [brightness: 0.2]},
          %Config.DRM{backlight: [device: "/sys/class/backlight/panel"]}
        ] do
      config = Normalize.config!(%Config{output: output})

      assert_raise ArgumentError, ~r/display\/backlight control is not implemented/, fn ->
        EmergeSkia.validate_config!(config)
      end
    end

    for output <- [%Config.DRM{}, %Config.Window{backend: :macos}] do
      config = Normalize.config!(%Config{output: output, renderer: [target_fps: 30]})

      assert_raise ArgumentError,
                   ~r/target_fps is currently supported only by headless and Wayland/,
                   fn ->
                     EmergeSkia.validate_config!(config)
                   end
    end
  end

  test "Wayland startup caps use the common native field, not headless delivery options" do
    for api <- [:raster, :opengl, :vulkan] do
      config =
        Normalize.config!(%Config{
          output: %Config.Window{backend: :wayland},
          renderer: [api: api, target_fps: 30]
        })

      assert :ok = EmergeSkia.validate_config!(config)
      opts = config |> Config.Native.options() |> EmergeSkia.Options.build_start_native_opts!()
      assert opts.target_fps == 30
      refute Map.has_key?(opts.headless, :target_fps)
    end
  end

  test "all input categories map to internal bits and no subscriptions means zero" do
    assert Config.Native.input_mask(%Config.Input{}) == 0
    assert Config.Native.input_mask(%Config.Input{events: Config.Input.all_events()}) == 255

    for {event, bit} <- Enum.zip(Config.Input.all_events(), [1, 2, 4, 8, 16, 32, 64, 128]) do
      assert Config.Native.input_mask(%Config.Input{events: [event]}) == bit
    end
  end

  test "input payloads have consistent names and typed actions" do
    assert Emerge.Input.from_native({:key, {:a, 1, [:shift]}}) == {:key, {:a, :press, [:shift]}}

    assert Emerge.Input.from_native({:cursor_button, {:left, 0, [], {1, 2}}}) ==
             {:pointer_button, {:left, :release, [], {1, 2}}}

    assert Emerge.Input.from_native({:resized, {800, 600, 1.0}}) == {:resize, {800, 600, 1.0}}
    assert Emerge.Input.from_native(:text_preedit_clear) == :text_preedit_clear
  end
end
