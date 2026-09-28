defmodule Emerge.Runtime.VideoEndpoints do
  @moduledoc false
  use GenServer

  alias VideoInterop.Frame
  @prefix {__MODULE__, :renderer}

  # One endpoint owner per viewport, supervised by that viewport. There is no global server.
  def start_link(viewport), do: GenServer.start_link(__MODULE__, viewport)

  @spec register(pid(), term()) :: :ok
  def register(viewport, renderer) when is_pid(viewport) do
    {owner, _renderer} = :persistent_term.get({@prefix, viewport})
    GenServer.call(owner, {:register, renderer})
  end

  @spec unregister(pid(), term()) :: :ok
  def unregister(viewport, renderer \\ :any) when is_pid(viewport) do
    case :persistent_term.get({@prefix, viewport}, :missing) do
      :missing ->
        :ok

      {owner, _} ->
        try do
          GenServer.call(owner, {:unregister, renderer})
        catch
          :exit, _ -> erase_if_owner(viewport, owner)
        end
    end
  end

  @impl true
  def init(viewport) do
    Process.flag(:trap_exit, true)
    monitor = Process.monitor(viewport)
    :persistent_term.put({@prefix, viewport}, {self(), nil})
    {:ok, %{viewport: viewport, monitor: monitor, renderer: nil}}
  end

  @impl true
  def handle_call({:register, renderer}, _from, state) do
    :persistent_term.put({@prefix, state.viewport}, {self(), renderer})
    {:reply, :ok, %{state | renderer: renderer}}
  end

  def handle_call({:unregister, expected}, _from, state) do
    if expected == :any or expected == state.renderer do
      :persistent_term.put({@prefix, state.viewport}, {self(), nil})
      {:reply, :ok, %{state | renderer: nil}}
    else
      {:reply, :ok, state}
    end
  end

  @impl true
  def handle_info({:DOWN, ref, :process, _pid, _reason}, %{monitor: ref} = state),
    do: {:stop, :normal, state}

  @impl true
  def terminate(_reason, state), do: erase_if_owner(state.viewport, self())

  defp erase_if_owner(viewport, owner) do
    case :persistent_term.get({@prefix, viewport}, :missing) do
      {^owner, _} -> :persistent_term.erase({@prefix, viewport})
      _ -> :ok
    end

    :ok
  end

  @spec submit(pid(), atom(), Frame.t()) :: :ok | {:error, term()}
  def submit(viewport, target, %Frame{} = frame) when is_pid(viewport) and is_atom(target) do
    case :persistent_term.get({@prefix, viewport}, :missing) do
      :missing -> consume_error(frame, :viewport_not_ready)
      {_owner, nil} -> consume_error(frame, :viewport_not_ready)
      {_owner, renderer} -> submit_to_renderer(renderer, target, frame)
    end
  end

  defp submit_to_renderer(renderer, target, frame) do
    with :ok <- VideoInterop.validate(frame),
         :ok <- EmergeSkia.submit_video_frame(renderer, target, frame) do
      :ok
    else
      {:error, {:transferred, reason}} -> {:error, reason}
      {:error, {:caller_owned, reason}} -> consume_error(frame, reason)
      {:error, reason} -> consume_error(frame, reason)
    end
  end

  defp consume_error(frame, reason) do
    :ok = VideoInterop.release(frame)
    {:error, reason}
  end
end
