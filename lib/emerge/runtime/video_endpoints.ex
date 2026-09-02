defmodule Emerge.Runtime.VideoEndpoints do
  @moduledoc false

  alias VideoInterop.Frame

  @prefix {__MODULE__, :renderer}

  @spec register(pid(), module(), term()) :: :ok
  def register(viewport, renderer_module, renderer)
      when is_pid(viewport) and is_atom(renderer_module) do
    :persistent_term.put({@prefix, viewport}, {renderer_module, renderer})
  end

  @spec unregister(pid()) :: :ok
  def unregister(viewport) when is_pid(viewport) do
    :persistent_term.erase({@prefix, viewport})
    :ok
  end

  @spec submit(pid(), atom(), Frame.t()) :: :ok | {:error, term()}
  def submit(viewport, target, %Frame{} = frame) when is_pid(viewport) and is_atom(target) do
    case endpoint(viewport) do
      {:ok, {renderer_module, renderer}} ->
        submit_to_renderer(renderer_module, renderer, target, frame)

      {:error, reason} ->
        consume_error(frame, reason)
    end
  end

  defp endpoint(viewport) do
    case :persistent_term.get({@prefix, viewport}, :missing) do
      :missing -> {:error, :viewport_not_ready}
      endpoint -> {:ok, endpoint}
    end
  end

  defp submit_to_renderer(renderer_module, renderer, target, frame) do
    with :ok <- VideoInterop.validate(frame),
         true <- function_exported?(renderer_module, :submit_video_frame, 3),
         :ok <- renderer_module.submit_video_frame(renderer, target, frame) do
      :ok
    else
      false -> consume_error(frame, :video_submission_unsupported)
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
