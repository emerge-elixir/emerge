defmodule Emerge.ConfigReconciliationTest do
  use ExUnit.Case, async: false
  import ExUnit.CaptureLog
  alias Emerge.Config
  alias Emerge.Runtime.Viewport.Lifecycle

  defmodule Adapter do
    @behaviour Emerge.Runtime.Viewport.Renderer

    def start(config, opts) do
      observer = Keyword.fetch!(opts, :observer)
      control = Keyword.fetch!(opts, :control)

      if Agent.get_and_update(control, &{&1, :normal}) == :block do
        send(observer, {:starting, self(), config})

        receive do
          :continue_start -> :ok
        after
          5_000 -> raise "test startup gate timed out"
        end
      end

      Agent.start_link(fn -> %{config: config, observer: observer, outcome: :ok} end)
    end

    def configure(renderer, previous, desired) do
      outcome =
        Agent.get_and_update(renderer, fn state ->
          send(state.observer, {:configured, previous, desired})
          # Deliberately model a partial side effect before returning failure.
          {state.outcome, %{state | config: desired, outcome: :ok}}
        end)

      case outcome do
        :raise -> raise "adapter failed"
        :exit -> exit(:adapter_failed)
        other -> other
      end
    end

    def stop(renderer), do: Agent.stop(renderer)
    def running?(renderer), do: Process.alive?(renderer)
    def set_input_target(_, _), do: :ok
    def set_log_target(_, _), do: :ok
    def set_input_mask(_, _), do: :ok

    def upload_tree(renderer, tree) do
      Agent.get(renderer, &send(&1.observer, {:uploaded, &1.config.output.title}))
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
      {:ok, %{observer: Keyword.fetch!(opts, :observer), title: "initial", invalid?: false},
       %Config{
         output: %Config.Window{title: "initial"},
         runtime: [adapter: Adapter, adapter_opts: opts, health_check_interval_ms: nil]
       }}
    end

    def config(%Config{output: %Config.Window{} = output} = previous, state) do
      send(state.observer, {:previous, previous})
      output = %Config.Window{output | title: state.title}
      if state.invalid?, do: :invalid, else: %Config{previous | output: output}
    end

    def render(state) do
      send(state.observer, :rendered)
      text("unchanged")
    end

    def handle_info({:change, changes}, state),
      do: {:noreply, Viewport.rerender(Map.merge(state, changes))}
  end

  setup do
    control = start_supervised!({Agent, fn -> :normal end})
    viewport = start_supervised!({Screen, observer: self(), control: control})
    renderer = Emerge.renderer(viewport)
    assert_receive {:previous, _}
    assert_receive :rendered
    assert_receive {:uploaded, "initial"}
    %{viewport: viewport, renderer: renderer, control: control}
  end

  for outcome <- [{:error, :busy}, :raise, :exit, :unexpected] do
    test "retains desired intent after #{inspect(outcome)} and retries unchanged intent",
         context do
      %{viewport: viewport, renderer: renderer} = context
      Agent.update(renderer, &%{&1 | outcome: unquote(Macro.escape(outcome))})

      capture_log(fn ->
        change(viewport, %{title: "desired"})
      end)

      assert_receive {:configured, previous, desired}
      assert previous.output.title == "initial"
      assert desired.output.title == "desired"
      runtime = runtime(viewport)
      assert runtime.config == desired
      assert runtime.renderer_config == previous
      assert {:error, _} = runtime.config_error
      assert :sys.get_state(runtime.lifecycle).config.config == desired
      refute_receive :rendered, 20

      assert change(viewport, %{}).output.title == "desired"
      assert_receive {:configured, ^previous, ^desired}
      assert_receive :rendered
      assert runtime(viewport).renderer_config == desired
      assert runtime(viewport).config_error == nil
      assert Emerge.renderer(viewport) == renderer

      change(viewport, %{})
      assert_receive :rendered
      refute_receive {:configured, _, _}, 20
    end
  end

  test "reversing a partially failed update reapplies the complete old snapshot", context do
    %{viewport: viewport, renderer: renderer} = context
    Agent.update(renderer, &%{&1 | outcome: {:error, :busy}})
    capture_log(fn -> change(viewport, %{title: "partial"}) end)
    assert_receive {:configured, initial, _}
    assert Agent.get(renderer, & &1.config.output.title) == "partial"

    assert change(viewport, %{title: "initial"}).output.title == "partial"
    assert_receive {:configured, ^initial, ^initial}
    assert_receive :rendered
    assert Agent.get(renderer, & &1.config.output.title) == "initial"
    assert runtime(viewport).config_error == nil
  end

  test "invalid callback output preserves pending desired intent without applying it", context do
    %{viewport: viewport, renderer: renderer} = context
    Agent.update(renderer, &%{&1 | outcome: {:error, :busy}})
    capture_log(fn -> change(viewport, %{title: "desired"}) end)
    assert_receive {:configured, _, desired}
    before = runtime(viewport)

    capture_log(fn -> change(viewport, %{invalid?: true, title: "invalid"}) end)
    assert runtime(viewport).config == desired
    assert runtime(viewport).renderer_config == before.renderer_config
    assert runtime(viewport).config_error == before.config_error
    refute_receive {:configured, _, _}, 20
    refute_receive :rendered, 20
  end

  test "recovery replays changes made while startup is in progress", context do
    %{viewport: viewport, renderer: renderer, control: control} = context
    old = runtime(viewport)
    restart_config = :sys.get_state(old.lifecycle).config
    assert :ok = Lifecycle.discard(old.lifecycle)

    send(viewport, {
      :emerge_viewport,
      :renderer_unavailable,
      old.lifecycle,
      old.renderer_generation,
      :ok
    })

    assert runtime(viewport).renderer == nil
    Agent.update(control, fn _ -> :block end)
    # Native lifecycle retries are automatic; drive the same handler for this
    # deterministic custom adapter, blocking inside start rather than sleeping.
    send(old.lifecycle, :retry)
    assert_receive {:starting, owner, startup}
    assert startup.output.title == "initial"

    update = Task.async(fn -> Lifecycle.configure(owner, restart_config) end)
    assert :ok = Task.await(update, 1_000)

    send(viewport, {:change, %{title: "during restart"}})
    assert_receive {:previous, previous}
    assert previous.output.title == "initial"
    send(owner, :continue_start)

    assert_receive {:configured, ^startup, desired}
    assert desired.output.title == "during restart"
    assert_receive {:uploaded, "during restart"}
    current = runtime(viewport)
    assert current.renderer != renderer
    assert current.renderer_config == desired
    assert current.config == desired
    assert :sys.get_state(owner).startup_config == startup
    assert :sys.get_state(owner).config.config == desired
    refute_receive {:configured, _, _}, 20

    # A delayed ready notification cannot replace the adopted session/snapshot.
    send(viewport, {
      :emerge_viewport,
      :renderer_ready,
      owner,
      old.renderer_generation,
      renderer,
      viewport,
      startup
    })

    assert runtime(viewport).renderer == current.renderer
    assert runtime(viewport).renderer_config == desired
  end

  defp change(viewport, changes) do
    send(viewport, {:change, changes})
    assert_receive {:previous, previous}
    runtime(viewport)
    previous
  end

  defp runtime(viewport), do: :sys.get_state(viewport).__emerge__
end
