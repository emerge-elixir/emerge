defmodule EmergeSkia.VideoConsumerSession do
  @moduledoc """
  Ownership-aware stream opened for one exact native video-target incarnation.

  The native resource is the admission authority. The monitor process exists only
  to close that resource when the logical owner exits; frame transfer calls the
  NIF directly so no unacknowledged BEAM mailbox boundary is introduced.
  """

  use GenServer

  alias EmergeSkia.Native
  alias VideoInterop.{ConsumerContractError, Format, Frame}
  alias VideoInterop.DMABuf

  @enforce_keys [:ref, :monitor]
  defstruct @enforce_keys

  @type t :: %__MODULE__{ref: reference(), monitor: pid()}

  @spec open(EmergeSkia.VideoTarget.t(), VideoInterop.Format.t(), pid()) ::
          {:ok, t()} | {:error, term()}
  def open(%EmergeSkia.VideoTarget{ref: target_ref}, format, owner)
      when is_pid(owner) and node(owner) == node() do
    %Format{
      width: width,
      height: height,
      storage: %DMABuf.Format{fourcc: fourcc, modifier: modifier}
    } = format

    case normalize_open(
           Native.video_consumer_session_open(target_ref, width, height, fourcc, modifier)
         ) do
      {:ok, ref} ->
        case GenServer.start(__MODULE__, {owner, ref}) do
          {:ok, monitor} ->
            {:ok, %__MODULE__{ref: ref, monitor: monitor}}

          {:error, reason} ->
            :ok = Native.video_consumer_session_close(ref)
            {:error, {:owner_monitor_start_failed, reason}}
        end

      {:error, _reason} = error ->
        error
    end
  end

  @spec transfer(t(), Frame.t()) :: VideoInterop.ConsumerSession.transfer_result()
  def transfer(%__MODULE__{ref: ref}, %Frame{} = frame) do
    ref
    |> Native.video_consumer_session_submit(frame)
    |> normalize_transfer_result()
  end

  @doc false
  @spec normalize_transfer_result(term()) :: VideoInterop.ConsumerSession.transfer_result()
  def normalize_transfer_result({:ok, receipt}) when receipt in [:transferred, :released],
    do: {:ok, receipt}

  def normalize_transfer_result({:error, {ownership, reason}})
      when ownership in [:caller_owned, :transferred],
      do: {:error, {ownership, normalize_transfer_reason(reason)}}

  def normalize_transfer_result(other) do
    raise ConsumerContractError,
      operation: :transfer,
      result: {:invalid_native_receipt, other}
  end

  @spec close(t()) :: :ok
  def close(%__MODULE__{ref: ref, monitor: monitor}) do
    :ok = Native.video_consumer_session_close(ref)

    if Process.alive?(monitor) do
      GenServer.cast(monitor, :stop)
    end

    :ok
  end

  @impl true
  def init({owner, ref}) do
    {:ok, %{owner_monitor: Process.monitor(owner), ref: ref}}
  end

  @impl true
  def handle_info({:DOWN, monitor, :process, _owner, _reason}, %{owner_monitor: monitor} = state) do
    :ok = Native.video_consumer_session_close(state.ref)
    {:stop, :normal, state}
  end

  def handle_info(_message, state), do: {:noreply, state}

  @impl true
  def handle_cast(:stop, state), do: {:stop, :normal, state}

  @impl true
  def terminate(_reason, state) do
    _ = Native.video_consumer_session_close(state.ref)
    :ok
  end

  defp normalize_open({:ok, ref}) when is_reference(ref), do: {:ok, ref}
  defp normalize_open({:error, "target_busy"}), do: {:error, :target_busy}

  defp normalize_open({:error, reason}) when is_binary(reason) do
    if String.contains?(reason, [
         "stale video target",
         "stale video renderer",
         "unknown video target",
         "video registry is closed"
       ]) do
      {:error, :stale_target}
    else
      {:error, reason}
    end
  end

  defp normalize_open({:error, reason}), do: {:error, reason}

  defp normalize_transfer_reason("video target is inactive"), do: :inactive

  defp normalize_transfer_reason(reason) when is_binary(reason) do
    if String.contains?(reason, [
         "stale video target",
         "stale video renderer",
         "unknown video target",
         "video registry is closed",
         "video consumer stream is closed"
       ]) do
      :stale_target
    else
      reason
    end
  end

  defp normalize_transfer_reason(reason), do: reason
end

defimpl VideoInterop.ConsumerSession, for: EmergeSkia.VideoConsumerSession do
  def transfer(session, frame), do: EmergeSkia.VideoConsumerSession.transfer(session, frame)
  def close(session), do: EmergeSkia.VideoConsumerSession.close(session)
end
