defmodule EmergeSkia.MixProject do
  use Mix.Project

  def project do
    [
      app: :emerge_skia,
      version: "0.1.0",
      elixir: "~> 1.19",
      start_permanent: Mix.env() == :prod,
      deps: deps()
    ]
  end

  def application do
    [
      extra_applications: [:logger]
    ]
  end

  defp deps do
    [
      {:rustler, "~> 0.37.0"}
    ]
  end
end
