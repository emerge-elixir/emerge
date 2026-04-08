defmodule EmergeDemo.AppSelector.App do
  @moduledoc """
  Solve app for the example app selector.
  """

  alias EmergeDemo.AppSelector

  use Solve

  def child_spec(opts) do
    %{
      id: {__MODULE__, Keyword.get(opts, :name, __MODULE__)},
      start: {__MODULE__, :start_link, [opts]},
      type: :worker,
      restart: :permanent
    }
  end

  @impl Solve
  def controllers do
    [
      controller!(name: :screens, module: AppSelector.Screens)
    ]
  end
end
