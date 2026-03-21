defmodule Emerge.ViewportTest do
  use ExUnit.Case, async: false

  import Emerge.UI

  defmodule FakeRenderer do
    @behaviour Emerge.Viewport.Renderer

    @impl true
    def start(_skia_opts, _renderer_opts) do
      Agent.start_link(fn -> %{ops: [], running?: true, target: nil} end)
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

    defp log_op(state, op), do: %{state | ops: [op | state.ops]}
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

  test "mount starts renderer and uploads initial tree" do
    {:ok, pid} = CounterViewport.start_link(count: 3)

    renderer = Emerge.Viewport.renderer(pid)
    operations = FakeRenderer.ops(renderer)

    assert {:set_input_target, ^pid} = Enum.at(operations, 0)
    assert {:upload_tree, %Emerge.Element{type: :row}} = Enum.at(operations, 1)

    assert Emerge.Viewport.user_state(pid) == %{count: 3}

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

    patch_count =
      renderer
      |> FakeRenderer.ops()
      |> Enum.count(fn
        {:patch_tree, _tree} -> true
        _ -> false
      end)

    assert patch_count == 1

    GenServer.stop(pid)
  end

  test "rerender requests are coalesced" do
    {:ok, pid} = CounterViewport.start_link(count: 0)
    renderer = Emerge.Viewport.renderer(pid)

    GenServer.cast(pid, {:emerge_viewport, :rerender})
    GenServer.cast(pid, {:emerge_viewport, :rerender})
    GenServer.cast(pid, {:emerge_viewport, :rerender})

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
end
