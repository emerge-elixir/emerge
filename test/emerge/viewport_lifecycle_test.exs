defmodule Emerge.ViewportLifecycleTest do
  use ExUnit.Case, async: false
  @moduletag capture_log: true

  defmodule Screen do
    use Emerge

    @impl Viewport
    def mount(opts) do
      {:ok, %{count: 0, keep_open: Keyword.get(opts, :keep_open, false), closed: false},
       [
         otp_app: :emerge,
         backend: :headless,
         rendering_api: :raster,
         width: 32,
         height: 24,
         headless: [target: Keyword.fetch!(opts, :target)],
         viewport: [renderer_check_interval_ms: 25]
       ]}
    end

    @impl Viewport
    def render(state), do: text("count #{state.count}")

    @impl Viewport
    def handle_close(_reason, %{keep_open: true} = state), do: {:noreply, %{state | closed: true}}
    def handle_close(_reason, state), do: {:stop, :normal, state}

    @impl Viewport
    def handle_info(:increment, state),
      do: {:noreply, Viewport.rerender(%{state | count: state.count + 1})}
  end

  test "Emerge has no automatically started application callback" do
    assert Application.spec(:emerge, :mod) == []
  end

  test "renderer replacement preserves public PID, callback state and message routing" do
    {:ok, viewport} = Screen.start_link(target: self(), name: __MODULE__.ScreenName)
    renderer = Emerge.renderer(viewport)
    old_generation = :sys.get_state(viewport).__emerge__.renderer_generation
    send(viewport, :increment)
    assert eventually(fn -> :sys.get_state(viewport).count == 1 end)
    assert :ok = EmergeSkia.stop(renderer)

    assert eventually(fn ->
             replacement = Emerge.renderer(viewport)
             replacement != nil and replacement != renderer and EmergeSkia.running?(replacement)
           end)

    assert Process.whereis(__MODULE__.ScreenName) == viewport
    assert :sys.get_state(viewport).count == 1

    send(
      viewport,
      {:emerge_viewport_renderer, old_generation, {:emerge_skia_close, :window_close_requested}}
    )

    send(viewport, :increment)
    assert eventually(fn -> :sys.get_state(viewport).count == 2 end)
    GenServer.stop(viewport)
  end

  test "killed callback releases renderer and endpoint even when the handle is retained" do
    {:ok, viewport} = Screen.start_link(target: self())
    Process.unlink(viewport)
    renderer = Emerge.renderer(viewport)
    runtime = :sys.get_state(viewport).__emerge__
    ref = Process.monitor(viewport)
    Process.exit(viewport, :kill)
    assert_receive {:DOWN, ^ref, :process, ^viewport, :killed}

    assert eventually(fn -> clean?(renderer) end)
    assert eventually(fn -> not Process.alive?(runtime.lifecycle_supervisor) end)
    frame = VideoInterop.Frame.binary(<<0, 0, 0>>, width: 1, height: 1, pixel_format: :rgb888)
    assert {:error, :viewport_not_ready} = Emerge.submit_video_frame(viewport, :preview, frame)
    assert :ok = EmergeSkia.stop(renderer)
  end

  test "killed lifecycle owner cannot leave a live orphan native session" do
    {:ok, viewport} = Screen.start_link(target: self())
    Process.unlink(viewport)
    renderer = Emerge.renderer(viewport)
    owner = :sys.get_state(viewport).__emerge__.lifecycle
    ref = Process.monitor(viewport)
    Process.exit(owner, :kill)
    assert_receive {:DOWN, ^ref, :process, ^viewport, _}, 8_000
    assert eventually(fn -> clean?(renderer) end)
  end

  test "a stopped private supervisor ends the public instance rather than leaving a zombie" do
    {:ok, viewport} = Screen.start_link(target: self())
    renderer = Emerge.renderer(viewport)
    supervisor = :sys.get_state(viewport).__emerge__.lifecycle_supervisor
    ref = Process.monitor(viewport)
    Supervisor.stop(supervisor, :normal)
    assert_receive {:DOWN, ^ref, :process, ^viewport, :normal}, 5_000
    assert clean?(renderer)
  end

  test "a dead session relay is replaced without changing the callback PID" do
    {:ok, viewport} = Screen.start_link(target: self())
    old = Emerge.renderer(viewport)
    relay = :sys.get_state(viewport).__emerge__.renderer_relay
    Process.exit(relay, :kill)

    assert eventually(fn ->
             current = Emerge.renderer(viewport)
             current != nil and current != old and EmergeSkia.running?(current)
           end)

    GenServer.stop(viewport)
  end

  test "queued normal close beats recovery and still honors the callback's close override" do
    {:ok, viewport} = Screen.start_link(target: self(), keep_open: true)
    renderer = Emerge.renderer(viewport)
    relay = :sys.get_state(viewport).__emerge__.renderer_relay
    :erlang.suspend_process(relay)
    send(relay, {:emerge_skia_close, :window_close_requested})
    assert :ok = EmergeSkia.stop(renderer)
    assert eventually(fn -> Emerge.renderer(viewport) == nil end)
    Process.sleep(350)
    assert Emerge.renderer(viewport) == nil
    :erlang.resume_process(relay)
    assert eventually(fn -> :sys.get_state(viewport).closed end)
    Process.sleep(350)
    assert Process.alive?(viewport)
    assert Emerge.renderer(viewport) == nil
    GenServer.stop(viewport)
  end

  test "concurrent low-level stops observe the same cleanup completion" do
    {:ok, viewport} = Screen.start_link(target: self())
    renderer = Emerge.renderer(viewport)
    results = 1..8 |> Task.async_stream(fn _ -> EmergeSkia.stop(renderer) end) |> Enum.to_list()
    assert Enum.all?(results, &(&1 == {:ok, :ok}))
    assert clean?(renderer)
    GenServer.stop(viewport)
  end

  test "PRIME completion also requires the relay's proven drain, not just native teardown" do
    {:ok, viewport} = Screen.start_link(target: self())
    renderer = Emerge.renderer(viewport)
    GenServer.stop(viewport)
    assert clean?(renderer)
    completion = :atomics.new(1, signed: false)

    session = %EmergeSkia.HeadlessPrimeSession{
      pid: viewport,
      renderer: renderer,
      completion: completion
    }

    assert {:ok, %{state: :quarantined, cleanup_complete: false}} =
             EmergeSkia.renderer_status(session)

    assert {:error, :cleanup_unproven} = EmergeSkia.stop(session)
    :atomics.put(completion, 1, 1)
    assert {:ok, %{state: :stopped, cleanup_complete: true}} = EmergeSkia.renderer_status(session)
    assert :ok = EmergeSkia.stop(session)
  end

  test "low-level owner monitoring is independent of resource garbage collection" do
    test = self()

    owner =
      spawn(fn ->
        {:ok, renderer} =
          EmergeSkia.start(
            otp_app: :emerge,
            backend: :headless,
            rendering_api: :raster,
            width: 8,
            height: 8,
            headless: [target: test],
            owner: self()
          )

        send(test, {:owned_renderer, renderer})

        receive do
          :exit -> :ok
        end
      end)

    assert_receive {:owned_renderer, renderer}, 5_000
    send(owner, :exit)
    assert eventually(fn -> clean?(renderer) end)
    assert :ok = EmergeSkia.stop(renderer, timeout: 0)
    assert :ok = EmergeSkia.stop(renderer, timeout: 0)
  end

  defp clean?(renderer) do
    match?(
      {:ok, %{state: :stopped, cleanup_complete: true}},
      EmergeSkia.renderer_status(renderer)
    )
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
