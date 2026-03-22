defmodule Emerge.Engine.ReconcileTest do
  use ExUnit.Case, async: false

  use Emerge.UI

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

    state = Emerge.Engine.diff_state_new()
    {full_bin, state, _assigned} = Emerge.Engine.encode_full(state, tree_a)

    tree = EmergeSkia.Native.tree_new()
    assert_ok(EmergeSkia.Native.tree_upload(tree, full_bin))

    {patch_bin, _next_state, _assigned} = Emerge.Engine.diff_state_update(state, tree_b)
    assert_ok(EmergeSkia.Native.tree_patch(tree, patch_bin))
  end

  test "keyed scroll containers preserve child lists after scramble" do
    items = base_items()
    scrambled = scramble_items(items)

    tree_a = comparison_tree(items)
    tree_b = comparison_tree(scrambled)

    state = Emerge.Engine.diff_state_new()
    {_full_bin, state, _assigned} = Emerge.Engine.encode_full(state, tree_a)
    {_patch_bin, _next_state, assigned} = Emerge.Engine.diff_state_update(state, tree_b)

    assert extract_stable_rows(assigned) == expected_rows(scrambled)

    ids = collect_ids(assigned)
    assert length(ids) == length(Enum.uniq(ids))
  end

  test "patched tree preserves keyed scroll children" do
    items = base_items()
    scrambled = scramble_items(items)

    tree_a = comparison_tree(items)
    tree_b = comparison_tree(scrambled)
    expected = expected_rows(scrambled)
    expected_count = count_nodes(tree_b)

    state = Emerge.Engine.diff_state_new()
    {full_bin, state, _assigned} = Emerge.Engine.encode_full(state, tree_a)
    {patch_bin, _next_state, _assigned} = Emerge.Engine.diff_state_update(state, tree_b)

    tree = EmergeSkia.Native.tree_new()
    assert_ok(EmergeSkia.Native.tree_upload(tree, full_bin))
    {:ok, roundtrip_bin} = EmergeSkia.Native.tree_patch_roundtrip(tree, patch_bin)

    roundtrip_tree = Emerge.Engine.Serialization.decode(roundtrip_bin)

    assert extract_stable_rows(roundtrip_tree) == expected
    assert EmergeSkia.Native.tree_node_count(tree) == expected_count
  end

  test "roundtrip handles reorder with inserts in keyed scroll list" do
    items_a = [%{label: "Alpha", count: 0, children: build_children("A", 5)}]

    items_b = [
      %{
        label: "Alpha",
        count: 0,
        children: [
          %{label: "A3", count: 0},
          %{label: "X1", count: 0},
          %{label: "A1", count: 0},
          %{label: "X2", count: 0},
          %{label: "A2", count: 0}
        ]
      }
    ]

    tree_a = stable_only_tree(items_a)
    tree_b = stable_only_tree(items_b)

    state = Emerge.Engine.diff_state_new()
    {full_bin, state, _assigned} = Emerge.Engine.encode_full(state, tree_a)
    {patch_bin, _next_state, _assigned} = Emerge.Engine.diff_state_update(state, tree_b)

    tree = EmergeSkia.Native.tree_new()
    assert_ok(EmergeSkia.Native.tree_upload(tree, full_bin))
    {:ok, roundtrip_bin} = EmergeSkia.Native.tree_patch_roundtrip(tree, patch_bin)

    roundtrip_tree = Emerge.Engine.Serialization.decode(roundtrip_bin)

    assert extract_single_column_rows(roundtrip_tree) == expected_rows(items_b)
  end

  test "mixed keyed children raise an error" do
    tree =
      column([width(:fill)], [
        column([width(:fill)], [
          el([key(:one)], text("One")),
          el([], text("Two"))
        ])
      ])

    assert_raise ArgumentError, ~r/All siblings must have key/, fn ->
      Emerge.Engine.Reconcile.assign_ids(tree)
    end
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
        Event.on_click({self(), {:demo_nav, page}})
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
        scrollbar_y(),
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

  defp base_items do
    [
      %{label: "Alpha", count: 0, children: build_children("A", 5)},
      %{label: "Bravo", count: 0, children: build_children("B", 5)},
      %{label: "Charlie", count: 0, children: build_children("C", 5)},
      %{label: "Delta", count: 0, children: build_children("D", 5)}
    ]
  end

  defp build_children(prefix, count) do
    Enum.map(1..count, fn idx -> %{label: "#{prefix}#{idx}", count: 0} end)
  end

  defp scramble_items(items) do
    rotated_items = Enum.drop(items, 1) ++ [hd(items)]
    all_children = Enum.flat_map(items, & &1.children)
    rotated_children = Enum.drop(all_children, 2) ++ Enum.take(all_children, 2)
    counts = Enum.map(rotated_items, &length(&1.children))

    {assigned_children, _} =
      Enum.map_reduce(counts, rotated_children, fn count, remaining ->
        {Enum.take(remaining, count), Enum.drop(remaining, count)}
      end)

    rotated_items
    |> Enum.zip(assigned_children)
    |> Enum.map(fn {item, children} -> %{item | children: children} end)
  end

  defp comparison_tree(items) do
    column([width(:fill)], [
      row([width(:fill), spacing(16)], [
        list_column(items, false),
        list_column(items, true)
      ])
    ])
  end

  defp stable_only_tree(items) do
    column([width(:fill)], [list_column(items, true)])
  end

  defp list_column(items, keyed?) do
    column([width(:fill), spacing(12)], Enum.map(items, &list_item(&1, keyed?)))
  end

  defp list_item(item, keyed?) do
    row_key = if keyed?, do: [key: {:stable, item.label}], else: []
    scroll_key = if keyed?, do: [key: {:stable, :scroll, item.label}], else: []

    column(
      [
        width(:fill),
        padding(10),
        Background.color(@panel),
        Border.rounded(8)
      ] ++ row_key,
      [
        el(
          [Font.size(12), Font.color(:white)] ++
            if(keyed?, do: [key: {:stable, :header, item.label}], else: []),
          text(item.label)
        ),
        el(
          [
            width(:fill),
            height(px(70)),
            padding(6),
            scrollbar_y(),
            Background.color(@card_a),
            Border.rounded(6)
          ] ++ scroll_key,
          column(
            [spacing(6)],
            Enum.map(item.children, fn child ->
              child_key =
                if keyed?, do: [key: {:stable, item.label, child.label}], else: []

              el(
                [
                  padding(6),
                  Background.color(@card_b),
                  Border.rounded(6)
                ] ++ child_key,
                el([Font.size(10), Font.color(:white)], text(child.label))
              )
            end)
          )
        )
      ]
    )
  end

  defp expected_rows(items) do
    items
    |> Enum.map(fn item -> {item.label, Enum.map(item.children, & &1.label)} end)
    |> Map.new()
  end

  defp extract_stable_rows(tree) do
    stable_column =
      tree
      |> element_children()
      |> List.first()
      |> element_children()
      |> Enum.at(1)

    rows_from_column(stable_column)
  end

  defp extract_single_column_rows(tree) do
    tree
    |> element_children()
    |> List.first()
    |> rows_from_column()
  end

  defp rows_from_column(column) do
    column
    |> element_children()
    |> Enum.map(fn item_el ->
      [header, scroll] = element_children(item_el)
      label = extract_text(header)

      child_labels =
        scroll
        |> element_children()
        |> List.first()
        |> element_children()
        |> Enum.map(&extract_text/1)

      {label, child_labels}
    end)
    |> Map.new()
  end

  defp element_children(%Emerge.Engine.Element{children: children}), do: children

  defp extract_text(%Emerge.Engine.Element{type: :text, attrs: %{content: content}}), do: content

  defp extract_text(%Emerge.Engine.Element{children: children}) do
    Enum.find_value(children, &extract_text/1)
  end

  defp collect_ids(%Emerge.Engine.Element{id: id, children: children}) do
    [id | Enum.flat_map(children, &collect_ids/1)]
  end

  defp count_nodes(%Emerge.Engine.Element{children: children}) do
    1 + Enum.reduce(children, 0, fn child, acc -> acc + count_nodes(child) end)
  end
end
