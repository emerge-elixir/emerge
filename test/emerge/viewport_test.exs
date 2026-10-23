defmodule Emerge.ViewportTest do
  use ExUnit.Case, async: false
  import ExUnit.CaptureLog
  use Emerge.UI

  defmodule FakeRenderer do
    @behaviour Emerge.Runtime.Viewport.Renderer
    @impl true
    def start(config, renderer_opts) do
      skia_opts = Emerge.Config.Native.options(config)

      Agent.start_link(fn ->
        %{
          ops: [],
          running?: true,
          target: nil,
          heartbeat_pid: nil,
          log_target: nil,
          skia_opts: skia_opts,
          renderer_opts: renderer_opts
        }
      end)
    end

    @impl true
    def stop(renderer) do
      renderer |> Agent.get(& &1.heartbeat_pid) |> stop_heartbeat()
      Agent.stop(renderer)
      :ok
    catch
      :exit, _reason -> :ok
    end

    @impl true
    def running?(renderer) do
      Agent.get(renderer, & &1.running?)
    end

    def set_running(renderer, running?) when is_boolean(running?) do
      Agent.update(renderer, fn state ->
        heartbeat_pid =
          if running? and is_pid(state.target) and is_nil(state.heartbeat_pid) do
            start_heartbeat(state.target)
          else
            stop_heartbeat_unless_running(state.heartbeat_pid, running?)
          end

        %{state | running?: running?, heartbeat_pid: heartbeat_pid}
      end)
    end

    def pause_heartbeat(renderer) do
      Agent.update(renderer, fn state ->
        stop_heartbeat(state.heartbeat_pid)
        %{state | heartbeat_pid: nil}
      end)
    end

    @impl true
    def set_input_target(renderer, pid) do
      Agent.update(renderer, fn state ->
        stop_heartbeat(state.heartbeat_pid)

        heartbeat_pid =
          if state.running? and is_pid(pid) do
            start_heartbeat(pid)
          else
            nil
          end

        state
        |> Map.put(:target, pid)
        |> Map.put(:heartbeat_pid, heartbeat_pid)
        |> log_op({:set_input_target, pid})
      end)

      :ok
    end

    @impl true
    def set_log_target(renderer, pid) do
      Agent.update(renderer, fn state ->
        state |> Map.put(:log_target, pid) |> log_op({:set_log_target, pid})
      end)

      :ok
    end

    @impl true
    def set_input_mask(renderer, mask) do
      Agent.update(renderer, &log_op(&1, {:set_input_mask, mask}))
      :ok
    end

    @impl true
    def upload_tree(renderer, tree) do
      diff_state = Emerge.Engine.diff_state_new(tree)
      Agent.update(renderer, &log_op(&1, {:upload_tree, diff_state.tree}))
      {diff_state, diff_state.tree}
    end

    @impl true
    def patch_tree(renderer, diff_state, tree) do
      {_patch_bin, next_state, assigned_tree} = Emerge.Engine.diff_state_update(diff_state, tree)
      Agent.update(renderer, &log_op(&1, {:patch_tree, assigned_tree}))
      {next_state, assigned_tree}
    end

    @impl true
    def configure(renderer, _previous, config) do
      Agent.update(renderer, &log_op(&1, {:configure, config}))
      :ok
    end

    def ops(renderer) do
      Agent.get(renderer, &Enum.reverse(&1.ops))
    end

    def skia_opts(renderer) do
      Agent.get(renderer, & &1.skia_opts)
    end

    def renderer_opts(renderer) do
      Agent.get(renderer, & &1.renderer_opts)
    end

    defp log_op(state, op) do
      %{state | ops: [op | state.ops]}
    end

    defp start_heartbeat(pid) do
      spawn(fn -> heartbeat_loop(pid) end)
    end

    defp heartbeat_loop(pid) do
      send(pid, Emerge.Runtime.Viewport.Renderer.heartbeat_message())
      Process.sleep(500)
      heartbeat_loop(pid)
    end

    defp stop_heartbeat(nil) do
      nil
    end

    defp stop_heartbeat(pid) do
      Process.exit(pid, :kill)
      nil
    end

    defp stop_heartbeat_unless_running(heartbeat_pid, true) do
      heartbeat_pid
    end

    defp stop_heartbeat_unless_running(heartbeat_pid, false) do
      stop_heartbeat(heartbeat_pid)
    end
  end

  defmodule NoConnectionRenderer do
    @behaviour Emerge.Runtime.Viewport.Renderer
    defdelegate configure(renderer, previous, config), to: Emerge.ViewportTest.FakeRenderer
    defdelegate start(skia_opts, renderer_opts), to: Emerge.ViewportTest.FakeRenderer
    defdelegate stop(renderer), to: Emerge.ViewportTest.FakeRenderer
    defdelegate running?(renderer), to: Emerge.ViewportTest.FakeRenderer
    defdelegate set_input_target(renderer, pid), to: Emerge.ViewportTest.FakeRenderer
    defdelegate set_log_target(renderer, pid), to: Emerge.ViewportTest.FakeRenderer
    defdelegate set_input_mask(renderer, mask), to: Emerge.ViewportTest.FakeRenderer
    defdelegate upload_tree(renderer, tree), to: Emerge.ViewportTest.FakeRenderer
    defdelegate patch_tree(renderer, diff_state, tree), to: Emerge.ViewportTest.FakeRenderer
  end

  defmodule NoConnectionViewport do
    use Emerge
    @impl Viewport
    def mount(_opts) do
      {:ok,
       %Emerge.Config{
         runtime: [
           adapter: Emerge.ViewportTest.NoConnectionRenderer,
           health_check_interval_ms: nil
         ]
       }}
    end

    @impl Viewport
    def render do
      el([], text("no connection"))
    end
  end

  defmodule BareSkiaViewport do
    use Emerge
    @impl Viewport
    def mount(_opts) do
      {:ok, %{},
       %Emerge.Config{
         output: %Emerge.Config.Window{title: "Viewport Defaults"},
         runtime: [adapter: Emerge.ViewportTest.FakeRenderer, health_check_interval_ms: nil]
       }}
    end

    @impl Viewport
    def render(_state) do
      el([], text("defaults"))
    end
  end

  defmodule OptsOnlyViewport do
    use Emerge
    @impl Viewport
    def mount(opts) do
      {:ok,
       %Emerge.Config{
         output: %Emerge.Config.Window{title: Keyword.get(opts, :title, "Opts Only")},
         runtime: [adapter: Emerge.ViewportTest.FakeRenderer, health_check_interval_ms: nil]
       }}
    end

    @impl Viewport
    def render do
      el([], text("opts only"))
    end
  end

  defmodule ScrollLinePixelsViewport do
    use Emerge
    @impl Viewport
    def mount(_opts) do
      {:ok,
       %Emerge.Config{
         output: %Emerge.Config.Window{title: "Scroll line pixels"},
         assets: [otp_app: :emerge],
         input: [scroll_line_pixels: 45],
         runtime: [adapter: Emerge.ViewportTest.FakeRenderer, health_check_interval_ms: nil]
       }}
    end

    @impl Viewport
    def render do
      el([], text("scroll line pixels"))
    end
  end

  defmodule CloseSignalLogViewport do
    use Emerge
    @impl Viewport
    def mount(_opts) do
      {:ok,
       %Emerge.Config{
         output: %Emerge.Config.Window{title: "Close signal log"},
         assets: [otp_app: :emerge],
         diagnostics: [logs: [:close]],
         runtime: [adapter: Emerge.ViewportTest.FakeRenderer, health_check_interval_ms: nil]
       }}
    end

    @impl Viewport
    def render do
      el([], text("close signal log"))
    end
  end

  defmodule CounterViewport do
    use Emerge
    @impl Viewport
    def mount(opts) do
      {:ok, %{count: Keyword.get(opts, :count, 0)},
       %Emerge.Config{
         assets: [otp_app: :emerge],
         runtime: [adapter: Emerge.ViewportTest.FakeRenderer, health_check_interval_ms: nil]
       }}
    end

    @impl Viewport
    def render(state) do
      row([spacing(8)], [
        Input.button(
          [
            key(:inc),
            Event.on_press(:increment),
            Background.color(color(:sky, 500)),
            Border.rounded(4)
          ],
          text("+")
        ),
        el([key(:count), Font.color(color(:white))], text(Integer.to_string(state.count)))
      ])
    end

    @impl Viewport
    def handle_info(:increment, state) do
      {:noreply, %{state | count: state.count + 1} |> Viewport.rerender()}
    end
  end

  defmodule KeyViewport do
    use Emerge
    @impl Viewport
    def mount(opts) do
      {:ok, %{count: Keyword.get(opts, :count, 0)},
       %Emerge.Config{
         assets: [otp_app: :emerge],
         runtime: [adapter: Emerge.ViewportTest.FakeRenderer, health_check_interval_ms: nil]
       }}
    end

    @impl Viewport
    def render(state) do
      row([spacing(8)], [
        Input.button(
          [
            key(:inc_key),
            Event.on_key_down(:enter, :increment),
            Background.color(color(:sky, 500)),
            Border.rounded(4)
          ],
          text("#{state.count}")
        )
      ])
    end

    @impl Viewport
    def handle_info(:increment, state) do
      {:noreply, %{state | count: state.count + 1} |> Viewport.rerender()}
    end
  end

  defmodule KeyPressViewport do
    use Emerge
    @impl Viewport
    def mount(opts) do
      {:ok, %{count: Keyword.get(opts, :count, 0)},
       %Emerge.Config{
         assets: [otp_app: :emerge],
         runtime: [adapter: Emerge.ViewportTest.FakeRenderer, health_check_interval_ms: nil]
       }}
    end

    @impl Viewport
    def render(state) do
      row([spacing(8)], [
        Input.button(
          [
            key(:press_key),
            Event.on_key_press(:space, :increment),
            Background.color(color(:rose, 500)),
            Border.rounded(4)
          ],
          text("#{state.count}")
        )
      ])
    end

    @impl Viewport
    def handle_info(:increment, state) do
      {:noreply, %{state | count: state.count + 1} |> Viewport.rerender()}
    end
  end

  defmodule InputViewport do
    use Emerge
    @impl Viewport
    def mount(opts) do
      {:ok, %{events: [], test_pid: Keyword.fetch!(opts, :test_pid)},
       %Emerge.Config{
         assets: [otp_app: :emerge],
         runtime: [adapter: Emerge.ViewportTest.FakeRenderer, health_check_interval_ms: nil]
       }}
    end

    @impl Viewport
    def render(_state) do
      el([], text("input viewport"))
    end

    @impl Viewport
    def handle_input(event, state) do
      send(state.test_pid, {:input_event, event})
      {:noreply, %{state | events: [event | state.events]}}
    end
  end

  defmodule RenderZeroViewport do
    use Emerge
    @impl Viewport
    def mount(_opts) do
      {:ok, %{rerenders: 0},
       %Emerge.Config{
         assets: [otp_app: :emerge],
         runtime: [adapter: Emerge.ViewportTest.FakeRenderer, health_check_interval_ms: nil]
       }}
    end

    @impl Viewport
    def render do
      el([], text("render zero"))
    end

    @impl Viewport
    def handle_info(:rerender, state) do
      {:noreply, %{state | rerenders: state.rerenders + 1} |> Viewport.rerender()}
    end
  end

  defmodule MousePayloadViewport do
    use Emerge

    @impl Viewport
    def mount(opts) do
      {:ok, %{target: Keyword.fetch!(opts, :target), tuple?: Keyword.get(opts, :tuple?, false)},
       %Emerge.Config{
         assets: [otp_app: :emerge],
         runtime: [adapter: Emerge.ViewportTest.FakeRenderer, health_check_interval_ms: nil]
       }}
    end

    @impl Viewport
    def render(state) do
      attrs =
        Enum.map(
          [:click, :mouse_down, :mouse_up, :mouse_move, :mouse_enter, :mouse_leave, :press],
          fn kind ->
            message = if state.tuple?, do: {:pointer, kind}, else: kind
            {String.to_existing_atom("on_#{kind}"), {state.target, message}}
          end
        )

      el(attrs, none())
    end
  end

  defmodule PayloadViewport do
    use Emerge
    @impl Viewport
    def mount(opts) do
      {:ok, %{test_pid: Keyword.fetch!(opts, :test_pid)},
       %Emerge.Config{
         assets: [otp_app: :emerge],
         runtime: [adapter: Emerge.ViewportTest.FakeRenderer, health_check_interval_ms: nil]
       }}
    end

    @impl Viewport
    def render(_state) do
      Input.text(
        [
          key(:field),
          Event.on_change(:input_changed),
          Event.on_scroll(:scrolled),
          Event.on_click(:click),
          Event.on_mouse_down(:mouse_down),
          Event.on_mouse_up(:mouse_up),
          Event.on_mouse_move(:mouse_move),
          Event.on_mouse_enter(:mouse_enter),
          Event.on_mouse_leave(:mouse_leave)
        ],
        ""
      )
    end

    @impl Viewport
    def wrap_payload(message, payload, event_type) do
      {:wrapped, message, payload, event_type}
    end

    @impl Viewport
    def handle_info({:wrapped, :scrolled, payload, :scroll}, state) do
      send(state.test_pid, {:wrapped_scroll, payload})
      {:noreply, state}
    end

    def handle_info({:wrapped, :input_changed, payload, :change}, state) do
      send(state.test_pid, {:wrapped_payload, payload})
      {:noreply, state}
    end

    def handle_info({:wrapped, message, payload, kind}, state)
        when kind in [:click, :mouse_down, :mouse_up, :mouse_move, :mouse_enter, :mouse_leave] do
      send(state.test_pid, {:wrapped_mouse, message, payload, kind})
      {:noreply, state}
    end
  end

  defmodule LivenessViewport do
    use Emerge
    @impl Viewport
    def mount(_opts) do
      {:ok, %{},
       %Emerge.Config{
         assets: [otp_app: :emerge],
         runtime: [adapter: Emerge.ViewportTest.FakeRenderer, health_check_interval_ms: nil]
       }}
    end

    @impl Viewport
    def render(_state) do
      el([], text("alive"))
    end
  end

  defmodule CloseAwareViewport do
    use Emerge
    @impl Viewport
    def mount(opts) do
      {:ok, %{closed?: false, test_pid: Keyword.fetch!(opts, :test_pid)},
       %Emerge.Config{
         assets: [otp_app: :emerge],
         runtime: [adapter: Emerge.ViewportTest.FakeRenderer, health_check_interval_ms: nil]
       }}
    end

    @impl Viewport
    def render(_state) do
      el([], text("close aware"))
    end

    @impl Viewport
    def handle_close(:window_close_requested, state) do
      send(state.test_pid, :close_requested)
      {:noreply, %{state | closed?: true}}
    end
  end

  defmodule RecoveringViewport do
    use Emerge
    @impl Viewport
    def mount(opts) do
      {:ok, %{mode: Keyword.get(opts, :mode, {:label, "ready"})},
       %Emerge.Config{
         assets: [otp_app: :emerge],
         runtime: [adapter: Emerge.ViewportTest.FakeRenderer, health_check_interval_ms: nil]
       }}
    end

    @impl Viewport
    def render(%{mode: :raise}) do
      raise "render boom"
    end

    def render(%{mode: {:label, label}}) do
      el([], text(label))
    end

    @impl Viewport
    def handle_info({:set_mode, mode}, state) do
      {:noreply, %{state | mode: mode} |> Viewport.rerender()}
    end
  end

  defmodule RenderBothViewport do
    use Emerge
    @impl Viewport
    def mount(_opts) do
      {:ok,
       %Emerge.Config{
         assets: [otp_app: :emerge],
         runtime: [adapter: Emerge.ViewportTest.FakeRenderer, health_check_interval_ms: nil]
       }}
    end

    @impl Viewport
    def render do
      el([], text("render both zero"))
    end

    @impl Viewport
    def render(_state) do
      el([], text("render both one"))
    end
  end

  defmodule RenderMissingViewport do
    use Emerge
    @impl Viewport
    def mount(_opts) do
      {:ok,
       %Emerge.Config{
         assets: [otp_app: :emerge],
         runtime: [adapter: Emerge.ViewportTest.FakeRenderer, health_check_interval_ms: nil]
       }}
    end
  end

  test "mount starts renderer and uploads initial tree" do
    {:ok, pid} = CounterViewport.start_link(count: 3)
    renderer = Emerge.renderer(pid)
    operations = FakeRenderer.ops(renderer)
    assert {:set_input_target, ^pid} = Enum.at(operations, 0)
    assert {:set_log_target, ^pid} = Enum.at(operations, 1)
    assert {:upload_tree, %Emerge.Engine.Element{type: :row}} = Enum.at(operations, 3)
    assert :sys.get_state(pid).count == 3
    assert pid in Emerge.Runtime.Viewport.ReloadGroup.local_members()
    GenServer.stop(pid)
  end

  test "rejects video submission to a non-Skia renderer" do
    {:ok, pid} = CounterViewport.start_link(count: 1)
    _renderer = Emerge.renderer(pid)
    frame = VideoInterop.Frame.binary(<<1, 2, 3>>, width: 1, height: 1, pixel_format: :rgb888)

    assert {:error, :video_submission_unsupported} =
             Emerge.submit_video_frame(pid, :preview, frame)

    GenServer.stop(pid)
  end

  test "submission consumes a frame when the viewport endpoint is absent" do
    frame = VideoInterop.Frame.binary(<<1, 2, 3>>, width: 1, height: 1, pixel_format: :rgb888)
    assert {:error, :viewport_not_ready} = Emerge.submit_video_frame(self(), :preview, frame)
  end

  test "viewport child spec waits for renderer teardown during supervisor shutdown" do
    assert %{shutdown: :infinity} = CounterViewport.child_spec([])
  end

  test "mount accepts config defaults and infers otp_app" do
    {:ok, pid} = BareSkiaViewport.start_link()
    renderer = Emerge.renderer(pid)
    assert Keyword.get(FakeRenderer.skia_opts(renderer), :otp_app) == :emerge
    assert Keyword.get(FakeRenderer.skia_opts(renderer), :title) == "Viewport Defaults"
    assert FakeRenderer.renderer_opts(renderer) == []
    GenServer.stop(pid)
  end

  test "mount accepts {:ok, config} and uses empty viewport state" do
    {:ok, pid} = OptsOnlyViewport.start_link()
    renderer = Emerge.renderer(pid)
    state = :sys.get_state(pid)
    assert Map.drop(state, [:__emerge__]) == %{}
    assert Keyword.get(FakeRenderer.skia_opts(renderer), :otp_app) == :emerge
    assert Keyword.get(FakeRenderer.skia_opts(renderer), :title) == "Opts Only"
    assert rendered_text(pid) == "opts only"
    GenServer.stop(pid)
  end

  test "mount can pass input.scroll_line_pixels to the renderer" do
    {:ok, pid} = ScrollLinePixelsViewport.start_link()
    renderer = Emerge.renderer(pid)
    assert Keyword.get(FakeRenderer.skia_opts(renderer), :scroll_line_pixels) == 45
    assert Keyword.get(FakeRenderer.skia_opts(renderer), :otp_app) == :emerge
    GenServer.stop(pid)
  end

  test "mount can pass emerge_skia close_signal_log option to the renderer" do
    {:ok, pid} = CloseSignalLogViewport.start_link()
    renderer = Emerge.renderer(pid)
    assert Keyword.get(FakeRenderer.skia_opts(renderer), :close_signal_log) == true
    assert Keyword.get(FakeRenderer.skia_opts(renderer), :otp_app) == :emerge
    GenServer.stop(pid)
  end

  test "render/0 viewports render and rerender successfully" do
    {:ok, pid} = RenderZeroViewport.start_link()
    renderer = Emerge.renderer(pid)
    assert rendered_text(pid) == "render zero"
    send(pid, :rerender)

    assert_eventually(fn ->
      :sys.get_state(pid).rerenders == 1 and count_renderer_ops(renderer, :patch_tree) == 1
    end)

    GenServer.stop(pid)
  end

  test "viewport modules must not define both render/0 and render/1" do
    {:ok, state, _continue} = Emerge.Runtime.Viewport.init_state(RenderBothViewport, [])

    assert_raise ArgumentError,
                 ~r/must define exactly one of render\/0 or render\/1, but defines both/,
                 fn -> Emerge.Runtime.Viewport.handle_continue_mount([], state) end
  end

  test "viewport modules must define one render callback" do
    {:ok, state, _continue} = Emerge.Runtime.Viewport.init_state(RenderMissingViewport, [])

    assert_raise ArgumentError, ~r/must define exactly one of render\/0 or render\/1/, fn ->
      Emerge.Runtime.Viewport.handle_continue_mount([], state)
    end
  end

  test "self-targeted element events route through mailbox and rerender" do
    {:ok, pid} = CounterViewport.start_link(count: 0)
    renderer = Emerge.renderer(pid)
    state = :sys.get_state(pid)

    {id_bin, _events} =
      Enum.find(state.__emerge__.diff_state.event_registry, fn {_id_bin, events} ->
        Map.has_key?(events, :press)
      end)

    send(pid, {:emerge_skia_event, {id_bin, :press}})
    assert_eventually(fn -> :sys.get_state(pid).count == 1 end)

    assert_eventually(fn ->
      patch_count =
        renderer
        |> FakeRenderer.ops()
        |> Enum.count(fn
          {:patch_tree, _tree} -> true
          _ -> false
        end)

      patch_count == 1
    end)

    GenServer.stop(pid)
  end

  test "native renderer log messages are forwarded to Logger" do
    log =
      capture_log(fn ->
        assert {:noreply, %{}} =
                 Emerge.Runtime.Viewport.handle_info(
                   {:emerge_skia_log, :warning, "drm", "DRM cursor: hardware plane enabled"},
                   %{}
                 )
      end)

    assert log =~ "EmergeSkia native[drm] DRM cursor: hardware plane enabled"
  end

  test "verbose event runtime traces stay below Logger's default level" do
    log =
      capture_log([level: :info], fn ->
        assert {:noreply, %{}} =
                 Emerge.Runtime.Viewport.handle_info(
                   {:emerge_skia_log, :info, "event_runtime", "input begin"},
                   %{}
                 )
      end)

    assert log == ""
  end

  test "rerender requests from callback state updates are coalesced" do
    {:ok, pid} = CounterViewport.start_link(count: 0)
    renderer = Emerge.renderer(pid)
    send(pid, :increment)
    send(pid, :increment)
    send(pid, :increment)

    assert_eventually(
      fn ->
        renderer
        |> FakeRenderer.ops()
        |> Enum.count(fn
          {:patch_tree, _tree} -> true
          _ -> false
        end) == 1 and :sys.get_state(pid).count == 3
      end,
      100
    )

    GenServer.stop(pid)
  end

  test "source reload notifications rerender mounted viewports" do
    {:ok, pid} = CounterViewport.start_link(count: 0)
    renderer = Emerge.renderer(pid)
    :ok = Emerge.notify_source_reloaded(%{source: :test})

    assert_eventually(fn ->
      renderer
      |> FakeRenderer.ops()
      |> Enum.count(fn
        {:patch_tree, _tree} -> true
        _ -> false
      end) == 1
    end)

    GenServer.stop(pid)
  end

  test "source reload rerender requests are coalesced" do
    {:ok, pid} = CounterViewport.start_link(count: 0)
    renderer = Emerge.renderer(pid)
    send(pid, {:emerge_viewport, :source_reloaded, %{source: :test}})
    send(pid, {:emerge_viewport, :source_reloaded, %{source: :test}})
    send(pid, {:emerge_viewport, :source_reloaded, %{source: :test}})

    assert_eventually(fn ->
      renderer
      |> FakeRenderer.ops()
      |> Enum.count(fn
        {:patch_tree, _tree} -> true
        _ -> false
      end) == 1
    end)

    GenServer.stop(pid)
  end

  test "source reload rerenders render/0 viewports after the module is unloaded" do
    module = compile_reloadable_render_zero_viewport()
    {:ok, pid} = module.start_link()
    renderer = Emerge.renderer(pid)
    assert :code.which(module) != :non_existing
    :code.purge(module)
    :code.delete(module)
    refute function_exported?(module, :render, 0)
    send(pid, {:emerge_viewport, :source_reloaded, %{source: :test}})

    assert_eventually(fn ->
      Process.alive?(pid) and count_renderer_ops(renderer, :patch_tree) == 1 and
        rendered_text(pid) == "reload zero"
    end)

    GenServer.stop(pid)
  end

  test "initial render failures keep viewport alive without starting the renderer" do
    log =
      capture_log(fn ->
        {:ok, pid} = RecoveringViewport.start_link(mode: :raise)

        assert_eventually(fn ->
          state = :sys.get_state(pid)

          Process.alive?(pid) and is_nil(state.__emerge__.renderer) and
            is_nil(state.__emerge__.diff_state) and
            pid in Emerge.Runtime.Viewport.ReloadGroup.local_members()
        end)

        GenServer.stop(pid)
      end)

    assert log =~ "initial render failed"
  end

  test "viewport can recover from an initial render failure on a later rerender" do
    log =
      capture_log(fn ->
        {:ok, pid} = RecoveringViewport.start_link(mode: :raise)
        send(pid, {:set_mode, {:label, "recovered"}})

        assert_eventually(fn ->
          state = :sys.get_state(pid)

          Process.alive?(pid) and state.__emerge__.renderer != nil and
            state.__emerge__.diff_state != nil and rendered_text(pid) == "recovered" and
            pid in Emerge.Runtime.Viewport.ReloadGroup.local_members()
        end)

        renderer = Emerge.renderer(pid)
        assert count_renderer_ops(renderer, :upload_tree) == 1
        GenServer.stop(pid)
      end)

    assert log =~ "initial render failed"
  end

  test "rerender failures keep the previous frame and the viewport alive" do
    {:ok, pid} = RecoveringViewport.start_link(mode: {:label, "before"})
    renderer = Emerge.renderer(pid)

    log =
      capture_log(fn ->
        send(pid, {:set_mode, :raise})

        assert_eventually(fn ->
          state = :sys.get_state(pid)

          Process.alive?(pid) and rendered_text(pid) == "before" and
            count_renderer_ops(renderer, :patch_tree) == 0 and not state.__emerge__.dirty? and
            not state.__emerge__.flush_scheduled?
        end)
      end)

    assert log =~ "rerender failed"
    GenServer.stop(pid)
  end

  test "viewport can recover after a failed rerender" do
    {:ok, pid} = RecoveringViewport.start_link(mode: {:label, "before"})
    renderer = Emerge.renderer(pid)

    _log =
      capture_log(fn ->
        send(pid, {:set_mode, :raise})

        assert_eventually(fn ->
          rendered_text(pid) == "before" and count_renderer_ops(renderer, :patch_tree) == 0
        end)
      end)

    send(pid, {:set_mode, {:label, "after"}})

    assert_eventually(fn ->
      rendered_text(pid) == "after" and count_renderer_ops(renderer, :patch_tree) == 1
    end)

    GenServer.stop(pid)
  end

  test "raw input events flow through handle_input callback" do
    {:ok, pid} = InputViewport.start_link(test_pid: self())
    send(pid, {:emerge_skia_event, {:focused, true}})
    assert_receive {:input_event, {:focus, true}}
    assert :sys.get_state(pid).events == [focus: true]
    GenServer.stop(pid)
  end

  test "payload wrapping callback is applied before dispatch" do
    {:ok, pid} = PayloadViewport.start_link(test_pid: self())
    state = :sys.get_state(pid)

    {id_bin, _events} =
      Enum.find(state.__emerge__.diff_state.event_registry, fn {_id_bin, events} ->
        Map.has_key?(events, :change)
      end)

    send(pid, {:emerge_skia_event, {id_bin, :change, "hello"}})
    assert_receive {:wrapped_payload, "hello"}

    send(pid, {:emerge_skia_event, {id_bin, :scroll, %{x: 0.25, y: 0.75}}})
    assert_receive {:wrapped_scroll, %{x: 0.25, y: 0.75}}

    GenServer.stop(pid)
  end

  test "mouse positions route to external PIDs with default atom and tuple wrapping" do
    for tuple? <- [false, true] do
      {:ok, pid} = MousePayloadViewport.start_link(target: self(), tuple?: tuple?)
      state = :sys.get_state(pid)
      [{id, _}] = Map.to_list(state.__emerge__.diff_state.event_registry)
      position = %{x: 35.5, y: -2.25}

      for kind <- [:click, :mouse_down, :mouse_up, :mouse_move, :mouse_enter, :mouse_leave] do
        send(pid, {:emerge_skia_event, {id, kind, position}})
        expected = if tuple?, do: {:pointer, kind, position}, else: {kind, position}
        assert_receive ^expected
      end

      send(pid, {:emerge_skia_event, {id, :press}})
      expected = if tuple?, do: {:pointer, :press}, else: :press
      assert_receive ^expected
      GenServer.stop(pid)
    end
  end

  test "all mouse positions use custom wrap_payload callbacks" do
    {:ok, pid} = PayloadViewport.start_link(test_pid: self())
    state = :sys.get_state(pid)
    [{id, _}] = Map.to_list(state.__emerge__.diff_state.event_registry)
    position = %{x: -2.25, y: 103.5}

    for kind <- [:click, :mouse_down, :mouse_up, :mouse_move, :mouse_enter, :mouse_leave] do
      send(pid, {:emerge_skia_event, {id, kind, position}})
      assert_receive {:wrapped_mouse, ^kind, ^position, ^kind}
    end

    GenServer.stop(pid)
  end

  test "key listener events route through the viewport mailbox" do
    {:ok, pid} = KeyViewport.start_link(count: 0)
    state = :sys.get_state(pid)
    route = Event.key_route_id(:key_down, :enter, [], :exact)

    {id_bin, _events} =
      Enum.find(state.__emerge__.diff_state.event_registry, fn {_id_bin, events} ->
        Map.has_key?(events, {:key_down, route})
      end)

    send(pid, {:emerge_skia_event, {id_bin, :key_down, route}})
    assert_eventually(fn -> :sys.get_state(pid).count == 1 end)
    GenServer.stop(pid)
  end

  test "key press listener events route through the viewport mailbox" do
    {:ok, pid} = KeyPressViewport.start_link(count: 0)
    state = :sys.get_state(pid)
    route = Event.key_route_id(:key_press, :space, [], :exact)

    {id_bin, _events} =
      Enum.find(state.__emerge__.diff_state.event_registry, fn {_id_bin, events} ->
        Map.has_key?(events, {:key_press, route})
      end)

    send(pid, {:emerge_skia_event, {id_bin, :key_press, route}})
    assert_eventually(fn -> :sys.get_state(pid).count == 1 end)
    GenServer.stop(pid)
  end

  test "renderer heartbeat watchdog stops viewport when heartbeats go stale" do
    {:ok, pid} = LivenessViewport.start_link()
    renderer = Emerge.renderer(pid)
    FakeRenderer.set_running(renderer, false)

    :sys.replace_state(pid, fn state ->
      update_in(
        state.__emerge__.last_renderer_heartbeat_at_ms,
        fn _last_seen -> System.monotonic_time(:millisecond) - 1500 end
      )
    end)

    ref = Process.monitor(pid)
    send(pid, {:emerge_viewport, :check_renderer})
    assert_receive {:DOWN, ^ref, :process, ^pid, :normal}
  end

  test "renderer heartbeat watchdog tolerates a stale heartbeat when renderer is still running" do
    {:ok, pid} = LivenessViewport.start_link()
    renderer = Emerge.renderer(pid)
    FakeRenderer.pause_heartbeat(renderer)

    :sys.replace_state(pid, fn state ->
      update_in(
        state.__emerge__.last_renderer_heartbeat_at_ms,
        fn _last_seen -> System.monotonic_time(:millisecond) - 1500 end
      )
    end)

    ref = Process.monitor(pid)
    send(pid, {:emerge_viewport, :check_renderer})

    assert :sys.get_state(pid).__emerge__.last_renderer_heartbeat_at_ms >
             System.monotonic_time(:millisecond) - 1000

    refute_receive {:DOWN, ^ref, :process, ^pid, :normal}, 100
    GenServer.stop(pid)
  end

  test "default handle_close stops viewport when close is received" do
    {:ok, pid} = LivenessViewport.start_link()
    ref = Process.monitor(pid)
    send(pid, {:emerge_skia_close, :window_close_requested})
    assert_receive {:DOWN, ^ref, :process, ^pid, :normal}
  end

  test "viewport can override close handling through handle_close" do
    {:ok, pid} = CloseAwareViewport.start_link(test_pid: self())
    send(pid, {:emerge_skia_close, :window_close_requested})
    assert_receive :close_requested
    assert Process.alive?(pid)
    assert :sys.get_state(pid).closed?
    GenServer.stop(pid)
  end

  test "stopping viewport stops renderer" do
    {:ok, pid} = CounterViewport.start_link(count: 1)
    renderer = Emerge.renderer(pid)
    ref = Process.monitor(renderer)
    GenServer.stop(pid)
    assert_receive {:DOWN, ^ref, :process, ^renderer, :normal}
  end

  test "supervisor shutdown stops renderer even when a renderer reference is still held" do
    {:ok, sup} = Supervisor.start_link([{LivenessViewport, []}], strategy: :one_for_one)
    [{_, pid, _, _}] = Supervisor.which_children(sup)
    renderer = Emerge.renderer(pid)
    ref = Process.monitor(renderer)
    :ok = Supervisor.stop(sup)
    assert_receive {:DOWN, ^ref, :process, ^renderer, :normal}
  end

  test "trapping exits preserves normal linked-process exit behavior" do
    {:ok, pid} = LivenessViewport.start_link()

    spawn(fn ->
      Process.link(pid)
      exit(:normal)
    end)

    assert_eventually(fn -> Process.alive?(pid) end)
    GenServer.stop(pid)
  end

  test "trapping exits preserves abnormal linked-process exit behavior and stops renderer" do
    previous_trap_exit = Process.flag(:trap_exit, true)

    try do
      capture_log(fn ->
        {:ok, pid} = LivenessViewport.start_link()
        renderer = Emerge.renderer(pid)
        viewport_ref = Process.monitor(pid)
        renderer_ref = Process.monitor(renderer)

        spawn(fn ->
          Process.link(pid)
          exit(:boom)
        end)

        assert_receive {:DOWN, ^viewport_ref, :process, ^pid, :boom}
        assert_receive {:DOWN, ^renderer_ref, :process, ^renderer, :normal}
        assert_receive {:EXIT, ^pid, :boom}
      end)
    after
      Process.flag(:trap_exit, previous_trap_exit)
    end
  end

  defp assert_eventually(fun, attempts \\ 40)

  defp assert_eventually(fun, attempts) when attempts > 0 do
    if fun.() do
      :ok
    else
      Process.sleep(10)
      assert_eventually(fun, attempts - 1)
    end
  end

  defp assert_eventually(_fun, 0) do
    flunk("condition was not met")
  end

  defp compile_reloadable_render_zero_viewport do
    suffix = System.unique_integer([:positive])
    module = Module.concat(__MODULE__, "ReloadableRenderZeroViewport#{suffix}")
    beam_dir = Path.join(System.tmp_dir!(), "emerge_viewport_test_#{suffix}")
    source_path = Path.join(beam_dir, "reloadable_render_zero_viewport.ex")
    source = "defmodule #{inspect(module)} do
  use Emerge

  @impl Viewport
  def mount(_opts) do
    {:ok,
     %Emerge.Config{assets: [otp_app: :emerge], runtime: [adapter: Emerge.ViewportTest.FakeRenderer, health_check_interval_ms: nil]}}
  end

  @impl Viewport
  def render, do: el([], text(\"reload zero\"))
end
"
    File.mkdir_p!(beam_dir)
    File.write!(source_path, source)

    assert {:ok, [^module], %{compile_warnings: [], runtime_warnings: []}} =
             Kernel.ParallelCompiler.compile_to_path([source_path], beam_dir,
               return_diagnostics: true
             )

    Code.prepend_path(beam_dir)

    on_exit(fn ->
      :code.purge(module)
      :code.delete(module)
      Code.delete_path(beam_dir)
      File.rm_rf!(beam_dir)
    end)

    module
  end

  defp count_renderer_ops(renderer, op_name) do
    renderer
    |> FakeRenderer.ops()
    |> Enum.count(fn
      {^op_name, _tree} -> true
      _ -> false
    end)
  end

  defp rendered_text(pid) do
    case :sys.get_state(pid) do
      %{
        __emerge__: %Emerge.Runtime.Viewport.State{
          diff_state: %Emerge.Engine.DiffState{
            tree: %Emerge.Engine.Element{
              children: [%Emerge.Engine.Element{attrs: %{content: content}}]
            }
          }
        }
      } ->
        content

      _ ->
        nil
    end
  end
end
