defmodule EmergeDemo.MixProject do
  use Mix.Project

  def project do
    [
      app: :emerge_demo,
      version: "0.1.0",
      elixir: "~> 1.19",
      start_permanent: Mix.env() == :prod,
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
      {:emerge, path: "../"},
      {:rustler, "~> 0.37.0", override: true},
      {:membrane_core, "~> 1.2"},
      {:membrane_h26x_plugin, "~> 0.10.1"},
      {:membrane_rtp_plugin, "~> 0.30.0"},
      {:membrane_rtp_h265_plugin, "~> 0.5.2"},
      {:membrane_drm_sink, path: "../../membrane_drm_sink"},
      {:nerves_wifibroadcast, path: "../../nerves-wifibroadcast"}
    ]
  end
end
