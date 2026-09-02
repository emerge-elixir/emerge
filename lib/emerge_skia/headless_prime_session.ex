defmodule EmergeSkia.HeadlessPrimeSession do
  @moduledoc false

  use GenServer

  require Logger

  alias EmergeSkia.Native
  alias VideoInterop.{AbandonmentGuard, Format, Frame, LeaseOwner, Rect, SyncFile}
  alias VideoInterop.DMABuf

  @internal_frame_message "emerge_skia_internal_prime_frame"
  @default_release_retry {:exponential, initial_ms: 10, max_ms: 1_000, max_attempts: :infinity}
  @dispatcher_close_retry_ms 50

  @enforce_keys [:pid, :renderer]
  defstruct @enforce_keys

  @type t :: %__MODULE__{pid: pid(), renderer: reference()}

  @spec start(map()) :: {:ok, t()} | {:error, term()}
  def start(native_opts) do
    producer = self()

    case GenServer.start(__MODULE__, {producer, native_opts}) do
      {:ok, pid} -> GenServer.call(pid, :renderer, :infinity)
      {:error, {:native_start, error}} -> {:error, error}
      {:error, reason} -> {:error, reason}
    end
  end

  @spec stop(t()) :: :ok | {:error, term()}
  def stop(%__MODULE__{pid: pid}) do
    GenServer.call(pid, :stop, :infinity)
  catch
    :exit, {:noproc, _details} -> :ok
    :exit, {:normal, _details} -> :ok
  end

  @spec running?(t()) :: boolean()
  def running?(%__MODULE__{pid: pid}) do
    GenServer.call(pid, :running?)
  catch
    :exit, _reason -> false
  end

  @doc false
  @spec release_backend_token(reference()) :: :ok
  def release_backend_token(token), do: Native.headless_prime_release_backend_token(token)

  @impl true
  def init({producer, native_opts}) do
    Process.flag(:trap_exit, true)

    external_target = native_opts.headless.target
    frame_message = native_opts.headless.frame_message
    max_active = native_opts.headless.prime.max_in_flight

    case Native.headless_prime_release_dispatcher_new() do
      {:ok, release_dispatcher} ->
        init_with_dispatcher(
          producer,
          native_opts,
          external_target,
          frame_message,
          max_active,
          release_dispatcher
        )

      error ->
        {:stop, {:native_start, error}}
    end
  end

  @impl true
  def handle_call(:renderer, _from, state) do
    {:reply, {:ok, %__MODULE__{pid: self(), renderer: state.renderer}}, state}
  end

  def handle_call(:running?, _from, state) do
    {:reply, state.mode == :open and Native.is_running(state.renderer), state}
  end

  def handle_call(:stop, _from, %{mode: :quarantined, shutdown_result: result} = state) do
    {:reply, result, state}
  end

  def handle_call(:stop, from, state) do
    state = %{state | stop_waiters: [from | state.stop_waiters]}
    {:noreply, begin_draining(state)}
  end

  @impl true
  def handle_info({@internal_frame_message, frame}, %{mode: :open} = state) when is_list(frame) do
    {:noreply, forward_frame(frame, state)}
  end

  def handle_info({@internal_frame_message, frame}, state) when is_list(frame) do
    release_unissued_frame(frame)
    {:noreply, state}
  end

  def handle_info(
        {:video_interop_lease_owner_drained, owner},
        %{lease_owner: owner, mode: :quarantined} = state
      ) do
    # Quarantine is permanent. In particular, consumer-close uncertainty must never be converted
    # into a delayed native stop merely because LeaseOwner later reports its own holders drained.
    {:noreply, state}
  end

  def handle_info({:video_interop_lease_owner_drained, owner}, %{lease_owner: owner} = state) do
    finish_shutdown(state, nil)
  end

  def handle_info(:retry_release_dispatcher_close, %{mode: :dispatcher_closing} = state) do
    close_release_dispatcher(state)
  end

  def handle_info({:DOWN, monitor, :process, _pid, _reason}, %{producer_monitor: monitor} = state) do
    {:noreply, begin_draining(state)}
  end

  def handle_info(
        {:DOWN, monitor, :process, _pid, _reason},
        %{destination_monitor: monitor} = state
      ) do
    {:noreply, begin_draining(state)}
  end

  def handle_info({:EXIT, owner, :normal}, %{lease_owner: owner} = state), do: {:noreply, state}

  def handle_info({:EXIT, owner, reason}, %{lease_owner: owner} = state) do
    finish_shutdown(state, {:lease_owner_exit, reason})
  end

  def handle_info(
        {:video_interop_lease_release_failed, owner, token, metadata, reason},
        %{lease_owner: owner} = state
      ) do
    Logger.error(
      "headless PRIME lease release failed; VideoInterop.LeaseOwner will retry " <>
        "token=#{inspect(token)} metadata=#{inspect(metadata)} reason=#{inspect(reason)}"
    )

    {:noreply,
     %{
       state
       | release_failure_count: state.release_failure_count + 1,
         last_release_failure: {token, metadata, reason}
     }}
  end

  def handle_info({:emerge_skia_log, level, source, message}, state) do
    send(state.producer, {:emerge_skia_log, level, source, message})
    {:noreply, state}
  end

  def handle_info(_message, state), do: {:noreply, state}

  @impl true
  def terminate(_reason, state) do
    _state = close_destination(state)
    :ok
  end

  defp init_with_dispatcher(
         producer,
         native_opts,
         external_target,
         frame_message,
         max_active,
         release_dispatcher
       ) do
    lease_owner_result =
      safe_start_lease_owner(
        producer: self(),
        release: {__MODULE__, :release_backend_token, []},
        release_retry: @default_release_retry,
        abandonment_guard_factory: {__MODULE__, :new_abandonment_guard, [release_dispatcher]},
        max_active: max_active,
        notify: self(),
        notify_releases: false
      )

    case lease_owner_result do
      {:ok, lease_owner} ->
        rendering_api = native_opts.rendering_api
        native_opts = put_native_relay(native_opts, self())

        case safe_native_start(native_opts) do
          renderer when is_reference(renderer) ->
            _ = Native.set_log_target(renderer, producer)
            producer_monitor = Process.monitor(producer)
            {destination, destination_monitor} = external_destination(external_target)

            {:ok,
             %{
               renderer: renderer,
               lease_owner: lease_owner,
               release_dispatcher: release_dispatcher,
               destination: destination,
               destination_monitor: destination_monitor,
               producer: producer,
               frame_message: frame_message,
               rendering_api: rendering_api,
               producer_monitor: producer_monitor,
               mode: :open,
               stop_waiters: [],
               release_failure_count: 0,
               last_release_failure: nil
             }}

          error ->
            :ok = LeaseOwner.close(lease_owner)
            cleanup_result = Native.headless_prime_release_dispatcher_close(release_dispatcher)
            {:stop, {:native_start, combine_startup_cleanup(error, cleanup_result)}}
        end

      error ->
        cleanup_result = Native.headless_prime_release_dispatcher_close(release_dispatcher)
        {:stop, {:native_start, combine_startup_cleanup(error, cleanup_result)}}
    end
  end

  @doc false
  @spec prime_stream_contract(map(), DMABuf.Descriptor.t(), Frame.acquire_sync()) :: %{
          acquire_sync: Format.acquire_sync(),
          modifier: DMABuf.Modifier.t() | :per_buffer
        }
  def prime_stream_contract(%{kind: "vulkan"}, _descriptor, _acquire_sync),
    do: %{acquire_sync: :sync_file, modifier: 0}

  def prime_stream_contract(_rendering_api, %DMABuf.Descriptor{} = descriptor, acquire_sync) do
    %{
      acquire_sync: stream_acquire_sync(acquire_sync),
      modifier: stream_modifier(descriptor.objects)
    }
  end

  defp stream_acquire_sync(:implicit), do: :implicit
  defp stream_acquire_sync(%SyncFile{}), do: :sync_file

  defp stream_modifier(objects) do
    case objects |> Enum.map(& &1.modifier) |> Enum.uniq() do
      [modifier] -> modifier
      _modifiers -> :per_buffer
    end
  end

  defp safe_start_lease_owner(opts) do
    LeaseOwner.start_link(opts)
  rescue
    error -> {:error, {:exception, error}}
  catch
    kind, reason -> {:error, {kind, reason}}
  end

  defp safe_native_start(native_opts) do
    Native.start_opts(native_opts)
  rescue
    error -> {:error, {:exception, error}}
  catch
    kind, reason -> {:error, {kind, reason}}
  end

  defp combine_startup_cleanup(error, :ok), do: error

  defp combine_startup_cleanup(error, cleanup_error),
    do: {error, {:dispatcher_cleanup, cleanup_error}}

  @doc false
  @spec new_abandonment_guard(pid(), reference(), reference(), reference()) ::
          {:ok, AbandonmentGuard.t()} | {:error, term()}
  def new_abandonment_guard(owner, token, holder, release_dispatcher) do
    case Native.headless_prime_abandonment_guard_new(
           owner,
           token,
           holder,
           release_dispatcher
         ) do
      {:ok, resource} -> {:ok, AbandonmentGuard.new(resource, Native)}
      {:error, _reason} = error -> error
    end
  end

  defp put_native_relay(native_opts, relay) do
    headless = %{
      native_opts.headless
      | target: relay,
        frame_message: @internal_frame_message
    }

    %{native_opts | headless: headless}
  end

  defp forward_frame(frame, %{destination: :disconnected} = state) do
    release_unissued_frame(frame)
    state
  end

  defp forward_frame(frame, state) do
    backend_token = frame_value!(frame, "backend_token")
    descriptor = frame_value!(frame, "descriptor")
    acquire_sync = frame_value!(frame, "acquire_sync")
    width = frame_value!(frame, "width")
    height = frame_value!(frame, "height")
    metadata = %{sequence: frame_value!(frame, "sequence"), width: width, height: height}
    stream_contract = prime_stream_contract(state.rendering_api, descriptor, acquire_sync)

    case LeaseOwner.issue(state.lease_owner, backend_token, metadata: metadata) do
      {:ok, lease} ->
        video_frame = %Frame{
          coded_width: width,
          coded_height: height,
          visible_rect: %Rect{x: 0, y: 0, width: width, height: height},
          format: %Format{
            width: width,
            height: height,
            framerate: nil,
            storage: %DMABuf.Format{
              fourcc: VideoInterop.DMABuf.FourCC.from_string!("AB24"),
              modifier: stream_contract.modifier
            },
            interlace_mode: :progressive,
            alpha_mode: :premultiplied,
            acquire_sync: stream_contract.acquire_sync
          },
          storage: descriptor,
          acquire_sync: acquire_sync,
          lease: lease
        }

        deliver_issued_frame(frame, video_frame, state)

      {:error, {:caller_owned, _reason}} ->
        release_backend_token(backend_token)
        state

      {:error, {:transferred, _reason}} ->
        state
    end
  end

  defp deliver_issued_frame(_frame, video_frame, %{destination: {:external, target}} = state) do
    if Process.alive?(target) do
      send(target, {output_message_tag(state.frame_message), video_frame})
      state
    else
      VideoInterop.release(video_frame.lease)
      begin_draining(state)
    end
  end

  defp release_unissued_frame(frame) do
    frame
    |> frame_value!("backend_token")
    |> release_backend_token()
  end

  defp frame_value!(frame, key) do
    case List.keyfind(frame, key, 0) do
      {^key, value} -> value
      nil -> raise ArgumentError, "headless PRIME frame is missing #{inspect(key)}"
    end
  end

  defp output_message_tag("emerge_skia_frame"), do: :emerge_skia_frame
  defp output_message_tag(message), do: message

  defp begin_draining(%{mode: :open} = state) do
    state = close_destination(state)
    _ = LeaseOwner.close(state.lease_owner)
    %{state | mode: :draining}
  end

  defp begin_draining(state), do: state

  defp close_destination(%{destination: {:external, _pid}} = state) do
    if state.destination_monitor, do: Process.demonitor(state.destination_monitor, [:flush])
    %{state | destination: :disconnected, destination_monitor: nil}
  end

  defp close_destination(state), do: state

  defp external_destination(target) when is_pid(target) do
    {{:external, target}, Process.monitor(target)}
  end

  defp external_destination(nil), do: {:disconnected, nil}

  @doc false
  @spec shutdown_result_for_test(term(), term() | nil) :: :ok | {:error, term()}
  def shutdown_result_for_test(native_result, lease_owner_error) do
    native_error =
      case native_result do
        {:ok, :ok} -> nil
        :ok -> nil
        {:error, reason} -> {:native_stop_failed, reason}
        other -> {:invalid_native_stop_result, other}
      end

    case {lease_owner_error, native_error} do
      {nil, nil} -> :ok
      {nil, error} -> {:error, error}
      {error, nil} -> {:error, error}
      {error, native_error} -> {:error, {error, native_error}}
    end
  end

  defp finish_shutdown(state, lease_owner_error) do
    native_result = safe_native_stop(state.renderer)

    if is_nil(lease_owner_error) do
      state =
        Map.merge(state, %{
          mode: :dispatcher_closing,
          shutdown_native_result: native_result,
          shutdown_lease_owner_error: nil
        })

      close_release_dispatcher(state)
    else
      # Unknown LeaseOwner ownership is permanent quarantine. Closing the
      # dispatcher here would suppress guards that may still be the only path
      # capable of retiring a published holder.
      result = shutdown_result_for_test(native_result, lease_owner_error)
      Enum.each(state.stop_waiters, &GenServer.reply(&1, result))

      {:noreply,
       %{state | mode: :quarantined, stop_waiters: []} |> Map.put(:shutdown_result, result)}
    end
  end

  defp close_release_dispatcher(state) do
    case Native.headless_prime_release_dispatcher_close(state.release_dispatcher) do
      :ok ->
        result =
          shutdown_result_for_test(
            state.shutdown_native_result,
            state.shutdown_lease_owner_error
          )

        Enum.each(state.stop_waiters, &GenServer.reply(&1, result))
        state = %{state | mode: :stopped, stop_waiters: []}

        case result do
          :ok -> {:stop, :normal, state}
          {:error, reason} -> {:stop, {:shutdown_failed, reason}, state}
        end

      {:error, {:timeout, _reason}} ->
        Process.send_after(self(), :retry_release_dispatcher_close, @dispatcher_close_retry_ms)
        {:noreply, state}

      {:error, reason} ->
        result = append_dispatcher_error(state, reason)
        Enum.each(state.stop_waiters, &GenServer.reply(&1, result))

        {:noreply,
         %{state | mode: :quarantined, stop_waiters: []} |> Map.put(:shutdown_result, result)}
    end
  end

  defp append_dispatcher_error(state, reason) do
    dispatcher_error = {:dispatcher_close_failed, reason}

    case shutdown_result_for_test(
           state.shutdown_native_result,
           state.shutdown_lease_owner_error
         ) do
      :ok -> {:error, dispatcher_error}
      {:error, error} -> {:error, {error, dispatcher_error}}
    end
  end

  defp safe_native_stop(renderer) do
    Native.stop(renderer)
  rescue
    error -> {:error, {:exception, error}}
  catch
    kind, reason -> {:error, {kind, reason}}
  end
end
