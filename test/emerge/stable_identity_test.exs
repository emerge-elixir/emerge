defmodule Emerge.Engine.StableIdentityTest do
  use ExUnit.Case, async: true
  use Emerge.UI
  alias Emerge.Engine.{DiffState, Patch}

  defp update(state, tree) do
    {bytes, next, _} = DiffState.diff_and_encode(state, tree)
    {^bytes, binary} = DiffState.diff_and_encode_binary(state, tree)
    {^bytes, routed, edits} = DiffState.diff_and_encode_routed(state, tree)
    assert binary.vdom == next.vdom and routed.vdom == next.vdom
    {Patch.decode(bytes), next, edits}
  end

  test "independent keep fields open exactly the changed path" do
    branch = fn n -> column([], List.duplicate(none(), n)) end

    path = fn changed, n ->
      Enum.reduce(7..2//-1, branch.(n), fn i, child ->
        attrs =
          case {i, changed} do
            {2, true} -> [padding(12)]
            {3, _} -> [padding(5)]
            {4, true} -> [spacing(8)]
            {7, true} -> [Font.color(:blue)]
            _ -> []
          end

        column(attrs, [child, branch.(n)])
      end)
    end

    binaries =
      for n <- [1, 1000] do
        state = DiffState.new(path.(false, n))
        {bytes, _} = DiffState.diff_and_encode_binary(state, path.(true, n))
        [{:patch, target, body}] = Patch.decode(bytes)
        assert target == state.vdom.id
        assert <<"EMRG", 10, ^target::64, ^target::64, 6::32, _::binary>> = bytes

        Enum.reduce(2..7, body, fn depth, {:node, _, :column, attrs, edges} ->
          assert attrs ==
                   %{
                     2 => %{padding: 12.0},
                     3 => :same,
                     4 => %{spacing: 8.0},
                     5 => :same,
                     6 => :same,
                     7 => %{font_color: :blue}
                   }[depth]

          if depth == 7 do
            assert edges == :same
          else
            assert [child: path, child: ref] = edges
            assert is_integer(ref)
            path
          end
        end)

        byte_size(bytes)
      end

    assert [n, n] = binaries
    wide = List.duplicate(none(), 10_000)

    {bytes, _} =
      DiffState.diff_and_encode_binary(
        DiffState.new(column([], wide)),
        column([padding(3)], wide)
      )

    raw = Emerge.Engine.AttrCodec.encode_attrs(%{padding: 3})
    len = byte_size(raw)
    assert <<"EMRG", 10, 1::64, 1::64, 1::32, 1::64, 3, 1, ^len::32, ^raw::binary>> = bytes
  end

  test "layout scale changes descendant sites but keeps IDs and the closed wire branch" do
    leaves = List.duplicate(none(), 10_000)
    before = DiffState.new(column([], leaves))
    {bytes, after_scale} = DiffState.diff_and_encode_binary(before, column([scale(1.25)], leaves))
    id = before.vdom.id

    assert [{:patch, ^id, {:node, ^id, :column, %{layout_scale: 1.25}, :same}}] =
             Patch.decode(bytes)

    assert byte_size(bytes) == 50

    for {a, b} <- Enum.zip(before.vdom.children, after_scale.vdom.children) do
      assert a.id == b.id
      <<branch::32, site::32>> = a.stamp
      assert <<^branch::32, new_site::32>> = b.stamp
      refute site == new_site
    end
  end

  test "equal branches have separate occurrences; edges and routes still compare exactly" do
    view = fn order, callback ->
      column([], Enum.map(order, fn k -> el([key(k), Event.on_click(callback)], text("Pay")) end))
    end

    {_, a, _} = update(DiffState.new(), view.([:a, :b], :old))
    [x, y] = a.vdom.children
    assert <<branch::32, site::32>> = x.stamp
    assert <<^branch::32, site2::32>> = y.stamp
    refute x.id == y.id or site == site2
    {[], b, edits} = update(a, view.([:a, :b], :new))
    assert map_size(edits) == 2
    assert b.vdom.stamp == a.vdom.stamp
    id = a.vdom.id
    order = [child: y.id, child: x.id]

    assert {[{:patch, ^id, {:node, ^id, :column, :same, ^order}}], c, %{}} =
             update(b, view.([:b, :a], :new))

    assert c.vdom.stamp == a.vdom.stamp
    assert :erts_debug.same(c.index.keys, a.index.keys)
    twins = [el([], text("Pay")), el([], text("Pay"))]
    state = DiffState.new(column([], twins))
    {[], same, %{}} = update(state, column([], Enum.reverse(twins)))
    assert Enum.map(same.vdom.children, & &1.id) == Enum.map(state.vdom.children, & &1.id)
  end

  test "reusing a key with another kind retires the old occurrence and its route" do
    child = el([key(:item), Event.on_click(:old)], none())
    state = DiffState.new(column([], [child]))
    id = state.index.keys.item
    old_route = <<id::64>>
    replacement = row([key(:item), Event.on_click(:new)], [])

    assert_raise ArgumentError, ~r/duplicate explicit key/, fn ->
      DiffState.diff_and_encode_binary(state, column([], [replacement, child]))
    end

    {[_], next, edits} = update(state, column([], [replacement]))
    new_id = next.index.keys.item
    new_route = <<new_id::64>>
    refute id == new_id or Map.has_key?(next.index.nodes, id)
    assert %{^old_route => nil, ^new_route => %{click: {_, :new}}} = edits
    refute Map.has_key?(next.event_registry, old_route)
    {[_], back, _} = update(next, column([], [child]))
    assert back.index.keys.item > new_id
  end

  test "detached imports cannot claim one supplied ID with two different keys" do
    state = DiffState.new(column([], [row([key(:a)], []), row([key(:b)], [])]))
    [a, b] = state.tree.children
    duplicate = %{state.tree | children: [a, %{b | id: a.id}]}

    assert_raise ArgumentError, ~r/duplicate occurrence ID/, fn ->
      Patch.diff(state.tree, duplicate)
    end

    assert_raise ArgumentError, ~r/duplicate occurrence ID/, fn ->
      Patch.diff(duplicate, state.tree)
    end

    assert [] == Patch.diff(state.tree, state.tree)
  end

  test "stable VNodes share child lists without reusing stale assigned fields" do
    view = fn frame, padding, label ->
      tip = %{text("tip") | frame: frame}
      kids = for s <- ["first", label, "third", "last"], do: %{text(s) | frame: frame}
      %{column([padding(padding), Nearby.above(tip)], kids) | frame: frame}
    end

    box = %{x: 3, y: 4, width: 12, height: 18}
    old = DiffState.new(view.(nil, 1, "second"))
    {_, binary} = DiffState.diff_and_encode_binary(old, view.(nil, 2, "second"))
    assert is_nil(binary.tree)
    assert :erts_debug.same(binary.vdom.children, old.vdom.children)
    assert :erts_debug.same(binary.vdom.nearby, old.vdom.nearby)

    assert {<<>>, assigned, tree} = DiffState.diff_and_encode(binary, view.(box, 2, "second"))
    assert tree === assigned.tree and tree.frame == box
    assert :erts_debug.same(hd(tree.children).attrs, hd(assigned.vdom.children).attrs)
    assert Enum.all?(tree.children, &(&1.frame == box))
    assert [above: %{frame: ^box}] = tree.nearby

    {patch, routed, %{}} = DiffState.diff_and_encode_routed(assigned, view.(nil, 2, "edited"))
    assert is_nil(routed.tree) and byte_size(patch) > 0
    assert :erts_debug.same(Enum.drop(routed.vdom.children, 2), Enum.drop(old.vdom.children, 2))
    {<<>>, _, tree} = DiffState.diff_and_encode(routed, view.(nil, 2, "edited"))
    assert Enum.all?(tree.children, &is_nil(&1.frame))
  end

  test "nested swaps keep claimed routes even when their old wrapper disappears" do
    leaf = el([key(:leaf), Event.on_click(:leaf)], none())
    nest = fn outer, inner -> column([key(outer)], [column([key(inner)], [leaf])]) end

    for {a, b} <- [{:a, :b}, {:b, :a}] do
      state = DiffState.new(el([], nest.(a, b)))
      keys = state.index.keys
      {[_], changed, edits} = update(state, nest.(b, a))
      assert edits == %{} and changed.index.keys == keys
      assert changed.vdom.id == keys[b] and hd(changed.vdom.children).id == keys[a]
      refute Map.has_key?(changed.index.nodes, state.vdom.id)
      {[_], cleared, deleted} = update(changed, nil)

      assert map_size(deleted) == 1 and cleared.index.nodes == %{} and
               cleared.index.branch.values == %{}

      {[_], mounted, _} = update(cleared, leaf)
      assert mounted.vdom.id >= changed.next_id
    end
  end

  test "a forward scroll binding uses the final key index, never the old index" do
    tree = fn keys ->
      column([], [
        column([key(:owner), scroll_y(:target)], []),
        column([key(:host)], Enum.map(keys, &el([key(&1)], none())))
      ])
    end

    state = DiffState.new(tree.([:target]))
    token = hd(state.vdom.children).attrs.scroll_position_y.declaration
    assert hd(state.vdom.children).attrs.scroll_position_y.id == state.index.keys[:target]
    {[_], absent, _} = update(state, tree.([]))
    assert %{id: 0, declaration: ^token} = hd(absent.vdom.children).attrs.scroll_position_y
    {[_], back, _} = update(absent, tree.([:target]))
    assert hd(back.vdom.children).attrs.scroll_position_y.id == back.index.keys[:target]
    refute back.index.keys[:target] == state.index.keys[:target]
  end

  test "pending siblings close in declaration order against the complete key index" do
    scene = fn targets ->
      column([], [
        el([key(:first), scroll_x(:a)], none()),
        el([key(:second), scroll_y(:b)], none()),
        column([key(:targets)], Enum.map(targets, &el([key(&1)], none())))
      ])
    end

    initial = DiffState.new(scene.([:a, :b]))
    [x, y, _] = initial.vdom.children
    assert x.attrs.scroll_position_x.id == initial.index.keys.a
    assert y.attrs.scroll_position_y.id == initial.index.keys.b
    # Both distinct resolved descriptors are new, so the first gets the earlier token.
    assert x.own < y.own

    {[_], missing, _} = update(initial, scene.([]))
    [x, y, _] = missing.vdom.children
    assert x.attrs.scroll_position_x.id == 0 and y.attrs.scroll_position_y.id == 0
    {[_], back, _} = update(missing, scene.([:b, :a]))
    [x, y, _] = back.vdom.children
    assert x.attrs.scroll_position_x.id == back.index.keys.a
    assert y.attrs.scroll_position_y.id == back.index.keys.b
  end

  test "a changed sibling does not change the declared site of its neighbour" do
    view = fn w, size -> column([Font.size(size)], [el([width(px(w))], none()), text("Pay")]) end
    a = DiffState.new(view.(10, 16))
    {_, b, _} = update(a, view.(20, 16))
    assert List.last(a.vdom.children).stamp == List.last(b.vdom.children).stamp
    {_, c, _} = update(b, view.(20, 24))
    assert <<branch::32, site::32>> = List.last(b.vdom.children).stamp
    assert <<^branch::32, other::32>> = List.last(c.vdom.children).stamp
    refute site == other
  end

  test "descriptor references released twice and reclaimed in one update survive exactly once" do
    alias Emerge.Engine.Reconcile.Labels
    {id, pool} = Labels.bind(%Labels{}, nil, :red)
    {^id, pool} = Labels.bind(pool, nil, :red)
    pool = pool |> Labels.release(id) |> Labels.release(id)
    {^id, pool} = Labels.bind(pool, nil, :red)
    pool = Labels.finish(pool)
    assert pool.values == %{red: {id, 1}} and pool.retired == []
    pool = pool |> Labels.release(id) |> Labels.finish()
    assert pool.values == %{} and pool.by_token == %{}
  end

  test "unequal colliding descriptors stay distinct and token exhaustion cannot wrap" do
    alias Emerge.Engine.Reconcile.Labels
    # Find two different local values with the same BEAM hash, not just the
    # same user-visible __attrs_hash. Interning still compares exact terms.
    {a, b} =
      Enum.reduce_while(1..100_000, %{}, fn n, seen ->
        val = <<n::32>>
        hash = :erlang.phash2(val)

        case Map.fetch(seen, hash) do
          {:ok, other} -> {:halt, {other, val}}
          :error -> {:cont, Map.put(seen, hash, val)}
        end
      end)

    assert a != b and :erlang.phash2(a) == :erlang.phash2(b)
    {id, labels} = Labels.bind(%Labels{}, nil, a)
    {other, labels} = Labels.bind(labels, nil, b)
    refute id == other
    assert {^id, _} = Labels.bind(labels, nil, a)

    assert_raise ArgumentError, ~r/exhausted/, fn ->
      Labels.bind(%{labels | next: 0x100000000}, nil, :new)
    end

    assert_raise ArgumentError, ~r/exhausted/, fn ->
      DiffState.diff_and_encode_binary(%{DiffState.new() | next_id: 0x10000000000000000}, none())
    end
  end

  test "sparse declarations replay against the preceding state, including Nearby and new owners" do
    leaf = fn message -> el([key(:label), Event.on_click(message)], text("Pay")) end
    scene = fn k, content -> column([key(:root)], [column([key(k)], content)]) end
    initial = scene.(:left, [leaf.(:one)])

    steps = [
      initial,
      scene.(:new, [leaf.(:two)]),
      leaf.(:two),
      el([key(:outer), Nearby.above(leaf.(:two))], none()),
      el([key(:outer), Nearby.below(leaf.(:three))], none()),
      scene.(:left, []),
      scene.(:left, [leaf.(:four)]),
      nil,
      initial
    ]

    assert_replays(steps)
  end

  test "simultaneous edits, moves and deletions replay on edit and undo" do
    :rand.seed(:exsss, {7, 13, 31})

    steps =
      for i <- 1..24 do
        {left, right} =
          1..9
          |> Enum.shuffle()
          |> Enum.take(:rand.uniform(9))
          |> Enum.map(fn k -> el([key(k), Event.on_click({i, k})], text("#{rem(i + k, 3)}")) end)
          |> Enum.split(:rand.uniform(5) - 1)

        row([], [column([key(:left)], left), column([key(:right)], right)])
      end

    assert_replays(steps ++ Enum.reverse(steps))
  end

  defp assert_replays(steps) do
    Enum.reduce(steps, {DiffState.new(), nil}, fn tree, {state, previous} ->
      {ops, next, _} = update(state, tree)

      replay =
        case ops do
          [] ->
            previous

          [{:patch, target, sparse}] ->
            old = Map.new(flatten(previous), fn n -> {elem(n, 1), n} end)
            replacement = expand(sparse, old)
            if target == 0, do: replacement, else: replace(previous, target, replacement)
        end

      expected =
        if tree do
          [{:patch, 0, body}] =
            next.tree |> Emerge.Engine.Serialization.encode_tree() |> Patch.decode()

          body
        end

      assert replay == expected
      {next, replay}
    end)
  end

  # Independent pure declaration oracle, not a renderer or a hit oracle.
  defp flatten(nil), do: []

  defp flatten({:node, _, _, _, kids} = n),
    do: [n | Enum.flat_map(kids, fn {_, c} -> flatten(c) end)]

  defp expand(nil, _), do: nil
  defp expand(id, old) when is_integer(id), do: Map.fetch!(old, id)

  defp expand({:node, id, kind, attrs, kids}, old) do
    attrs = if attrs == :same, do: elem(Map.fetch!(old, id), 3), else: attrs

    kids =
      if kids == :same,
        do: elem(Map.fetch!(old, id), 4),
        else: Enum.map(kids, fn {tag, c} -> {tag, expand(c, old)} end)

    {:node, id, kind, attrs, kids}
  end

  defp replace({:node, id, _, _, _}, id, replacement), do: replacement

  defp replace({:node, id, kind, attrs, kids}, target, replacement) do
    {:node, id, kind, attrs,
     for({tag, c} <- kids, n = replace(c, target, replacement), do: {tag, n})}
  end

  test "an intact card moves by reference and promotes by reference" do
    card = el([key(:card), padding(2)], text("Pay"))

    view = fn left, right ->
      row([], [column([key(:left)], left), column([key(:right)], right)])
    end

    {_, a, _} = update(DiffState.new(), view.([card], []))
    [left, right] = a.vdom.children
    [before] = left.children
    id = before.id
    root = a.vdom.id
    l = left.id
    r = right.id

    {[
       {:patch, ^root,
        {:node, ^root, :row, :same,
         [
           child: {:node, ^l, :column, :same, []},
           child: {:node, ^r, :column, :same, [child: ^id]}
         ]}}
     ], b, _} = update(a, view.([], [card]))

    <<branch::32, site::32>> = before.stamp
    [_, %{children: [moved]}] = b.vdom.children
    assert <<^branch::32, new_site::32>> = moved.stamp
    refute site == new_site
    assert hd(moved.children).id == hd(before.children).id

    assert {[{:patch, ^root, ^id}], %{vdom: %{id: ^id, stamp: <<^branch::32, 0::32>>}}, %{}} =
             update(b, card)
  end
end
