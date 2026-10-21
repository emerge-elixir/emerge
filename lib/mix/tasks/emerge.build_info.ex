defmodule Mix.Tasks.Emerge.BuildInfo do
  use Mix.Task

  @shortdoc "Explain native build defaults, features and exact artifact selection"
  @moduledoc """
  Reports the native build configuration without compiling or loading the NIF.

      mix emerge.build_info
      mix emerge.build_info --backend headless:opengl --target aarch64-unknown-linux-gnu
      mix emerge.build_info --systems
      mix emerge.build_info --profiles

  If Emerge has not compiled yet, run the standalone equivalent:

      elixir deps/emerge/scripts/build-info.exs

  The Mix task reads application configuration. The standalone script reports
  environment defaults unless explicit `--backend` options are supplied. Neither
  command probes devices or verifies that release archives have been published.
  """
  @impl true
  def run(args) do
    EmergeSkia.BuildInfo.run(args, Application.get_all_env(:emerge))
  end
end
