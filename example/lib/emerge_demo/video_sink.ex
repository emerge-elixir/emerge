defmodule EmergeDemo.VideoSink do
  @moduledoc false

  use Membrane.Sink

  alias Membrane.{Buffer, PrimeFormat}

  def_options(
    target: [spec: EmergeSkia.VideoTarget.t()],
    notify_to: [spec: pid() | nil, default: nil]
  )

  def_input_pad(:input,
    accepted_format: %PrimeFormat{},
    flow_control: :manual,
    demand_unit: :buffers
  )

  @impl true
  def handle_init(_ctx, opts) do
    {[], %{target: opts.target, notify_to: opts.notify_to, first_frame_received?: false}}
  end

  @impl true
  def handle_stream_format(:input, %PrimeFormat{} = format, _ctx, state) do
    validate_target_size!(state.target, format)

    notify(
      state.notify_to,
      {:video_sink, :stream_format,
       %{width: format.width, height: format.height, framerate: format.framerate}}
    )

    {[], state}
  end

  @impl true
  def handle_start_of_stream(:input, _ctx, state) do
    notify(state.notify_to, {:video_sink, :start_of_stream})
    {[demand: :input], state}
  end

  @impl true
  def handle_end_of_stream(:input, _ctx, state) do
    notify(state.notify_to, {:video_sink, :end_of_stream})
    {[], state}
  end

  @impl true
  def handle_buffer(:input, %Buffer{pts: pts, metadata: %{drm_prime: desc}}, _ctx, state) do
    :erlang.garbage_collect(self())

    case EmergeSkia.submit_prime(state.target, desc) do
      :ok ->
        state =
          if state.first_frame_received? do
            state
          else
            notify(state.notify_to, {:video_sink, :first_frame_submitted, pts})
            %{state | first_frame_received?: true}
          end

        {[demand: :input], state}

      {:error, reason} ->
        notify(state.notify_to, {:video_sink, :submit_failed, inspect(reason)})
        raise "failed to submit prime frame to EmergeSkia: #{inspect(reason)}"
    end
  end

  def handle_buffer(:input, %Buffer{}, _ctx, state) do
    notify(state.notify_to, {:video_sink, :submit_failed, "missing drm_prime metadata"})
    raise "expected drm_prime metadata on buffer"
  end

  defp validate_target_size!(target, format) do
    if target.width != format.width or target.height != format.height do
      raise ArgumentError,
            "video target size #{target.width}x#{target.height} does not match prime stream #{format.width}x#{format.height}"
    end
  end

  defp notify(nil, _message), do: :ok
  defp notify(pid, message) when is_pid(pid), do: send(pid, message)
end
