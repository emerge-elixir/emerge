defmodule EmergeDemo.Application do
  @moduledoc false

  use Application

  @impl true
  def start(_type, _args) do
    opts = [strategy: :one_for_one, name: EmergeDemo.Supervisor]
    Supervisor.start_link(children(), opts)
  end

  def children(env \\ Mix.env()) do
    case env do
      :test ->
        []

      :dev ->
        base_children() ++ [hot_reload_child()]

      _other ->
        base_children()
    end
  end

  defp base_children do
    [
      %{
        id: EmergeDemo.State,
        start: {EmergeDemo.State, :start_link, [[name: EmergeDemo.State]]},
        type: :worker,
        restart: :permanent
      },
      EmergeDemo
    ]
  end

  defp hot_reload_child do
    {Emerge.CodeReloader,
     dirs: [
       Path.expand("..", __DIR__),
       Path.expand("../../../lib", __DIR__),
       Path.expand("../../../../solve/lib", __DIR__)
     ],
     reloadable_apps: [:emerge_demo, :emerge, :solve]}
  end
end
