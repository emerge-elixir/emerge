defmodule Emerge.DiffStateTest do
  use ExUnit.Case, async: true

  import Emerge.UI

  test "diff_state_update returns patches and updated state" do
    state = Emerge.diff_state_new()

    layout =
      row([id: :root], [
        el([id: :left], text("left")),
        el([id: :right], text("right"))
      ])

    {bin, next_state, assigned} = Emerge.diff_state_update(state, layout)

    assert is_binary(bin)
    assert assigned.id == next_state.tree.id
  end

  test "encode_full returns a full-tree binary and updates state" do
    state = Emerge.diff_state_new()

    layout =
      column([id: :root], [
        el([id: :a], text("one"))
      ])

    {bin, next_state, assigned} = Emerge.encode_full(state, layout)

    assert is_binary(bin)
    assert assigned.id == next_state.tree.id
  end

  test "encode_full_with_empty_patch returns empty patch stream" do
    state = Emerge.diff_state_new()

    layout =
      row([id: :root], [
        el([id: :left], text("left"))
      ])

    {full_bin, patch_bin, _state, _assigned} = Emerge.encode_full_with_empty_patch(state, layout)

    assert is_binary(full_bin)
    assert patch_bin == <<>>
  end

  test "reordering keyed siblings preserves ids" do
    state = Emerge.diff_state_new()

    layout1 =
      row([id: :root], [
        el([key(:a)], text("a")),
        el([key(:b)], text("b")),
        el([key(:c)], text("c"))
      ])

    {_bin1, state, assigned1} = Emerge.diff_state_update(state, layout1)

    layout2 =
      row([id: :root], [
        el([key(:c)], text("c")),
        el([key(:a)], text("a")),
        el([key(:b)], text("b"))
      ])

    {_bin2, _state, assigned2} = Emerge.diff_state_update(state, layout2)

    ids1 = content_id_map(assigned1)
    ids2 = content_id_map(assigned2)

    assert ids1["a"] == ids2["a"]
    assert ids1["b"] == ids2["b"]
    assert ids1["c"] == ids2["c"]
  end

  test "keyed helper preserves ids on reorder" do
    state = Emerge.diff_state_new()

    layout1 =
      row([id: :root], [
        keyed(:a, el(text("a"))),
        keyed(:b, el(text("b"))),
        keyed(:c, el(text("c")))
      ])

    {_bin1, state, assigned1} = Emerge.diff_state_update(state, layout1)

    layout2 =
      row([id: :root], [
        keyed(:c, el(text("c"))),
        keyed(:a, el(text("a"))),
        keyed(:b, el(text("b")))
      ])

    {_bin2, _state, assigned2} = Emerge.diff_state_update(state, layout2)

    ids1 = content_id_map(assigned1)
    ids2 = content_id_map(assigned2)

    assert ids1["a"] == ids2["a"]
    assert ids1["b"] == ids2["b"]
    assert ids1["c"] == ids2["c"]
  end

  test "duplicate keys raise" do
    state = Emerge.diff_state_new()

    layout =
      row([id: :root], [
        keyed(:dup, el(text("a"))),
        keyed(:dup, el(text("b")))
      ])

    assert_raise ArgumentError, ~r/duplicate explicit id\/key/, fn ->
      Emerge.diff_state_update(state, layout)
    end
  end

  test "mixed keyed and unkeyed reorder keeps keyed ids stable" do
    state = Emerge.diff_state_new()

    layout1 =
      row([id: :root], [
        keyed(:a, el(text("a"))),
        el(text("u1")),
        keyed(:b, el(text("b")))
      ])

    {_bin1, state, assigned1} = Emerge.diff_state_update(state, layout1)

    layout2 =
      row([id: :root], [
        el(text("u1")),
        keyed(:b, el(text("b"))),
        keyed(:a, el(text("a")))
      ])

    {_bin2, _state, assigned2} = Emerge.diff_state_update(state, layout2)

    ids1 = content_id_map(assigned1)
    ids2 = content_id_map(assigned2)

    assert ids1["a"] == ids2["a"]
    assert ids1["b"] == ids2["b"]
    refute ids1["u1"] == ids2["u1"]
  end

  test "keyed insert/remove keeps existing ids stable" do
    state = Emerge.diff_state_new()

    layout1 =
      column([id: :root], [
        keyed(:a, el(text("a"))),
        keyed(:b, el(text("b"))),
        keyed(:c, el(text("c")))
      ])

    {_bin1, state, assigned1} = Emerge.diff_state_update(state, layout1)

    layout2 =
      column([id: :root], [
        keyed(:c, el(text("c"))),
        keyed(:b, el(text("b"))),
        keyed(:d, el(text("d")))
      ])

    {_bin2, _state, assigned2} = Emerge.diff_state_update(state, layout2)

    ids1 = content_id_map(assigned1)
    ids2 = content_id_map(assigned2)

    assert ids1["b"] == ids2["b"]
    assert ids1["c"] == ids2["c"]
    refute Map.has_key?(ids2, "a")
    assert Map.has_key?(ids2, "d")
  end

  test "duplicate keys across the tree raise" do
    state = Emerge.diff_state_new()

    layout =
      row([id: :root], [
        column([key(:left)], [
          keyed(:dup, el(text("left")))
        ]),
        column([key(:right)], [
          keyed(:dup, el(text("right")))
        ])
      ])

    assert_raise ArgumentError, ~r/duplicate explicit id\/key/, fn ->
      Emerge.diff_state_update(state, layout)
    end
  end

  test "unkeyed reorder changes ids" do
    state = Emerge.diff_state_new()

    layout1 =
      row([id: :root], [
        el(text("a")),
        el(text("b")),
        el(text("c"))
      ])

    {_bin1, state, assigned1} = Emerge.diff_state_update(state, layout1)

    layout2 =
      row([id: :root], [
        el(text("c")),
        el(text("a")),
        el(text("b"))
      ])

    {_bin2, _state, assigned2} = Emerge.diff_state_update(state, layout2)

    ids1 = content_id_map(assigned1)
    ids2 = content_id_map(assigned2)

    refute ids1["a"] == ids2["a"]
    refute ids1["b"] == ids2["b"]
    refute ids1["c"] == ids2["c"]
  end

  test "mixed insert keeps keyed ids stable" do
    state = Emerge.diff_state_new()

    layout1 =
      row([id: :root], [
        keyed(:a, el(text("a"))),
        el(text("u1")),
        keyed(:b, el(text("b")))
      ])

    {_bin1, state, assigned1} = Emerge.diff_state_update(state, layout1)

    layout2 =
      row([id: :root], [
        el(text("u2")),
        keyed(:a, el(text("a"))),
        el(text("u1")),
        keyed(:b, el(text("b")))
      ])

    {_bin2, _state, assigned2} = Emerge.diff_state_update(state, layout2)

    ids1 = content_id_map(assigned1)
    ids2 = content_id_map(assigned2)

    assert ids1["a"] == ids2["a"]
    assert ids1["b"] == ids2["b"]
  end

  defp content_id_map(%Emerge.Element{children: children}) do
    children
    |> Enum.map(fn child ->
      text = child.children |> hd() |> Map.get(:attrs) |> Map.get(:content)
      {text, child.id}
    end)
    |> Map.new()
  end
end
