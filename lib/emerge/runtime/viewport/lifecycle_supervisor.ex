defmodule Emerge.Runtime.Viewport.LifecycleSupervisor do
  @moduledoc false
  use Supervisor

  alias Emerge.Runtime.VideoEndpoints
  alias Emerge.Runtime.Viewport.Lifecycle

  def start_link(viewport, config), do: Supervisor.start_link(__MODULE__, {viewport, config})

  def lifecycle(supervisor) do
    Enum.find_value(Supervisor.which_children(supervisor), fn
      {Lifecycle, pid, :worker, _} -> pid
      _ -> nil
    end)
  end

  @impl true
  def init({viewport, config}) do
    # An internal owner failure ends this instance rather than silently adopting old handles.
    # The public callback PID stays the application endpoint and the native monitored owner.
    children = [
      %{id: VideoEndpoints, start: {VideoEndpoints, :start_link, [viewport]}},
      %{id: Lifecycle, start: {Lifecycle, :start_link, [{viewport, config}]}, shutdown: 6_000}
    ]

    Supervisor.init(children, strategy: :one_for_all, max_restarts: 0)
  end
end
