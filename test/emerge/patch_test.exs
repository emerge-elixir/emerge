defmodule Emerge.Engine.PatchTest do
  use ExUnit.Case, async: true
  use Emerge.UI
  alias Emerge.Engine.{DiffState, Patch, Serialization}
  alias Emerge.UI.{Background, Border, Font}
  alias EmergeSkia.Native

  defp list(keys, attrs \\ []) do
    row(
      [key(:root)],
      Enum.map(keys, fn k -> el([key(k) | Keyword.get(attrs, k, [])], text(to_string(k))) end)
    )
  end

  # Exercise both public encoders and the actual stateful native reader. A
  # full final declaration is a cold control, never a fallback during patch.
  defp replay(a, b) do
    {first, old, before} = DiffState.diff_and_encode(DiffState.new(), a)
    {bytes, _, after_tree} = DiffState.diff_and_encode(old, b)
    ops = Patch.decode(bytes)
    assert first == Serialization.encode_tree(before)
    assert Patch.encode(ops) == bytes
    assert Patch.encode(Patch.diff(before, after_tree)) == bytes
    tree = Native.tree_new()
    assert {:ok, ^first} = Native.tree_upload_roundtrip(tree, first)
    assert {:ok, patched} = Native.tree_patch_roundtrip(tree, bytes)
    assert {:ok, ^patched} = Native.tree_roundtrip(Serialization.encode_tree(after_tree))
    {ops, before, after_tree}
  end

  defp sparse_order(before, next, op, get_edges) do
    old = Map.new(get_edges.(before), fn {_, c} -> {c.id, c} end)
    expected = get_edges.(next)
    id = before.id
    assert {:patch, ^id, {:node, ^id, _, :same, edges}} = op
    assert length(edges) == length(expected)

    Enum.zip_with(edges, expected, fn {tag, body}, {expected_tag, child} ->
      assert tag == expected_tag

      if Map.has_key?(old, child.id) do
        assert body == child.id
      else
        assert {:node, id, _, attrs, kids} = body
        assert id == child.id and is_map(attrs) and is_list(kids)
      end
    end)
  end

  for {name, before, after_keys} <- [
        {"keyed reorder references the final sequence", [:a, :b, :c], [:c, :a, :b]},
        {"keyed tail insertion defines only the new branch", [:a, :b], [:a, :b, :c]},
        {"keyed removal omits the old edge", [:a, :b, :c], [:a, :c]},
        {"insert and reorder reference existing occurrences", [:a, :b, :c], [:b, :a, :c, :d]},
        {"insert preserves survivor order", [:a, :b, :c], [:a, :b, :c, :d]},
        {"multiple insertions preserve survivor order", [:a, :b, :c], [:a, :b, :x, :c, :y]},
        {"remove and insert replay without reordering survivors", [:a, :b, :c, :d],
         [:a, :c, :d, :x]},
        {"multiple removes and inserts replay without reordering survivors", [:a, :b, :c, :d, :e],
         [:a, :d, :e, :x, :y]},
        {"multiple inserts with a reorder replay", [:a, :b, :c, :d], [:b, :a, :x, :c, :y, :d]},
        {"removing the last child opens an explicit empty list", [:a], []}
      ] do
    test name do
      {[op], before, next} = replay(list(unquote(before)), list(unquote(after_keys)))
      sparse_order(before, next, op, &Patch.edges(&1, fn c -> c end))
      survivors = Map.new(before.children, &{&1.key, &1.id})
      for c <- next.children, Map.has_key?(survivors, c.key), do: assert(c.id == survivors[c.key])
    end
  end

  test "attribute change and insertion have one enclosing replacement" do
    {[op], before, _} = replay(list([:a]), list([:a, :b], a: [padding(8)]))
    id = before.id
    a = hd(before.children).id

    assert {:patch, ^id,
            {:node, ^id, :row, :same,
             [
               child: {:node, ^a, :el, %{padding: 8.0}, :same},
               child: {:node, _, :el, %{}, [child: {:node, _, :text, _, []}]}
             ]}} = op
  end

  test "attribute change targets exactly that occurrence and no child IDs" do
    {[op], before, _} = replay(list([:a, :b]), list([:a, :b], a: [padding(4)]))
    id = hd(before.children).id
    assert {:patch, ^id, {:node, ^id, :el, %{padding: 4.0}, :same}} = op
  end

  test "no patches when tree is identical" do
    assert {[], _, _} = replay(list([:a, :b]), list([:a, :b]))
    assert Patch.encode([]) == <<>>
  end

  test "unkeyed reorder edits three leaf payloads without changing occurrence IDs" do
    view = fn chars -> row([], Enum.map(chars, &el([], text(&1)))) end

    {[{:patch, _, {:node, _, :row, :same, edges}}], before, next} =
      replay(view.(["a", "b", "c"]), view.(["c", "a", "b"]))

    assert Enum.map(before.children, & &1.id) == Enum.map(next.children, & &1.id)

    assert for(
             {:child, {:node, _, :el, :same, [child: {:node, _, :text, %{content: s}, :same}]}} <-
               edges,
             do: s
           ) == ["c", "a", "b"]
  end

  test "mixed keyed/unkeyed reorder raises" do
    assert_raise ArgumentError, ~r/All siblings must have key/, fn ->
      DiffState.new(row([], [el([key(:a)], none()), none()]))
    end
  end

  test "vacant, empty, reference, and both independent keep fields roundtrip" do
    for {target, body} <- [
          {0, {:node, 2, :el, %{}, []}},
          {1, nil},
          {1, 17},
          {2, {:node, 2, :el, %{}, :same}},
          {2, {:node, 2, :el, :same, []}}
        ] do
      op = Patch.patch(target, body)
      assert [^op] = Patch.decode(Patch.encode(op))
    end
  end

  # Keep the names of outstanding native Nearby regressions from the v9
  # checkpoint. Their assertions describe the sparse operation, not opcodes.
  for {name, a, b} <- [
        {"nearby slot change preserves keyed node id and emits set_nearby_mounts only",
         [above: :tip], [below: :tip]},
        {"adding keyed nearby emits insert_nearby_subtree without set_nearby_mounts", [],
         [above: :tip]},
        {"removing keyed nearby emits remove without set_nearby_mounts", [above: :tip], []},
        {"nearby keyed reorder emits set_nearby_mounts without inserts or removes",
         [above: :above, below: :below], [below: :below, above: :above]},
        {"nearby remove and insert without survivor reordering emits final order and roundtrips natively",
         [above: :above, below: :below, on_left: :left, on_right: :right],
         [above: :above, on_left: :left, on_right: :right, in_front: :front]}
      ] do
    test name do
      host = fn mounts ->
        attrs =
          Enum.map(mounts, fn {tag, k} ->
            apply(Nearby, tag, [el([key(k)], text(to_string(k)))])
          end)

        column([key(:root)], [el([key(:host) | attrs], text("Host"))])
      end

      {[op], before, next} = replay(host.(unquote(a)), host.(unquote(b)))
      sparse_order(hd(before.children), hd(next.children), op, &Patch.edges(&1, fn c -> c end))
    end
  end

  test "attr-only update keeps all assigned ids stable" do
    view = fn attrs ->
      column([key(:root)], [
        el(
          [key(:host), Nearby.above(el([key(:tip)], text("Tip"))) | attrs],
          el([key(:child)], text("Child"))
        )
      ])
    end

    {[op], before, next} = replay(view.([]), view.([padding(8)]))
    host = hd(before.children)
    id = host.id
    assert hd(next.children).nearby == host.nearby
    assert hd(next.children).children == host.children
    assert {:patch, ^id, {:node, ^id, :el, %{padding: 8.0}, :same}} = op
  end

  test "stateful patch roundtrip matches full tree" do
    content =
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

    view = fn content ->
      column([key(:root)], [
        row([width(fill())], [
          column([width(px(120)), padding(6)], [el([], text("Menu"))]),
          content
        ])
      ])
    end

    replay(view.(%{content | attrs: Map.put(content.attrs, :scroll_y, 40.0)}), view.(content))
  end
end
