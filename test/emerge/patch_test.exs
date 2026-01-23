defmodule Emerge.PatchTest do
  use ExUnit.Case, async: true

  import Emerge.UI

  alias Emerge.Patch
  alias Emerge.Tree

  test "diff detects attribute changes and inserts" do
    {old, _state} =
      Tree.assign_ids(
        column([id: :root], [
          el([padding(4), id: :item_a], text("a"))
        ])
      )

    {new, _state} =
      Tree.assign_ids(
        column([id: :root], [
          el([padding(8), id: :item_a], text("a")),
          el(text("b"))
        ])
      )

    patches = Patch.diff(old, new)

    assert {:set_attrs, _id, _} = Enum.find(patches, fn
      {:set_attrs, id, _} when is_integer(id) -> true
      _ -> false
    end)

    assert Enum.any?(patches, fn
             {:insert_subtree, _parent, _index, _} -> true
             _ -> false
           end)
  end

  test "diff emits removes when ids disappear" do
    {old, _state} =
      Tree.assign_ids(
        column([id: :root], [
          el([id: :a], text("a")),
          el([id: :b], text("b"))
        ])
      )

    {new, _state} =
      Tree.assign_ids(
        column([id: :root], [
          el([id: :a], text("a"))
        ])
      )

    patches = Patch.diff(old, new)
    assert Enum.any?(patches, fn
             {:remove, id} when is_integer(id) -> true
             _ -> false
           end)
  end

  test "encode produces a command stream" do
    layout =
      column([id: :root], [
        el([id: {:card, 1}], text("a"))
      ])

    {layout, _state} = Tree.assign_ids(layout)
    patches = Patch.diff(layout, layout)
    binary = Patch.encode(patches)

    assert is_binary(binary)
  end

  test "decode returns patches for attrs, children, insert, remove" do
    {subtree, _state} =
      Tree.assign_ids(el([id: :item_b], text("b")))

    patches = [
      {:set_attrs, 1, %{padding: 4}},
      {:set_children, 2, [1]},
      {:insert_subtree, 2, 1, subtree},
      {:remove, 1}
    ]

    binary = Patch.encode(patches)
    decoded = Patch.decode(binary)

    assert normalize_patches(decoded) == normalize_patches(patches)
  end

  test "decode preserves nil parent ids" do
    {subtree, _state} = Tree.assign_ids(el([id: :item_x], text("x")))
    binary = Patch.encode([{:insert_subtree, nil, 0, subtree}])

    assert [{:insert_subtree, nil, 0, decoded}] = Patch.decode(binary)
    assert normalize_element(decoded) == normalize_element(subtree)
  end

  test "encode/decode round trip for diffs" do
    {old, _state} =
      Tree.assign_ids(
        column([id: :root], [
          el([id: :a], text("a")),
          el([id: :b], text("b"))
        ])
      )

    {new, _state} =
      Tree.assign_ids(
        column([id: :root], [
          el([padding(6), id: :a], text("a")),
          el([id: :c], text("c"))
        ])
      )

    patches = Patch.diff(old, new)
    binary = Patch.encode(patches)
    decoded = Patch.decode(binary)

    assert normalize_patches(decoded) == normalize_patches(patches)
  end

  test "diff state keeps ids stable across frames" do
    state = Emerge.DiffState.new()

    layout1 =
      column([id: :root], [
        el([id: :a], text("one")),
        el([id: :b], text("two"))
      ])

    {bin1, state, tree1} = Emerge.DiffState.diff_and_encode(state, layout1)
    assert is_binary(bin1)

    layout2 =
      column([id: :root], [
        el([id: :a], text("one")),
        el([id: :b], text("two")),
        el([id: :c], text("three"))
      ])

    {bin2, _state, tree2} = Emerge.DiffState.diff_and_encode(state, layout2)
    assert is_binary(bin2)

    ids1 = Enum.map(tree1.children, & &1.id)
    ids2 = Enum.map(tree2.children, & &1.id)

    assert Enum.at(ids1, 0) == Enum.at(ids2, 0)
    assert Enum.at(ids1, 1) == Enum.at(ids2, 1)
  end

  test "keyed reorder emits set_children without inserts or removes" do
    state = Emerge.DiffState.new()

    layout1 =
      column([id: :root], [
        el([key(:a)], text("a")),
        el([key(:b)], text("b")),
        el([key(:c)], text("c"))
      ])

    {_bin1, state, tree1} = Emerge.DiffState.diff_and_encode(state, layout1)

    layout2 =
      column([id: :root], [
        el([key(:c)], text("c")),
        el([key(:a)], text("a")),
        el([key(:b)], text("b"))
      ])

    {bin2, _state, _tree2} = Emerge.DiffState.diff_and_encode(state, layout2)
    patches = Patch.decode(bin2)

    assert Enum.any?(patches, fn
             {:set_children, id, _} when id == tree1.id -> true
             _ -> false
           end)

    refute Enum.any?(patches, fn
             {:insert_subtree, _, _, _} -> true
             {:remove, _} -> true
             _ -> false
           end)
  end

  test "keyed insert emits insert_subtree without set_children" do
    state = Emerge.DiffState.new()

    layout1 =
      row([id: :root], [
        el([key(:a)], text("a")),
        el([key(:b)], text("b"))
      ])

    {_bin1, state, tree1} = Emerge.DiffState.diff_and_encode(state, layout1)

    layout2 =
      row([id: :root], [
        el([key(:a)], text("a")),
        el([key(:b)], text("b")),
        el([key(:c)], text("c"))
      ])

    {bin2, _state, _tree2} = Emerge.DiffState.diff_and_encode(state, layout2)
    patches = Patch.decode(bin2)

    assert Enum.any?(patches, fn
             {:insert_subtree, id, _index, _} when id == tree1.id -> true
             _ -> false
           end)

    refute Enum.any?(patches, fn
             {:set_children, id, _} when id == tree1.id -> true
             _ -> false
           end)
  end

  test "keyed remove emits remove without set_children" do
    state = Emerge.DiffState.new()

    layout1 =
      row([id: :root], [
        el([key(:a)], text("a")),
        el([key(:b)], text("b")),
        el([key(:c)], text("c"))
      ])

    {_bin1, state, tree1} = Emerge.DiffState.diff_and_encode(state, layout1)

    layout2 =
      row([id: :root], [
        el([key(:a)], text("a")),
        el([key(:c)], text("c"))
      ])

    {bin2, _state, _tree2} = Emerge.DiffState.diff_and_encode(state, layout2)
    patches = Patch.decode(bin2)

    assert Enum.any?(patches, fn
             {:remove, _id} -> true
             _ -> false
           end)

    refute Enum.any?(patches, fn
             {:set_children, id, _} when id == tree1.id -> true
             _ -> false
           end)
  end

  test "keyed attribute change emits set_attrs only for that node" do
    state = Emerge.DiffState.new()

    layout1 =
      column([id: :root], [
        el([key(:a)], text("a")),
        el([key(:b)], text("b"))
      ])

    {_bin1, state, tree1} = Emerge.DiffState.diff_and_encode(state, layout1)

    layout2 =
      column([id: :root], [
        el([key(:a), padding(4)], text("a")),
        el([key(:b)], text("b"))
      ])

    {bin2, _state, _tree2} = Emerge.DiffState.diff_and_encode(state, layout2)
    patches = Patch.decode(bin2)

    ids = content_id_map(tree1)
    a_id = Map.fetch!(ids, "a")

    set_attrs =
      Enum.filter(patches, fn
        {:set_attrs, _, _} -> true
        _ -> false
      end)

    assert length(set_attrs) == 1
    assert {:set_attrs, ^a_id, _} = hd(set_attrs)
  end

  test "no patches when tree is identical" do
    state = Emerge.DiffState.new()

    layout =
      column([id: :root], [
        el([key(:a)], text("a")),
        el([key(:b)], text("b"))
      ])

    {_bin1, state, _tree1} = Emerge.DiffState.diff_and_encode(state, layout)
    {bin2, _state, _tree2} = Emerge.DiffState.diff_and_encode(state, layout)

    assert Patch.decode(bin2) == []
  end

  test "no extra patches when attrs unchanged but children reorder keyed" do
    state = Emerge.DiffState.new()

    layout1 =
      row([id: :root], [
        el([key(:a)], text("a")),
        el([key(:b)], text("b")),
        el([key(:c)], text("c"))
      ])

    {_bin1, state, tree1} = Emerge.DiffState.diff_and_encode(state, layout1)

    layout2 =
      row([id: :root], [
        el([key(:c)], text("c")),
        el([key(:a)], text("a")),
        el([key(:b)], text("b"))
      ])

    {bin2, _state, _tree2} = Emerge.DiffState.diff_and_encode(state, layout2)
    patches = Patch.decode(bin2)

    refute Enum.any?(patches, fn
             {:set_attrs, id, _} when id == tree1.id -> true
             _ -> false
           end)
  end

  test "set_children preserves child ordering" do
    state = Emerge.DiffState.new()

    layout1 =
      column([id: :root], [
        el([key(:a)], text("a")),
        el([key(:b)], text("b")),
        el([key(:c)], text("c"))
      ])

    {_bin1, state, tree1} = Emerge.DiffState.diff_and_encode(state, layout1)

    layout2 =
      column([id: :root], [
        el([key(:c)], text("c")),
        el([key(:a)], text("a")),
        el([key(:b)], text("b"))
      ])

    {bin2, _state, tree2} = Emerge.DiffState.diff_and_encode(state, layout2)
    patches = Patch.decode(bin2)

    assert {:set_children, id, children} =
             Enum.find(patches, fn
               {:set_children, id, _} when id == tree1.id -> true
               _ -> false
             end)

    assert id == tree1.id
    assert children == Enum.map(tree2.children, & &1.id)
  end

  test "unkeyed reorder emits set_attrs but no inserts/removes" do
    state = Emerge.DiffState.new()

    layout1 =
      row([id: :root], [
        el(text("a")),
        el(text("b")),
        el(text("c"))
      ])

    {_bin1, state, tree1} = Emerge.DiffState.diff_and_encode(state, layout1)

    layout2 =
      row([id: :root], [
        el(text("c")),
        el(text("a")),
        el(text("b"))
      ])

    {bin2, _state, _tree2} = Emerge.DiffState.diff_and_encode(state, layout2)
    patches = Patch.decode(bin2)

    set_attrs =
      Enum.filter(patches, fn
        {:set_attrs, _, _} -> true
        _ -> false
      end)

    assert length(set_attrs) == 3

    refute Enum.any?(patches, fn
             {:insert_subtree, _, _, _} -> true
             {:remove, _} -> true
             _ -> false
           end)

    refute Enum.any?(patches, fn
             {:set_children, id, _} when id == tree1.id -> true
             _ -> false
           end)
  end

  test "mixed reorder emits set_children with insert/remove" do
    state = Emerge.DiffState.new()

    layout1 =
      row([id: :root], [
        el([key(:a)], text("a")),
        el(text("u1")),
        el([key(:b)], text("b"))
      ])

    {_bin1, state, tree1} = Emerge.DiffState.diff_and_encode(state, layout1)

    layout2 =
      row([id: :root], [
        el(text("u1")),
        el([key(:b)], text("b")),
        el([key(:a)], text("a"))
      ])

    {bin2, _state, _tree2} = Emerge.DiffState.diff_and_encode(state, layout2)
    patches = Patch.decode(bin2)

    assert Enum.any?(patches, fn
             {:set_children, id, _} when id == tree1.id -> true
             _ -> false
           end)

    assert Enum.any?(patches, fn
             {:insert_subtree, _, _, _} -> true
             {:remove, _} -> true
             _ -> false
           end)
  end

  test "insert with reordering existing nodes emits set_children" do
    state = Emerge.DiffState.new()

    layout1 =
      row([id: :root], [
        el([key(:a)], text("a")),
        el([key(:b)], text("b")),
        el([key(:c)], text("c"))
      ])

    {_bin1, state, tree1} = Emerge.DiffState.diff_and_encode(state, layout1)

    layout2 =
      row([id: :root], [
        el([key(:b)], text("b")),
        el([key(:a)], text("a")),
        el([key(:c)], text("c")),
        el([key(:d)], text("d"))
      ])

    {bin2, _state, _tree2} = Emerge.DiffState.diff_and_encode(state, layout2)
    patches = Patch.decode(bin2)

    assert Enum.any?(patches, fn
             {:set_children, id, _} when id == tree1.id -> true
             _ -> false
           end)
  end

  test "insert preserving existing order skips set_children" do
    state = Emerge.DiffState.new()

    layout1 =
      row([id: :root], [
        el([key(:a)], text("a")),
        el([key(:b)], text("b")),
        el([key(:c)], text("c"))
      ])

    {_bin1, state, tree1} = Emerge.DiffState.diff_and_encode(state, layout1)

    layout2 =
      row([id: :root], [
        el([key(:a)], text("a")),
        el([key(:b)], text("b")),
        el([key(:c)], text("c")),
        el([key(:d)], text("d"))
      ])

    {bin2, _state, _tree2} = Emerge.DiffState.diff_and_encode(state, layout2)
    patches = Patch.decode(bin2)

    refute Enum.any?(patches, fn
             {:set_children, id, _} when id == tree1.id -> true
             _ -> false
           end)
  end

  test "multiple inserts preserving existing order skip set_children" do
    state = Emerge.DiffState.new()

    layout1 =
      row([id: :root], [
        el([key(:a)], text("a")),
        el([key(:b)], text("b")),
        el([key(:c)], text("c"))
      ])

    {_bin1, state, tree1} = Emerge.DiffState.diff_and_encode(state, layout1)

    layout2 =
      row([id: :root], [
        el([key(:a)], text("a")),
        el([key(:b)], text("b")),
        el([key(:x)], text("x")),
        el([key(:c)], text("c")),
        el([key(:y)], text("y"))
      ])

    {bin2, _state, _tree2} = Emerge.DiffState.diff_and_encode(state, layout2)
    patches = Patch.decode(bin2)

    refute Enum.any?(patches, fn
             {:set_children, id, _} when id == tree1.id -> true
             _ -> false
           end)
  end

  test "remove and insert without reordering preserves order without set_children" do
    state = Emerge.DiffState.new()

    layout1 =
      row([id: :root], [
        el([key(:a)], text("a")),
        el([key(:b)], text("b")),
        el([key(:c)], text("c")),
        el([key(:d)], text("d"))
      ])

    {_bin1, state, tree1} = Emerge.DiffState.diff_and_encode(state, layout1)

    layout2 =
      row([id: :root], [
        el([key(:a)], text("a")),
        el([key(:c)], text("c")),
        el([key(:d)], text("d")),
        el([key(:x)], text("x"))
      ])

    {bin2, _state, _tree2} = Emerge.DiffState.diff_and_encode(state, layout2)
    patches = Patch.decode(bin2)

    refute Enum.any?(patches, fn
             {:set_children, id, _} when id == tree1.id -> true
             _ -> false
           end)
  end

  test "multiple inserts with one reorder emit set_children" do
    state = Emerge.DiffState.new()

    layout1 =
      row([id: :root], [
        el([key(:a)], text("a")),
        el([key(:b)], text("b")),
        el([key(:c)], text("c")),
        el([key(:d)], text("d"))
      ])

    {_bin1, state, tree1} = Emerge.DiffState.diff_and_encode(state, layout1)

    layout2 =
      row([id: :root], [
        el([key(:b)], text("b")),
        el([key(:a)], text("a")),
        el([key(:x)], text("x")),
        el([key(:c)], text("c")),
        el([key(:y)], text("y")),
        el([key(:d)], text("d"))
      ])

    {bin2, _state, _tree2} = Emerge.DiffState.diff_and_encode(state, layout2)
    patches = Patch.decode(bin2)

    assert Enum.any?(patches, fn
             {:set_children, id, _} when id == tree1.id -> true
             _ -> false
           end)
  end

  defp content_id_map(%Emerge.Element{children: children}) do
    children
    |> Enum.map(fn child ->
      text = child.children |> hd() |> Map.get(:attrs) |> Map.get(:content)
      {text, child.id}
    end)
    |> Map.new()
  end

  defp normalize_patches(patches) do
    Enum.map(patches, &normalize_patch/1)
  end

  defp normalize_patch({:set_attrs, id, attrs}), do: {:set_attrs, id, normalize_attrs(attrs)}
  defp normalize_patch({:set_children, id, children}), do: {:set_children, id, children}
  defp normalize_patch({:insert_subtree, parent, index, subtree}),
    do: {:insert_subtree, parent, index, normalize_element(subtree)}
  defp normalize_patch({:remove, id}), do: {:remove, id}

  defp normalize_element(%Emerge.Element{} = element) do
    %{
      element
      | attrs: normalize_attrs(element.attrs),
        children: Enum.map(element.children, &normalize_element/1)
    }
  end

  defp normalize_attrs(attrs) do
    attrs
    |> Emerge.Tree.strip_runtime_attrs()
    |> Enum.map(fn {key, value} -> {key, normalize_value(value)} end)
    |> Map.new()
  end

  defp normalize_value(value) when is_number(value), do: value * 1.0

  defp normalize_value(%Emerge.Element{} = element), do: normalize_element(element)

  defp normalize_value(value) when is_map(value) do
    value
    |> Enum.map(fn {key, val} -> {key, normalize_value(val)} end)
    |> Map.new()
  end

  defp normalize_value(value) when is_list(value), do: Enum.map(value, &normalize_value/1)

  defp normalize_value({a, b, c, d}),
    do: {normalize_value(a), normalize_value(b), normalize_value(c), normalize_value(d)}

  defp normalize_value({a, b, c}),
    do: {normalize_value(a), normalize_value(b), normalize_value(c)}

  defp normalize_value({a, b}), do: {normalize_value(a), normalize_value(b)}

  defp normalize_value(value), do: value
end
