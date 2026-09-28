defmodule Emerge.Runtime.Viewport.Lifecycle do
  @moduledoc false
  use GenServer

  alias Emerge.Runtime.VideoEndpoints
  alias Emerge.Runtime.Viewport.Renderer.Skia
  alias Emerge.Runtime.Viewport.SessionRelay

  def start_link(opts), do: GenServer.start_link(__MODULE__, opts)
  def acquire(pid), do: GenServer.call(pid, :acquire, 25_000)
  def close(pid), do: GenServer.call(pid, :close, 6_000)
  def discard(pid), do: GenServer.call(pid, :discard, 6_000)
  def check(pid), do: send(pid, :check)

  @impl true
  def init({viewport, config}) do
    Process.flag(:trap_exit, true)
    ref = Process.monitor(viewport)

    {:ok,
     %{
       viewport: viewport,
       monitor: ref,
       config: config,
       renderer: nil,
       cleanup_requested: false,
       generation: nil,
       relay: nil,
       retry: nil,
       check_timer: nil,
       check_tag: nil,
       attempts: 0,
       closed: false
     }}
  end

  @impl true
  def handle_call(:acquire, _from, %{closed: true} = state),
    do: {:reply, {:error, :closed}, state}

  def handle_call(:acquire, _from, %{cleanup_requested: true, renderer: renderer} = state)
      when not is_nil(renderer), do: {:reply, {:error, :cleanup_pending}, state}

  def handle_call(:acquire, _from, %{renderer: nil, relay: relay} = state)
      when is_pid(relay), do: {:reply, {:error, :cleanup_pending}, state}

  def handle_call(:acquire, _from, %{renderer: nil, retry: retry} = state)
      when not is_nil(retry), do: {:reply, {:error, :retry_pending}, state}

  def handle_call(:acquire, _from, %{renderer: nil, attempts: attempts} = state)
      when attempts >= 20, do: {:reply, {:error, :retry_exhausted}, state}

  def handle_call(:acquire, _from, %{renderer: nil} = state) do
    {reply, state} = start_renderer(state)
    {:reply, reply, state}
  end

  def handle_call(:acquire, _from, state),
    do: {:reply, {:ok, state.renderer, state.generation, state.relay}, state}

  def handle_call(:discard, _from, state) do
    {result, state} = stop_renderer(state)
    state = if result == :ok, do: schedule_retry(state), else: schedule_check(state)
    {:reply, result, state}
  end

  def handle_call(:close, _from, state) do
    state = cancel_retry(%{state | closed: true})
    {result, state} = stop_renderer(state)
    {:reply, result, state}
  end

  @impl true
  def handle_info({:DOWN, ref, :process, _pid, _reason}, %{monitor: ref} = state),
    do: {:stop, :normal, state}

  def handle_info(:retry, %{closed: false, renderer: nil} = state) do
    {reply, state} = start_renderer(%{state | retry: nil})

    case reply do
      {:ok, renderer, generation, relay} ->
        send(
          state.viewport,
          {:emerge_viewport, :renderer_ready, self(), generation, renderer, relay}
        )

      {:error, reason} ->
        send(state.viewport, {:emerge_viewport, :renderer_error, self(), reason})
    end

    {:noreply, state}
  end

  def handle_info(
        {:session_retired, generation, relay, closed},
        %{generation: generation, relay: relay} = state
      ) do
    state = %{state | relay: nil, closed: state.closed or closed}
    {:noreply, if(state.closed, do: cancel_retry(state), else: schedule_retry(state))}
  end

  def handle_info({:EXIT, relay, _reason}, %{relay: relay, renderer: nil} = state) do
    {:noreply, schedule_retry(%{state | relay: nil})}
  end

  def handle_info(
        {:EXIT, relay, _reason},
        %{relay: relay, renderer: renderer, closed: false} = state
      )
      when not is_nil(renderer) do
    renderer_unavailable(%{state | relay: nil})
  end

  def handle_info({:check, tag}, %{check_tag: tag} = state), do: handle_info(:check, state)

  def handle_info(:check, %{renderer: renderer, closed: false} = state)
      when not is_nil(renderer) do
    module = state.config.renderer_module

    case invoke(fn -> module.running?(renderer) end) do
      true ->
        {:noreply, schedule_check(state)}

      _ ->
        renderer_unavailable(state)
    end
  end

  # Only the native adapter opts into session-tagged input/close/log routing. Custom adapters
  # retain their existing callback PID contract and can adopt status/recovery separately.
  def handle_info({:emerge_skia_close, _reason} = message, state) do
    send(state.viewport, {:emerge_viewport_renderer, state.generation, message})
    {:noreply, state}
  end

  def handle_info({tag, _, _, _} = message, state) when tag == :emerge_skia_log do
    send(state.viewport, {:emerge_viewport_renderer, state.generation, message})
    {:noreply, state}
  end

  def handle_info({tag, _} = message, state)
      when tag in [:emerge_skia_event, :emerge_viewport_renderer] do
    send(state.viewport, {:emerge_viewport_renderer, state.generation, message})
    {:noreply, state}
  end

  def handle_info(_message, state), do: {:noreply, state}

  @impl true
  def terminate(_reason, state) do
    _ = stop_renderer(cancel_retry(state))
    :ok
  end

  defp renderer_unavailable(state) do
    {result, state} = stop_renderer(state, 0)

    pending = result != :ok and cleanup_pending?(state)
    notification = if pending, do: :pending, else: result

    if result != :ok and not pending do
      send(state.viewport, {:emerge_viewport, :renderer_error, self(), result})
    end

    send(
      state.viewport,
      {:emerge_viewport, :renderer_unavailable, self(), state.generation, notification}
    )

    state =
      cond do
        result == :ok -> schedule_retry(state)
        pending -> schedule_check(state)
        true -> %{state | closed: true}
      end

    {:noreply, state}
  end

  defp start_renderer(state) do
    config = state.config

    opts =
      if config.renderer_module == Skia,
        do: Keyword.put(config.skia_opts, :owner, state.viewport),
        else: config.skia_opts

    case invoke(fn -> config.renderer_module.start(opts, config.renderer_opts) end) do
      {:ok, renderer} ->
        generation = make_ref()

        relay =
          if config.renderer_module == Skia,
            do: SessionRelay.start_link(state.viewport, generation),
            else: state.viewport

        state = %{
          state
          | renderer: renderer,
            cleanup_requested: false,
            generation: generation,
            relay: relay,
            retry: nil
        }

        if Process.alive?(state.viewport) do
          {{:ok, renderer, generation, relay}, schedule_check(state)}
        else
          {_, state} = stop_renderer(state)
          {{:error, :owner_down}, %{state | closed: true}}
        end

      {:error, reason} ->
        {{:error, reason}, schedule_retry(state)}

      other ->
        {{:error, {:invalid_renderer_start, other}}, schedule_retry(state)}
    end
  end

  defp stop_renderer(state, timeout \\ 5_000)
  defp stop_renderer(%{renderer: nil} = state, _timeout), do: {:ok, state}

  defp stop_renderer(state, timeout) do
    :ok = VideoEndpoints.unregister(state.viewport, state.renderer)

    result =
      invoke(fn ->
        if state.config.renderer_module == Skia do
          stopped =
            if timeout == 0 and state.cleanup_requested do
              case EmergeSkia.renderer_status(state.renderer) do
                {:ok, %{cleanup_complete: true}} -> :ok
                result -> {:error, {:cleanup_pending, result}}
              end
            else
              EmergeSkia.stop(state.renderer, timeout: timeout)
            end

          case stopped do
            {:error, :unsupported} -> state.config.renderer_module.stop(state.renderer)
            result -> result
          end
        else
          state.config.renderer_module.stop(state.renderer)
        end
      end)

    # Only retire the mailbox after all native senders have joined. Its acknowledgement
    # fences queued close requests before a replacement can be admitted.
    state = %{state | cleanup_requested: true}

    if result == :ok do
      relay =
        if state.relay && state.relay != state.viewport && Process.alive?(state.relay) do
          send(state.relay, {:retire, self()})
          state.relay
        end

      {:ok, %{state | renderer: nil, relay: relay}}
    else
      {result, state}
    end
  end

  defp cleanup_pending?(%{config: %{renderer_module: Skia}, renderer: renderer}) do
    case EmergeSkia.renderer_status(renderer) do
      {:ok, %{state: :quarantined}} -> false
      {:error, :unsupported} -> false
      _ -> true
    end
  end

  defp cleanup_pending?(_state), do: false

  defp schedule_check(state) do
    if state.config.renderer_module == Skia do
      if state.check_timer, do: Process.cancel_timer(state.check_timer)
      tag = make_ref()

      timer =
        Process.send_after(self(), {:check, tag}, state.config.renderer_check_interval_ms || 500)

      %{state | check_timer: timer, check_tag: tag}
    else
      state
    end
  end

  defp schedule_retry(%{relay: relay} = state) when is_pid(relay), do: state

  defp schedule_retry(%{closed: false, retry: nil, attempts: attempts} = state)
       when attempts < 20 do
    if state.config.renderer_module == Skia do
      delay = min(250 * Integer.pow(2, min(attempts, 5)), 5_000)
      %{state | attempts: attempts + 1, retry: Process.send_after(self(), :retry, delay)}
    else
      state
    end
  end

  defp schedule_retry(state), do: state

  defp cancel_retry(state) do
    if state.retry, do: Process.cancel_timer(state.retry)
    %{state | retry: nil}
  end

  defp invoke(fun) do
    fun.()
  rescue
    error -> {:error, Exception.message(error)}
  catch
    kind, reason -> {:error, {kind, reason}}
  end
end
