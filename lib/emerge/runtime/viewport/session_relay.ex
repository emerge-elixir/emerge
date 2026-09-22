defmodule Emerge.Runtime.Viewport.SessionRelay do
  @moduledoc false

  # Each incarnation has its own mailbox. Retirement is acknowledged only after
  # queued native messages are drained, so a normal window close cannot race with
  # cleanup and be misclassified as a renderer failure requiring replacement.
  def start_link(viewport, generation) do
    spawn_link(fn ->
      monitor = Process.monitor(viewport)
      loop(viewport, generation, monitor, false)
    end)
  end

  defp loop(viewport, generation, monitor, closed) do
    receive do
      {:retire, owner} ->
        send(owner, {:session_retired, generation, self(), closed})

      {:DOWN, ^monitor, :process, ^viewport, _} ->
        :ok

      {:emerge_skia_close, _} = message ->
        send(viewport, {:emerge_viewport_renderer, generation, message})
        loop(viewport, generation, monitor, true)

      message ->
        send(viewport, {:emerge_viewport_renderer, generation, message})
        loop(viewport, generation, monitor, closed)
    end
  end
end
