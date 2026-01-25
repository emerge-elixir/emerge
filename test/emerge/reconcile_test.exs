defmodule Emerge.ReconcileTest do
  use ExUnit.Case, async: false

  import Emerge.UI

  alias Emerge.UI.{Background, Border, Font}

  @bg {:color_rgb, {26, 26, 40}}
  @panel {:color_rgb, {40, 40, 60}}
  @card_a {:color_rgb, {60, 60, 120}}
  @card_b {:color_rgb, {60, 90, 60}}
  @card_c {:color_rgb, {90, 60, 90}}
  @text_dim {:color_rgb, {180, 180, 200}}

  test "page switch patches apply cleanly" do
    tree_a = demo_tree(:overview)
    tree_b = demo_tree(:alignment)

    state = Emerge.diff_state_new()
    {full_bin, state, _assigned} = Emerge.encode_full(state, tree_a)

    tree = EmergeSkia.Native.tree_new()
    assert_ok(EmergeSkia.Native.tree_upload(tree, full_bin))

    {patch_bin, _next_state, _assigned} = Emerge.diff_state_update(state, tree_b)
    assert_ok(EmergeSkia.Native.tree_patch(tree, patch_bin))
  end

  defp demo_tree(page) do
    column(
      [
        width(:fill),
        height(:fill),
        padding(16),
        spacing(12),
        Background.color(@bg)
      ],
      [
        header_section(),
        row([width(:fill), height(:fill), spacing(12)], [
          menu_panel(page),
          content_panel(page)
        ])
      ]
    )
  end

  defp header_section() do
    el(
      [
        width(:fill),
        padding(12),
        Background.color(@panel),
        Border.rounded(10)
      ],
      column([spacing(4)], [
        el([Font.size(18), Font.color(:white)], text("Demo")),
        el([Font.size(12), Font.color(@text_dim)], text("Page switch test"))
      ])
    )
  end

  defp menu_panel(current_page) do
    items = [
      {"Overview", :overview},
      {"Alignment", :alignment},
      {"Scroll", :scroll}
    ]

    column(
      [
        width(px(180)),
        padding(10),
        spacing(8),
        Background.color(@panel),
        Border.rounded(10)
      ],
      Enum.map(items, fn {label, page} ->
        menu_item(label, page, current_page)
      end)
    )
  end

  defp menu_item(label, page, current_page) do
    active = page == current_page
    bg = if active, do: @card_a, else: @panel
    text_color = if active, do: :white, else: @text_dim

    el(
      [
        width(:fill),
        padding(8),
        Background.color(bg),
        Border.rounded(8),
        on_click({self(), {:demo_nav, page}})
      ],
      el([Font.size(12), Font.color(text_color)], text(label))
    )
  end

  defp content_panel(page) do
    el(
      [
        width(:fill),
        height(:fill),
        padding(12),
        scroll_y(0),
        scrollbar_y(),
        clip_y(),
        Background.color(@panel),
        Border.rounded(10)
      ],
      render_page(page)
    )
  end

  defp render_page(:overview) do
    column([width(:fill), spacing(10)], [
      el([Font.size(16), Font.color(:white)], text("Overview")),
      row([width(:fill), spacing(8)], [
        feature_card("Rows", @card_a),
        feature_card("Columns", @card_b),
        feature_card("Nesting", @card_c)
      ]),
      wrapped_row([width(:fill), spacing(6)], [
        chip("Wrap"),
        chip("Row"),
        chip("Demo"),
        chip("Extra"),
        chip("Items")
      ])
    ])
  end

  defp render_page(:alignment) do
    column([width(:fill), spacing(10)], [
      el([Font.size(16), Font.color(:white)], text("Alignment")),
      row([width(:fill), spacing(8)], [
        el([padding(8), Background.color(@card_a), Border.rounded(6)], text("Left")),
        el(
          [padding(8), Background.color(@card_b), Border.rounded(6), center_x()],
          text("Center")
        ),
        el(
          [padding(8), Background.color(@card_c), Border.rounded(6), align_right()],
          text("Right")
        )
      ]),
      el(
        [
          width(:fill),
          height(px(80)),
          Background.color({:color_rgb, {50, 50, 70}}),
          Border.rounded(6)
        ],
        el(
          [
            width(:fill),
            height(:fill),
            center_x(),
            center_y(),
            Font.size(14),
            Font.color(:white)
          ],
          text("Centered content")
        )
      )
    ])
  end

  defp render_page(_page) do
    column([width(:fill), spacing(6)], [
      el([Font.size(16), Font.color(:white)], text("Other")),
      el([Font.size(12), Font.color(@text_dim)], text("Placeholder"))
    ])
  end

  defp feature_card(label, color) do
    el(
      [
        width(:fill),
        padding(10),
        Background.color(color),
        Border.rounded(6)
      ],
      el([Font.size(12), Font.color(:white)], text(label))
    )
  end

  defp chip(label) do
    el(
      [
        padding(6),
        Background.color({:color_rgb, {55, 60, 90}}),
        Border.rounded(12),
        Font.size(11),
        Font.color(:white)
      ],
      text(label)
    )
  end

  defp assert_ok(result) do
    case result do
      :ok -> :ok
      {:ok, _} -> :ok
      other -> flunk("expected :ok, got #{inspect(other)}")
    end
  end
end
