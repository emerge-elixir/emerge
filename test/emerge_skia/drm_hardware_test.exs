defmodule EmergeSkia.DrmHardwareTest do
  use ExUnit.Case, async: false
  @moduletag :drm_hardware
  @moduletag capture_log: true
  @moduletag timeout: 90_000

  defmodule Screen do
    use Emerge
    @impl Viewport
    def mount(opts), do: {:ok, %{count: 0}, Keyword.merge([otp_app: :emerge, stats: true], opts)}
    @impl Viewport
    def render(state),
      do:
        el(
          [Background.color(color(:slate, 800)), Font.size(48)],
          text("DRM viewport #{state.count}")
        )

    @impl Viewport
    def handle_info(:tick, state),
      do: {:noreply, Viewport.rerender(%{state | count: state.count + 1})}
  end

  test "two explicitly selected outputs keep presenting across stop, restart and owner kill" do
    card = System.fetch_env!("EMERGE_DRM_TEST_CARD")
    {:ok, outputs} = EmergeSkia.drm_outputs(drm_card: card)
    [a, b | _] = Enum.filter(outputs, &(&1.connected and &1.modes != []))

    api =
      case System.get_env("EMERGE_DRM_TEST_API", "opengl") do
        "vulkan" -> :vulkan
        "raster" -> :raster
        "opengl" -> :opengl
      end

    options = fn output ->
      mode = Enum.find(output.modes, & &1.preferred) || hd(output.modes)

      [
        backend: :drm,
        drm_card: card,
        drm_output: output.name,
        drm_mode: mode,
        rendering_api: api,
        vulkan_drm_node: card,
        drm_startup_retries: 0,
        viewport: [renderer_check_interval_ms: 50]
      ]
    end

    {:ok, first} = Screen.start_link(options.(a))
    {:ok, second} = Screen.start_link(options.(b))
    Process.unlink(first)
    Process.unlink(second)

    on_exit(fn ->
      for viewport <- [first, second], Process.alive?(viewport), do: GenServer.stop(viewport)
    end)

    renderer_a = Emerge.renderer(first)
    renderer_b = Emerge.renderer(second)
    assert is_reference(renderer_a)
    assert is_reference(renderer_b)
    assert eventually(fn -> frames(renderer_a) > 0 and frames(renderer_b) > 0 end)
    assert {:error, _} = EmergeSkia.start(Keyword.put(options.(a), :otp_app, :emerge))

    Enum.each(1..3, fn _ ->
      old = Emerge.renderer(first)
      assert :ok = EmergeSkia.stop(old)

      assert eventually(fn ->
               current = Emerge.renderer(first)
               current != nil and current != old and frames(current) > 0
             end)

      count = frames(renderer_b)
      send(second, :tick)
      assert eventually(fn -> frames(renderer_b) > count end)
      assert Emerge.renderer(second) == renderer_b
    end)

    retained = Emerge.renderer(first)
    ref = Process.monitor(first)
    Process.exit(first, :kill)
    assert_receive {:DOWN, ^ref, :process, ^first, :killed}, 5_000

    assert eventually(fn ->
             match?({:ok, %{cleanup_complete: true}}, EmergeSkia.renderer_status(retained))
           end)

    count = frames(renderer_b)
    send(second, :tick)
    assert eventually(fn -> frames(renderer_b) > count end)

    {:ok, replacement} = Screen.start_link(options.(a))
    assert eventually(fn -> frames(Emerge.renderer(replacement)) > 0 end)
    GenServer.stop(replacement)
    GenServer.stop(second)

    assert eventually(fn ->
             match?({:ok, %{cleanup_complete: true}}, EmergeSkia.renderer_status(renderer_b))
           end)
  end

  defp frames(nil), do: 0

  defp frames(renderer) do
    case EmergeSkia.stats(renderer, :peek) do
      {:ok, %{frames: %{frame_count: count}}} -> count
      _ -> 0
    end
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
