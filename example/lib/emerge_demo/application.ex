defmodule EmergeDemo.Application do
  @moduledoc false

  use Application

  @impl true
  def start(_type, _args) do
    opts = [strategy: :one_for_one, name: EmergeDemo.Supervisor]
    Supervisor.start_link(children(), opts)
  end

  def children(env \\ Mix.env())
  def children(:test), do: []
  def children(:dev), do: base_children() ++ [hot_reload_child()]
  def children(_other), do: base_children()

  defp base_children do
    [
      EmergeDemo.TodoApp.child_spec([]),
      EmergeDemo
    ]
  end

  defp hot_reload_child do
    {Emerge.Runtime.CodeReloader,
     dirs: [
       Path.expand("..", __DIR__)
     ],
     reloadable_apps: [:emerge_demo]}
  end
end
