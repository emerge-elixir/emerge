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

    def set_input_target(renderer, target),
      do: notify(renderer, {:input_target, renderer, target})

    def set_log_target(renderer, target), do: notify(renderer, {:log_target, renderer, target})
    def set_input_mask(renderer, mask), do: notify(renderer, {:input_mask, renderer, mask})

    defp notify(renderer, message) do
      config = Agent.get(renderer, & &1)
      send(Keyword.fetch!(config.runtime.adapter_opts, :observer), message)
      :ok
    end

    def upload_tree(renderer, tree) do
      notify(renderer, {:uploaded, renderer})
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
        renderer: Keyword.get(opts, :renderer, []),
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

      {:ok, %{observer: observer, events: [], fps: nil},
       %Config{
         output: output,
         renderer: [api: :raster],
         runtime: [health_check_interval_ms: 60_000]
       }}
    end

    def config(
          %Config{input: %Config.Input{} = input, renderer: %Config.Renderer{} = rendering} =
            previous,
          state
        ) do
      send(state.observer, {:native_previous, previous})
      input = %Config.Input{input | events: state.events}
      rendering = %Config.Renderer{rendering | target_fps: state.fps}
      %Config{previous | input: input, renderer: rendering}
    end

    def render(state) do
      send(state.observer, :native_rendered)
      text("unchanged")
    end

    def handle_info({:events, events}, state),
      do: {:noreply, Viewport.rerender(%{state | events: events})}

    def handle_info({:fps, fps}, state),
      do: {:noreply, Viewport.rerender(%{state | fps: fps})}
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
    lifecycle = :sys.get_state(pid).__emerge__.lifecycle
    check_tag = :sys.get_state(lifecycle).check_tag
    send(pid, {:events, [:key]})
    assert_receive {:native_previous, _}
    assert_receive :native_rendered
    runtime = :sys.get_state(pid).__emerge__
    assert runtime.config.input.events == [:key]
    assert :sys.get_state(runtime.lifecycle).config.config.input.events == [:key]
    send(pid, {:fps, 30})
    assert_receive {:native_previous, previous}
    assert previous.renderer.target_fps == nil
    assert_receive :native_rendered
    assert Emerge.renderer(pid) == renderer
    assert :sys.get_state(runtime.lifecycle).config.config.renderer.target_fps == 30
    assert :sys.get_state(lifecycle).check_tag == check_tag
    assert :ok = EmergeSkia.stop(renderer)
    Emerge.Runtime.Viewport.Lifecycle.check(runtime.lifecycle)
    assert_receive {:native_previous, previous}, 3_000
    assert previous.input.events == [:key]
    assert previous.renderer.target_fps == 30
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

  test "initial off acquires and binds once without rendering; on reuses the owner" do
    {:ok, pid} = Screen.start_link(observer: self(), display: :off)
    renderer = Emerge.renderer(pid)
    assert is_pid(renderer)
    assert_receive {:previous, _}
    assert_receive {:started, started}
    assert started.output.display == :off
    assert_receive {:input_target, ^renderer, ^pid}
    assert_receive {:log_target, ^renderer, ^pid}
    assert_receive {:input_mask, ^renderer, 0}
    assert :sys.get_state(pid).__emerge__.renderer_bound?
    assert :sys.get_state(pid).__emerge__.diff_state == nil
    refute_receive :rendered, 30
    refute_receive {:uploaded, _}, 30

    send(pid, {:change, %{brightness: 0.1}})
    assert_receive {:previous, previous}
    assert previous.output.display == :off
    assert Emerge.renderer(pid) == renderer
    assert Agent.get(renderer, & &1.output.backlight.brightness) == 0.1
    refute_receive :rendered, 30
    refute_receive {:input_target, _, _}, 30
    refute_receive {:uploaded, _}, 30

    send(pid, {:change, %{display: :on}})
    assert_receive {:previous, previous}
    assert previous.output.backlight.brightness == 0.1
    assert_receive :rendered
    assert_receive {:uploaded, ^renderer}
    assert Emerge.renderer(pid) == renderer
    assert Agent.get(renderer, & &1.output.display) == :on
    refute_receive {:started, _}, 30
    refute_receive {:input_target, _, _}, 30
    GenServer.stop(pid)
    refute Process.alive?(renderer)
  end

  for options <- [
        [api: :vulkan, device: "/dev/dri/renderD128"],
        [api: :raster, raster_present: :gpu_upload]
      ] do
    test "#{options[:api]} off updates and resume reuse the public owner without off uploads" do
      options = unquote(Macro.escape(options))

      {:ok, pid} =
        Screen.start_link(observer: self(), display: :off, renderer: options)

      renderer = Emerge.renderer(pid)
      assert_receive {:started, config}
      assert config.renderer.api == options[:api]
      assert config.output.display == :off
      assert_receive {:previous, _}
      assert_receive {:input_target, ^renderer, ^pid}
      assert_receive {:log_target, ^renderer, ^pid}
      assert_receive {:input_mask, ^renderer, 0}
      refute_receive :rendered, 30
      refute_receive {:uploaded, _}, 30

      for brightness <- [0.0, 0.2, nil] do
        send(pid, {:change, %{brightness: brightness}})
        assert_receive {:previous, _}
        :sys.get_state(pid)
        assert Agent.get(renderer, & &1.output.backlight.brightness) == brightness
        assert Emerge.renderer(pid) == renderer
        refute_receive :rendered, 30
        refute_receive {:uploaded, _}, 30
      end

      send(pid, {:change, %{display: :on}})
      assert_receive {:previous, previous}
      assert previous.output.backlight.brightness == nil
      assert_receive :rendered
      assert_receive {:uploaded, ^renderer}
      assert Emerge.renderer(pid) == renderer
      refute_receive {:started, _}, 30
      GenServer.stop(pid)
    end
  end

  test "stop before first upload releases the off owner" do
    {:ok, pid} = Screen.start_link(observer: self(), display: :off)
    renderer = Emerge.renderer(pid)
    assert is_pid(renderer)
    GenServer.stop(pid)
    refute Process.alive?(renderer)
    refute_receive :rendered, 30
    refute_receive {:uploaded, _}, 30
  end

  test "recovered off owners are rebound without rendering or reusing a diff" do
    alias Emerge.Runtime.Viewport.Lifecycle
    {:ok, pid} = Screen.start_link(observer: self(), display: :off)
    renderer = Emerge.renderer(pid)
    assert_receive {:started, _}
    assert_receive {:input_target, ^renderer, ^pid}
    runtime = :sys.get_state(pid).__emerge__
    :ok = Lifecycle.discard(runtime.lifecycle)

    send(
      pid,
      {:emerge_viewport, :renderer_unavailable, runtime.lifecycle, runtime.renderer_generation,
       :ok}
    )

    assert Emerge.renderer(pid) == nil
    # Custom adapters do not auto-retry; drive the lifecycle's actual retry path.
    send(runtime.lifecycle, :retry)
    assert_receive {:started, config}
    assert config.output.display == :off
    assert_receive {:input_target, replacement, ^pid}
    assert replacement != renderer
    assert Emerge.renderer(pid) == replacement
    assert :sys.get_state(pid).__emerge__.renderer_bound?
    assert :sys.get_state(pid).__emerge__.diff_state == nil
    refute_receive :rendered, 30
    refute_receive {:uploaded, _}, 30
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
