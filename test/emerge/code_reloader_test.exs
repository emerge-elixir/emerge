defmodule Emerge.Runtime.CodeReloaderTest do
  use ExUnit.Case, async: false

  import ExUnit.CaptureLog
  use Emerge.UI

  alias Emerge.Runtime.CodeReloader
  alias Emerge.Runtime.CodeReloader.MixListener

  defmodule FakeRenderer do
    @behaviour Emerge.Runtime.Viewport.Renderer

    @impl true
    def start(_skia_opts, _renderer_opts) do
      Agent.start_link(fn -> %{ops: [], running?: true} end)
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

    @impl true
    def set_input_target(renderer, pid) do
      Agent.update(renderer, &log_op(&1, {:set_input_target, pid}))
      :ok
    end

    @impl true
    def set_log_target(renderer, pid) do
      Agent.update(renderer, &log_op(&1, {:set_log_target, pid}))
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

    def ops(renderer), do: Agent.get(renderer, &Enum.reverse(&1.ops))

    defp log_op(state, op), do: %{state | ops: [op | state.ops]}
  end

  defmodule ReloadViewport do
    use Emerge

    @impl Viewport
    def mount(_opts) do
      {:ok, %{},
       emerge_skia: [otp_app: :emerge],
       viewport: [
         renderer_module: Emerge.Runtime.CodeReloaderTest.FakeRenderer,
         renderer_check_interval_ms: nil
       ]}
    end

    @impl Viewport
    def render(_state), do: el([], text("reload"))
  end

  defmodule FakeWatcher do
    use GenServer

    @behaviour Emerge.Runtime.CodeReloader.Watcher

    @impl true
    def start_link(opts) do
      {genserver_opts, init_opts} = Keyword.split(opts, [:name])
      GenServer.start_link(__MODULE__, init_opts, genserver_opts)
    end

    def emit(pid, path) when is_pid(pid) and is_binary(path) do
      GenServer.cast(pid, {:emit, path})
    end

    @impl true
    def init(opts) do
      send(Keyword.fetch!(opts, :test_pid), {:watcher_started, self()})
      {:ok, %{target: Keyword.fetch!(opts, :target)}}
    end

    @impl true
    def handle_cast({:emit, path}, state) do
      send(state.target, {:emerge_code_reloader, :file_changed, path})
      {:noreply, state}
    end
  end

  defmodule MissingDependencyWatcher do
    @behaviour Emerge.Runtime.CodeReloader.Watcher

    @impl true
    def start_link(_opts) do
      {:error,
       {:missing_dependency,
        "Emerge.Runtime.CodeReloader requires the :file_system dependency for watcher support."}}
    end
  end

  defmodule FakeCompiler do
    def reload(reloadable_apps, opts) do
      send(
        Keyword.fetch!(opts, :test_pid),
        {:compiler_reloaded, reloadable_apps, Keyword.fetch!(opts, :paths)}
      )

      Keyword.fetch!(opts, :result)
    end
  end

  defmodule PurgedModule do
    def ping, do: :pong
  end

  test "debounces repeated file changes into a single compile" do
    {:ok, reloader_pid} =
      CodeReloader.start_link(
        dirs: ["/workspace/emerge/lib"],
        reloadable_apps: [:emerge],
        debounce_ms: 10,
        watcher: FakeWatcher,
        watcher_opts: [test_pid: self()],
        compiler: FakeCompiler,
        compiler_opts: [test_pid: self(), result: :noop],
        mix_listener: nil
      )

    assert_receive {:watcher_started, watcher_pid}

    FakeWatcher.emit(watcher_pid, "/workspace/emerge/lib/emerge/code_reloader.ex")
    FakeWatcher.emit(watcher_pid, "/workspace/emerge/lib/emerge/code_reloader.ex")
    FakeWatcher.emit(watcher_pid, "/workspace/emerge/lib/emerge/viewport.ex")
    FakeWatcher.emit(watcher_pid, "/workspace/emerge/lib/emerge/viewport.txt")

    assert_receive {:compiler_reloaded, [:emerge], paths}

    assert paths == [
             "/workspace/emerge/lib/emerge/code_reloader.ex",
             "/workspace/emerge/lib/emerge/viewport.ex"
           ]

    refute_receive {:compiler_reloaded, _, _}, 50

    GenServer.stop(reloader_pid)
  end

  test "successful watcher compile rerenders mounted viewports" do
    {:ok, viewport_pid} = ReloadViewport.start_link()
    renderer = Emerge.renderer(viewport_pid)

    {:ok, reloader_pid} =
      CodeReloader.start_link(
        dirs: ["/workspace/emerge/lib"],
        reloadable_apps: [:emerge],
        debounce_ms: 10,
        watcher: FakeWatcher,
        watcher_opts: [test_pid: self()],
        compiler: FakeCompiler,
        compiler_opts: [test_pid: self(), result: :ok],
        mix_listener: nil
      )

    assert_receive {:watcher_started, watcher_pid}

    FakeWatcher.emit(watcher_pid, "/workspace/emerge/lib/emerge/code_reloader.ex")

    assert_receive {:compiler_reloaded, [:emerge], _paths}

    assert_eventually(fn -> patch_count(renderer) == 1 end)

    GenServer.stop(reloader_pid)
    GenServer.stop(viewport_pid)
  end

  test "failed watcher compile does not rerender mounted viewports" do
    {:ok, viewport_pid} = ReloadViewport.start_link()
    renderer = Emerge.renderer(viewport_pid)

    {:ok, reloader_pid} =
      CodeReloader.start_link(
        dirs: ["/workspace/emerge/lib"],
        reloadable_apps: [:emerge],
        debounce_ms: 10,
        watcher: FakeWatcher,
        watcher_opts: [test_pid: self()],
        compiler: FakeCompiler,
        compiler_opts: [test_pid: self(), result: {:error, "boom"}],
        mix_listener: nil
      )

    log =
      capture_log(fn ->
        assert_receive {:watcher_started, watcher_pid}

        FakeWatcher.emit(watcher_pid, "/workspace/emerge/lib/emerge/code_reloader.ex")

        assert_receive {:compiler_reloaded, [:emerge], _paths}
        Process.sleep(50)
        assert patch_count(renderer) == 0

        GenServer.stop(reloader_pid)
        GenServer.stop(viewport_pid)
      end)

    assert log =~ "Emerge hot reload failed"
    assert log =~ "boom"
  end

  test "fails fast when watcher dependency is unavailable" do
    trap_exit? = Process.flag(:trap_exit, true)
    on_exit(fn -> Process.flag(:trap_exit, trap_exit?) end)

    assert {:error, reason} =
             CodeReloader.start_link(
               dirs: [__DIR__],
               reloadable_apps: [:emerge],
               watcher: MissingDependencyWatcher,
               mix_listener: nil
             )

    case reason do
      message when is_binary(message) ->
        assert message =~ ":file_system"

      {:missing_dependency, message} ->
        assert message =~ ":file_system"

      :ignore ->
        assert true
    end
  end

  test "mix listener ignores compiles from the current os process" do
    start_supervised!(MixListener)
    :ok = MixListener.subscribe(self(), [:emerge])

    {:ok, viewport_pid} = ReloadViewport.start_link()
    renderer = Emerge.renderer(viewport_pid)

    send(
      MixListener,
      {:modules_compiled,
       %{
         app: :emerge,
         os_pid: System.pid(),
         modules_diff: %{
           added: [],
           changed: [],
           removed: [],
           timestamp: System.system_time(:second)
         }
       }}
    )

    Process.sleep(50)
    assert patch_count(renderer) == 0

    GenServer.stop(viewport_pid)
  end

  test "mix listener purges modules and rerenders on external compiles" do
    start_supervised!(MixListener)
    :ok = MixListener.subscribe(self(), [:emerge])

    {:ok, viewport_pid} = ReloadViewport.start_link()
    renderer = Emerge.renderer(viewport_pid)

    assert PurgedModule.ping() == :pong
    assert :code.is_loaded(PurgedModule) != false

    send(
      MixListener,
      {:modules_compiled,
       %{
         app: :emerge,
         os_pid: "external-os-pid",
         modules_diff: %{
           added: [],
           changed: [PurgedModule],
           removed: [],
           timestamp: System.system_time(:second)
         }
       }}
    )

    assert_eventually(fn -> patch_count(renderer) == 1 end)
    assert_eventually(fn -> :code.is_loaded(PurgedModule) == false end)

    GenServer.stop(viewport_pid)
  end

  defp patch_count(renderer) do
    renderer
    |> FakeRenderer.ops()
    |> Enum.count(fn
      {:patch_tree, _tree} -> true
      _ -> false
    end)
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
