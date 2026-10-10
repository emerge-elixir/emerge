defmodule Emerge.ConfigCallbackTest do
  use ExUnit.Case, async: false
  import ExUnit.CaptureLog
  alias Emerge.Config

  defmodule Adapter do
    @behaviour Emerge.Runtime.Viewport.Renderer
    def start(config, opts) do
      send(Keyword.fetch!(opts, :observer), {:started, config})
      Agent.start_link(fn -> config end)
    end

    def configure(renderer, _old, config), do: Agent.update(renderer, fn _ -> config end)
    def stop(renderer), do: Agent.stop(renderer)
    def running?(renderer), do: Process.alive?(renderer)
    def set_input_target(_, _), do: :ok
    def set_log_target(_, _), do: :ok
    def set_input_mask(_, _), do: :ok

    def upload_tree(_, tree) do
      diff = Emerge.Engine.diff_state_new(tree)
      {diff, diff.tree}
    end

    def patch_tree(_, diff, tree) do
      {_, next, assigned} = Emerge.Engine.diff_state_update(diff, tree)
      {next, assigned}
    end
  end

  defmodule Screen do
    use Emerge

    def mount(opts) do
      observer = Keyword.fetch!(opts, :observer)

      config = %Config{
        output: %Config.DRM{
          connector: "DSI-1",
          backlight: [device: "/sys/class/backlight/panel", brightness: 0.7]
        },
        runtime: [
          adapter: Adapter,
          adapter_opts: [observer: observer],
          health_check_interval_ms: nil
        ]
      }

      {:ok,
       %{
         observer: observer,
         display: Keyword.get(opts, :display, :on),
         brightness: 0.7,
         bad?: false
       }, config}
    end

    def config(
          %Config{output: %Config.DRM{backlight: %Config.Backlight{} = backlight} = output} =
            previous,
          state
        ) do
      send(state.observer, {:previous, previous})
      if state.bad?, do: raise("bad config")
      backlight = %Config.Backlight{backlight | brightness: state.brightness}
      output = %Config.DRM{output | display: state.display, backlight: backlight}
      %Config{previous | output: output}
    end

    def render(state) do
      send(state.observer, :rendered)
      text("unchanged tree")
    end

    def handle_info({:change, changes}, state),
      do: {:noreply, Viewport.rerender(Map.merge(state, changes))}
  end

  defmodule NativeScreen do
    use Emerge

    def mount(opts) do
      observer = Keyword.fetch!(opts, :observer)
      output = %Config.Headless{target: observer, size: {8, 8}}
      {:ok, %{observer: observer, events: []}, %Config{output: output, renderer: [api: :raster]}}
    end

    def config(%Config{input: %Config.Input{} = input} = previous, state) do
      send(state.observer, {:native_previous, previous})
      input = %Config.Input{input | events: state.events}
      %Config{previous | input: input}
    end

    def render(state) do
      send(state.observer, :native_rendered)
      text("unchanged")
    end

    def handle_info({:events, events}, state),
      do: {:noreply, Viewport.rerender(%{state | events: events})}
  end

  defmodule StatelessScreen do
    use Emerge

    def mount(opts),
      do:
        {:ok,
         %Config{runtime: [adapter: Adapter, adapter_opts: opts, health_check_interval_ms: nil]}}

    def config(%Config{output: %Config.Window{} = output} = previous) do
      output = %Config.Window{output | title: "Stateless"}
      %Config{previous | output: output}
    end

    def render, do: text("stateless")
  end

  test "config/1 runs before a stateless viewport starts its renderer" do
    {:ok, pid} = StatelessScreen.start_link(observer: self())
    assert Emerge.renderer(pid)
    assert_receive {:started, %Config{output: %Config.Window{title: "Stateless"}}}
    GenServer.stop(pid)
  end

  test "native config-only updates survive renderer replacement with the same callback PID" do
    {:ok, pid} = NativeScreen.start_link(observer: self())
    renderer = Emerge.renderer(pid)
    assert_receive {:native_previous, _}
    assert_receive :native_rendered
    send(pid, {:events, [:key]})
    assert_receive {:native_previous, _}
    assert_receive :native_rendered
    runtime = :sys.get_state(pid).__emerge__
    assert runtime.config.input.events == [:key]
    assert :sys.get_state(runtime.lifecycle).config.config.input.events == [:key]
    assert :ok = EmergeSkia.stop(renderer)
    Emerge.Runtime.Viewport.Lifecycle.check(runtime.lifecycle)
    assert_receive {:native_previous, previous}, 3_000
    assert previous.input.events == [:key]
    assert_receive :native_rendered
    assert Emerge.renderer(pid) != renderer
    assert Process.alive?(pid)
    GenServer.stop(pid)
  end

  test "mount and previous callback values are normalized; updates preserve other fields" do
    {:ok, pid} = Screen.start_link(observer: self())
    renderer = Emerge.renderer(pid)

    assert_receive {:previous,
                    %Config{output: %Config.DRM{backlight: %Config.Backlight{brightness: 0.7}}}}

    assert_receive {:started, _}
    assert_receive :rendered
    send(pid, {:change, %{brightness: 0.2}})
    assert_receive {:previous, previous}
    assert previous.output.backlight.device == "/sys/class/backlight/panel"
    assert_receive :rendered
    assert Agent.get(renderer, & &1.output.backlight.brightness) == 0.2
    send(pid, {:change, %{brightness: 0.3}})
    assert_receive {:previous, previous}
    assert previous.output.backlight.brightness == 0.2
    assert_receive :rendered
    GenServer.stop(pid)
  end

  test "initial off config does not start renderer or call render; on uses latest desired config" do
    {:ok, pid} = Screen.start_link(observer: self(), display: :off)
    assert Emerge.renderer(pid) == nil
    assert_receive {:previous, _}
    refute_receive {:started, _}, 30
    refute_receive :rendered, 30
    send(pid, {:change, %{brightness: 0.1}})
    assert_receive {:previous, previous}
    assert previous.output.display == :off
    refute_receive :rendered, 30
    send(pid, {:change, %{display: :on}})
    assert_receive {:previous, previous}
    assert previous.output.backlight.brightness == 0.1
    assert_receive :rendered
    assert_receive {:started, started}
    assert started.output.backlight.brightness == 0.1
    assert started.output.display == :on
    GenServer.stop(pid)
  end

  test "config failure retains previous valid intent and does not invoke renderer" do
    {:ok, pid} = Screen.start_link(observer: self())
    renderer = Emerge.renderer(pid)
    assert_receive {:previous, _}
    assert_receive {:started, _}
    assert_receive :rendered

    log =
      capture_log(fn ->
        send(pid, {:change, %{bad?: true}})
        assert_receive {:previous, _}
        # Flush has completed when this synchronous state query returns.
        :sys.get_state(pid)
      end)

    assert log =~ "bad config"
    assert Agent.get(renderer, & &1.output.backlight.brightness) == 0.7
    refute_receive :rendered, 30
    GenServer.stop(pid)
  end
end
