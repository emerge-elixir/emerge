defmodule Emerge.CodeReloader do
  @moduledoc """
  Watches Elixir source files, recompiles selected Mix apps, and rerenders live viewports.

  The reloader is opt-in and configured entirely through child options.
  """

  use GenServer

  require Logger

  alias Emerge.CodeReloader.Compiler
  alias Emerge.CodeReloader.MixListener
  alias Emerge.CodeReloader.Watcher.FileSystem

  @default_debounce_ms 100

  defmodule State do
    @moduledoc false

    @enforce_keys [:compiler, :compiler_opts, :debounce_ms, :dirs, :reloadable_apps, :watcher_pid]
    defstruct compiler: nil,
              compiler_opts: [],
              debounce_ms: 100,
              dirs: [],
              mix_listener: nil,
              pending_paths: MapSet.new(),
              reloadable_apps: [],
              timer_ref: nil,
              watcher_pid: nil
  end

  @type start_opt ::
          {:dirs, [Path.t()]}
          | {:reloadable_apps, [atom()]}
          | {:debounce_ms, non_neg_integer()}
          | {:watcher, module()}
          | {:watcher_opts, keyword()}
          | {:compiler, module()}
          | {:compiler_opts, keyword()}
          | {:reloadable_compilers, [atom()]}
          | {:reloadable_args, [String.t()]}
          | {:mix_listener, module() | nil}
          | GenServer.option()

  @spec start_link([start_opt()]) :: GenServer.on_start()
  def start_link(opts) when is_list(opts) do
    {genserver_opts, init_opts} = Keyword.split(opts, [:name, :timeout, :debug, :spawn_opt])
    GenServer.start_link(__MODULE__, init_opts, genserver_opts)
  end

  @spec child_spec([start_opt()]) :: Supervisor.child_spec()
  def child_spec(opts) do
    %{
      id: Keyword.get(opts, :name, __MODULE__),
      start: {__MODULE__, :start_link, [opts]},
      type: :worker
    }
  end

  @impl true
  def init(opts) do
    with :ok <- ensure_mix_available(),
         {:ok, dirs} <- validate_dirs(opts),
         {:ok, reloadable_apps} <- validate_reloadable_apps(opts),
         {:ok, debounce_ms} <- validate_debounce_ms(opts),
         {:ok, watcher_module} <- validate_module_opt(opts, :watcher, FileSystem),
         {:ok, compiler_module} <- validate_module_opt(opts, :compiler, Compiler),
         {:ok, watcher_pid} <- start_watcher(watcher_module, dirs, opts) do
      subscribe_mix_listener(Keyword.get(opts, :mix_listener, MixListener), reloadable_apps)

      {:ok,
       %State{
         compiler: compiler_module,
         compiler_opts: compiler_opts(opts),
         debounce_ms: debounce_ms,
         dirs: dirs,
         mix_listener: Keyword.get(opts, :mix_listener, MixListener),
         reloadable_apps: reloadable_apps,
         watcher_pid: watcher_pid
       }}
    else
      {:error, reason} -> {:stop, reason}
    end
  end

  @impl true
  def handle_info({:emerge_code_reloader, :file_changed, path}, %State{} = state)
      when is_binary(path) do
    if reloadable_path?(path, state.dirs) do
      {:noreply, track_path(state, path)}
    else
      {:noreply, state}
    end
  end

  def handle_info(
        {:emerge_code_reloader, :compile_pending, timer_ref},
        %State{timer_ref: timer_ref} = state
      ) do
    paths = state.pending_paths |> MapSet.to_list() |> Enum.sort()
    state = %{state | pending_paths: MapSet.new(), timer_ref: nil}
    {:noreply, compile_paths(paths, state)}
  end

  def handle_info({:emerge_code_reloader, :compile_pending, _timer_ref}, state) do
    {:noreply, state}
  end

  def handle_info(_message, state) do
    {:noreply, state}
  end

  @impl true
  def terminate(_reason, %State{mix_listener: nil}), do: :ok

  def terminate(_reason, %State{mix_listener: mix_listener}) do
    _ = mix_listener.unsubscribe(self())
    :ok
  end

  defp ensure_mix_available do
    cond do
      not Code.ensure_loaded?(Mix.Project) ->
        {:error, "Emerge.CodeReloader requires Mix to be available in the running VM."}

      is_nil(Mix.Project.get()) ->
        {:error, "Emerge.CodeReloader requires a current Mix project."}

      true ->
        :ok
    end
  end

  defp validate_dirs(opts) do
    dirs =
      opts
      |> Keyword.get(:dirs, [])
      |> Enum.map(&Path.expand/1)
      |> Enum.uniq()

    if dirs == [] do
      {:error, "Emerge.CodeReloader requires a non-empty :dirs list."}
    else
      {:ok, dirs}
    end
  end

  defp validate_reloadable_apps(opts) do
    apps = Keyword.get(opts, :reloadable_apps, [])

    cond do
      apps == [] ->
        {:error, "Emerge.CodeReloader requires a non-empty :reloadable_apps list."}

      Enum.all?(apps, &is_atom/1) ->
        {:ok, Enum.uniq(apps)}

      true ->
        {:error, "Emerge.CodeReloader :reloadable_apps must be a list of atoms."}
    end
  end

  defp validate_debounce_ms(opts) do
    debounce_ms = Keyword.get(opts, :debounce_ms, @default_debounce_ms)

    if is_integer(debounce_ms) and debounce_ms >= 0 do
      {:ok, debounce_ms}
    else
      {:error, "Emerge.CodeReloader :debounce_ms must be a non-negative integer."}
    end
  end

  defp validate_module_opt(opts, key, default) do
    module = Keyword.get(opts, key, default)

    if is_atom(module) do
      {:ok, module}
    else
      {:error, "Emerge.CodeReloader #{inspect(key)} must be a module."}
    end
  end

  defp start_watcher(watcher_module, dirs, opts) do
    watcher_opts =
      opts
      |> Keyword.get(:watcher_opts, [])
      |> Keyword.merge(dirs: dirs, target: self())

    case watcher_module.start_link(watcher_opts) do
      {:ok, watcher_pid} -> {:ok, watcher_pid}
      {:error, reason} -> {:error, format_watcher_reason(reason)}
      other -> {:error, "Failed to start #{inspect(watcher_module)}: #{inspect(other)}"}
    end
  end

  defp subscribe_mix_listener(nil, _reloadable_apps), do: :ok

  defp subscribe_mix_listener(mix_listener, reloadable_apps) do
    case mix_listener.subscribe(self(), reloadable_apps) do
      :ok ->
        :ok

      {:error, :not_running} ->
        Logger.warning(
          "Emerge.CodeReloader external compile notifications are unavailable because #{inspect(mix_listener)} is not running. Add listeners: [#{inspect(mix_listener)}] to your mix.exs project config to enable them."
        )

      {:error, reason} ->
        Logger.warning(
          "Emerge.CodeReloader could not subscribe to #{inspect(mix_listener)}: #{inspect(reason)}"
        )
    end
  end

  defp compiler_opts(opts) do
    opts
    |> Keyword.get(:compiler_opts, [])
    |> Keyword.merge(
      reloadable_args: Keyword.get(opts, :reloadable_args, ["--no-all-warnings"]),
      reloadable_compilers: Keyword.get(opts, :reloadable_compilers, [:elixir])
    )
  end

  defp reloadable_path?(path, dirs) do
    expanded_path = Path.expand(path)

    Path.extname(expanded_path) == ".ex" and
      not build_path?(expanded_path) and
      Enum.any?(dirs, &path_in_dir?(expanded_path, &1))
  end

  defp build_path?(path) do
    path
    |> Path.split()
    |> Enum.member?("_build")
  end

  defp path_in_dir?(path, dir) do
    path == dir or String.starts_with?(path, dir <> "/")
  end

  defp track_path(%State{} = state, path) do
    expanded_path = Path.expand(path)
    timer_ref = schedule_compile(state.timer_ref, state.debounce_ms)

    %{state | pending_paths: MapSet.put(state.pending_paths, expanded_path), timer_ref: timer_ref}
  end

  defp schedule_compile(nil, debounce_ms), do: make_timer_ref(debounce_ms)

  defp schedule_compile(timer_ref, debounce_ms) do
    Process.cancel_timer(timer_ref)
    make_timer_ref(debounce_ms)
  end

  defp make_timer_ref(debounce_ms) do
    timer_ref = make_ref()
    Process.send_after(self(), {:emerge_code_reloader, :compile_pending, timer_ref}, debounce_ms)
    timer_ref
  end

  defp compile_paths([], %State{} = state), do: state

  defp compile_paths(paths, %State{} = state) do
    case state.compiler.reload(
           state.reloadable_apps,
           Keyword.merge(state.compiler_opts, paths: paths)
         ) do
      :ok ->
        :ok =
          Emerge.Viewport.notify_source_reloaded(%{
            source: :watcher,
            apps: state.reloadable_apps,
            paths: paths
          })

        state

      :noop ->
        state

      {:error, message} ->
        Logger.error(["Emerge hot reload failed:\n", message])
        state
    end
  end

  defp format_watcher_reason(reason) when is_binary(reason), do: reason
  defp format_watcher_reason(reason), do: inspect(reason)
end
