defmodule Emerge.Engine.DiffStateTest do
  use ExUnit.Case, async: true

  use Emerge.UI

  test "diff_state_update returns patches and updated state" do
    state = Emerge.Engine.diff_state_new()

    layout =
      row([key(:root)], [
        el([key(:left)], text("left")),
        el([key(:right)], text("right"))
      ])

    {bin, next_state, assigned} = Emerge.Engine.diff_state_update(state, layout)

    assert is_binary(bin)
    assert assigned.id == next_state.tree.id
  end

  test "encode_full returns a full-tree binary and updates state" do
    state = Emerge.Engine.diff_state_new()

    layout =
      column([key(:root)], [
        el([key(:a)], text("one"))
      ])

    {bin, next_state, assigned} = Emerge.Engine.encode_full(state, layout)

    assert is_binary(bin)
    assert assigned.id == next_state.tree.id
  end

  test "encode_full_with_empty_patch returns empty patch stream" do
    state = Emerge.Engine.diff_state_new()

    layout =
      row([key(:root)], [
        el([key(:left)], text("left"))
      ])

    {full_bin, patch_bin, _state, _assigned} =
      Emerge.Engine.encode_full_with_empty_patch(state, layout)

    assert is_binary(full_bin)
    assert patch_bin == <<>>
  end

  test "reordering keyed siblings preserves ids" do
    state = Emerge.Engine.diff_state_new()

    layout1 =
      row([key(:root)], [
        el([key(:a)], text("a")),
        el([key(:b)], text("b")),
        el([key(:c)], text("c"))
      ])

    {_bin1, state, assigned1} = Emerge.Engine.diff_state_update(state, layout1)

    layout2 =
      row([key(:root)], [
        el([key(:c)], text("c")),
        el([key(:a)], text("a")),
        el([key(:b)], text("b"))
      ])

    {_bin2, _state, assigned2} = Emerge.Engine.diff_state_update(state, layout2)

    ids1 = content_id_map(assigned1)
    ids2 = content_id_map(assigned2)

    assert ids1["a"] == ids2["a"]
    assert ids1["b"] == ids2["b"]
    assert ids1["c"] == ids2["c"]
  end

  test "keyed preserves ids on reorder" do
    state = Emerge.Engine.diff_state_new()

    layout1 =
      row([key(:root)], [
        el([key(:a)], text("a")),
        el([key(:b)], text("b")),
        el([key(:c)], text("c"))
      ])

    {_bin1, state, assigned1} = Emerge.Engine.diff_state_update(state, layout1)

    layout2 =
      row([key(:root)], [
        el([key(:c)], text("c")),
        el([key(:a)], text("a")),
        el([key(:b)], text("b"))
      ])

    {_bin2, _state, assigned2} = Emerge.Engine.diff_state_update(state, layout2)

    ids1 = content_id_map(assigned1)
    ids2 = content_id_map(assigned2)

    assert ids1["a"] == ids2["a"]
    assert ids1["b"] == ids2["b"]
    assert ids1["c"] == ids2["c"]
  end

  test "duplicate keys raise" do
    state = Emerge.Engine.diff_state_new()

    layout =
      row([key(:root)], [
        el([key(:dup)], text("a")),
        el([key(:dup)], text("b"))
      ])

    assert_raise ArgumentError, ~r/duplicate explicit id\/key/, fn ->
      Emerge.Engine.diff_state_update(state, layout)
    end
  end

  test "mixed keyed and unkeyed siblings raise" do
    state = Emerge.Engine.diff_state_new()

    layout =
      row([key(:root)], [
        el([key(:a)], text("a")),
        el([], text("u1")),
        el([key(:b)], text("b"))
      ])

    assert_raise ArgumentError, ~r/All siblings must have key/, fn ->
      Emerge.Engine.diff_state_update(state, layout)
    end
  end

  test "keyed insert/remove keeps existing ids stable" do
    state = Emerge.Engine.diff_state_new()

    layout1 =
      column([key(:root)], [
        el([key(:a)], text("a")),
        el([key(:b)], text("b")),
        el([key(:c)], text("c"))
      ])

    {_bin1, state, assigned1} = Emerge.Engine.diff_state_update(state, layout1)

    layout2 =
      column([key(:root)], [
        el([key(:c)], text("c")),
        el([key(:b)], text("b")),
        el([key(:d)], text("d"))
      ])

    {_bin2, _state, assigned2} = Emerge.Engine.diff_state_update(state, layout2)

    ids1 = content_id_map(assigned1)
    ids2 = content_id_map(assigned2)

    assert ids1["b"] == ids2["b"]
    assert ids1["c"] == ids2["c"]
    refute Map.has_key?(ids2, "a")
    assert Map.has_key?(ids2, "d")
  end

  test "duplicate keys across the tree raise" do
    state = Emerge.Engine.diff_state_new()

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
      Emerge.Engine.diff_state_update(state, layout)
    end
  end

  test "unkeyed reorder changes ids" do
    state = Emerge.Engine.diff_state_new()

    layout1 =
      row([key(:root)], [
        el([], text("a")),
        el([], text("b")),
        el([], text("c"))
      ])

    {_bin1, state, assigned1} = Emerge.Engine.diff_state_update(state, layout1)

    layout2 =
      row([key(:root)], [
        el([], text("c")),
        el([], text("a")),
        el([], text("b"))
      ])

    {_bin2, _state, assigned2} = Emerge.Engine.diff_state_update(state, layout2)

    ids1 = content_id_map(assigned1)
    ids2 = content_id_map(assigned2)

    refute ids1["a"] == ids2["a"]
    refute ids1["b"] == ids2["b"]
    refute ids1["c"] == ids2["c"]
  end

  test "mixed insert with keys and unkeyed raises" do
    state = Emerge.Engine.diff_state_new()

    layout1 =
      row([key(:root)], [
        el([key(:a)], text("a")),
        el([], text("u1")),
        el([key(:b)], text("b"))
      ])

    assert_raise ArgumentError, ~r/All siblings must have key/, fn ->
      Emerge.Engine.diff_state_update(state, layout1)
    end
  end

  test "on_change is registered in event registry" do
    layout = Emerge.UI.Input.text([key(:field), Event.on_change({self(), :changed})], "hello")
    state = Emerge.Engine.diff_state_new(layout)
    id_bin = :erlang.term_to_binary(state.tree.id)

    assert {:ok, {pid, :changed}} = Emerge.Engine.lookup_event(state, id_bin, :change)
    assert pid == self()
  end

  test "on_focus and on_blur are registered in event registry" do
    layout =
      Emerge.UI.Input.text(
        [
          key(:field),
          Event.on_focus({self(), :focused}),
          Event.on_blur({self(), :blurred})
        ],
        "hello"
      )

    state = Emerge.Engine.diff_state_new(layout)
    id_bin = :erlang.term_to_binary(state.tree.id)

    assert {:ok, {focus_pid, :focused}} = Emerge.Engine.lookup_event(state, id_bin, :focus)
    assert {:ok, {blur_pid, :blurred}} = Emerge.Engine.lookup_event(state, id_bin, :blur)
    assert focus_pid == self()
    assert blur_pid == self()
  end

  test "on_press is registered in event registry" do
    layout =
      Emerge.UI.Input.button(
        [
          key(:save),
          Event.on_press({self(), :pressed})
        ],
        Emerge.UI.text("Save")
      )

    state = Emerge.Engine.diff_state_new(layout)
    id_bin = :erlang.term_to_binary(state.tree.id)

    assert {:ok, {pid, :pressed}} = Emerge.Engine.lookup_event(state, id_bin, :press)
    assert pid == self()
  end

  test "swipe handlers are registered in event registry" do
    layout =
      Emerge.UI.Input.button(
        [
          key(:save),
          Event.on_swipe_up({self(), :swiped_up}),
          Event.on_swipe_down({self(), :swiped_down}),
          Event.on_swipe_left({self(), :swiped_left}),
          Event.on_swipe_right({self(), :swiped_right})
        ],
        Emerge.UI.text("Save")
      )

    state = Emerge.Engine.diff_state_new(layout)
    id_bin = :erlang.term_to_binary(state.tree.id)

    assert {:ok, {_, :swiped_up}} = Emerge.Engine.lookup_event(state, id_bin, :swipe_up)
    assert {:ok, {_, :swiped_down}} = Emerge.Engine.lookup_event(state, id_bin, :swipe_down)
    assert {:ok, {_, :swiped_left}} = Emerge.Engine.lookup_event(state, id_bin, :swipe_left)
    assert {:ok, {_, :swiped_right}} = Emerge.Engine.lookup_event(state, id_bin, :swipe_right)
  end

  test "key listeners are registered in event registry" do
    layout =
      Emerge.UI.Input.button(
        [
          key(:save),
          Event.on_key_down(:enter, {self(), :pressed}),
          Event.on_key_up([key: :escape, mods: [:ctrl]], {self(), :cancelled}),
          Event.on_key_press(:space, {self(), :cycled})
        ],
        Emerge.UI.text("Save")
      )

    state = Emerge.Engine.diff_state_new(layout)
    id_bin = :erlang.term_to_binary(state.tree.id)

    assert {:ok, {press_pid, :pressed}} =
             Emerge.Engine.lookup_event(
               state,
               id_bin,
               {:key_down, Event.key_route_id(:key_down, :enter, [], :exact)}
             )

    assert {:ok, {cancel_pid, :cancelled}} =
             Emerge.Engine.lookup_event(
               state,
               id_bin,
               {:key_up, Event.key_route_id(:key_up, :escape, [:ctrl], :exact)}
             )

    assert {:ok, {cycle_pid, :cycled}} =
             Emerge.Engine.lookup_event(
               state,
               id_bin,
               {:key_press, Event.key_route_id(:key_press, :space, [], :exact)}
             )

    assert press_pid == self()
    assert cancel_pid == self()
    assert cycle_pid == self()
  end

  test "virtual key hold event is registered in event registry" do
    layout =
      Emerge.UI.Input.button(
        [
          key(:soft_a),
          Event.virtual_key(tap: {:text, "a"}, hold: {:event, {self(), :show_alternates}})
        ],
        Emerge.UI.text("A")
      )

    state = Emerge.Engine.diff_state_new(layout)
    id_bin = :erlang.term_to_binary(state.tree.id)

    assert {:ok, {pid, :show_alternates}} =
             Emerge.Engine.lookup_event(state, id_bin, :virtual_key_hold)

    assert pid == self()
  end

  test "text input registers click and mouse handlers alongside on_change" do
    layout =
      Emerge.UI.Input.text(
        [
          key(:field),
          Event.on_click({self(), :clicked}),
          Event.on_mouse_enter({self(), :entered}),
          Event.on_mouse_leave({self(), :left}),
          Event.on_mouse_move({self(), :moved}),
          Event.on_change({self(), :changed})
        ],
        "hello"
      )

    state = Emerge.Engine.diff_state_new(layout)
    id_bin = :erlang.term_to_binary(state.tree.id)

    assert {:ok, {_, :clicked}} = Emerge.Engine.lookup_event(state, id_bin, :click)
    assert {:ok, {_, :entered}} = Emerge.Engine.lookup_event(state, id_bin, :mouse_enter)
    assert {:ok, {_, :left}} = Emerge.Engine.lookup_event(state, id_bin, :mouse_leave)
    assert {:ok, {_, :moved}} = Emerge.Engine.lookup_event(state, id_bin, :mouse_move)
    assert {:ok, {_, :changed}} = Emerge.Engine.lookup_event(state, id_bin, :change)
  end

  test "multiline input registers change and key handlers" do
    layout =
      Emerge.UI.Input.multiline(
        [
          key(:notes),
          Event.on_change({self(), :changed}),
          Event.on_key_down(:enter, {self(), :submitted})
        ],
        "hello\nworld"
      )

    state = Emerge.Engine.diff_state_new(layout)
    id_bin = :erlang.term_to_binary(state.tree.id)

    assert {:ok, {_, :changed}} = Emerge.Engine.lookup_event(state, id_bin, :change)

    assert {:ok, {_, :submitted}} =
             Emerge.Engine.lookup_event(
               state,
               id_bin,
               {:key_down, Event.key_route_id(:key_down, :enter, [], :exact)}
             )
  end

  test "dispatch_event with payload appends payload to tuple message" do
    layout =
      Emerge.UI.Input.text([key(:field), Event.on_change({self(), {:changed, :field}})], "hello")

    state = Emerge.Engine.diff_state_new(layout)
    id_bin = :erlang.term_to_binary(state.tree.id)

    assert :ok == Emerge.Engine.dispatch_event(state, id_bin, :change, "hello!")
    assert_receive {:changed, :field, "hello!"}
  end

  test "dispatch_event with payload wraps non-tuple message" do
    layout = Emerge.UI.Input.text([key(:field), Event.on_change({self(), :changed})], "hello")
    state = Emerge.Engine.diff_state_new(layout)
    id_bin = :erlang.term_to_binary(state.tree.id)

    assert :ok == Emerge.Engine.dispatch_event(state, id_bin, :change, "hello!")
    assert_receive {:changed, "hello!"}
  end

  test "dispatch_event routes focus and blur events" do
    layout =
      Emerge.UI.Input.text(
        [
          key(:field),
          Event.on_focus({self(), :focused}),
          Event.on_blur({self(), :blurred})
        ],
        "hello"
      )

    state = Emerge.Engine.diff_state_new(layout)
    id_bin = :erlang.term_to_binary(state.tree.id)

    assert :ok == Emerge.Engine.dispatch_event(state, id_bin, :focus)
    assert_receive :focused

    assert :ok == Emerge.Engine.dispatch_event(state, id_bin, :blur)
    assert_receive :blurred
  end

  test "dispatch_event routes press events" do
    layout =
      Emerge.UI.Input.button(
        [key(:save), Event.on_press({self(), :pressed})],
        Emerge.UI.text("Save")
      )

    state = Emerge.Engine.diff_state_new(layout)
    id_bin = :erlang.term_to_binary(state.tree.id)

    assert :ok == Emerge.Engine.dispatch_event(state, id_bin, :press)
    assert_receive :pressed
  end

  test "dispatch_event routes key events" do
    layout =
      Emerge.UI.Input.button(
        [
          key(:save),
          Event.on_key_down(:enter, {self(), :pressed}),
          Event.on_key_press(:space, {self(), :cycled})
        ],
        Emerge.UI.text("Save")
      )

    state = Emerge.Engine.diff_state_new(layout)
    id_bin = :erlang.term_to_binary(state.tree.id)

    assert :ok ==
             Emerge.Engine.dispatch_event(
               state,
               id_bin,
               {:key_down, Event.key_route_id(:key_down, :enter, [], :exact)}
             )

    assert_receive :pressed

    assert :ok ==
             Emerge.Engine.dispatch_event(
               state,
               id_bin,
               {:key_press, Event.key_route_id(:key_press, :space, [], :exact)}
             )

    assert_receive :cycled
  end

  test "dispatch_event routes virtual key hold events" do
    layout =
      Emerge.UI.Input.button(
        [
          key(:soft_a),
          Event.virtual_key(tap: {:text, "a"}, hold: {:event, {self(), :show_alternates}})
        ],
        Emerge.UI.text("A")
      )

    state = Emerge.Engine.diff_state_new(layout)
    id_bin = :erlang.term_to_binary(state.tree.id)

    assert :ok == Emerge.Engine.dispatch_event(state, id_bin, :virtual_key_hold)
    assert_receive :show_alternates
  end

  defp content_id_map(%Emerge.Engine.Element{children: children}) do
    children
    |> Enum.map(fn child ->
      text = child.children |> hd() |> Map.get(:attrs) |> Map.get(:content)
      {text, child.id}
    end)
    |> Map.new()
  end
end
