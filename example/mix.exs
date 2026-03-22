defmodule EmergeDemo.MixProject do
  use Mix.Project

  def project do
    [
      app: :emerge_demo,
      version: "0.1.0",
      elixir: "~> 1.19",
      start_permanent: Mix.env() == :prod,
      listeners: [Emerge.Runtime.CodeReloader.MixListener],
      deps: deps()
    ]
  end

  # Run "mix help compile.app" to learn about applications.
  def application do
    [
      extra_applications: [:logger],
      mod: {EmergeDemo.Application, []}
    ]
  end

  # Run "mix help deps" to learn about dependencies.
  defp deps do
    [
      {:emerge, path: "../."},
      {:solve, path: "../../solve"},
      {:file_system, "~> 1.0", only: :dev}
    ]
  end
end
