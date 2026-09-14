defmodule Emerge.Mix.Docs do
  @moduledoc false

  def config(root, version, source_url, env \\ System.get_env()) do
    internal = internal_docs_extras(root, env)

    [
      main: "readme",
      source_url: source_url,
      source_ref: "v#{version}",
      assets: %{
        "assets" => "assets",
        "guides/tutorials/assets" => "assets"
      },
      before_closing_body_tag: &before_closing_body_tag/1,
      extras: public_docs_extras(root) ++ internal,
      groups_for_extras: docs_groups_for_extras(internal),
      groups_for_modules: [
        Viewport: [Emerge, Emerge.Runtime.Viewport],
        UI: ~r/^Emerge\.UI(\.|$)/,
        Assets: ~r/^Emerge\.Assets(\.|$)/,
        Runtime: ~r/^Emerge\.Runtime\./,
        Rendering: ~r/^EmergeSkia(\.|$)/,
        Engine: ~r/^Emerge\.Engine(\.|$)/
      ]
    ]
  end

  defp internal_docs_extras(root, env) do
    if include_internal_docs?(env) do
      optional_internal_docs_extras(root)
    else
      []
    end
  end

  defp public_docs_extras(root) do
    [
      "README.md",
      "guides/tutorials/set_up_viewport.md",
      "guides/tutorials/describe_ui.md",
      "guides/tutorials/use_assets.md",
      "guides/tutorials/state_management.md",
      "guides/migrations/0.4.md",
      "guides/reference/native-renderer-builds.md"
    ]
    |> Enum.filter(&File.exists?(Path.join(root, &1)))
  end

  defp optional_internal_docs_extras(root) do
    [
      "guides/internals/architecture.md",
      "guides/internals/assets-images.md",
      "guides/internals/beam-performance-constraints.md",
      "guides/internals/macos-backend.md",
      "guides/internals/feature-roadmap.md",
      "guides/internals/emrg-format.md",
      "guides/internals/gradient-colors.md",
      "guides/internals/events.md",
      "guides/internals/layout-refresh-render-flow.md",
      "guides/internals/nearby-semantics.md",
      "guides/internals/tree-patching.md",
      "guides/internals/video-interop-architecture.md"
    ]
    |> Enum.filter(&File.exists?(Path.join(root, &1)))
  end

  defp docs_groups_for_extras(internal) do
    [
      Tutorials: ~r/guides\/tutorials\/.*/,
      Migrations: ~r/guides\/migrations\/.*/,
      Reference: ~r/guides\/reference\/.*/
    ] ++
      if internal == [] do
        []
      else
        [Internals: ~r/guides\/internals\/.*/]
      end
  end

  defp include_internal_docs?(env) do
    Map.get(env, "EMERGE_INCLUDE_INTERNAL_DOCS", "false") not in ["0", "false"]
  end

  def before_closing_body_tag(:html) do
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

  def before_closing_body_tag(_), do: ""
end
