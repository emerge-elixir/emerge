defmodule EmergeSkia.DRMTouchWakeHardwareTest do
  use ExUnit.Case, async: false
  @moduletag :drm_hardware
  @moduletag timeout: 60_000

  # Interactive: run this file alone on an authorized panel/touchscreen. No
  # synthetic input, device permissions or system power settings are changed.
  defmodule Screen do
    use Emerge
    def mount(opts), do: {:ok, %{observer: opts[:observer]}, opts[:config]}

    def render(_state) do
      el(
        [
          width(fill()),
          height(fill()),
          Background.color(Emerge.UI.Color.color_rgb(20, 70, 120)),
          Event.on_press(:wake_through)
        ],
        text("Touch wake: the waking gesture must not activate this area")
      )
    end

    def handle_input({:display_wake, gesture}, state) do
      send(state.observer, {:woke, gesture})
      {:noreply, state}
    end

    def handle_input({:touch, _} = event, state) do
      send(state.observer, {:raw_touch, event})
      {:noreply, state}
    end

    def handle_input(_, state), do: {:noreply, state}

    def handle_info(:wake_through, state) do
      send(state.observer, :wake_through)
      {:noreply, state}
    end
  end

  test "an off panel wakes once without delivering the waking contact to the UI" do
    gesture =
      case System.fetch_env!("EMERGE_DRM_TEST_WAKE_GESTURE") do
        "nil" -> nil
        "single_tap" -> :single_tap
        "double_tap" -> :double_tap
        "long_press" -> :long_press
      end

    rendering =
      case System.get_env("EMERGE_DRM_TEST_API", "opengl") do
        "opengl" -> [api: :opengl]
        "raster" -> [api: :raster, raster_present: :cpu]
        "gpu_upload" -> [api: :raster, raster_present: :gpu_upload]
        "vulkan" -> [api: :vulkan, device: System.fetch_env!("EMERGE_DRM_TEST_VULKAN_NODE")]
      end

    config = %Emerge.Config{
      output: %Emerge.Config.DRM{
        card: System.fetch_env!("EMERGE_DRM_TEST_CARD"),
        connector: System.fetch_env!("EMERGE_DRM_TEST_CONNECTOR"),
        display: :off,
        wake_on_touch: gesture,
        backlight: [device: System.get_env("EMERGE_DRM_TEST_BACKLIGHT")]
      },
      renderer: rendering,
      input: [events: [:touch, :display_wake]],
      assets: [otp_app: :emerge],
      diagnostics: [stats: true],
      runtime: [startup_retries: 0]
    }

    {:ok, viewport} = Screen.start_link(observer: self(), config: config)
    renderer = Emerge.renderer(viewport)

    try do
      Process.sleep(500)
      assert :sys.get_state(viewport).__emerge__.config.output.display == :off

      IO.puts(
        "Apply only the selected touch gesture: #{inspect(gesture)}. Release after the screen lights; do not touch again."
      )

      if is_nil(gesture) do
        refute_receive {:woke, _}, 5_000
        assert :sys.get_state(viewport).__emerge__.config.output.display == :off
      else
        assert_receive {:woke, ^gesture}, 30_000

        assert eventually(fn ->
                 runtime = :sys.get_state(viewport).__emerge__
                 runtime.config.output.display == :on && runtime.config_pending == nil
               end)

        assert {:ok, %{frames: %{frame_count: count}}} = EmergeSkia.stats(renderer, :peek)
        assert count > 0
        refute_receive :wake_through, 1_000
        refute_receive {:raw_touch, _}, 100
        refute_receive {:woke, _}, 100
      end
    after
      GenServer.stop(viewport)
    end

    assert {:ok, %{cleanup_complete: true}} = EmergeSkia.renderer_status(renderer)
  end

  defp eventually(fun, attempts \\ 250)
  defp eventually(fun, 0), do: fun.()

  defp eventually(fun, attempts) do
    if fun.(),
      do: true,
      else:
        (
          Process.sleep(20)
          eventually(fun, attempts - 1)
        )
  end
end
