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
        before_closing_body_tag: &before_closing_body_tag/1,
        extras: [
          "README.md",
          "guides/internals/architecture.md",
          "guides/internals/assets-images.md",
          "guides/internals/feature-roadmap.md",
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
