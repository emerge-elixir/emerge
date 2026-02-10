defmodule EmergeSkia.MixProject do
  use Mix.Project

  def project do
    [
      app: :emerge_skia,
      version: "0.1.0",
      elixir: "~> 1.19",
      start_permanent: Mix.env() == :prod,
      deps: deps(),
      name: "EmergeSkia",
      docs: [
        main: "readme",
        extras: [
          "README.md",
          "guides/internals/architecture.md",
          "guides/internals/emrg-format.md",
          "guides/internals/events.md",
          "guides/internals/scrolling.md",
          "guides/internals/tree-patching.md"
        ],
        groups_for_extras: [
          Internals: ~r/guides\/internals\/.*/
        ]
      ]
    ]
  end

  def application do
    [
      extra_applications: [:logger]
    ]
  end

  defp deps do
    [
      {:rustler, "~> 0.37.0"},
      {:ex_doc, "~> 0.35", only: :dev, runtime: false}
    ]
  end
end
