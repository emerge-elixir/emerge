defmodule Emerge.Docs.Screenshots do
  @moduledoc false

  use Emerge.UI

  alias Emerge.Docs.Examples

  @project_root Path.expand("../../..", __DIR__)

  def specs do
    [
      %{
        id: "counter-basic",
        width: 272,
        height: 72,
        density: 2,
        destinations: [asset_path("assets/counter-basic.png")],
        build: &counter_basic/0
      },
      %{
        id: "dashboard-functions",
        width: 560,
        height: 160,
        density: 2,
        destinations: [asset_path("assets/dashboard-functions.png")],
        build: &dashboard_functions/0
      },
      %{
        id: "describe-ui-escaping-layout-dropdown",
        width: 360,
        height: 250,
        density: 2,
        destinations: [
          asset_path("assets/describe-ui-escaping-layout-dropdown.png"),
          asset_path("guides/tutorials/assets/describe-ui-escaping-layout-dropdown.png")
        ],
        build: &describe_ui_escaping_layout_dropdown/0
      }
    ] ++ Examples.screenshot_specs()
  end

  def render_png(spec) do
    density = Map.get(spec, :density, 1)

    EmergeSkia.render_to_png(spec.build.(),
      otp_app: :emerge,
      width: spec.width * density,
      height: spec.height * density,
      scale: density * 1.0,
      asset_mode: :await
    )
  end

  defp asset_path(relative_path), do: Path.join(@project_root, relative_path)

  defp counter_basic do
    row(
      [
        Background.color(color(:slate, 800)),
        Font.color(color(:white)),
        spacing(12),
        padding(12)
      ],
      [
        counter_button([Event.on_press(:increment)], "+"),
        el([padding(10)], text("Count: 3")),
        counter_button([Event.on_press(:decrement)], "-")
      ]
    )
  end

  defp dashboard_functions do
    column(
      [
        width(fill()),
        padding(20),
        spacing(12),
        Background.color(color(:slate, 900)),
        Border.rounded(12)
      ],
      [
        el([Font.size(22), Font.color(color(:white))], text("Overview")),
        row([spacing(12)], Enum.map(dashboard_summary_stats(), &dashboard_stat_card/1))
      ]
    )
  end

  defp describe_ui_escaping_layout_dropdown do
    column([width(fill()), height(fill()), padding(24), spacing(12)], [
      row(
        [
          width(fill()),
          padding(12),
          spacing(12),
          Background.color(color(:slate, 100)),
          Border.rounded(10)
        ],
        [
          el(
            [width(fill()), center_y(), Font.color(color(:slate, 600))],
            text("Selected: 3 items")
          ),
          dropdown_trigger()
        ]
      ),
      el(
        [Font.size(12), Font.color(color(:slate, 500))],
        text("This help text stays where the column placed it.")
      )
    ])
  end

  defp dashboard_summary_stats do
    [
      {"Open", "12"},
      {"Closed", "34"},
      {"Owners", "5"}
    ]
  end

  defp dropdown_trigger do
    Input.button(
      [
        padding(12),
        Background.color(color(:slate, 700)),
        Border.rounded(8),
        Font.color(color(:white)),
        Event.on_press(:open_menu),
        Nearby.below(dropdown_menu())
      ],
      text("Actions")
    )
  end

  defp dropdown_menu do
    el(
      [
        align_right(),
        padding(8),
        Background.color(color(:white)),
        Border.rounded(10),
        Border.width(1),
        Border.color(color(:slate, 200))
      ],
      column([spacing(4)], [
        dropdown_menu_item("Rename", :rename),
        dropdown_menu_item("Duplicate", :duplicate),
        dropdown_menu_item("Delete", :delete)
      ])
    )
  end

  defp dropdown_menu_item(label, event) do
    Input.button(
      [
        width(fill()),
        padding(10),
        Background.color(color(:slate, 50)),
        Border.rounded(6),
        Font.color(color(:slate, 700)),
        Event.on_press(event)
      ],
      text(label)
    )
  end

  defp counter_button(attrs, label) do
    Input.button(
      attrs ++
        [
          padding(10),
          Background.color(color(:sky, 500)),
          Border.rounded(8)
        ],
      text(label)
    )
  end

  defp dashboard_stat_card({label, value}) do
    el(
      [
        width(fill()),
        padding(12),
        Background.color(color(:slate, 800)),
        Border.rounded(8)
      ],
      column([spacing(4)], [
        el([Font.color(color(:slate, 300))], text(label)),
        el([Font.size(20), Font.color(color(:white))], text(value))
      ])
    )
  end
end
