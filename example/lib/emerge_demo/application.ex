defmodule EmergeDemo.Application do
  @moduledoc false

  use Application

  @impl true
  def start(_type, _args) do
    opts = [strategy: :one_for_one, name: EmergeDemo.Supervisor]
    Supervisor.start_link(children(), opts)
  end

  if Mix.env() == :test do
    def children(), do: []
  else
    def children() do
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
  end
end
