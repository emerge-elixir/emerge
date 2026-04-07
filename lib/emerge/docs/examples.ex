defmodule Emerge.Docs.Examples do
  @moduledoc false

  @project_root Path.expand("../../..", __DIR__)
  @examples_root Path.join(__DIR__, "examples")

  @examples [
    %{id: "ui-background-overview", file: "ui-background-overview.exs", width: 340, height: 400},
    %{id: "ui-align-el", file: "ui-align-el.exs", width: 320, height: 160},
    %{id: "ui-align-row", file: "ui-align-row.exs", width: 360, height: 88},
    %{id: "ui-align-column", file: "ui-align-column.exs", width: 220, height: 240},
    %{id: "ui-space-padding", file: "ui-space-padding.exs", width: 360, height: 108},
    %{id: "ui-space-spacing", file: "ui-space-spacing.exs", width: 320, height: 132},
    %{id: "ui-space-evenly", file: "ui-space-evenly.exs", width: 360, height: 82},
    %{id: "ui-size-fixed", file: "ui-size-fixed.exs", width: 244, height: 70},
    %{id: "ui-size-shrink-fill", file: "ui-size-shrink-fill.exs", width: 360, height: 74},
    %{id: "ui-size-weighted-fill", file: "ui-size-weighted-fill.exs", width: 360, height: 56},
    %{id: "ui-size-min-max", file: "ui-size-min-max.exs", width: 360, height: 74},
    %{id: "ui-scroll-vertical", file: "ui-scroll-vertical.exs", width: 240, height: 180},
    %{id: "ui-scroll-horizontal", file: "ui-scroll-horizontal.exs", width: 360, height: 84},
    %{id: "ui-scroll-both", file: "ui-scroll-both.exs", width: 320, height: 180},
    %{id: "ui-border-radius-width", file: "ui-border-radius-width.exs", width: 332, height: 106},
    %{id: "ui-border-shadows", file: "ui-border-shadows.exs", width: 420, height: 154},
    %{id: "ui-font-overview", file: "ui-font-overview.exs", width: 320, height: 182},
    %{id: "ui-font-alignment", file: "ui-font-alignment.exs", width: 320, height: 188},
    %{id: "ui-transform-translate", file: "ui-transform-translate.exs", width: 340, height: 108},
    %{
      id: "ui-transform-rotate-scale",
      file: "ui-transform-rotate-scale.exs",
      width: 360,
      height: 126
    },
    %{id: "ui-transform-alpha", file: "ui-transform-alpha.exs", width: 320, height: 96},
    %{
      id: "ui-transform-slot-vs-visual",
      file: "ui-transform-slot-vs-visual.exs",
      width: 360,
      height: 180
    }
  ]

  @prelude """
  import Kernel, except: [min: 2, max: 2]

  import Emerge.UI
  import Emerge.UI.Color
  import Emerge.UI.Size
  import Emerge.UI.Space
  import Emerge.UI.Scroll
  import Emerge.UI.Align

  alias Emerge.UI.{
    Animation,
    Background,
    Border,
    Event,
    Font,
    Input,
    Interactive,
    Nearby,
    Svg,
    Transform
  }
  """

  def specs, do: @examples

  def spec!(id) do
    Enum.find(@examples, &(&1.id == id)) ||
      raise ArgumentError, "unknown docs example: #{inspect(id)}"
  end

  def path!(id) do
    id
    |> spec!()
    |> Map.fetch!(:file)
    |> then(&Path.join(@examples_root, &1))
  end

  def asset!(id), do: "#{id}.png"

  def code!(id) do
    id
    |> path!()
    |> File.read!()
    |> String.trim()
  end

  def code_block!(id) do
    ["```elixir", code!(id), "```"]
    |> Enum.join("\n")
  end

  def image_tag!(id, alt, opts \\ []) do
    spec = spec!(id)
    width = Keyword.get(opts, :width, spec.width)

    ~s(<img src="assets/#{asset!(id)}" alt="#{alt}" width="#{width}">)
  end

  def tree!(id) do
    {tree, _binding} = Code.eval_string(@prelude <> "\n" <> code!(id), [], file: path!(id))
    tree
  end

  def screenshot_specs do
    Enum.map(@examples, fn spec ->
      %{
        id: spec.id,
        width: spec.width,
        height: spec.height,
        density: 2,
        destinations: [asset_path("assets/#{asset!(spec.id)}")],
        build: fn -> tree!(spec.id) end
      }
    end)
  end

  defmacro external_resources(ids_ast) do
    ids = Macro.expand(ids_ast, __CALLER__)

    resources =
      Enum.map(ids, fn id ->
        quote do
          @external_resource unquote(Emerge.Docs.Examples.path!(id))
        end
      end)

    {:__block__, [], resources}
  end

  defp asset_path(relative_path), do: Path.join(@project_root, relative_path)
end
