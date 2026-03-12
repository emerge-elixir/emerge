defmodule EmergeDemo.Application do
  @moduledoc false

  use Application

  @impl true
  def start(_type, _args) do
    children =
      if Keyword.get(Application.get_env(:emerge_demo, __MODULE__, []), :auto_start?, true) do
        [Supervisor.child_spec({EmergeDemo.Runtime, []}, restart: :temporary)]
      else
        []
      end

    opts = [strategy: :one_for_one, name: EmergeDemo.Supervisor]
    Supervisor.start_link(children, opts)
  end
end
