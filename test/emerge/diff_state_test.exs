defmodule Emerge.DiffStateTest do
  use ExUnit.Case, async: true

  import Emerge.UI

  test "diff_state_update returns patches and updated state" do
    state = Emerge.diff_state_new()

    layout =
      row([key(:root)], [
        el([key(:left)], text("left")),
        el([key(:right)], text("right"))
      ])

    {bin, next_state, assigned} = Emerge.diff_state_update(state, layout)

    assert is_binary(bin)
    assert assigned.id == next_state.tree.id
  end

  test "encode_full returns a full-tree binary and updates state" do
    state = Emerge.diff_state_new()

    layout =
      column([key(:root)], [
        el([key(:a)], text("one"))
      ])

    {bin, next_state, assigned} = Emerge.encode_full(state, layout)

    assert is_binary(bin)
    assert assigned.id == next_state.tree.id
  end

  test "encode_full_with_empty_patch returns empty patch stream" do
    state = Emerge.diff_state_new()

    layout =
      row([key(:root)], [
        el([key(:left)], text("left"))
      ])

    {full_bin, patch_bin, _state, _assigned} = Emerge.encode_full_with_empty_patch(state, layout)

    assert is_binary(full_bin)
    assert patch_bin == <<>>
  end

  test "reordering keyed siblings preserves ids" do
    state = Emerge.diff_state_new()

    layout1 =
      row([key(:root)], [
        el([key(:a)], text("a")),
        el([key(:b)], text("b")),
        el([key(:c)], text("c"))
      ])

    {_bin1, state, assigned1} = Emerge.diff_state_update(state, layout1)

    layout2 =
      row([key(:root)], [
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

  test "keyed preserves ids on reorder" do
    state = Emerge.diff_state_new()

    layout1 =
      row([key(:root)], [
        el([key(:a)], text("a")),
        el([key(:b)], text("b")),
        el([key(:c)], text("c"))
      ])

    {_bin1, state, assigned1} = Emerge.diff_state_update(state, layout1)

    layout2 =
      row([key(:root)], [
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

  test "duplicate keys raise" do
    state = Emerge.diff_state_new()

    layout =
      row([key(:root)], [
        el([key(:dup)], text("a")),
        el([key(:dup)], text("b"))
      ])

    assert_raise ArgumentError, ~r/duplicate explicit id\/key/, fn ->
      Emerge.diff_state_update(state, layout)
    end
  end

  test "mixed keyed and unkeyed siblings raise" do
    state = Emerge.diff_state_new()

    layout =
      row([key(:root)], [
        el([key(:a)], text("a")),
        el(text("u1")),
        el([key(:b)], text("b"))
      ])

    assert_raise ArgumentError, ~r/All siblings must have key/, fn ->
      Emerge.diff_state_update(state, layout)
    end
  end

  test "keyed insert/remove keeps existing ids stable" do
    state = Emerge.diff_state_new()

    layout1 =
      column([key(:root)], [
        el([key(:a)], text("a")),
        el([key(:b)], text("b")),
        el([key(:c)], text("c"))
      ])

    {_bin1, state, assigned1} = Emerge.diff_state_update(state, layout1)

    layout2 =
      column([key(:root)], [
        el([key(:c)], text("c")),
        el([key(:b)], text("b")),
        el([key(:d)], text("d"))
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
      row([key(:root)], [
        column([key(:left)], [
          el([key(:dup)], text("left"))
        ]),
        column([key(:right)], [
          el([key(:dup)], text("right"))
        ])
      ])

    assert_raise ArgumentError, ~r/duplicate explicit id\/key/, fn ->
      Emerge.diff_state_update(state, layout)
    end
  end

  test "unkeyed reorder changes ids" do
    state = Emerge.diff_state_new()

    layout1 =
      row([key(:root)], [
        el(text("a")),
        el(text("b")),
        el(text("c"))
      ])

    {_bin1, state, assigned1} = Emerge.diff_state_update(state, layout1)

    layout2 =
      row([key(:root)], [
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

  test "mixed insert with keys and unkeyed raises" do
    state = Emerge.diff_state_new()

    layout1 =
      row([key(:root)], [
        el([key(:a)], text("a")),
        el(text("u1")),
        el([key(:b)], text("b"))
      ])

    assert_raise ArgumentError, ~r/All siblings must have key/, fn ->
      Emerge.diff_state_update(state, layout1)
    end
  end

  test "on_change is registered in event registry" do
    layout = Emerge.UI.Input.text("hello", [key(:field), on_change({self(), :changed})])
    state = Emerge.diff_state_new(layout)
    id_bin = :erlang.term_to_binary(state.tree.id)

    assert {:ok, {pid, :changed}} = Emerge.lookup_event(state, id_bin, :change)
    assert pid == self()
  end

  test "on_focus and on_blur are registered in event registry" do
    layout =
      Emerge.UI.Input.text("hello", [
        key(:field),
        on_focus({self(), :focused}),
        on_blur({self(), :blurred})
      ])

    state = Emerge.diff_state_new(layout)
    id_bin = :erlang.term_to_binary(state.tree.id)

    assert {:ok, {focus_pid, :focused}} = Emerge.lookup_event(state, id_bin, :focus)
    assert {:ok, {blur_pid, :blurred}} = Emerge.lookup_event(state, id_bin, :blur)
    assert focus_pid == self()
    assert blur_pid == self()
  end

  test "text input registers click and mouse handlers alongside on_change" do
    layout =
      Emerge.UI.Input.text("hello", [
        key(:field),
        on_click({self(), :clicked}),
        on_mouse_enter({self(), :entered}),
        on_mouse_leave({self(), :left}),
        on_mouse_move({self(), :moved}),
        on_change({self(), :changed})
      ])

    state = Emerge.diff_state_new(layout)
    id_bin = :erlang.term_to_binary(state.tree.id)

    assert {:ok, {_, :clicked}} = Emerge.lookup_event(state, id_bin, :click)
    assert {:ok, {_, :entered}} = Emerge.lookup_event(state, id_bin, :mouse_enter)
    assert {:ok, {_, :left}} = Emerge.lookup_event(state, id_bin, :mouse_leave)
    assert {:ok, {_, :moved}} = Emerge.lookup_event(state, id_bin, :mouse_move)
    assert {:ok, {_, :changed}} = Emerge.lookup_event(state, id_bin, :change)
  end

  test "dispatch_event with payload appends payload to tuple message" do
    layout =
      Emerge.UI.Input.text("hello", [key(:field), on_change({self(), {:changed, :field}})])

    state = Emerge.diff_state_new(layout)
    id_bin = :erlang.term_to_binary(state.tree.id)

    assert :ok == Emerge.dispatch_event(state, id_bin, :change, "hello!")
    assert_receive {:changed, :field, "hello!"}
  end

  test "dispatch_event with payload wraps non-tuple message" do
    layout = Emerge.UI.Input.text("hello", [key(:field), on_change({self(), :changed})])
    state = Emerge.diff_state_new(layout)
    id_bin = :erlang.term_to_binary(state.tree.id)

    assert :ok == Emerge.dispatch_event(state, id_bin, :change, "hello!")
    assert_receive {:changed, "hello!"}
  end

  test "dispatch_event routes focus and blur events" do
    layout =
      Emerge.UI.Input.text("hello", [
        key(:field),
        on_focus({self(), :focused}),
        on_blur({self(), :blurred})
      ])

    state = Emerge.diff_state_new(layout)
    id_bin = :erlang.term_to_binary(state.tree.id)

    assert :ok == Emerge.dispatch_event(state, id_bin, :focus)
    assert_receive :focused

    assert :ok == Emerge.dispatch_event(state, id_bin, :blur)
    assert_receive :blurred
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
