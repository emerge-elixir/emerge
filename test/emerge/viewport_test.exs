defmodule Emerge.ViewportTest do
  use ExUnit.Case, async: false

  import ExUnit.CaptureLog
  import Emerge.UI

  defmodule FakeRenderer do
    @behaviour Emerge.Viewport.Renderer

    @impl true
    def start(skia_opts, renderer_opts) do
      Agent.start_link(fn ->
        %{
          ops: [],
          running?: true,
          target: nil,
          skia_opts: skia_opts,
          renderer_opts: renderer_opts
        }
      end)
    end

    @impl true
    def stop(renderer) do
      Agent.stop(renderer)
      :ok
    catch
      :exit, _reason -> :ok
    end

    @impl true
    def running?(renderer), do: Agent.get(renderer, & &1.running?)

    def set_running(renderer, running?) when is_boolean(running?) do
      Agent.update(renderer, &Map.put(&1, :running?, running?))
    end

    @impl true
    def set_input_target(renderer, pid) do
      Agent.update(renderer, fn state ->
        state
        |> Map.put(:target, pid)
        |> log_op({:set_input_target, pid})
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
      diff_state = Emerge.diff_state_new(tree)
      Agent.update(renderer, &log_op(&1, {:upload_tree, diff_state.tree}))
      {diff_state, diff_state.tree}
    end

    @impl true
    def patch_tree(renderer, diff_state, tree) do
      {_patch_bin, next_state, assigned_tree} = Emerge.diff_state_update(diff_state, tree)
      Agent.update(renderer, &log_op(&1, {:patch_tree, assigned_tree}))
      {next_state, assigned_tree}
    end

    def ops(renderer), do: Agent.get(renderer, &Enum.reverse(&1.ops))

    def skia_opts(renderer), do: Agent.get(renderer, & &1.skia_opts)

    def renderer_opts(renderer), do: Agent.get(renderer, & &1.renderer_opts)

    defp log_op(state, op), do: %{state | ops: [op | state.ops]}
  end

  defmodule BareSkiaViewport do
    use Emerge.Viewport

    @impl Viewport
    def mount(_opts) do
      {:ok, %{},
       [
         title: "Viewport Defaults",
         viewport: [
           renderer_module: Emerge.ViewportTest.FakeRenderer,
           renderer_check_interval_ms: nil
         ]
       ]}
    end

    @impl Viewport
    def render(_state), do: el([], text("defaults"))
  end

  defmodule CounterViewport do
    use Emerge.Viewport

    @impl Viewport
    def mount(opts) do
      {:ok, %{count: Keyword.get(opts, :count, 0)},
       emerge_skia: [otp_app: :emerge],
       viewport: [
         renderer_module: Emerge.ViewportTest.FakeRenderer,
         renderer_check_interval_ms: nil
       ]}
    end

    @impl Viewport
    def render(state) do
      row([spacing(8)], [
        Input.button([key(:inc), on_press(:increment)], [text("+")]),
        el([key(:count)], text(Integer.to_string(state.count)))
      ])
    end

    @impl true
    def handle_info(:increment, state) do
      state =
        state
        |> Viewport.update_user_state(&Map.update!(&1, :count, fn count -> count + 1 end))
        |> Viewport.schedule_rerender()

      {:noreply, state}
    end
  end

  defmodule InputViewport do
    use Emerge.Viewport

    @impl Viewport
    def mount(opts) do
      {:ok, %{events: [], test_pid: Keyword.fetch!(opts, :test_pid)},
       emerge_skia: [otp_app: :emerge],
       viewport: [
         renderer_module: Emerge.ViewportTest.FakeRenderer,
         renderer_check_interval_ms: nil
       ]}
    end

    @impl Viewport
    def render(_state), do: el([], text("input viewport"))

    @impl Viewport
    def handle_input(event, state) do
      send(state.test_pid, {:input_event, event})
      {:ok, %{state | events: [event | state.events]}, rerender: false}
    end
  end

  defmodule PayloadViewport do
    use Emerge.Viewport

    @impl Viewport
    def mount(opts) do
      {:ok, %{test_pid: Keyword.fetch!(opts, :test_pid)},
       emerge_skia: [otp_app: :emerge],
       viewport: [
         renderer_module: Emerge.ViewportTest.FakeRenderer,
         renderer_check_interval_ms: nil
       ]}
    end

    @impl Viewport
    def render(_state) do
      Input.text([key(:field), on_change(:input_changed)], "")
    end

    @impl Viewport
    def wrap_payload(message, payload, event_type) do
      {:wrapped, message, payload, event_type}
    end

    @impl true
    def handle_info({:wrapped, :input_changed, payload, :change}, state) do
      send(Viewport.user_state(state).test_pid, {:wrapped_payload, payload})
      {:noreply, state}
    end
  end

  defmodule LivenessViewport do
    use Emerge.Viewport

    @impl Viewport
    def mount(_opts) do
      {:ok, %{},
       emerge_skia: [otp_app: :emerge],
       viewport: [
         renderer_module: Emerge.ViewportTest.FakeRenderer,
         renderer_check_interval_ms: nil
       ]}
    end

    @impl Viewport
    def render(_state), do: el([], text("alive"))
  end

  defmodule RecoveringViewport do
    use Emerge.Viewport

    @impl Viewport
    def mount(opts) do
      {:ok, %{mode: Keyword.get(opts, :mode, {:label, "ready"})},
       emerge_skia: [otp_app: :emerge],
       viewport: [
         renderer_module: Emerge.ViewportTest.FakeRenderer,
         renderer_check_interval_ms: nil
       ]}
    end

    @impl Viewport
    def render(%{mode: :raise}), do: raise("render boom")
    def render(%{mode: {:label, label}}), do: el([], text(label))

    @impl true
    def handle_info({:set_mode, mode}, state) do
      state =
        state
        |> Viewport.update_user_state(&Map.put(&1, :mode, mode))
        |> Viewport.schedule_rerender()

      {:noreply, state}
    end
  end

  test "mount starts renderer and uploads initial tree" do
    {:ok, pid} = CounterViewport.start_link(count: 3)

    renderer = Emerge.Viewport.renderer(pid)
    operations = FakeRenderer.ops(renderer)

    assert {:set_input_target, ^pid} = Enum.at(operations, 0)
    assert {:upload_tree, %Emerge.Element{type: :row}} = Enum.at(operations, 1)

    assert Emerge.Viewport.user_state(pid) == %{count: 3}
    assert pid in Emerge.Viewport.ReloadGroup.local_members()

    GenServer.stop(pid)
  end

  test "mount accepts bare skia opts and infers otp_app" do
    {:ok, pid} = BareSkiaViewport.start_link()

    renderer = Emerge.Viewport.renderer(pid)

    assert Keyword.get(FakeRenderer.skia_opts(renderer), :otp_app) == :emerge
    assert Keyword.get(FakeRenderer.skia_opts(renderer), :title) == "Viewport Defaults"
    assert FakeRenderer.renderer_opts(renderer) == []

    GenServer.stop(pid)
  end

  test "self-targeted element events route through mailbox and rerender" do
    {:ok, pid} = CounterViewport.start_link(count: 0)
    renderer = Emerge.Viewport.renderer(pid)

    state = :sys.get_state(pid)

    {id_bin, _events} =
      Enum.find(state.diff_state.event_registry, fn {_id_bin, events} ->
        Map.has_key?(events, :press)
      end)

    send(pid, {:emerge_skia_event, {id_bin, :press}})

    assert_eventually(fn -> Emerge.Viewport.user_state(pid).count == 1 end)

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

  test "rerender requests are coalesced" do
    {:ok, pid} = CounterViewport.start_link(count: 0)
    renderer = Emerge.Viewport.renderer(pid)

    GenServer.cast(pid, {:emerge_viewport, :rerender})
    GenServer.cast(pid, {:emerge_viewport, :rerender})
    GenServer.cast(pid, {:emerge_viewport, :rerender})

    assert_eventually(
      fn ->
        renderer
        |> FakeRenderer.ops()
        |> Enum.count(fn
          {:patch_tree, _tree} -> true
          _ -> false
        end) == 1
      end,
      100
    )

    GenServer.stop(pid)
  end

  test "source reload notifications rerender mounted viewports" do
    {:ok, pid} = CounterViewport.start_link(count: 0)
    renderer = Emerge.Viewport.renderer(pid)

    :ok = Emerge.Viewport.notify_source_reloaded(%{source: :test})

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

  test "source reload notifications are coalesced" do
    {:ok, pid} = CounterViewport.start_link(count: 0)
    renderer = Emerge.Viewport.renderer(pid)

    :ok = Emerge.Viewport.notify_source_reloaded(%{source: :test})
    :ok = Emerge.Viewport.notify_source_reloaded(%{source: :test})
    :ok = Emerge.Viewport.notify_source_reloaded(%{source: :test})

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

  test "initial render failures keep viewport alive without starting the renderer" do
    log =
      capture_log(fn ->
        {:ok, pid} = RecoveringViewport.start_link(mode: :raise)

        assert_eventually(fn ->
          state = :sys.get_state(pid)

          Process.alive?(pid) and is_nil(state.renderer) and is_nil(state.diff_state) and
            pid in Emerge.Viewport.ReloadGroup.local_members()
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

          Process.alive?(pid) and state.renderer != nil and state.diff_state != nil and
            rendered_text(pid) == "recovered" and
            pid in Emerge.Viewport.ReloadGroup.local_members()
        end)

        renderer = Emerge.Viewport.renderer(pid)
        assert count_renderer_ops(renderer, :upload_tree) == 1

        GenServer.stop(pid)
      end)

    assert log =~ "initial render failed"
  end

  test "rerender failures keep the previous frame and the viewport alive" do
    {:ok, pid} = RecoveringViewport.start_link(mode: {:label, "before"})
    renderer = Emerge.Viewport.renderer(pid)

    log =
      capture_log(fn ->
        send(pid, {:set_mode, :raise})

        assert_eventually(fn ->
          state = :sys.get_state(pid)

          Process.alive?(pid) and rendered_text(pid) == "before" and
            count_renderer_ops(renderer, :patch_tree) == 0 and not state.dirty? and
            not state.flush_scheduled?
        end)
      end)

    assert log =~ "rerender failed"

    GenServer.stop(pid)
  end

  test "viewport can recover after a failed rerender" do
    {:ok, pid} = RecoveringViewport.start_link(mode: {:label, "before"})
    renderer = Emerge.Viewport.renderer(pid)

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

    assert_receive {:input_event, {:focused, true}}
    assert Emerge.Viewport.user_state(pid).events == [{:focused, true}]

    GenServer.stop(pid)
  end

  test "payload wrapping callback is applied before dispatch" do
    {:ok, pid} = PayloadViewport.start_link(test_pid: self())

    state = :sys.get_state(pid)

    {id_bin, _events} =
      Enum.find(state.diff_state.event_registry, fn {_id_bin, events} ->
        Map.has_key?(events, :change)
      end)

    send(pid, {:emerge_skia_event, {id_bin, :change, "hello"}})

    assert_receive {:wrapped_payload, "hello"}

    GenServer.stop(pid)
  end

  test "renderer liveness check stops viewport when renderer closes" do
    {:ok, pid} = LivenessViewport.start_link()
    renderer = Emerge.Viewport.renderer(pid)
    FakeRenderer.set_running(renderer, false)

    ref = Process.monitor(pid)
    send(pid, {:emerge_viewport, :check_renderer})

    assert_receive {:DOWN, ^ref, :process, ^pid, :normal}
  end

  test "stopping viewport stops renderer" do
    {:ok, pid} = CounterViewport.start_link(count: 1)
    renderer = Emerge.Viewport.renderer(pid)

    ref = Process.monitor(renderer)
    GenServer.stop(pid)

    assert_receive {:DOWN, ^ref, :process, ^renderer, :normal}
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

  defp assert_eventually(_fun, 0), do: flunk("condition was not met")

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
      %Emerge.Viewport.State{
        diff_state: %Emerge.DiffState{
          tree: %Emerge.Element{children: [%Emerge.Element{attrs: %{content: content}}]}
        }
      } ->
        content

      _ ->
        nil
    end
  end
end
