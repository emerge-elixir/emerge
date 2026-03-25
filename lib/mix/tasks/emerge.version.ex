defmodule Mix.Tasks.Emerge.Version do
  @moduledoc false
  use Mix.Task

  @shortdoc "Prints the current Emerge package version"

  @impl true
  def run(_args) do
    Mix.Project.config()[:version]
    |> IO.puts()
  end
end
