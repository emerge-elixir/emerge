defmodule Emerge.Runtime.CodeReloader.MixListener do
  @moduledoc false

  use GenServer

  @name __MODULE__

  @spec start_link(keyword()) :: GenServer.on_start()
  def start_link(_opts) do
    GenServer.start_link(__MODULE__, %{}, name: @name)
  end

  @spec subscribe(pid(), [atom()]) :: :ok | {:error, :not_running}
  def subscribe(pid, reloadable_apps) when is_pid(pid) and is_list(reloadable_apps) do
    if Process.whereis(@name) do
      GenServer.call(@name, {:subscribe, pid, reloadable_apps})
    else
      {:error, :not_running}
    end
  end

  @spec unsubscribe(pid()) :: :ok | {:error, :not_running}
  def unsubscribe(pid) when is_pid(pid) do
    if Process.whereis(@name) do
      GenServer.call(@name, {:unsubscribe, pid})
    else
      {:error, :not_running}
    end
  end

  @impl true
  def init(state) do
    {:ok, Map.put(state, :subscribers, %{})}
  end

  @impl true
  def handle_call({:subscribe, pid, reloadable_apps}, _from, state) do
    state = put_subscriber(state, pid, reloadable_apps)
    {:reply, :ok, state}
  end

  def handle_call({:unsubscribe, pid}, _from, state) do
    {:reply, :ok, drop_subscriber(state, pid)}
  end

  @impl true
  def handle_info({:DOWN, ref, :process, pid, _reason}, state) do
    subscribers =
      case state.subscribers do
        %{^pid => %{ref: ^ref}} -> Map.delete(state.subscribers, pid)
        _ -> state.subscribers
      end

    {:noreply, %{state | subscribers: subscribers}}
  end

  def handle_info({:modules_compiled, info}, state) do
    if same_process_compile?(info) or not interested_app?(state.subscribers, info.app) do
      {:noreply, state}
    else
      purge_modules(info.modules_diff)

      :ok =
        Emerge.notify_source_reloaded(%{
          source: :mix_listener,
          app: info.app,
          modules_diff: info.modules_diff
        })

      {:noreply, state}
    end
  end

  def handle_info(_message, state) do
    {:noreply, state}
  end

  defp put_subscriber(state, pid, reloadable_apps) do
    subscribers =
      Map.update(state.subscribers, pid, new_subscriber(pid, reloadable_apps), fn subscriber ->
        %{subscriber | apps: MapSet.new(reloadable_apps)}
      end)

    %{state | subscribers: subscribers}
  end

  defp new_subscriber(pid, reloadable_apps) do
    %{apps: MapSet.new(reloadable_apps), ref: Process.monitor(pid)}
  end

  defp drop_subscriber(state, pid) do
    case Map.pop(state.subscribers, pid) do
      {nil, subscribers} ->
        %{state | subscribers: subscribers}

      {%{ref: ref}, subscribers} ->
        Process.demonitor(ref, [:flush])
        %{state | subscribers: subscribers}
    end
  end

  defp same_process_compile?(%{os_pid: os_pid}), do: os_pid == System.pid()

  defp interested_app?(subscribers, app) do
    Enum.any?(subscribers, fn {_pid, subscriber} -> MapSet.member?(subscriber.apps, app) end)
  end

  defp purge_modules(modules_diff) do
    modules_diff
    |> modules_to_purge()
    |> Enum.each(fn module ->
      :code.purge(module)
      :code.delete(module)
    end)
  end

  defp modules_to_purge(%{changed: changed, removed: removed}) do
    changed
    |> Enum.concat(removed)
    |> MapSet.new()
    |> MapSet.to_list()
  end
end
