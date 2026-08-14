defmodule EmergeSkia.HeadlessPrimeSession do
  @moduledoc false

  use GenServer

  require Logger

  alias EmergeSkia.Native
  alias VideoInterop.{AbandonmentGuard, Format, Frame, LeaseOwner, Rect}
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

  @spec connect(t(), EmergeSkia.VideoTarget.t(), keyword()) ::
          {:ok, reference()} | {:error, term()}
  def connect(%__MODULE__{pid: pid}, %EmergeSkia.VideoTarget{} = target, opts \\ []) do
    GenServer.call(pid, {:connect, target, opts}, :infinity)
  catch
    :exit, reason -> {:error, {:source_down, reason}}
  end

  @spec disconnect(t()) :: :ok | {:error, term()}
  def disconnect(%__MODULE__{pid: pid}) do
    GenServer.call(pid, :disconnect, :infinity)
  catch
    :exit, {:noproc, _details} -> :ok
    :exit, {:normal, _details} -> :ok
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

  def handle_call({:connect, _target, _opts}, _from, %{mode: mode} = state)
      when mode != :open do
    {:reply, {:error, :source_stopping}, state}
  end

  def handle_call({:connect, target, opts}, _from, state) do
    with {:ok, format, notify_to} <- output_format(state, target, opts),
         :ok <- validate_target_size(target, format) do
      state = close_destination(state)

      case VideoInterop.open_consumer(target, format, owner: self()) do
        {:ok, session} ->
          connection_ref = make_ref()

          notify(
            notify_to,
            {:emerge_video_output, state.producer, connection_ref, :connected}
          )

          {:reply, {:ok, connection_ref},
           %{state | destination: {:consumer, session, connection_ref, notify_to, false}}}

        {:error, reason} ->
          {:reply, {:error, reason}, state}
      end
    else
      {:error, reason} -> {:reply, {:error, reason}, state}
    end
  end

  def handle_call(:disconnect, _from, state) do
    {:reply, :ok, close_destination(state)}
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
               width: native_opts.width,
               height: native_opts.height,
               acquire_sync: output_acquire_sync(native_opts.rendering_api),
               modifier: output_modifier(native_opts.rendering_api),
               producer_monitor: producer_monitor,
               mode: :open,
               stop_waiters: [],
               release_failure_count: 0,
               last_release_failure: nil,
               destination_close_error: nil
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

    case LeaseOwner.issue(state.lease_owner, backend_token, metadata: metadata) do
      {:ok, lease} ->
        video_frame = %Frame{
          coded_width: width,
          coded_height: height,
          visible_rect: %Rect{x: 0, y: 0, width: width, height: height},
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

  defp deliver_issued_frame(frame, video_frame, %{destination: {:external, target}} = state) do
    if Process.alive?(target) do
      output_frame = canonical_output_frame(frame, video_frame)
      send(target, {output_message_tag(state.frame_message), output_frame})
      state
    else
      VideoInterop.release(video_frame)
      begin_draining(state)
    end
  end

  defp deliver_issued_frame(
         frame,
         video_frame,
         %{destination: {:consumer, session, ref, notify_to, first?}} = state
       ) do
    case EmergeSkia.submit_video_frame(session, video_frame) do
      :ok when not first? ->
        sequence = frame_value!(frame, "sequence")

        notify(
          notify_to,
          {:emerge_video_output, state.producer, ref, {:first_frame_accepted, sequence}}
        )

        %{state | destination: {:consumer, session, ref, notify_to, true}}

      :ok ->
        state

      {:error, reason} ->
        case submission_error_action(reason) do
          :keep ->
            state

          :disconnect ->
            notify(notify_to, {:emerge_video_output, state.producer, ref, {:error, reason}})
            close_destination(state)
        end
    end
  end

  defp canonical_output_frame(frame, video_frame) do
    frame
    |> List.keydelete("backend_token", 0)
    |> List.keydelete("descriptor", 0)
    |> List.keydelete("acquire_sync", 0)
    |> List.keystore("dmabuf", 0, {"dmabuf", video_frame})
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

    case destination_close_shutdown_mode(Map.get(state, :destination_close_error)) do
      :draining ->
        %{state | mode: :draining}

      :quarantined ->
        error = Map.fetch!(state, :destination_close_error)
        # A failed consumer close leaves holder ownership unknown. Do not stop the native producer
        # or close its abandonment-guard dispatcher: either action could recycle a backend slot
        # that the consumer still owns. Quarantine immediately so stop callers cannot hang waiting
        # for a drain notification that the failed consumer may never produce.
        result = {:error, error}
        Enum.each(state.stop_waiters, &GenServer.reply(&1, result))

        state
        |> Map.merge(%{mode: :quarantined, stop_waiters: [], shutdown_result: result})
    end
  end

  defp begin_draining(state), do: state

  defp destination_close_shutdown_mode(nil), do: :draining
  defp destination_close_shutdown_mode(_error), do: :quarantined

  @doc false
  def destination_close_shutdown_mode_for_test(error),
    do: destination_close_shutdown_mode(error)

  defp close_destination(%{destination: {:consumer, session, ref, notify_to, _first?}} = state) do
    previous_close_error = Map.get(state, :destination_close_error)

    close_error =
      case safe_close_consumer(session) do
        :ok -> previous_close_error
        {:error, reason} -> previous_close_error || {:consumer_close_failed, reason}
      end

    notify(notify_to, {:emerge_video_output, state.producer, ref, :disconnected})

    state
    |> Map.merge(%{destination: :disconnected, destination_monitor: nil})
    |> Map.put(:destination_close_error, close_error)
  end

  defp close_destination(%{destination: {:external, _pid}} = state) do
    if state.destination_monitor, do: Process.demonitor(state.destination_monitor, [:flush])
    %{state | destination: :disconnected, destination_monitor: nil}
  end

  defp close_destination(state), do: state

  defp safe_close_consumer(session) do
    :ok = VideoInterop.close_consumer(session)
  rescue
    error -> {:error, {:exception, error}}
  catch
    kind, reason -> {:error, {kind, reason}}
  end

  defp external_destination(target) when is_pid(target) do
    {{:external, target}, Process.monitor(target)}
  end

  defp external_destination(nil), do: {:disconnected, nil}

  defp output_format(state, %EmergeSkia.VideoTarget{}, opts) do
    unsupported = Keyword.keys(opts) -- [:notify, :acquire_sync]
    notify_to = Keyword.get(opts, :notify)
    acquire_sync = Keyword.get(opts, :acquire_sync, state.acquire_sync)

    cond do
      unsupported != [] ->
        {:error, {:unsupported_options, unsupported}}

      not is_nil(notify_to) and
          (not is_pid(notify_to) or node(notify_to) != node()) ->
        {:error, :notify_must_be_a_local_pid}

      acquire_sync not in [:implicit, :sync_file, :per_frame] ->
        {:error, :invalid_acquire_sync_policy}

      true ->
        {:ok,
         %Format{
           width: state.width,
           height: state.height,
           framerate: nil,
           storage: %DMABuf.Format{
             fourcc: VideoInterop.DMABuf.FourCC.from_string!("AB24"),
             modifier: state.modifier
           },
           interlace_mode: :progressive,
           alpha_mode: :premultiplied,
           acquire_sync: acquire_sync
         }, notify_to}
    end
  end

  defp output_acquire_sync(:vulkan), do: :sync_file
  defp output_acquire_sync(_rendering_api), do: :per_frame

  defp output_modifier(_rendering_api), do: 0

  defp validate_target_size(%EmergeSkia.VideoTarget{mode: mode}, _format) when mode != :prime,
    do: {:error, {:wrong_mode, mode}}

  defp validate_target_size(%EmergeSkia.VideoTarget{width: width, height: height}, %Format{
         width: width,
         height: height
       }),
       do: :ok

  defp validate_target_size(
         %EmergeSkia.VideoTarget{width: expected_width, height: expected_height},
         %Format{width: width, height: height}
       ),
       do: {:error, {:wrong_size, {width, height}, {expected_width, expected_height}}}

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

  @doc false
  @spec submission_error_action(term()) :: :keep | :disconnect
  def submission_error_action(:inactive), do: :keep
  def submission_error_action(_reason), do: :disconnect

  defp finish_shutdown(state, lease_owner_error) do
    ownership_error =
      combine_ownership_errors(Map.get(state, :destination_close_error), lease_owner_error)

    native_result = safe_native_stop(state.renderer)

    if is_nil(ownership_error) do
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
      result = shutdown_result_for_test(native_result, ownership_error)
      Enum.each(state.stop_waiters, &GenServer.reply(&1, result))

      {:noreply,
       %{state | mode: :quarantined, stop_waiters: []} |> Map.put(:shutdown_result, result)}
    end
  end

  defp combine_ownership_errors(nil, nil), do: nil
  defp combine_ownership_errors(error, nil), do: error
  defp combine_ownership_errors(nil, error), do: error
  defp combine_ownership_errors(left, right), do: {left, right}

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

  defp notify(nil, _message), do: :ok
  defp notify(pid, message), do: send(pid, message)
end
