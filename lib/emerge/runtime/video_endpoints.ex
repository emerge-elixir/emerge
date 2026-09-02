defmodule Emerge.Runtime.VideoEndpoints do
  @moduledoc false

  alias VideoInterop.Frame

  @prefix {__MODULE__, :renderer}

  @spec register(pid(), term()) :: :ok
  def register(viewport, renderer) when is_pid(viewport) do
    :persistent_term.put({@prefix, viewport}, renderer)
  end

  @spec unregister(pid()) :: :ok
  def unregister(viewport) when is_pid(viewport) do
    :persistent_term.erase({@prefix, viewport})
    :ok
  end

  @spec submit(pid(), atom(), Frame.t()) :: :ok | {:error, term()}
  def submit(viewport, target, %Frame{} = frame) when is_pid(viewport) and is_atom(target) do
    case :persistent_term.get({@prefix, viewport}, :missing) do
      :missing -> consume_error(frame, :viewport_not_ready)
      renderer -> submit_to_renderer(renderer, target, frame)
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
