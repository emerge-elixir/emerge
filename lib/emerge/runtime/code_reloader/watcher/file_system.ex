defmodule Emerge.Runtime.CodeReloader.Watcher.FileSystem do
  @moduledoc false

  use GenServer

  @behaviour Emerge.Runtime.CodeReloader.Watcher

  @impl true
  def start_link(opts) when is_list(opts) do
    if Code.ensure_loaded?(FileSystem) do
      {genserver_opts, init_opts} = Keyword.split(opts, [:name])
      GenServer.start_link(__MODULE__, init_opts, genserver_opts)
    else
      {:error,
       {:missing_dependency,
        "Emerge.Runtime.CodeReloader requires the :file_system dependency for watcher support. Add {:file_system, \"~> 1.0\", only: :dev} to the consuming application's deps."}}
    end
  end

  @impl true
  def init(opts) do
    dirs = Keyword.fetch!(opts, :dirs)
    target = Keyword.fetch!(opts, :target)

    watcher_opts =
      opts
      |> Keyword.drop([:dirs, :target])
      |> Keyword.merge(dirs: dirs)

    case apply(FileSystem, :start_link, [watcher_opts]) do
      {:ok, watcher_pid} ->
        :ok = apply(FileSystem, :subscribe, [watcher_pid])
        {:ok, %{target: target, watcher_pid: watcher_pid}}

      other ->
        {:stop, other}
    end
  end

  @impl true
  def handle_info(
        {:file_event, watcher_pid, {path, _events}},
        %{watcher_pid: watcher_pid} = state
      ) do
    paths = if is_list(path), do: path, else: [path]

    paths
    |> Enum.filter(&is_binary/1)
    |> Enum.each(fn file_path ->
      send(state.target, {:emerge_code_reloader, :file_changed, file_path})
    end)

    {:noreply, state}
  end

  def handle_info({:file_event, watcher_pid, :stop}, %{watcher_pid: watcher_pid} = state) do
    {:stop, {:watcher_stopped, watcher_pid}, state}
  end

  def handle_info(_message, state) do
    {:noreply, state}
  end
end
