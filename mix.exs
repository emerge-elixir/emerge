defmodule Emerge.MixProject do
  use Mix.Project

  @nerves_rust_target_triple_mapping %{
    "armv6-nerves-linux-gnueabihf" => "arm-unknown-linux-gnueabihf",
    "armv7-nerves-linux-gnueabihf" => "armv7-unknown-linux-gnueabihf",
    "aarch64-nerves-linux-gnu" => "aarch64-unknown-linux-gnu",
    "x86_64-nerves-linux-musl" => "x86_64-unknown-linux-musl"
  }

  @rustler_passthrough_env_keys [
    "CC",
    "CXX",
    "CFLAGS",
    "CPPFLAGS",
    "CXXFLAGS",
    "LDFLAGS",
    "NERVES_SDK_SYSROOT",
    "NERVES_TOOLCHAIN",
    "PKG_CONFIG_SYSROOT_DIR",
    "PKG_CONFIG_LIBDIR",
    "PKG_CONFIG_PATH",
    "TARGET_ARCH",
    "TARGET_OS",
    "TARGET_ABI",
    "TARGET_VENDOR"
  ]

  def project do
    [
      app: :emerge,
      version: "0.1.0",
      elixir: "~> 1.19",
      start_permanent: Mix.env() == :prod,
      rustler_opts: rustler_opts(),
      deps: deps(),
      name: "Emerge",
      docs: [
        main: "readme",
        before_closing_body_tag: &before_closing_body_tag/1,
        extras: [
          "README.md",
          "guides/internals/architecture.md",
          "guides/internals/assets-images.md",
          "guides/internals/feature-roadmap.md",
          "guides/internals/emrg-format.md",
          "guides/internals/events.md",
          "guides/internals/tree-patching.md"
        ],
        groups_for_extras: [
          Internals: ~r/guides\/internals\/.*/
        ],
        groups_for_modules: [
          "Public API": [Emerge],
          UI: ~r/^Emerge\.UI(\.|$)/,
          Assets: ~r/^Emerge\.Assets(\.|$)/,
          Runtime: ~r/^Emerge\.Runtime\./,
          Rendering: ~r/^EmergeSkia(\.|$)/,
          Engine: ~r/^Emerge\.Engine(\.|$)/
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

  defp rustler_opts do
    env = System.get_env()

    case rustler_target(env) do
      nil ->
        []

      target ->
        [
          target: target,
          env: rustler_cross_env(target, env)
        ]
    end
  end

  defp rustler_target(env) do
    rustler_target_from_cc(env) || rustler_target_from_target_env(env)
  end

  defp rustler_target_from_cc(env) do
    env
    |> Map.get("CC")
    |> compiler_prefix()
    |> then(&Map.get(@nerves_rust_target_triple_mapping, &1))
  end

  defp rustler_target_from_target_env(env) do
    with arch when is_binary(arch) and arch != "" <- Map.get(env, "TARGET_ARCH"),
         os when is_binary(os) and os != "" <- Map.get(env, "TARGET_OS"),
         abi when is_binary(abi) and abi != "" <- target_abi(env, os),
         vendor when is_binary(vendor) and vendor != "" <- target_vendor(env, os) do
      "#{arch}-#{vendor}-#{os}-#{abi}"
    else
      _ -> nil
    end
  end

  defp compiler_prefix(nil), do: nil

  defp compiler_prefix(compiler) do
    compiler
    |> String.split(~r/\s+/, trim: true)
    |> List.first()
    |> case do
      nil ->
        nil

      path ->
        path
        |> Path.basename()
        |> String.split("-")
        |> Enum.drop(-1)
        |> Enum.join("-")
    end
  end

  defp target_vendor(env, "linux") do
    case Map.get(env, "TARGET_VENDOR") do
      nil -> "unknown"
      "" -> "unknown"
      vendor -> vendor
    end
  end

  defp target_vendor(env, _os), do: Map.get(env, "TARGET_VENDOR")

  defp target_abi(env, "linux") do
    case Map.get(env, "TARGET_ABI") do
      nil -> "gnu"
      "" -> "gnu"
      abi -> abi
    end
  end

  defp target_abi(env, _os), do: Map.get(env, "TARGET_ABI")

  defp rustler_cross_env(target, env) do
    target_key =
      target
      |> String.upcase()
      |> String.replace("-", "_")

    env
    |> passthrough_env()
    |> maybe_put_env("CARGO_TARGET_#{target_key}_LINKER", Map.get(env, "CC"))
    |> maybe_put_env("HOST_CC", Map.get(env, "HOST_CC") || System.find_executable("cc"))
    |> maybe_put_env("HOST_CXX", Map.get(env, "HOST_CXX") || System.find_executable("c++"))
  end

  defp passthrough_env(env) do
    Enum.reduce(@rustler_passthrough_env_keys, [], fn key, acc ->
      maybe_put_env(acc, key, Map.get(env, key))
    end)
  end

  defp maybe_put_env(env, _key, nil), do: env
  defp maybe_put_env(env, _key, ""), do: env
  defp maybe_put_env(env, key, value), do: [{key, value} | env]

  defp before_closing_body_tag(:html) do
    """
    <script type="module">
      import mermaid from "https://cdn.jsdelivr.net/npm/mermaid@11/dist/mermaid.esm.min.mjs";

      mermaid.initialize({ startOnLoad: false });

      async function render() {
        const blocks = document.querySelectorAll(
          "pre > code.language-mermaid, pre > code.mermaid, pre > code[class*='mermaid']"
        );

        for (const code of blocks) {
          const pre = code.parentElement;
          if (!pre || pre.tagName !== "PRE") continue;

          const container = document.createElement("div");
          container.className = "mermaid";
          container.textContent = code.textContent;

          pre.replaceWith(container);
        }

        await mermaid.run({ querySelector: ".mermaid" });
      }

      if (document.readyState === "loading") {
        document.addEventListener("DOMContentLoaded", () => {
          render().catch(console.error);
        });
      } else {
        render().catch(console.error);
      }
    </script>
    """
  end

  defp before_closing_body_tag(_), do: ""
end
