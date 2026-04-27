defmodule Emerge.Engine.PatchTest do
  use ExUnit.Case, async: true

  use Emerge.UI

  alias Emerge.Engine.Patch
  alias Emerge.Engine.Tree
  alias Emerge.UI.{Background, Border, Font}

  test "diff detects attribute changes and inserts" do
    {old, _state} =
      Tree.assign_ids(
        column([key(:root)], [
          el([padding(4), key(:item_a)], text("a"))
        ])
      )

    {new, _state} =
      Tree.assign_ids(
        column([key(:root)], [
          el([padding(8), key(:item_a)], text("a")),
          el([key(:item_b)], text("b"))
        ])
      )

    patches = Patch.diff(old, new)

    assert {:set_attrs, _id, _} =
             Enum.find(patches, fn
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
        column([key(:root)], [
          el([key(:a)], text("a")),
          el([key(:b)], text("b"))
        ])
      )

    {new, _state} =
      Tree.assign_ids(
        column([key(:root)], [
          el([key(:a)], text("a"))
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
      column([key(:root)], [
        el([key({:card, 1})], text("a"))
      ])

    {layout, _state} = Tree.assign_ids(layout)
    patches = Patch.diff(layout, layout)
    binary = Patch.encode(patches)

    assert is_binary(binary)
  end

  test "decode returns patches for attrs, children, insert, remove" do
    {subtree, _state} =
      Tree.assign_ids(el([key(:item_b)], text("b")))

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
    {subtree, _state} = Tree.assign_ids(el([key(:item_x)], text("x")))
    binary = Patch.encode([{:insert_subtree, nil, 0, subtree}])

    assert [{:insert_subtree, nil, 0, decoded}] = Patch.decode(binary)
    assert normalize_element(decoded) == normalize_element(subtree)
  end

  test "encode/decode round trip for diffs" do
    {old, _state} =
      Tree.assign_ids(
        column([key(:root)], [
          el([key(:a)], text("a")),
          el([key(:b)], text("b"))
        ])
      )

    {new, _state} =
      Tree.assign_ids(
        column([key(:root)], [
          el([padding(6), key(:a)], text("a")),
          el([key(:c)], text("c"))
        ])
      )

    patches = Patch.diff(old, new)
    binary = Patch.encode(patches)
    decoded = Patch.decode(binary)

    assert normalize_patches(decoded) == normalize_patches(patches)
  end

  test "diff state keeps ids stable across frames" do
    state = Emerge.Engine.DiffState.new()

    layout1 =
      column([key(:root)], [
        el([key(:a)], text("one")),
        el([key(:b)], text("two"))
      ])

    {bin1, state, tree1} = Emerge.Engine.DiffState.diff_and_encode(state, layout1)
    assert is_binary(bin1)

    layout2 =
      column([key(:root)], [
        el([key(:a)], text("one")),
        el([key(:b)], text("two")),
        el([key(:c)], text("three"))
      ])

    {bin2, _state, tree2} = Emerge.Engine.DiffState.diff_and_encode(state, layout2)
    assert is_binary(bin2)

    ids1 = Enum.map(tree1.children, & &1.id)
    ids2 = Enum.map(tree2.children, & &1.id)

    assert Enum.at(ids1, 0) == Enum.at(ids2, 0)
    assert Enum.at(ids1, 1) == Enum.at(ids2, 1)
  end

  test "keyed reorder emits set_children without inserts or removes" do
    layout1 =
      column([key(:root)], [
        el([key(:a)], text("a")),
        el([key(:b)], text("b")),
        el([key(:c)], text("c"))
      ])

    layout2 =
      column([key(:root)], [
        el([key(:c)], text("c")),
        el([key(:a)], text("a")),
        el([key(:b)], text("b"))
      ])

    {patches, tree1, _tree2} = diff_state_native_patch_roundtrip(layout1, layout2)

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
    layout1 =
      row([key(:root)], [
        el([key(:a)], text("a")),
        el([key(:b)], text("b"))
      ])

    layout2 =
      row([key(:root)], [
        el([key(:a)], text("a")),
        el([key(:b)], text("b")),
        el([key(:c)], text("c"))
      ])

    {patches, tree1, _tree2} = diff_state_native_patch_roundtrip(layout1, layout2)

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
    layout1 =
      row([key(:root)], [
        el([key(:a)], text("a")),
        el([key(:b)], text("b")),
        el([key(:c)], text("c"))
      ])

    layout2 =
      row([key(:root)], [
        el([key(:a)], text("a")),
        el([key(:c)], text("c"))
      ])

    {patches, tree1, _tree2} = diff_state_native_patch_roundtrip(layout1, layout2)

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
    layout1 =
      column([key(:root)], [
        el([key(:a)], text("a")),
        el([key(:b)], text("b"))
      ])

    layout2 =
      column([key(:root)], [
        el([key(:a), padding(4)], text("a")),
        el([key(:b)], text("b"))
      ])

    {patches, tree1, _tree2} = diff_state_native_patch_roundtrip(layout1, layout2)

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
    layout =
      column([key(:root)], [
        el([key(:a)], text("a")),
        el([key(:b)], text("b"))
      ])

    {patches, _tree1, _tree2} = diff_state_native_patch_roundtrip(layout, layout)

    assert patches == []
  end

  test "no extra patches when attrs unchanged but children reorder keyed" do
    layout1 =
      row([key(:root)], [
        el([key(:a)], text("a")),
        el([key(:b)], text("b")),
        el([key(:c)], text("c"))
      ])

    layout2 =
      row([key(:root)], [
        el([key(:c)], text("c")),
        el([key(:a)], text("a")),
        el([key(:b)], text("b"))
      ])

    {patches, tree1, _tree2} = diff_state_native_patch_roundtrip(layout1, layout2)

    refute Enum.any?(patches, fn
             {:set_attrs, id, _} when id == tree1.id -> true
             _ -> false
           end)
  end

  test "set_children preserves child ordering" do
    layout1 =
      column([key(:root)], [
        el([key(:a)], text("a")),
        el([key(:b)], text("b")),
        el([key(:c)], text("c"))
      ])

    layout2 =
      column([key(:root)], [
        el([key(:c)], text("c")),
        el([key(:a)], text("a")),
        el([key(:b)], text("b"))
      ])

    {patches, tree1, tree2} = diff_state_native_patch_roundtrip(layout1, layout2)

    assert {:set_children, id, children} =
             Enum.find(patches, fn
               {:set_children, id, _} when id == tree1.id -> true
               _ -> false
             end)

    assert id == tree1.id
    assert children == Enum.map(tree2.children, & &1.id)
  end

  test "unkeyed reorder emits set_attrs but no inserts/removes" do
    layout1 =
      row([key(:root)], [
        el([], text("a")),
        el([], text("b")),
        el([], text("c"))
      ])

    layout2 =
      row([key(:root)], [
        el([], text("c")),
        el([], text("a")),
        el([], text("b"))
      ])

    {patches, tree1, _tree2} = diff_state_native_patch_roundtrip(layout1, layout2)

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

  test "mixed keyed/unkeyed reorder raises" do
    state = Emerge.Engine.DiffState.new()

    layout1 =
      row([key(:root)], [
        el([key(:a)], text("a")),
        el([], text("u1")),
        el([key(:b)], text("b"))
      ])

    assert_raise ArgumentError, ~r/All siblings must have key/, fn ->
      Emerge.Engine.DiffState.diff_and_encode(state, layout1)
    end
  end

  test "insert with reordering existing nodes emits set_children" do
    layout1 =
      row([key(:root)], [
        el([key(:a)], text("a")),
        el([key(:b)], text("b")),
        el([key(:c)], text("c"))
      ])

    layout2 =
      row([key(:root)], [
        el([key(:b)], text("b")),
        el([key(:a)], text("a")),
        el([key(:c)], text("c")),
        el([key(:d)], text("d"))
      ])

    {patches, tree1, _tree2} = diff_state_native_patch_roundtrip(layout1, layout2)

    assert Enum.any?(patches, fn
             {:set_children, id, _} when id == tree1.id -> true
             _ -> false
           end)
  end

  test "stateful patch roundtrip matches full tree" do
    tree_a = demo_tree(40.0)
    tree_b = demo_tree(nil)

    {tree_a, _state} = Tree.assign_ids(tree_a)
    {tree_b, _state} = Tree.assign_ids(tree_b)

    bin_a = Emerge.Engine.Serialization.encode_tree(tree_a)
    bin_b = Emerge.Engine.Serialization.encode_tree(tree_b)

    patches = Patch.diff(tree_a, tree_b)
    patch_bin = Patch.encode(patches)

    tree = EmergeSkia.Native.tree_new()

    upload_roundtrip = unwrap_binary(EmergeSkia.Native.tree_upload_roundtrip(tree, bin_a))
    expected_upload = unwrap_binary(EmergeSkia.Native.tree_roundtrip(bin_a))

    assert upload_roundtrip == expected_upload

    patch_roundtrip = unwrap_binary(EmergeSkia.Native.tree_patch_roundtrip(tree, patch_bin))
    expected = unwrap_binary(EmergeSkia.Native.tree_roundtrip(bin_b))

    assert patch_roundtrip == expected
  end

  test "insert preserving existing order skips set_children" do
    layout1 =
      row([key(:root)], [
        el([key(:a)], text("a")),
        el([key(:b)], text("b")),
        el([key(:c)], text("c"))
      ])

    layout2 =
      row([key(:root)], [
        el([key(:a)], text("a")),
        el([key(:b)], text("b")),
        el([key(:c)], text("c")),
        el([key(:d)], text("d"))
      ])

    {patches, tree1, _tree2} = diff_state_native_patch_roundtrip(layout1, layout2)

    refute Enum.any?(patches, fn
             {:set_children, id, _} when id == tree1.id -> true
             _ -> false
           end)
  end

  test "multiple inserts preserving existing order skip set_children" do
    layout1 =
      row([key(:root)], [
        el([key(:a)], text("a")),
        el([key(:b)], text("b")),
        el([key(:c)], text("c"))
      ])

    layout2 =
      row([key(:root)], [
        el([key(:a)], text("a")),
        el([key(:b)], text("b")),
        el([key(:x)], text("x")),
        el([key(:c)], text("c")),
        el([key(:y)], text("y"))
      ])

    {patches, tree1, _tree2} = diff_state_native_patch_roundtrip(layout1, layout2)

    refute Enum.any?(patches, fn
             {:set_children, id, _} when id == tree1.id -> true
             _ -> false
           end)
  end

  test "remove and insert without reordering emits final set_children and roundtrips natively" do
    layout1 =
      row([key(:root)], [
        el([key(:a)], text("a")),
        el([key(:b)], text("b")),
        el([key(:c)], text("c")),
        el([key(:d)], text("d"))
      ])

    layout2 =
      row([key(:root)], [
        el([key(:a)], text("a")),
        el([key(:c)], text("c")),
        el([key(:d)], text("d")),
        el([key(:x)], text("x"))
      ])

    {patches, tree1, tree2} = diff_state_native_patch_roundtrip(layout1, layout2)

    assert {:set_children, id, children} =
             Enum.find(patches, fn
               {:set_children, id, _} when id == tree1.id -> true
               _ -> false
             end)

    assert id == tree1.id
    assert children == Enum.map(tree2.children, & &1.id)
  end

  test "multiple removes and inserts without survivor reordering roundtrip natively" do
    layout1 =
      row([key(:root)], [
        el([key(:a)], text("a")),
        el([key(:b)], text("b")),
        el([key(:c)], text("c")),
        el([key(:d)], text("d")),
        el([key(:e)], text("e"))
      ])

    layout2 =
      row([key(:root)], [
        el([key(:a)], text("a")),
        el([key(:d)], text("d")),
        el([key(:e)], text("e")),
        el([key(:x)], text("x")),
        el([key(:y)], text("y"))
      ])

    {patches, tree1, tree2} = diff_state_native_patch_roundtrip(layout1, layout2)

    assert Enum.any?(patches, fn
             {:remove, _id} -> true
             _ -> false
           end)

    assert Enum.any?(patches, fn
             {:insert_subtree, id, _index, _subtree} when id == tree1.id -> true
             _ -> false
           end)

    assert Enum.any?(patches, fn
             {:set_children, id, children} when id == tree1.id ->
               children == Enum.map(tree2.children, & &1.id)

             _ ->
               false
           end)
  end

  test "multiple inserts with one reorder emit set_children" do
    layout1 =
      row([key(:root)], [
        el([key(:a)], text("a")),
        el([key(:b)], text("b")),
        el([key(:c)], text("c")),
        el([key(:d)], text("d"))
      ])

    layout2 =
      row([key(:root)], [
        el([key(:b)], text("b")),
        el([key(:a)], text("a")),
        el([key(:x)], text("x")),
        el([key(:c)], text("c")),
        el([key(:y)], text("y")),
        el([key(:d)], text("d"))
      ])

    {patches, tree1, _tree2} = diff_state_native_patch_roundtrip(layout1, layout2)

    assert Enum.any?(patches, fn
             {:set_children, id, _} when id == tree1.id -> true
             _ -> false
           end)
  end

  test "nearby slot change preserves keyed node id and emits set_nearby_mounts only" do
    layout1 =
      column([key(:root)], [
        el([key(:host), Nearby.above(el([key(:tip)], text("Tip")))], text("Host"))
      ])

    layout2 =
      column([key(:root)], [
        el([key(:host), Nearby.below(el([key(:tip)], text("Tip")))], text("Host"))
      ])

    {patches, tree1, tree2} = diff_state_native_patch_roundtrip(layout1, layout2)

    host1 = hd(tree1.children)
    host2 = hd(tree2.children)
    [{:above, tip1}] = host1.nearby
    [{:below, tip2}] = host2.nearby

    assert tip1.id == tip2.id

    assert Enum.any?(patches, fn
             {:set_nearby_mounts, id, mounts} when id == host1.id ->
               mounts == [{:below, tip2.id}]

             _ ->
               false
           end)

    refute Enum.any?(patches, fn
             {:insert_nearby_subtree, _, _, _, _} -> true
             {:remove, id} when id == tip1.id -> true
             _ -> false
           end)
  end

  test "adding keyed nearby emits insert_nearby_subtree without set_nearby_mounts" do
    layout1 =
      column([key(:root)], [
        el([key(:host)], text("Host"))
      ])

    layout2 =
      column([key(:root)], [
        el([key(:host), Nearby.above(el([key(:tip)], text("Tip")))], text("Host"))
      ])

    {patches, tree1, tree2} = diff_state_native_patch_roundtrip(layout1, layout2)

    host1 = hd(tree1.children)
    host2 = hd(tree2.children)
    [{:above, tip2}] = host2.nearby

    assert Enum.any?(patches, fn
             {:insert_nearby_subtree, id, 0, :above, subtree} when id == host1.id ->
               subtree.id == tip2.id

             _ ->
               false
           end)

    refute Enum.any?(patches, fn
             {:set_nearby_mounts, id, _mounts} when id == host1.id -> true
             {:insert_subtree, _, _, _} -> true
             {:remove, _} -> true
             _ -> false
           end)
  end

  test "removing keyed nearby emits remove without set_nearby_mounts" do
    layout1 =
      column([key(:root)], [
        el([key(:host), Nearby.above(el([key(:tip)], text("Tip")))], text("Host"))
      ])

    layout2 =
      column([key(:root)], [
        el([key(:host)], text("Host"))
      ])

    {patches, tree1, _tree2} = diff_state_native_patch_roundtrip(layout1, layout2)

    host1 = hd(tree1.children)
    [{:above, tip1}] = host1.nearby

    assert Enum.any?(patches, fn
             {:remove, id} when id == tip1.id -> true
             _ -> false
           end)

    refute Enum.any?(patches, fn
             {:set_nearby_mounts, id, _mounts} when id == host1.id -> true
             {:insert_nearby_subtree, _, _, _, _} -> true
             _ -> false
           end)
  end

  test "nearby keyed reorder emits set_nearby_mounts without inserts or removes" do
    layout1 =
      column([key(:root)], [
        el(
          [
            key(:host),
            Nearby.above(el([key(:above)], text("Above"))),
            Nearby.below(el([key(:below)], text("Below")))
          ],
          text("Host")
        )
      ])

    layout2 =
      column([key(:root)], [
        el(
          [
            key(:host),
            Nearby.below(el([key(:below)], text("Below"))),
            Nearby.above(el([key(:above)], text("Above")))
          ],
          text("Host")
        )
      ])

    {patches, tree1, tree2} = diff_state_native_patch_roundtrip(layout1, layout2)

    host1 = hd(tree1.children)
    host2 = hd(tree2.children)
    [{:above, above1}, {:below, below1}] = host1.nearby
    [{:below, below2}, {:above, above2}] = host2.nearby

    assert above1.id == above2.id
    assert below1.id == below2.id

    assert Enum.any?(patches, fn
             {:set_nearby_mounts, id, mounts} when id == host1.id ->
               mounts == [{:below, below2.id}, {:above, above2.id}]

             _ ->
               false
           end)

    removed_ids = [above1.id, below1.id]

    refute Enum.any?(patches, fn
             {:insert_nearby_subtree, _, _, _, _} -> true
             {:remove, id} -> id in removed_ids
             _ -> false
           end)
  end

  test "nearby remove and insert without survivor reordering emits final order and roundtrips natively" do
    layout1 =
      column([key(:root)], [
        el(
          [
            key(:host),
            Nearby.above(el([key(:above)], text("Above"))),
            Nearby.below(el([key(:below)], text("Below"))),
            Nearby.on_left(el([key(:left)], text("Left"))),
            Nearby.on_right(el([key(:right)], text("Right")))
          ],
          text("Host")
        )
      ])

    layout2 =
      column([key(:root)], [
        el(
          [
            key(:host),
            Nearby.above(el([key(:above)], text("Above"))),
            Nearby.on_left(el([key(:left)], text("Left"))),
            Nearby.on_right(el([key(:right)], text("Right"))),
            Nearby.in_front(el([key(:front)], text("Front")))
          ],
          text("Host")
        )
      ])

    {patches, tree1, tree2} = diff_state_native_patch_roundtrip(layout1, layout2)

    host1 = hd(tree1.children)
    host2 = hd(tree2.children)
    expected_mounts = Enum.map(host2.nearby, fn {slot, vnode} -> {slot, vnode.id} end)

    assert Enum.any?(patches, fn
             {:remove, _id} -> true
             _ -> false
           end)

    assert Enum.any?(patches, fn
             {:insert_nearby_subtree, id, _index, :in_front, _subtree} when id == host1.id ->
               true

             _ ->
               false
           end)

    assert Enum.any?(patches, fn
             {:set_nearby_mounts, id, mounts} when id == host1.id ->
               mounts == expected_mounts

             _ ->
               false
           end)
  end

  test "attr-only update keeps all assigned ids stable" do
    layout1 =
      column([key(:root)], [
        el(
          [key(:host), Nearby.above(el([key(:tip)], text("Tip")))],
          el([key(:child)], text("Child"))
        )
      ])

    layout2 =
      column([key(:root)], [
        el(
          [key(:host), padding(8), Nearby.above(el([key(:tip)], text("Tip")))],
          el([key(:child)], text("Child"))
        )
      ])

    {patches, tree1, tree2} = diff_state_native_patch_roundtrip(layout1, layout2)

    ids1 = key_node_id_map(tree1)
    ids2 = key_node_id_map(tree2)

    assert ids1 == ids2

    set_attrs =
      Enum.filter(patches, fn
        {:set_attrs, _, _} -> true
        _ -> false
      end)

    assert [{:set_attrs, id, _attrs}] = set_attrs
    assert id == ids1[:host]

    refute Enum.any?(patches, fn
             {:set_children, _, _} -> true
             {:set_nearby_mounts, _, _} -> true
             {:insert_subtree, _, _, _} -> true
             {:insert_nearby_subtree, _, _, _, _} -> true
             {:remove, _} -> true
             _ -> false
           end)
  end

  defp content_id_map(%Emerge.Engine.Element{children: children}) do
    children
    |> Enum.map(fn child ->
      text = child.children |> hd() |> Map.get(:attrs) |> Map.get(:content)
      {text, child.id}
    end)
    |> Map.new()
  end

  defp key_node_id_map(%Emerge.Engine.Element{} = element) do
    key_entries = if is_nil(element.key), do: [], else: [{element.key, element.id}]

    child_entries = Enum.flat_map(element.children, &key_node_id_map/1)

    nearby_entries =
      Enum.flat_map(element.nearby, fn {_slot, child} ->
        key_node_id_map(child)
      end)

    Map.new(key_entries ++ child_entries ++ nearby_entries)
  end

  defp normalize_patches(patches) do
    Enum.map(patches, &normalize_patch/1)
  end

  defp normalize_patch({:set_attrs, id, attrs}), do: {:set_attrs, id, normalize_attrs(attrs)}
  defp normalize_patch({:set_children, id, children}), do: {:set_children, id, children}

  defp normalize_patch({:insert_subtree, parent, index, subtree}),
    do: {:insert_subtree, parent, index, normalize_element(subtree)}

  defp normalize_patch({:remove, id}), do: {:remove, id}

  defp normalize_element(%Emerge.Engine.Element{} = element) do
    %{
      element
      | key: nil,
        attrs: normalize_attrs(element.attrs),
        children: Enum.map(element.children, &normalize_element/1),
        nearby:
          Enum.map(element.nearby, fn {slot, nearby} -> {slot, normalize_element(nearby)} end)
    }
  end

  defp normalize_attrs(attrs) do
    attrs
    |> Emerge.Engine.Tree.strip_runtime_attrs()
    |> Enum.map(fn {key, value} -> {key, normalize_value(value)} end)
    |> Map.new()
  end

  defp normalize_value(value) when is_number(value), do: value * 1.0

  defp normalize_value(%Emerge.Engine.Element{} = element), do: normalize_element(element)

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

  defp diff_state_native_patch_roundtrip(layout1, layout2) do
    state = Emerge.Engine.DiffState.new()
    {_initial_patch_bin, state, tree1} = Emerge.Engine.DiffState.diff_and_encode(state, layout1)
    {patch_bin, _state, tree2} = Emerge.Engine.DiffState.diff_and_encode(state, layout2)

    tree = EmergeSkia.Native.tree_new()
    full_bin1 = Emerge.Engine.Serialization.encode_tree(tree1)
    full_bin2 = Emerge.Engine.Serialization.encode_tree(tree2)

    upload_roundtrip = unwrap_binary(EmergeSkia.Native.tree_upload_roundtrip(tree, full_bin1))
    expected_upload = unwrap_binary(EmergeSkia.Native.tree_roundtrip(full_bin1))

    assert upload_roundtrip == expected_upload

    patch_roundtrip = unwrap_binary(EmergeSkia.Native.tree_patch_roundtrip(tree, patch_bin))
    expected = unwrap_binary(EmergeSkia.Native.tree_roundtrip(full_bin2))

    assert patch_roundtrip == expected

    {Patch.decode(patch_bin), tree1, tree2}
  end

  defp demo_tree(scroll_y) do
    tree =
      column([key(:root)], [
        row([width(fill())], [
          column([width(px(120)), padding(6)], [
            el([], text("Menu"))
          ]),
          column(
            [
              width(fill()),
              height(fill()),
              padding(8),
              scrollbar_y(),
              Background.color({:color_rgb, {40, 40, 60}}),
              Border.rounded(6)
            ],
            [
              el([Font.size(14), Font.color(:white)], text("Page")),
              el([Font.size(12), Font.color({:color_rgb, {180, 180, 200}})], text("Content"))
            ]
          )
        ])
      ])

    maybe_add_scroll(tree, scroll_y)
  end

  defp maybe_add_scroll(tree, nil), do: tree

  defp maybe_add_scroll(
         %Emerge.Engine.Element{
           children: [%Emerge.Engine.Element{children: [menu, content]} = row]
         } = tree,
         value
       ) do
    updated_content = %{content | attrs: Map.put(content.attrs, :scroll_y, value)}
    %{tree | children: [%{row | children: [menu, updated_content]}]}
  end

  defp unwrap_binary({:ok, bin}) when is_binary(bin), do: bin
  defp unwrap_binary(bin) when is_binary(bin), do: bin
  defp unwrap_binary(other), do: flunk("expected binary, got #{inspect(other)}")
end
